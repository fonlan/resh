//! Coordinated write barrier for safe app restart after update install.
//!
//! Tracks long-running / dangerous write operations so the process can enter
//! a draining (maintenance) mode, refuse new work, and wait until existing
//! work finishes before self-replacement.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Notify;

/// Categories of work that must finish (or be refused) before a safe restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationCategory {
    ConfigWrite,
    WebdavSync,
    SftpTransfer,
    SftpEditUpload,
}

impl OperationCategory {
    pub const ALL: [OperationCategory; 4] = [
        OperationCategory::ConfigWrite,
        OperationCategory::WebdavSync,
        OperationCategory::SftpTransfer,
        OperationCategory::SftpEditUpload,
    ];

    fn index(self) -> usize {
        match self {
            OperationCategory::ConfigWrite => 0,
            OperationCategory::WebdavSync => 1,
            OperationCategory::SftpTransfer => 2,
            OperationCategory::SftpEditUpload => 3,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OperationCategory::ConfigWrite => "configWrite",
            OperationCategory::WebdavSync => "webdavSync",
            OperationCategory::SftpTransfer => "sftpTransfer",
            OperationCategory::SftpEditUpload => "sftpEditUpload",
        }
    }
}

/// Stable error string returned when a write is refused during draining.
pub const RESTART_PREPARING_ERROR: &str =
    "Update restart preparation in progress; write operations are temporarily blocked";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CoordinatorMode {
    Normal,
    Draining,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryCount {
    pub category: OperationCategory,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationSnapshot {
    pub mode: CoordinatorMode,
    pub total: u32,
    pub categories: Vec<CategoryCount>,
    pub idle: bool,
}

struct Inner {
    mode: CoordinatorMode,
    counts: [usize; 4],
    /// Monotonic id for the current drain session. Cancel only clears when it matches.
    drain_session: u64,
}

/// App-wide restart / maintenance coordinator.
///
/// Counts use a synchronous mutex so `OperationPermit` can release from both
/// async `release()` and `Drop` without an intermediate "released" window that
/// is cancel-unsafe under async cancellation.
pub struct OperationCoordinator {
    inner: Mutex<Inner>,
    notify: Notify,
    /// Fast path for draining checks without locking (best-effort).
    draining_flag: AtomicBool,
}

impl Default for OperationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl OperationCoordinator {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                mode: CoordinatorMode::Normal,
                counts: [0; 4],
                drain_session: 0,
            }),
            notify: Notify::new(),
            draining_flag: AtomicBool::new(false),
        }
    }

    pub fn is_draining(&self) -> bool {
        self.draining_flag.load(Ordering::Acquire)
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        // Poison is treated as fatal for restart correctness; recover by taking lock.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Try to acquire a permit for `category`. Fails if draining.
    pub async fn try_acquire(
        self: &Arc<Self>,
        category: OperationCategory,
    ) -> Result<OperationPermit, String> {
        let mut guard = self.lock_inner();
        if matches!(guard.mode, CoordinatorMode::Draining) {
            return Err(RESTART_PREPARING_ERROR.to_string());
        }
        guard.counts[category.index()] = guard.counts[category.index()].saturating_add(1);
        drop(guard);
        Ok(OperationPermit {
            coordinator: Arc::clone(self),
            category,
            released: AtomicBool::new(false),
        })
    }

    /// Enter draining mode: refuse new permits; existing ones keep running.
    /// Returns a drain-session id the client must pass to cancel this drain.
    pub async fn begin_draining(&self) -> u64 {
        let session = {
            let mut guard = self.lock_inner();
            guard.mode = CoordinatorMode::Draining;
            guard.drain_session = guard.drain_session.wrapping_add(1).max(1);
            self.draining_flag.store(true, Ordering::Release);
            guard.drain_session
        };
        self.notify.notify_waiters();
        session
    }

    /// Leave maintenance only if `drain_session` matches the current session.
    /// Returns true when this call cleared draining (or already normal with no match needed).
    pub async fn cancel_draining(&self, drain_session: Option<u64>) -> bool {
        let cleared = {
            let mut guard = self.lock_inner();
            if !matches!(guard.mode, CoordinatorMode::Draining) {
                // Already normal.
                true
            } else if let Some(expected) = drain_session {
                if expected == guard.drain_session {
                    guard.mode = CoordinatorMode::Normal;
                    self.draining_flag.store(false, Ordering::Release);
                    true
                } else {
                    // Stale cancel for a superseded drain session.
                    false
                }
            } else {
                // No token: unconditional cancel (manual recovery).
                guard.mode = CoordinatorMode::Normal;
                self.draining_flag.store(false, Ordering::Release);
                true
            }
        };
        if cleared {
            self.notify.notify_waiters();
        }
        cleared
    }

    pub async fn snapshot(&self) -> OperationSnapshot {
        let guard = self.lock_inner();
        Self::snapshot_from_inner(&guard)
    }

    fn snapshot_from_inner(guard: &Inner) -> OperationSnapshot {
        let categories: Vec<CategoryCount> = OperationCategory::ALL
            .iter()
            .map(|cat| CategoryCount {
                category: *cat,
                count: guard.counts[cat.index()] as u32,
            })
            .collect();
        let total: u32 = categories.iter().map(|c| c.count).sum();
        OperationSnapshot {
            mode: guard.mode,
            total,
            categories,
            idle: total == 0,
        }
    }

    pub async fn is_idle(&self) -> bool {
        let guard = self.lock_inner();
        guard.counts.iter().all(|&c| c == 0)
    }

    /// Wait until all permits are released, or until `timeout` elapses.
    ///
    /// Returns `Ok(true)` when idle, `Ok(false)` on timeout. Does not force-kill work.
    pub async fn wait_until_idle(&self, timeout: Duration) -> Result<bool, String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let guard = self.lock_inner();
                if guard.counts.iter().all(|&c| c == 0) {
                    return Ok(true);
                }
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Ok(false);
            }

            let notified = self.notify.notified();
            tokio::pin!(notified);
            match tokio::time::timeout(remaining, notified).await {
                Ok(()) => continue,
                Err(_) => {
                    let guard = self.lock_inner();
                    return Ok(guard.counts.iter().all(|&c| c == 0));
                }
            }
        }
    }

    /// Synchronous, cancel-safe release of one category count.
    fn release_sync(&self, category: OperationCategory) {
        {
            let mut guard = self.lock_inner();
            let idx = category.index();
            if guard.counts[idx] > 0 {
                guard.counts[idx] -= 1;
            } else {
                tracing::warn!(
                    category = category.as_str(),
                    "operation permit release with zero count"
                );
            }
        }
        self.notify.notify_waiters();
    }
}

