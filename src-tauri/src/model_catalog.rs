//! models.dev model catalog: fetch, local disk cache, TTL-based scheduled
//! refresh, and id matching.
//!
//! The catalog lives in resh's own app data directory
//! (`<app_data_dir>/model-catalog.json`), NOT under any plugin or tool cache
//! path, so it travels with the resh installation on any machine. The on-disk
//! document stores the raw api.json body plus fetch metadata:
//!
//! ```json
//! { "fetchedAt": 1787036509332, "body": { "<provider>": { "models": { ... } } } }
//! ```
//!
//! Freshness is driven by `TTL_DAYS` (a cache older than that is stale) and a
//! background task in `main.rs` that checks every `CHECK_INTERVAL` (7 天) and
//! refreshes when stale (定时更新). Lookups additionally refresh lazily when
//! the cache is stale, and a failed fetch keeps the last good cache.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub const CATALOG_URL: &str = "https://models.dev/api.json";
pub const CACHE_FILE_NAME: &str = "model-catalog.json";
/// A cached catalog older than this is considered stale (days).
pub const TTL_DAYS: u64 = 7;
/// Cadence of the background scheduled freshness check (7 天更新一次).
/// Lookups additionally refresh lazily when the cache is stale, so a missed
/// tick is covered by the next form lookup.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// How many runner-up candidates a lookup returns at most.
const MAX_CANDIDATES: usize = 8;

/// First-party vendors preferred when the same bare model id appears under
/// several providers (tie-break, mirrors models.dev's own layout).
const OFFICIAL_PROVIDERS: &[&str] = &[
    "deepseek",
    "openai",
    "anthropic",
    "google",
    "xai",
    "moonshotai",
    "zai",
    "zhipuai",
    "minimax",
    "stepfun",
    "xiaomi",
    "meta",
    "cohere",
    "nvidia",
    "alibaba",
];

/// One flattened models.dev entry (wire-safe view for the frontend).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    /// models.dev provider id that carries this model.
    pub provider: String,
    /// Full model id as models.dev keys it (may carry a vendor/ prefix).
    pub id: String,
    /// Display name.
    pub name: Option<String>,
    /// Context window in tokens, when known.
    pub context: Option<u64>,
    /// Output limit in tokens, when known.
    pub output: Option<u64>,
}

/// Result of one lookup: the best match plus the runner-up candidates.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogLookup {
    pub best: Option<CatalogEntry>,
    pub candidates: Vec<CatalogEntry>,
}

/// The status view shown on the AI settings page.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatus {
    pub entries: usize,
    pub providers: usize,
    /// Epoch ms the cache was last fetched (0 = never).
    pub fetched_at: u64,
    pub fresh: bool,
    pub ttl_days: u64,
    /// Epoch ms of the next scheduled refresh (fetched_at + TTL).
    pub next_auto_refresh_at: u64,
}

/// The cached raw document on disk: api.json body plus fetch metadata.
#[derive(Serialize, Deserialize)]
struct CacheDocument {
    fetched_at: u64,
    body: serde_json::Value,
}

struct Snapshot {
    entries: Vec<CatalogEntry>,
    providers: usize,
    fetched_at: u64,
    initialized: bool,
}

pub struct ModelCatalog {
    cache_path: PathBuf,
    snapshot: Arc<Mutex<Snapshot>>,
    /// Serializes network refreshes so concurrent lookups share one fetch.
    refresh_lock: Arc<Mutex<()>>,
}

