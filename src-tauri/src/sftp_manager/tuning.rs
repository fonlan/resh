use std::time::{Duration, Instant};

use russh_sftp::extensions::LimitsExtension;

pub(super) const SFTP_REQUEST_TIMEOUT_SECS: u64 = 60;
pub(super) const DOWNLOAD_CHUNK_SIZE_SAFE: u64 = 32 * 1024;
pub(super) const DOWNLOAD_CHUNK_SIZE_BALANCED: u64 = 64 * 1024;
pub(super) const DOWNLOAD_CHUNK_SIZE_FAST: u64 = 128 * 1024;
pub(super) const DOWNLOAD_MAX_INFLIGHT_SAFE: usize = 8;
pub(super) const DOWNLOAD_MAX_INFLIGHT_BALANCED: usize = 32;
pub(super) const DOWNLOAD_MAX_INFLIGHT_FAST: usize = 48;
pub(super) const DOWNLOAD_TARGET_OUTSTANDING_BYTES_SAFE: u64 = 1024 * 1024;
pub(super) const DOWNLOAD_TARGET_OUTSTANDING_BYTES_BALANCED: u64 = 4 * 1024 * 1024;
pub(super) const DOWNLOAD_TARGET_OUTSTANDING_BYTES_FAST: u64 = 8 * 1024 * 1024;
pub(super) const DOWNLOAD_CHUNK_ROUNDING_BYTES: u64 = 32 * 1024;
pub(super) const MIN_CHUNK_SIZE_BYTES: u64 = 4 * 1024;
pub(super) const MAX_CHUNK_SIZE_BYTES: u64 = 1024 * 1024;
pub(super) const UPLOAD_CHUNK_SIZE_SAFE: u64 = 64 * 1024;
pub(super) const UPLOAD_CHUNK_SIZE_BALANCED: u64 = 128 * 1024;
pub(super) const UPLOAD_CHUNK_SIZE_FAST: u64 = 256 * 1024;
pub(super) const UPLOAD_MAX_INFLIGHT_SAFE: usize = 6;
pub(super) const UPLOAD_MAX_INFLIGHT_BALANCED: usize = 12;
pub(super) const UPLOAD_MAX_INFLIGHT_FAST: usize = 16;
pub(super) const UPLOAD_CHUNK_WRITE_TIMEOUT_SECS: u64 = 30;
pub(super) const UPLOAD_TIMEOUT_DOWNGRADE_THRESHOLD: u32 = 2;
pub(super) const UPLOAD_MAX_RETRIES_PER_CHUNK: u8 = 2;
pub(super) const DOWNLOAD_CHUNK_READ_TIMEOUT_SECS: u64 = 30;
pub(super) const DOWNLOAD_TIMEOUT_DOWNGRADE_THRESHOLD: u32 = 2;
pub(super) const DOWNLOAD_FALLBACK_LOCK_TIMEOUT_THRESHOLD: u32 = 4;
pub(super) const DOWNLOAD_STALL_FORCE_SINGLE_FLIGHT_SECS: u64 = 20;
pub(super) const DOWNLOAD_RAMP_UP_SUCCESS_CHUNKS: u32 = 4;
pub(super) const DOWNLOAD_CHUNK_GROWTH_SUCCESS_CHUNKS: u32 = 4;
pub(super) const DOWNLOAD_BDP_TARGET_MULTIPLIER: f64 = 1.5;
pub(super) const DOWNLOAD_MAX_RETRIES_PER_CHUNK: u8 = 2;
pub(super) const MAX_INFLIGHT_LIMIT: usize = 64;
pub(super) const TRANSFER_DIAG_INTERVAL_SECS: u64 = 2;
pub(super) const DEFAULT_MAX_CONCURRENT_TRANSFERS: u32 = 2;
pub(super) const DEFAULT_MAX_CONCURRENT_TRANSFERS_PER_SESSION: u32 = 2;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SftpServerLimits {
    pub(super) max_packet_len: Option<u64>,
    pub(super) max_read_len: Option<u64>,
    pub(super) max_write_len: Option<u64>,
    pub(super) max_open_handles: Option<u64>,
}

