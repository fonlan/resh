// Shared HTTP client construction for WebDAV, AI, and updater.

use crate::config::types::Proxy;
use reqwest::{redirect::Policy, Client, Proxy as ReqwestProxy};
use std::error::Error as _;
use std::time::Duration;

/// Options for building a reqwest client with optional proxy support.
#[derive(Debug, Clone)]
pub struct HttpClientOptions {
    pub user_agent: String,
    pub connect_timeout: Duration,
    pub timeout: Duration,
    pub max_redirects: usize,
    /// When true, disable the system proxy environment variables (HTTP_PROXY etc.).
    pub disable_system_proxy: bool,
}

impl Default for HttpClientOptions {
    fn default() -> Self {
        Self {
            user_agent: format!("Resh/{}", env!("CARGO_PKG_VERSION")),
            connect_timeout: Duration::from_secs(10),
            timeout: Duration::from_secs(60),
            max_redirects: 5,
            disable_system_proxy: true,
        }
    }
}

/// Build a reqwest `Client` with an optional Resh `Proxy` configuration.
///
/// Proxy failures return an error instead of silently falling back to a direct
/// connection (which would leak traffic outside the configured proxy path).
pub fn build_http_client(
    proxy: Option<&Proxy>,
    options: &HttpClientOptions,
) -> Result<Client, String> {
    let mut builder = Client::builder()
        .user_agent(&options.user_agent)
        .connect_timeout(options.connect_timeout)
        .timeout(options.timeout)
        .redirect(Policy::limited(options.max_redirects));

    if options.disable_system_proxy {
        builder = builder.no_proxy();
    }

    if let Some(p) = proxy {
        let reqwest_proxy = build_reqwest_proxy(p)?;
        builder = builder.proxy(reqwest_proxy);

        if p.ignore_ssl_errors {
            tracing::warn!(
                "HTTP client: ignoring SSL certificate validation for proxy {}",
                p.name
            );
            builder = builder.danger_accept_invalid_certs(true);
        }
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

/// Convert a Resh `Proxy` into a reqwest proxy.
///
/// Uses `socks5h` so DNS is resolved through the proxy (matches AI/WebDAV).
pub fn build_reqwest_proxy(proxy: &Proxy) -> Result<ReqwestProxy, String> {
    let scheme = match proxy.proxy_type.as_str() {
        "socks5" => "socks5h",
        "http" => "http",
        other => {
            return Err(format!(
                "Unsupported proxy type '{}'. Expected 'http' or 'socks5'.",
                other
            ));
        }
    };

    if proxy.host.trim().is_empty() {
        return Err("Proxy host is empty".to_string());
    }
    if proxy.port == 0 {
        return Err("Proxy port is invalid (0)".to_string());
    }

    let proxy_url = format!("{}://{}:{}", scheme, proxy.host.trim(), proxy.port);
    let mut p = ReqwestProxy::all(&proxy_url)
        .map_err(|e| format!("Invalid proxy URL '{}': {}", proxy_url, e))?;

    if let Some(ref user) = proxy.username {
        if !user.is_empty() {
            let pass = proxy.password.as_deref().unwrap_or("");
            p = p.basic_auth(user, pass);
        }
    }

    Ok(p)
}

/// Maximum characters kept from a single cause message, and how many causes are appended.
///
/// Transport errors arrive from third-party crates, so their text is bounded and treated as
/// untrusted: it is sanitized before it ever reaches a log line or a user-facing message.
const MAX_DETAIL_CHARS: usize = 240;
const MAX_CAUSE_DEPTH: usize = 4;

/// Short category for a transport-level reqwest failure.
fn transport_failure_label(error: &reqwest::Error) -> &'static str {
    // A connect timeout sets both flags; report the more specific "timed out" first.
    if error.is_timeout() {
        "timed out"
    } else if error.is_connect() {
        "connection failed"
    } else if error.is_body() {
        "response body interrupted"
    } else if error.is_redirect() {
        "too many redirects"
    } else if error.is_decode() {
        "invalid response"
    } else if error.is_builder() {
        "invalid request configuration"
    } else {
        "request failed"
    }
}

/// Render a URL for diagnostics with credentials and query material removed.
///
/// Userinfo (`user:pass@`) is dropped entirely and the query/fragment is discarded, because
/// WebDAV/CalDAV servers routinely embed access tokens there. Scheme, host, port and path are
/// kept: they are the parts that make a failure diagnosable.
pub fn redact_url(url: &reqwest::Url) -> String {
    let mut redacted = String::from(url.scheme());
    redacted.push_str("://");
    if let Some(host) = url.host_str() {
        redacted.push_str(host);
    }
    if let Some(port) = url.port() {
        redacted.push_str(&format!(":{port}"));
    }
    redacted.push_str(url.path());
    redacted
}

/// Remove `scheme://user:pass@` userinfo from any URL embedded in third-party error text.
fn strip_url_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(scheme_end) = rest.find("://") {
        let authority_start = scheme_end + 3;
        out.push_str(&rest[..authority_start]);

        let tail = &rest[authority_start..];
        // Userinfo can only live in the authority, which ends at the first '/'.
        let authority_end = tail.find('/').unwrap_or(tail.len());
        match tail[..authority_end].find('@') {
            Some(at) => out.push_str(&tail[at + 1..authority_end]),
            None => out.push_str(&tail[..authority_end]),
        }
        rest = &tail[authority_end..];
    }

    out.push_str(rest);
    out
}

