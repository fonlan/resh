// Shared HTTP client construction for WebDAV, AI, and updater.

use crate::config::types::Proxy;
use reqwest::{Client, Proxy as ReqwestProxy, redirect::Policy};
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
}