impl SftpServerLimits {
    pub(super) fn from_extension(limits: LimitsExtension) -> Self {
        Self {
            max_packet_len: (limits.max_packet_len > 0).then_some(limits.max_packet_len),
            max_read_len: (limits.max_read_len > 0).then_some(limits.max_read_len),
            max_write_len: (limits.max_write_len > 0).then_some(limits.max_write_len),
            max_open_handles: (limits.max_open_handles > 0).then_some(limits.max_open_handles),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum TransferProfile {
    Safe,
    Balanced,
    Fast,
}

impl TransferProfile {
    pub(super) fn from_str(raw: &str) -> Self {
        match raw {
            "safe" => Self::Safe,
            "fast" => Self::Fast,
            _ => Self::Balanced,
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Balanced => "balanced",
            Self::Fast => "fast",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TransferRuntimeConfig {
    pub(super) profile: TransferProfile,
    pub(super) download_max_inflight: usize,
    pub(super) upload_max_inflight: usize,
    pub(super) chunk_size_min: u64,
    pub(super) chunk_size_max: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TransferTuning {
    pub(super) profile: TransferProfile,
    pub(super) download_chunk_size: u64,
    pub(super) download_chunk_size_min: u64,
    pub(super) download_chunk_size_max: u64,
    pub(super) download_target_outstanding_bytes: u64,
    pub(super) upload_chunk_size: u64,
    pub(super) download_max_inflight: usize,
    pub(super) upload_max_inflight: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SpeedSampler {
    pub(super) display_speed: f64,
    pub(super) last_sample_at: Instant,
    pub(super) last_sample_bytes: u64,
}

impl SpeedSampler {
    pub(super) fn new(start_at: Instant) -> Self {
        Self {
            display_speed: 0.0,
            last_sample_at: start_at,
            last_sample_bytes: 0,
        }
    }

    pub(super) fn sample(&mut self, now: Instant, transferred_bytes: u64) -> f64 {
        let sample_secs = now.duration_since(self.last_sample_at).as_secs_f64();
        if sample_secs > 0.0 && transferred_bytes >= self.last_sample_bytes {
            let instant_speed = (transferred_bytes - self.last_sample_bytes) as f64 / sample_secs;
            self.display_speed = if self.display_speed > 0.0 {
                (self.display_speed * 0.6) + (instant_speed * 0.4)
            } else {
                instant_speed
            };
        }
        self.last_sample_at = now;
        self.last_sample_bytes = transferred_bytes;
        self.display_speed
    }

    pub(super) fn current_speed(&self) -> f64 {
        self.display_speed
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TransferDiagnostics {
    pub(super) started_at: Instant,
    pub(super) last_logged_at: Instant,
    pub(super) timeout_count: u32,
    pub(super) consecutive_timeout_count: u32,
    pub(super) downgrade_count: u32,
    pub(super) retry_count: u32,
    pub(super) rtt_total_ms: f64,
    pub(super) rtt_samples: u64,
}

impl TransferDiagnostics {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            started_at: now,
            last_logged_at: now,
            timeout_count: 0,
            consecutive_timeout_count: 0,
            downgrade_count: 0,
            retry_count: 0,
            rtt_total_ms: 0.0,
            rtt_samples: 0,
        }
    }

    pub(super) fn record_rtt(&mut self, elapsed: Duration) {
        self.rtt_total_ms += elapsed.as_secs_f64() * 1000.0;
        self.rtt_samples += 1;
    }

    pub(super) fn avg_rtt_ms(&self) -> Option<f64> {
        if self.rtt_samples == 0 {
            None
        } else {
            Some(self.rtt_total_ms / self.rtt_samples as f64)
        }
    }

    pub(super) fn mark_timeout(&mut self) {
        self.timeout_count += 1;
        self.consecutive_timeout_count += 1;
    }

    pub(super) fn mark_retry(&mut self) {
        self.retry_count += 1;
    }

    pub(super) fn mark_success(&mut self) {
        self.consecutive_timeout_count = 0;
    }

    pub(super) fn mark_downgrade(&mut self) {
        self.downgrade_count += 1;
        self.consecutive_timeout_count = 0;
    }

    pub(super) fn should_log_progress(&self, now: Instant) -> bool {
        now.duration_since(self.last_logged_at) >= Duration::from_secs(TRANSFER_DIAG_INTERVAL_SECS)
    }

    pub(super) fn touch_log_time(&mut self, now: Instant) {
        self.last_logged_at = now;
    }
}
