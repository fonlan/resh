use super::types::GitHubReleaseDto;
use super::{GITHUB_ACCEPT, GITHUB_API_VERSION};
use crate::http::{build_http_client, HttpClientOptions};
use crate::config::types::Proxy;
use reqwest::StatusCode;
use std::time::Duration;

const CONNECT_TIMEOUT_SECS: u64 = 10;
const REQUEST_TIMEOUT_SECS: u64 = 30;
const MAX_RETRIES: u32 = 2;
const RETRY_BASE_MS: u64 = 400;

#[derive(Debug, Clone)]
pub struct GithubFetchCache {
    pub etag: Option<String>,
    pub body: Option<GitHubReleaseDto>,
    pub checked_at_unix: i64,
}

impl Default for GithubFetchCache {
    fn default() -> Self {
        Self {
            etag: None,
            body: None,
            checked_at_unix: 0,
        }
    }
}

#[derive(Debug)]
pub enum GithubFetchOutcome {
    /// Fresh release JSON from GitHub.
    Fresh {
        release: GitHubReleaseDto,
        etag: Option<String>,
    },
    /// HTTP 304: use previously cached release body.
    NotModified,
    RateLimited { retry_after_secs: Option<u64> },
}

pub async fn fetch_latest_stable_release(
    proxy: Option<&Proxy>,
    etag: Option<&str>,
) -> Result<GithubFetchOutcome, String> {
    let options = HttpClientOptions {
        user_agent: format!("Resh/{} (https://github.com/fonlan/resh)", env!("CARGO_PKG_VERSION")),
        connect_timeout: Duration::from_secs(CONNECT_TIMEOUT_SECS),
        timeout: Duration::from_secs(REQUEST_TIMEOUT_SECS),
        max_redirects: 5,
        disable_system_proxy: true,
    };
    let client = build_http_client(proxy, &options)?;
    let url = super::releases_latest_url();

    let mut last_err = String::new();
    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            let backoff = Duration::from_millis(RETRY_BASE_MS * (1 << (attempt - 1)));
            tokio::time::sleep(backoff).await;
        }

        match request_latest(&client, &url, etag).await {
            Ok(outcome) => return Ok(outcome),
            Err(e) if is_retryable(&e) && attempt < MAX_RETRIES => {
                tracing::warn!(
                    "GitHub releases request failed (attempt {}/{}): {}",
                    attempt + 1,
                    MAX_RETRIES + 1,
                    e
                );
                last_err = e;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err)
}

async fn request_latest(
    client: &reqwest::Client,
    url: &str,
    etag: Option<&str>,
) -> Result<GithubFetchOutcome, String> {
    let mut req = client
        .get(url)
        .header("Accept", GITHUB_ACCEPT)
        .header("X-GitHub-Api-Version", GITHUB_API_VERSION);

    if let Some(tag) = etag.filter(|s| !s.is_empty()) {
        req = req.header("If-None-Match", tag);
    }

    let response = req
        .send()
        .await
        .map_err(|e| format_reqwest_error("GitHub Releases API request failed", &e))?;

    let status = response.status();

    if status == StatusCode::NOT_MODIFIED {
        return Ok(GithubFetchOutcome::NotModified);
    }

    if status == StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 403 {
        // GitHub may use 403 for rate limit secondary / abuse.
        let retry_after = parse_retry_after(response.headers());
        let body = response.text().await.unwrap_or_default();
        if status == StatusCode::TOO_MANY_REQUESTS
            || body.to_ascii_lowercase().contains("rate limit")
        {
            return Ok(GithubFetchOutcome::RateLimited { retry_after_secs: retry_after });
        }
        return Err(format!(
            "GitHub API forbidden (HTTP 403): {}",
            truncate(&body, 300)
        ));
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "GitHub API error HTTP {}: {}",
            status.as_u16(),
            truncate(&body, 300)
        ));
    }

    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let release: GitHubReleaseDto = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub release JSON: {}", e))?;

    Ok(GithubFetchOutcome::Fresh {
        release,
        etag: new_etag,
    })
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

fn is_retryable(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection")
        || lower.contains("temporarily")
        || lower.contains("http 502")
        || lower.contains("http 503")
        || lower.contains("http 504")
}

fn format_reqwest_error(prefix: &str, err: &reqwest::Error) -> String {
    if err.is_timeout() {
        return format!("{}: request timed out", prefix);
    }
    if err.is_connect() {
        return format!("{}: connection failed ({})", prefix, err);
    }
    if err.is_request() {
        return format!("{}: {}", prefix, err);
    }
    format!("{}: {}", prefix, err)
}

fn truncate(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let shortened: String = t.chars().take(max).collect();
        format!("{}…", shortened)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_detection() {
        assert!(is_retryable("request timed out"));
        assert!(is_retryable("connection failed"));
        assert!(is_retryable("HTTP 503 unavailable"));
        assert!(!is_retryable("invalid json"));
    }
}