impl Clone for ModelCatalog {
    fn clone(&self) -> Self {
        Self {
            cache_path: self.cache_path.clone(),
            snapshot: Arc::clone(&self.snapshot),
            refresh_lock: Arc::clone(&self.refresh_lock),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The OS system proxy URL (http://host:port) for outbound HTTPS, when one is
/// configured. macOS: parsed from `scutil --proxy` (the system network
/// settings, e.g. a local clash/proxy client). Other platforms: `None` —
/// reqwest already honors HTTP(S)_PROXY environment variables by default.
#[cfg(target_os = "macos")]
fn system_proxy_url() -> Option<String> {
    let output = std::process::Command::new("scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_scutil_proxy(&String::from_utf8_lossy(&output.stdout))
}

/// Parse the `scutil --proxy` text (key : value lines) into an http proxy URL.
/// Prefers the HTTPS proxy, falls back to the HTTP proxy.
#[cfg(target_os = "macos")]
fn parse_scutil_proxy(text: &str) -> Option<String> {
    for prefix in ["HTTPS", "HTTP"] {
        let mut enabled = false;
        let mut host: Option<String> = None;
        let mut port: Option<u16> = None;
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if key == format!("{}Enable", prefix) {
                enabled = value == "1";
            } else if key == format!("{}Proxy", prefix) {
                if !value.is_empty() {
                    host = Some(value.to_string());
                }
            } else if key == format!("{}Port", prefix) {
                port = value.parse().ok();
            }
        }
        if enabled {
            if let (Some(host), Some(port)) = (host, port) {
                return Some(format!("http://{}:{}", host, port));
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn system_proxy_url() -> Option<String> {
    None
}

impl ModelCatalog {
    pub fn new(app_data_dir: &std::path::Path) -> Self {
        Self {
            cache_path: app_data_dir.join(CACHE_FILE_NAME),
            snapshot: Arc::new(Mutex::new(Snapshot {
                entries: Vec::new(),
                providers: 0,
                fetched_at: 0,
                initialized: false,
            })),
            refresh_lock: Arc::new(Mutex::new(())),
        }
    }

    /// The current status view (never loads or fetches).
    pub async fn status(&self) -> CatalogStatus {
        let snap = self.snapshot.lock().await;
        self.status_of(&snap)
    }

    fn status_of(&self, snap: &Snapshot) -> CatalogStatus {
        let now = now_ms();
        let fresh = snap.fetched_at > 0 && now.saturating_sub(snap.fetched_at) < TTL_DAYS * 24 * 60 * 60 * 1000;
        CatalogStatus {
            entries: snap.entries.len(),
            providers: snap.providers,
            fetched_at: snap.fetched_at,
            fresh,
            ttl_days: TTL_DAYS,
            next_auto_refresh_at: snap.fetched_at.saturating_add(TTL_DAYS * 24 * 60 * 60 * 1000),
        }
    }

    /// Load the disk cache once, then fetch when stale (or `force`). A failed
    /// fetch keeps the last good cache; with no cache at all the snapshot
    /// stays empty and the error is returned so callers can surface it.
    /// Concurrent callers share one in-flight refresh.
    /// @returns true when the snapshot may have changed.
    pub async fn ensure_fresh(&self, force: bool) -> Result<bool, String> {
        {
            let mut snap = self.snapshot.lock().await;
            if !snap.initialized {
                self.load_cache_locked(&mut snap);
                snap.initialized = true;
            }
        }
        let stale = {
            let snap = self.snapshot.lock().await;
            snap.fetched_at == 0
                || now_ms().saturating_sub(snap.fetched_at) >= TTL_DAYS * 24 * 60 * 60 * 1000
        };
        if !force && !stale {
            return Ok(false);
        }

        let _guard = self.refresh_lock.lock().await;
        // Re-check under the lock: another caller may have refreshed meanwhile.
        let stale_under_lock = {
            let snap = self.snapshot.lock().await;
            snap.fetched_at == 0
                || now_ms().saturating_sub(snap.fetched_at) >= TTL_DAYS * 24 * 60 * 60 * 1000
        };
        if !force && !stale_under_lock {
            return Ok(false);
        }

        let body = self.fetch().await?;
        let (entries, providers) = flatten(&body);
        let fetched_at = now_ms();
        {
            let mut snap = self.snapshot.lock().await;
            snap.entries = entries;
            snap.providers = providers;
            snap.fetched_at = fetched_at;
        }
        self.persist(&body, fetched_at);
        Ok(true)
    }

    /// Look up one model id: best match + runner-up candidates. The cache is
    /// loaded lazily and refreshed only when stale; a lookup never blocks on a
    /// network fetch that already failed (it falls back to whatever is cached).
    pub async fn lookup(&self, model_id: &str) -> Result<CatalogLookup, String> {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return Ok(CatalogLookup {
                best: None,
                candidates: Vec::new(),
            });
        }
        // Best-effort freshness; failures keep the cached snapshot.
        let _ = self.ensure_fresh(false).await;

        let snap = self.snapshot.lock().await;
        let mut candidates: Vec<&CatalogEntry> = snap
            .entries
            .iter()
            .filter(|entry| matches(entry, trimmed))
            .collect();
        if candidates.is_empty() {
            return Ok(CatalogLookup {
                best: None,
                candidates: Vec::new(),
            });
        }
        candidates.sort_by(|a, b| compare(a, b, trimmed));
        let best = candidates[0].clone();
        let candidates = candidates
            .into_iter()
            .take(MAX_CANDIDATES)
            .cloned()
            .collect();
        Ok(CatalogLookup {
            best: Some(best),
            candidates,
        })
    }

    // ── disk / wire ─────────────────────────────────────────────────────────

    fn load_cache_locked(&self, snap: &mut Snapshot) {
        let Ok(content) = fs::read_to_string(&self.cache_path) else {
            return;
        };
        let doc: CacheDocument = match serde_json::from_str(&content) {
            Ok(doc) => doc,
            Err(e) => {
                tracing::warn!("model catalog: cache is malformed ({e}); ignoring");
                return;
            }
        };
        let (entries, providers) = flatten(&doc.body);
        snap.entries = entries;
        snap.providers = providers;
        snap.fetched_at = doc.fetched_at;
        tracing::info!(
            "model catalog: loaded {} entries from cache (fetched {})",
            snap.entries.len(),
            snap.fetched_at
        );
    }

    async fn fetch(&self) -> Result<serde_json::Value, String> {
        let mut builder = reqwest::Client::builder()
            .user_agent(format!("Resh/{}", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(3));
        // Follow the OS system proxy when one is configured (e.g. a local
        // clash/proxy client): models.dev is often unreachable directly.
        // Without this the catalog fetch fails on machines that only reach
        // the internet through the system proxy.
        if let Some(proxy_url) = system_proxy_url() {
            match reqwest::Proxy::all(&proxy_url) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                }
                Err(e) => {
                    tracing::warn!("model catalog: invalid system proxy '{proxy_url}': {e}");
                }
            }
        }
        let client = builder
            .build()
            .map_err(|e| format!("build http client: {e}"))?;
        let response = client
            .get(CATALOG_URL)
            .send()
            .await
            .map_err(|e| format!("fetch {}: {e}", CATALOG_URL))?;
        if !response.status().is_success() {
            return Err(format!("fetch {}: HTTP {}", CATALOG_URL, response.status()));
        }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("parse {}: {e}", CATALOG_URL))?;
        Ok(body)
    }

    fn persist(&self, body: &serde_json::Value, fetched_at: u64) {
        let doc = CacheDocument {
            fetched_at,
            body: body.clone(),
        };
        let bytes = match serde_json::to_vec(&doc) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!("model catalog: serialize cache: {e}");
                return;
            }
        };
        if let Some(parent) = self.cache_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                tracing::warn!("model catalog: create cache dir: {e}");
                return;
            }
        }
        if let Err(e) = fs::write(&self.cache_path, bytes) {
            tracing::warn!("model catalog: write cache: {e}");
        }
    }
}

/// Flatten the api.json body into entries + provider count.
fn flatten(body: &serde_json::Value) -> (Vec<CatalogEntry>, usize) {
    let mut entries = Vec::new();
    let Some(providers_map) = body.as_object() else {
        return (entries, 0);
    };
    for (provider, raw) in providers_map {
        let Some(models) = raw.get("models").and_then(|m| m.as_object()) else {
            continue;
        };
        for (id, raw_model) in models {
            let name = raw_model
                .get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            let limit = raw_model.get("limit");
            let context = limit.and_then(|l| l.get("context")).and_then(|v| v.as_u64());
            let output = limit.and_then(|l| l.get("output")).and_then(|v| v.as_u64());
            entries.push(CatalogEntry {
                provider: provider.clone(),
                id: id.clone(),
                name,
                context,
                output,
            });
        }
    }
    (entries, providers_map.len())
}

/// The last path segment of a model id, lowercased (`Qwen/Qwen3.7-Max` → `qwen3.7-max`).
fn stripped_id(id: &str) -> String {
    id.rsplit('/')
        .next()
        .unwrap_or(id)
        .to_ascii_lowercase()
}

/// Bare ids match the last path segment case-insensitively; ids already
/// carrying a vendor/ prefix also match the full models.dev key verbatim.
fn matches(entry: &CatalogEntry, model_id: &str) -> bool {
    let needle = stripped_id(model_id);
    if stripped_id(&entry.id) == needle {
        return true;
    }
    model_id.contains('/') && entry.id.eq_ignore_ascii_case(model_id)
}

fn official_rank(provider: &str) -> usize {
    OFFICIAL_PROVIDERS
        .iter()
        .position(|p| *p == provider)
        .unwrap_or(OFFICIAL_PROVIDERS.len())
}

fn completeness(entry: &CatalogEntry) -> usize {
    if entry.context.is_some() && entry.output.is_some() {
        0
    } else {
        1
    }
}

/// Stable total order: exact full-id match, official vendor rank, entry
/// completeness, then provider id, then full id.
fn compare(a: &CatalogEntry, b: &CatalogEntry, model_id: &str) -> std::cmp::Ordering {
    let score = |e: &CatalogEntry| -> (usize, usize, usize) {
        let exact = if model_id.contains('/') && e.id.eq_ignore_ascii_case(model_id) {
            0
        } else {
            1
        };
        (exact, official_rank(&e.provider), completeness(e))
    };
    score(a)
        .cmp(&score(b))
        .then_with(|| a.provider.cmp(&b.provider))
        .then_with(|| a.id.cmp(&b.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_body() -> serde_json::Value {
        json!({
            "deepseek": { "models": {
                "deepseek-chat": { "name": "DeepSeek Chat", "limit": { "context": 1000000, "output": 384000 } }
            }},
            "abacus": { "models": {
                "qwen3.7-max": { "name": "Qwen3.7 Max", "limit": { "context": 1000000, "output": 64000 } }
            }},
            "alibaba": { "models": {
                "qwen3.7-max": { "name": "Qwen3.7 Max (Alibaba)", "limit": { "context": 200000, "output": 32000 } }
            }},
            "minimal": { "models": {
                "bare-model": { "name": "Bare" }
            }},
            "proxy": { "models": {
                "openai/gpt-4o-mini": { "name": "GPT-4o mini", "limit": { "context": 128000, "output": 16384 } }
            }}
        })
    }

    #[test]
    fn flatten_parses_limits_and_counts_providers() {
        let (entries, providers) = flatten(&sample_body());
        assert_eq!(providers, 5);
        assert_eq!(entries.len(), 5);
        let ds = entries.iter().find(|e| e.provider == "deepseek").unwrap();
        assert_eq!(ds.context, Some(1_000_000));
        assert_eq!(ds.output, Some(384_000));
        let bare = entries.iter().find(|e| e.id == "bare-model").unwrap();
        assert_eq!(bare.context, None);
        assert_eq!(bare.output, None);
    }

    #[test]
    fn lookup_prefers_official_vendor_and_exact_ids() {
        let (entries, _) = flatten(&sample_body());
        // Bare id matches the stripped segment.
        let deepseek = entries.iter().find(|e| e.id == "deepseek-chat").unwrap();
        assert!(matches(deepseek, "deepseek-chat"));

        // Full vendor-prefixed id matches the models.dev key verbatim.
        let proxied = entries
            .iter()
            .find(|e| e.id == "openai/gpt-4o-mini")
            .unwrap();
        assert!(matches(proxied, "openai/gpt-4o-mini"));
        // ...and the bare segment also matches it.
        assert!(matches(proxied, "gpt-4o-mini"));

        let qwen: Vec<&CatalogEntry> = entries
            .iter()
            .filter(|e| matches(e, "qwen3.7-max"))
            .collect();
        assert_eq!(qwen.len(), 2);
        let mut sorted = qwen.clone();
        sorted.sort_by(|a, b| compare(a, b, "qwen3.7-max"));
        // Official rank: alibaba (in OFFICIAL_PROVIDERS) beats abacus.
        assert_eq!(sorted[0].provider, "alibaba");

        // Unknown id matches nothing.
        assert!(!matches(deepseek, "nope"));
    }

    #[test]
    fn official_rank_orders_first_party_before_others() {
        assert_eq!(official_rank("deepseek"), 0);
        assert!(official_rank("alibaba") < official_rank("abacus"));
    }

    #[test]
    fn duplicate_ids_resolve_by_priority_chain() {
        // Same bare id under four providers with different traits:
        //  - "vendor1" non-official but complete
        //  - "openai" official but incomplete (no output)
        //  - "abacus" non-official, complete — loses to official
        //  - "alibaba" official + complete — wins
        let body = json!({
            "vendor1": { "models": {
                "dupe-model": { "name": "D", "limit": { "context": 1000, "output": 500 } }
            }},
            "openai": { "models": {
                "dupe-model": { "name": "D (openai)", "limit": { "context": 2000 } }
            }},
            "abacus": { "models": {
                "dupe-model": { "name": "D (abacus)", "limit": { "context": 3000, "output": 600 } }
            }},
            "alibaba": { "models": {
                "dupe-model": { "name": "D (alibaba)", "limit": { "context": 4000, "output": 700 } }
            }}
        });
        let (entries, _) = flatten(&body);
        let mut dupes: Vec<&CatalogEntry> = entries
            .iter()
            .filter(|e| matches(e, "dupe-model"))
            .collect();
        assert_eq!(dupes.len(), 4);
        dupes.sort_by(|a, b| compare(a, b, "dupe-model"));
        // Official-provider rank comes first: openai (rank 1) beats alibaba (rank 14).
        assert_eq!(dupes[0].provider, "openai");
        assert_eq!(dupes[1].provider, "alibaba");
        // Non-official entries fall back to completeness, then provider id.
        assert_eq!(dupes[2].provider, "abacus"); // complete
        assert_eq!(dupes[3].provider, "vendor1"); // incomplete, alphabetical after abacus
    }

    #[test]
    fn exact_vendor_prefixed_id_beats_bare_duplicates() {
        // A vendor-prefixed full id must beat a bare-id entry from an official
        // provider when the user typed the prefixed id verbatim.
        let body = json!({
            "openai": { "models": {
                "gpt-x": { "name": "GPT-X", "limit": { "context": 1000, "output": 500 } }
            }},
            "reseller": { "models": {
                "openai/gpt-x": { "name": "GPT-X (reseller)", "limit": { "context": 9999, "output": 999 } }
            }}
        });
        let (entries, _) = flatten(&body);
        let mut matches: Vec<&CatalogEntry> = entries
            .iter()
            .filter(|e| matches(e, "openai/gpt-x"))
            .collect();
        assert_eq!(matches.len(), 2);
        matches.sort_by(|a, b| compare(a, b, "openai/gpt-x"));
        // Exact full-id match wins over official-provider bare-id entry.
        assert_eq!(matches[0].provider, "reseller");
        assert_eq!(matches[0].id, "openai/gpt-x");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_scutil_proxy_output() {
        let text = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 7890\n  HTTPProxy : 127.0.0.1\n  HTTPSEnable : 1\n  HTTPSPort : 7890\n  HTTPSProxy : 127.0.0.1\n  SOCKSEnable : 1\n}";
        assert_eq!(
            parse_scutil_proxy(text).as_deref(),
            Some("http://127.0.0.1:7890")
        );
        // Disabled HTTPS falls back to HTTP.
        let http_only = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 8080\n  HTTPProxy : 10.0.0.1\n  HTTPSEnable : 0\n}";
        assert_eq!(
            parse_scutil_proxy(http_only).as_deref(),
            Some("http://10.0.0.1:8080")
        );
        // No proxy configured at all.
        assert_eq!(parse_scutil_proxy("<dictionary> {\n}"), None);
        // Enabled but missing host/port -> None.
        assert_eq!(parse_scutil_proxy("HTTPSEnable : 1\n"), None);
    }

    /// Real-network smoke test (ignored by default): verifies the catalog can
    /// be fetched through the system proxy on this machine.
    /// Run: cargo test model_catalog -- --ignored --nocapture
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore]
    async fn real_fetch_through_system_proxy() {
        let catalog = ModelCatalog::new(std::path::Path::new("/tmp/mp-real"));
        let body = catalog.fetch().await.expect("fetch should succeed");
        let (entries, providers) = flatten(&body);
        println!("fetched {providers} providers, {} entries", entries.len());
        assert!(entries.len() > 1000, "expected a full catalog, got {}", entries.len());
    }
}