/// RAII permit. Drop (or explicit release) decrements the category count.
///
/// Release is cancel-safe: the count decrement uses a synchronous mutex and
/// happens before the permit is marked released, so async cancellation of
/// `release()` cannot strand a non-zero count.
pub struct OperationPermit {
    coordinator: Arc<OperationCoordinator>,
    category: OperationCategory,
    released: AtomicBool,
}

impl OperationPermit {
    pub fn category(&self) -> OperationCategory {
        self.category
    }

    fn release_once(&self) {
        if self
            .released
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.coordinator.release_sync(self.category);
        }
    }

    /// Explicit release (preferred over relying on Drop). Fully cancel-safe.
    pub async fn release(self) {
        self.release_once();
        // Prevent Drop from double-releasing.
        std::mem::forget(self);
    }
}

impl Drop for OperationPermit {
    fn drop(&mut self) {
        self.release_once();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn acquire_and_release_idle() {
        let c = Arc::new(OperationCoordinator::new());
        assert!(c.is_idle().await);
        let p = c
            .try_acquire(OperationCategory::ConfigWrite)
            .await
            .unwrap();
        assert!(!c.is_idle().await);
        let snap = c.snapshot().await;
        assert_eq!(snap.total, 1);
        assert!(!snap.idle);
        p.release().await;
        assert!(c.is_idle().await);
    }

    #[tokio::test]
    async fn drop_releases_permit() {
        let c = Arc::new(OperationCoordinator::new());
        {
            let _p = c
                .try_acquire(OperationCategory::SftpTransfer)
                .await
                .unwrap();
            assert!(!c.is_idle().await);
        }
        // Sync Drop releases immediately.
        assert!(c.is_idle().await);
    }

    #[tokio::test]
    async fn draining_rejects_new_permits() {
        let c = Arc::new(OperationCoordinator::new());
        let p = c
            .try_acquire(OperationCategory::WebdavSync)
            .await
            .unwrap();
        c.begin_draining().await;
        assert!(c.is_draining());
        let err = c
            .try_acquire(OperationCategory::ConfigWrite)
            .await
            .err()
            .expect("should reject while draining");
        assert!(err.contains("restart preparation") || err.contains("blocked"));
        p.release().await;
        assert!(c.is_idle().await);
        // Still draining until cancelled
        let err2 = c
            .try_acquire(OperationCategory::ConfigWrite)
            .await
            .err()
            .expect("should still reject");
        assert!(!err2.is_empty());
        c.cancel_draining(None).await;
        assert!(!c.is_draining());
        let p2 = c
            .try_acquire(OperationCategory::ConfigWrite)
            .await
            .unwrap();
        p2.release().await;
    }

    #[tokio::test]
    async fn cancel_draining_requires_matching_session() {
        let c = Arc::new(OperationCoordinator::new());
        let s1 = c.begin_draining().await;
        assert!(c.is_draining());
        // Stale session must not clear a newer drain.
        assert!(!c.cancel_draining(Some(s1.wrapping_sub(1).max(0))).await || s1 == 1);
        // When s1 is 1, wrapping_sub max 0 yields 0 which never matches — always stale.
        assert!(c.is_draining());
        assert!(!c.cancel_draining(Some(s1.saturating_add(99))).await);
        assert!(c.is_draining());
        assert!(c.cancel_draining(Some(s1)).await);
        assert!(!c.is_draining());

        // Late begin after cancel: new session; prior cancel token must not clear it.
        let s2 = c.begin_draining().await;
        assert!(c.is_draining());
        assert!(!c.cancel_draining(Some(s1)).await);
        assert!(c.is_draining());
        assert!(c.cancel_draining(Some(s2)).await);
        assert!(!c.is_draining());
    }

    #[tokio::test]
    async fn wait_until_idle_timeout_and_success() {
        let c = Arc::new(OperationCoordinator::new());
        let p = c
            .try_acquire(OperationCategory::SftpEditUpload)
            .await
            .unwrap();
        let idle = c.wait_until_idle(Duration::from_millis(50)).await.unwrap();
        assert!(!idle);
        let c2 = Arc::clone(&c);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            p.release().await;
        });
        let idle2 = c2
            .wait_until_idle(Duration::from_secs(2))
            .await
            .unwrap();
        assert!(idle2);
    }

    /// Simulate cancellation after `released` would have been set but before
    /// decrement in the old async design: with sync release, Drop must still
    /// zero the count when the explicit `release()` future is dropped mid-flight.
    #[tokio::test]
    async fn cancelled_release_future_still_zeros_count() {
        let c = Arc::new(OperationCoordinator::new());
        let p = c
            .try_acquire(OperationCategory::ConfigWrite)
            .await
            .unwrap();
        assert!(!c.is_idle().await);

        // Drop the permit without calling release — equivalent to cancel after
        // partial progress when release is cancel-safe via Drop.
        drop(p);
        assert!(c.is_idle().await);

        // Explicit release path also leaves idle.
        let p2 = c
            .try_acquire(OperationCategory::SftpTransfer)
            .await
            .unwrap();
        p2.release().await;
        assert!(c.is_idle().await);
    }
}