/// Collapse a third-party error message into a single bounded, credential-free line.
fn sanitize_error_text(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    strip_url_userinfo(&collapsed)
        .chars()
        .take(MAX_DETAIL_CHARS)
        .collect()
}

/// Describe a transport-level reqwest failure for logs and diagnostic messages.
///
/// The result is safe to log and safe to surface in the UI: it never contains credentials, proxy
/// passwords, or URL query material. It keeps the failure category, the redacted URL, and the
/// cause chain (`dns error`, `certificate verify failed`, `Connection refused`, ...) — exactly the
/// information a generic "request failed" message throws away.
pub fn describe_transport_error(error: &reqwest::Error) -> String {
    let mut parts = vec![transport_failure_label(error).to_string()];
    if let Some(url) = error.url() {
        parts.push(format!("url: {}", redact_url(url)));
    }

    let mut cause = error.source();
    let mut depth = 0;
    while let Some(current) = cause {
        if depth >= MAX_CAUSE_DEPTH {
            break;
        }
        let text = sanitize_error_text(&current.to_string());
        if !text.is_empty() && !parts.iter().any(|part| part.contains(&text)) {
            parts.push(text);
        }
        depth += 1;
        cause = current.source();
    }

    parts.join("; ")
}

/// Resolve a proxy by id from the current proxy list.
///
/// Returns `Ok(None)` when `proxy_id` is empty / None (direct connection).
/// Returns `Err` when a non-empty id cannot be found (stale setting).
pub fn resolve_proxy_by_id<'a>(
    proxies: &'a [Proxy],
    proxy_id: Option<&str>,
) -> Result<Option<&'a Proxy>, String> {
    let Some(id) = proxy_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    proxies
        .iter()
        .find(|p| p.id == id)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "Update proxy '{}' no longer exists. Choose another proxy or direct connection.",
                id
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_proxy(proxy_type: &str) -> Proxy {
        Proxy {
            id: "p1".to_string(),
            name: "Test".to_string(),
            proxy_type: proxy_type.to_string(),
            host: "127.0.0.1".to_string(),
            port: 7890,
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            ignore_ssl_errors: false,
            synced: true,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn builds_http_proxy() {
        let proxy = sample_proxy("http");
        build_reqwest_proxy(&proxy).expect("http proxy");
    }

    #[test]
    fn builds_socks5h_proxy() {
        let proxy = sample_proxy("socks5");
        build_reqwest_proxy(&proxy).expect("socks5 proxy");
    }

    #[test]
    fn rejects_empty_host() {
        let mut proxy = sample_proxy("http");
        proxy.host = "  ".to_string();
        assert!(build_reqwest_proxy(&proxy).is_err());
    }

    #[test]
    fn rejects_unsupported_type() {
        let proxy = sample_proxy("ftp");
        assert!(build_reqwest_proxy(&proxy).is_err());
    }

    #[test]
    fn builds_client_without_proxy() {
        let client = build_http_client(None, &HttpClientOptions::default()).expect("client");
        // Client is opaque; building successfully is the contract.
        let _ = client;
    }

    #[test]
    fn builds_client_with_proxy() {
        let proxy = sample_proxy("socks5");
        let client =
            build_http_client(Some(&proxy), &HttpClientOptions::default()).expect("client");
        let _ = client;
    }

    #[test]
    fn resolve_proxy_none_for_empty() {
        let proxies = vec![sample_proxy("http")];
        assert!(resolve_proxy_by_id(&proxies, None).unwrap().is_none());
        assert!(resolve_proxy_by_id(&proxies, Some("")).unwrap().is_none());
        assert!(resolve_proxy_by_id(&proxies, Some("  ")).unwrap().is_none());
    }

    #[test]
    fn resolve_proxy_found() {
        let proxies = vec![sample_proxy("http")];
        let p = resolve_proxy_by_id(&proxies, Some("p1")).unwrap().unwrap();
        assert_eq!(p.id, "p1");
    }

    #[test]
    fn resolve_proxy_missing_errors() {
        let proxies = vec![sample_proxy("http")];
        let err = resolve_proxy_by_id(&proxies, Some("missing")).unwrap_err();
        assert!(err.contains("missing"));
    }

    #[test]
    fn redact_url_drops_userinfo_and_query_material() {
        let url = reqwest::Url::parse(
            "https://alice:s3cret@dav.example.com:8443/dav/files/bob/sync.json?access_token=abc#frag",
        )
        .unwrap();

        let redacted = redact_url(&url);

        assert_eq!(
            redacted,
            "https://dav.example.com:8443/dav/files/bob/sync.json"
        );
        assert!(!redacted.contains("s3cret"));
        assert!(!redacted.contains("alice"));
        assert!(!redacted.contains("access_token"));
    }

    #[test]
    fn sanitizes_inline_userinfo_and_bounds_length() {
        let text = format!(
            "error sending request to https://alice:s3cret@dav.example.com/dav/sync.json: {}",
            "x".repeat(1000)
        );

        let sanitized = sanitize_error_text(&text);

        assert!(!sanitized.contains("s3cret"));
        assert!(sanitized.contains("dav.example.com/dav/sync.json"));
        assert!(sanitized.chars().count() <= MAX_DETAIL_CHARS);
        assert!(!sanitized.contains('\n'));
    }

    /// A closed loopback port gives a deterministic transport failure without leaving the host.
    fn closed_loopback_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("loopback address").port();
        drop(listener);
        port
    }

    #[tokio::test]
    async fn describes_connection_failure_without_leaking_credentials() {
        let port = closed_loopback_port();
        let client = Client::builder().build().expect("client");
        let url = format!("http://alice:s3cret@127.0.0.1:{port}/dav/sync.json");

        let error = client.get(&url).send().await.expect_err("port is closed");
        let detail = describe_transport_error(&error);

        assert!(
            detail.contains("connection failed"),
            "unexpected detail: {detail}"
        );
        assert!(
            detail.contains(&format!("127.0.0.1:{port}")),
            "detail must keep the redacted target: {detail}"
        );
        assert!(
            !detail.contains("s3cret") && !detail.contains("alice:"),
            "detail leaked credentials: {detail}"
        );
    }

    #[tokio::test]
    async fn describes_dns_failure_and_names_the_host() {
        // `.invalid` is reserved by RFC 6761 and can never resolve, so this stays a resolution
        // failure whether the resolver answers NXDOMAIN or DNS is unreachable entirely.
        let client = Client::builder().build().expect("client");
        let error = client
            .get("https://nonexistent-host.invalid/dav/sync.json")
            .send()
            .await
            .expect_err("reserved TLD must not resolve");

        let detail = describe_transport_error(&error);

        assert!(
            detail.contains("connection failed"),
            "unexpected detail: {detail}"
        );
        assert!(
            detail.contains("nonexistent-host.invalid"),
            "detail must name the host: {detail}"
        );
        assert!(
            detail.len() > "connection failed".len(),
            "detail must carry a cause beyond the category: {detail}"
        );
    }

    #[tokio::test]
    async fn timeout_failures_report_timeouts() {
        // A listener that accepts but never replies forces the configured total timeout.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let held = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(stream);
        });

        let client = build_http_client(
            None,
            &HttpClientOptions {
                timeout: Duration::from_millis(200),
                connect_timeout: Duration::from_millis(200),
                ..HttpClientOptions::default()
            },
        )
        .expect("client");

        let error = client
            .get(format!("http://127.0.0.1:{port}/dav/sync.json"))
            .send()
            .await
            .expect_err("server never replies");
        let detail = describe_transport_error(&error);

        assert!(
            detail.starts_with("timed out"),
            "unexpected detail: {detail}"
        );

        held.abort();
    }
}
