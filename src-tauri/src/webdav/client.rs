use crate::config::types::Proxy;
use reqwest::{
    header::{ETAG, IF_MATCH, IF_NONE_MATCH},
    Client, Method, StatusCode,
};
use std::error::Error;
use std::fmt;
use std::time::Duration;

/// Bounded retry policy for connect-phase failures.
///
/// macOS Local Network privacy denies the very first connect while the permission alert is still
/// unanswered: TN3179 notes the system "may deny the operation immediately, before the user has
/// responded to the alert". Without a retry the user sees an unexplained hard sync failure moments
/// before approving access.
#[derive(Debug, Clone, Copy)]
struct ConnectRetryPolicy {
    attempts: u32,
    base_delay: Duration,
    max_delay: Duration,
}

impl Default for ConnectRetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 4,
            base_delay: Duration::from_millis(800),
            max_delay: Duration::from_secs(3),
        }
    }
}

pub struct WebDAVClient {
    base_url: String,
    username: String,
    password: String,
    client: Client,
    retry: ConnectRetryPolicy,
}

/// A downloaded WebDAV resource together with the entity tag observed during the GET or PROPFIND.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedResource {
    pub content: Vec<u8>,
    pub etag: Option<String>,
}

/// Write precondition used to prevent a stale sync from silently overwriting the remote file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadCondition {
    /// The file did not exist during GET, so only create it if it is still absent.
    CreateOnly,
    /// Replace only the exact entity tag returned by the preceding GET.
    IfMatch(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebDavError {
    /// Transport-level failure: DNS, TCP, TLS, proxy, timeout, or a truncated response body.
    ///
    /// The payload is a sanitized description of the underlying cause, meant for logs and
    /// diagnostics. `Display` deliberately stays generic so that user-facing text which gets
    /// copied into a bug report or a chat window cannot leak credentials.
    Request(String),
    Http(StatusCode),
    PreconditionFailed,
}

impl WebDavError {
    /// Sanitized transport cause when one was captured; `None` for HTTP-status failures.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Request(detail) => Some(detail.as_str()),
            Self::Http(_) | Self::PreconditionFailed => None,
        }
    }
}

/// Keep the sanitized cause of a transport failure instead of discarding the reqwest error.
///
/// The message shown to users stays generic (see `Display`); this is the only place the original
/// failure reason — which distinguishes a DNS problem from a rejected certificate or a dead
/// proxy — survives.
fn request_failure(error: reqwest::Error) -> WebDavError {
    WebDavError::Request(crate::http::describe_transport_error(&error))
}

impl fmt::Display for WebDavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(_) => f.write_str("WebDAV request failed"),
            Self::Http(status) => write!(f, "WebDAV request failed: HTTP {status}"),
            Self::PreconditionFailed => f.write_str("WebDAV resource changed concurrently"),
        }
    }
}

impl Error for WebDavError {}

impl WebDAVClient {
    /// Build a WebDAV client through the shared HTTP client construction.
    ///
    /// WebDAV used to hand-roll its own `reqwest::Client`, which silently diverged from every
    /// other subsystem: no connect/total timeout, a `Client::new()` fallback that dropped the
    /// configured proxy on any builder error, and hand-rolled proxy parsing. Routing through
    /// [`crate::http::build_http_client`] restores the shared timeouts, redirect policy and proxy
    /// semantics — and makes a broken proxy configuration fail loudly instead of quietly leaking
    /// traffic around the proxy.
    pub fn new(
        base_url: String,
        username: String,
        password: String,
        proxy: Option<Proxy>,
    ) -> Result<Self, String> {
        let client = crate::http::build_http_client(
            proxy.as_ref(),
            &crate::http::HttpClientOptions {
                user_agent: format!("Resh/{}", env!("CARGO_PKG_VERSION")),
                ..crate::http::HttpClientOptions::default()
            },
        )?;

        Ok(WebDAVClient {
            base_url,
            username,
            password,
            client,
            retry: ConnectRetryPolicy::default(),
        })
    }

    /// Override the connect retry policy. Only used by tests, which must not pay the production
    /// backoff while simulating a server that becomes reachable after the first refusal.
    #[cfg(test)]
    fn with_retry_policy(mut self, retry: ConnectRetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Send a request, retrying connect-phase failures with bounded backoff.
    ///
    /// Only connect failures are retried. They prove the request never reached the server, so a
    /// retry can never duplicate a write — that keeps the unconditional retry correct for the
    /// conditional `PUT` path as well as for `GET`. Response-phase failures (timeouts after the
    /// request was sent, truncated bodies, HTTP statuses) are not retried here.
    async fn send_with_connect_retry(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, WebDavError> {
        let mut delay = self.retry.base_delay;
        let mut attempt = 1;

        loop {
            match build().send().await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    if !error.is_connect() || attempt >= self.retry.attempts {
                        return Err(request_failure(error));
                    }

                    tracing::warn!(
                        attempt,
                        max_attempts = self.retry.attempts,
                        delay_ms = delay.as_millis() as u64,
                        detail = %crate::http::describe_transport_error(&error),
                        "WebDAV connect failed; retrying in case the local-network permission alert is still open"
                    );

                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(self.retry.max_delay);
                    attempt += 1;
                }
            }
        }
    }

    fn resource_url(&self, filename: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), filename)
    }

    /// Conditionally upload a resource and return the new ETag when the server supplies one.
    pub async fn upload_conditionally(
        &self,
        filename: &str,
        content: &[u8],
        condition: UploadCondition,
    ) -> Result<Option<String>, WebDavError> {
        let url = self.resource_url(filename);
        let body = content.to_vec();
        // Rebuilt per attempt: a refused connect means the request never left, so retrying cannot
        // turn into a duplicate conditional write.
        let response = self
            .send_with_connect_retry(|| {
                let request = self
                    .client
                    .put(&url)
                    .basic_auth(&self.username, Some(&self.password))
                    .body(body.clone());
                match &condition {
                    UploadCondition::CreateOnly => request.header(IF_NONE_MATCH, "*"),
                    UploadCondition::IfMatch(etag) => request.header(IF_MATCH, etag.as_str()),
                }
            })
            .await?;
        let status = response.status();
        // Some WebDAV implementations use 409 rather than the HTTP-standard 412 for a failed
        // conditional PUT. Both mean another writer changed or created the resource.
        if matches!(
            status,
            StatusCode::PRECONDITION_FAILED | StatusCode::CONFLICT
        ) {
            return Err(WebDavError::PreconditionFailed);
        }
        if !status.is_success() {
            tracing::error!(status = %status, "WebDAV upload returned a non-success status");
            return Err(WebDavError::Http(status));
        }

        Ok(response_etag(response.headers()))
    }

    pub async fn download(
        &self,
        filename: &str,
    ) -> Result<Option<DownloadedResource>, WebDavError> {
        let url = self.resource_url(filename);
        let response = self
            .send_with_connect_retry(|| {
                self.client
                    .get(&url)
                    .basic_auth(&self.username, Some(&self.password))
                    .header("Cache-Control", "no-cache")
                    .header("Pragma", "no-cache")
            })
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let status = response.status();
        if !status.is_success() {
            tracing::error!(status = %status, "WebDAV download returned a non-success status");
            return Err(WebDavError::Http(status));
        }

        let etag = response_etag(response.headers());
        let content = response.bytes().await.map_err(request_failure)?;
        let etag = match etag {
            Some(etag) => Some(etag),
            None => self.fetch_etag_with_propfind(filename).await,
        };

        Ok(Some(DownloadedResource {
            content: content.to_vec(),
            etag,
        }))
    }

    /// Some WebDAV servers omit ETag on GET but expose it through a depth-zero PROPFIND.
    /// Failure to retrieve an ETag here is intentionally non-fatal: the sync layer will refuse an
    /// unsafe overwrite and return a diagnosable SafeSyncUnavailable result instead.
    async fn fetch_etag_with_propfind(&self, filename: &str) -> Option<String> {
        let method = Method::from_bytes(b"PROPFIND").expect("PROPFIND is a valid HTTP method");
        let response = match self
            .client
            .request(method, self.resource_url(filename))
            .basic_auth(&self.username, Some(&self.password))
            .header("Depth", "0")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(
                r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:getetag /></d:prop></d:propfind>"#,
            )
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => return None,
        };

        if !response.status().is_success() {
            tracing::debug!(status = %response.status(), "WebDAV PROPFIND did not provide an ETag");
            return None;
        }

        if let Some(etag) = response_etag(response.headers()) {
            return Some(etag);
        }

        let body = response.bytes().await.ok()?;
        extract_getetag_from_propfind(&body)
    }
}

fn response_etag(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn extract_getetag_from_propfind(body: &[u8]) -> Option<String> {
    let document = std::str::from_utf8(body).ok()?;
    let lowercase = document.to_ascii_lowercase();
    let mut search_from = 0;

    while let Some(relative_match) = lowercase[search_from..].find("getetag") {
        let match_index = search_from + relative_match;
        let open_start = lowercase[..match_index].rfind('<')?;
        let open_end = lowercase[match_index..].find('>')? + match_index;
        let open_name = xml_local_name(&lowercase[open_start + 1..open_end]);
        if open_name != Some("getetag") {
            search_from = open_end + 1;
            continue;
        }

        let content_start = open_end + 1;
        let Some(relative_close_start) = lowercase[content_start..].find("</") else {
            return None;
        };
        let close_start = content_start + relative_close_start;
        let close_end = lowercase[close_start..].find('>')? + close_start;
        let close_name = xml_local_name(&lowercase[close_start + 2..close_end]);
        if close_name != Some("getetag") {
            search_from = close_end + 1;
            continue;
        }

        let etag = document[content_start..close_start].trim();
        if !etag.is_empty() {
            return Some(etag.to_string());
        }
        search_from = close_end + 1;
    }

    None
}

fn xml_local_name(tag: &str) -> Option<&str> {
    let tag = tag.trim().trim_start_matches('/');
    let name = tag
        .split(|character: char| character.is_ascii_whitespace())
        .next()?;
    name.rsplit(':').next().filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    #[tokio::test]
    async fn test_webdav_client_creation() {
        let client = WebDAVClient::new(
            "https://example.com/webdav".to_string(),
            "user".to_string(),
            "pass".to_string(),
            None,
        )
        .expect("client");

        assert_eq!(client.base_url, "https://example.com/webdav");
        assert_eq!(client.username, "user");
    }

    #[test]
    fn broken_proxy_configuration_fails_instead_of_going_direct() {
        // Regression guard: the old hand-rolled constructor logged a warning and silently kept
        // going without the proxy, which would route WebDAV traffic around the configured proxy.
        let proxy = Proxy {
            id: "p1".to_string(),
            name: "Broken".to_string(),
            proxy_type: "ftp".to_string(),
            host: "127.0.0.1".to_string(),
            port: 7890,
            username: None,
            password: None,
            ignore_ssl_errors: false,
            synced: true,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };

        let error = WebDAVClient::new(
            "https://example.com/webdav".to_string(),
            "user".to_string(),
            "pass".to_string(),
            Some(proxy),
        )
        .err()
        .expect("an unsupported proxy type must not be ignored");

        assert!(
            error.contains("ftp"),
            "error must name the rejected proxy type: {error}"
        );
    }

    #[test]
    fn malformed_proxy_host_fails_loudly() {
        let proxy = Proxy {
            id: "p2".to_string(),
            name: "Empty host".to_string(),
            proxy_type: "http".to_string(),
            host: "   ".to_string(),
            port: 7890,
            username: None,
            password: None,
            ignore_ssl_errors: false,
            synced: true,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };

        let error = WebDAVClient::new(
            "https://example.com/webdav".to_string(),
            "user".to_string(),
            "pass".to_string(),
            Some(proxy),
        )
        .err()
        .expect("an empty proxy host must not be ignored");

        assert!(
            error.to_lowercase().contains("host"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn webdav_errors_do_not_include_credentials() {
        let captured = "connection failed; url: https://dav.example.com/dav/sync.json";
        let request = WebDavError::Request(captured.to_string());

        // Display is what reaches copied user-facing text, so it must stay generic.
        assert_eq!(request.to_string(), "WebDAV request failed");
        assert!(
            !request.to_string().contains("dav.example.com"),
            "Display must not surface the captured cause"
        );
        assert_eq!(request.detail(), Some(captured));

        assert_eq!(
            WebDavError::PreconditionFailed.to_string(),
            "WebDAV resource changed concurrently"
        );
        assert!(WebDavError::PreconditionFailed.detail().is_none());
        assert!(WebDavError::Http(StatusCode::UNAUTHORIZED)
            .detail()
            .is_none());
    }

    #[tokio::test]
    async fn download_keeps_a_sanitized_transport_cause() {
        // A closed loopback port yields a deterministic connect failure without network access.
        let port = closed_loopback_port();

        let error = test_client(port)
            .with_retry_policy(FAST_RETRY)
            .download("sync.json")
            .await
            .expect_err("port is closed");

        let detail = error.detail().expect("transport failure must keep a cause");
        assert!(
            detail.contains("connection failed"),
            "unexpected detail: {detail}"
        );
        // test_client authenticates with basic_auth; the cause must never carry those credentials.
        assert!(
            !detail.contains("password"),
            "detail leaked credentials: {detail}"
        );
        assert_eq!(error.to_string(), "WebDAV request failed");
    }

    /// macOS denies the first connect while the Local Network alert is unanswered, so a refused
    /// connect must be retried instead of failing the whole sync.
    #[tokio::test]
    async fn connect_failure_is_retried_until_the_server_answers() {
        let port = closed_loopback_port();

        // The server only starts listening after the first attempt has already been refused.
        let server = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let (mut stream, _request) = read_request(stream).await;
            write_response(&mut stream, "200 OK", &["ETag: \"v9\""], b"{}").await;
        });

        let resource = test_client(port)
            .with_retry_policy(ConnectRetryPolicy {
                attempts: 5,
                base_delay: Duration::from_millis(60),
                max_delay: Duration::from_millis(120),
            })
            .download("sync.json")
            .await
            .expect("retry must survive the initial refusal")
            .expect("resource");

        assert_eq!(resource.content, b"{}");
        assert_eq!(resource.etag.as_deref(), Some("\"v9\""));
        server.await.unwrap();
    }

    /// A non-connect failure must NOT be retried: the request may already have reached the server.
    #[tokio::test]
    async fn http_status_failures_are_not_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut stream, _request) = read_request(stream).await;
            write_response(&mut stream, "500 Internal Server Error", &[], b"").await;
            // If the client retried, a second connection would arrive; the listener is dropped
            // when this task ends, so an unexpected retry surfaces as a connect error instead.
        });

        let error = test_client(address.port())
            .download("sync.json")
            .await
            .expect_err("HTTP 500 is an error");

        assert_eq!(error, WebDavError::Http(StatusCode::INTERNAL_SERVER_ERROR));
        server.await.unwrap();
    }

    #[test]
    fn extracts_namespaced_propfind_etag() {
        let body = br#"<d:multistatus xmlns:d="DAV:"><d:response><d:propstat><d:prop><d:getetag>"v2"</d:getetag></d:prop></d:propstat></d:response></d:multistatus>"#;
        assert_eq!(
            extract_getetag_from_propfind(body).as_deref(),
            Some("\"v2\"")
        );
    }

    #[tokio::test]
    async fn download_uses_get_etag_when_available() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut stream, request) = read_request(stream).await;
            write_response(&mut stream, "200 OK", &["ETag: \"v1\""], b"{}").await;
            request
        });

        let client = test_client(address.port());
        let resource = client.download("sync.json").await.unwrap().unwrap();

        assert_eq!(resource.content, b"{}");
        assert_eq!(resource.etag.as_deref(), Some("\"v1\""));
        assert!(server.await.unwrap().starts_with("GET /sync.json HTTP/1.1"));
    }

    #[tokio::test]
    async fn download_falls_back_to_depth_zero_propfind_for_etag() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut stream, get_request) = read_request(stream).await;
            write_response(&mut stream, "200 OK", &[], b"{}").await;

            let (stream, _) = listener.accept().await.unwrap();
            let (mut stream, propfind_request) = read_request(stream).await;
            write_response(
                &mut stream,
                "207 Multi-Status",
                &[],
                br#"<d:multistatus xmlns:d="DAV:"><d:response><d:propstat><d:prop><d:getetag>"v2"</d:getetag></d:prop></d:propstat></d:response></d:multistatus>"#,
            )
            .await;
            (get_request, propfind_request)
        });

        let resource = test_client(address.port())
            .download("sync.json")
            .await
            .unwrap()
            .unwrap();
        let (get_request, propfind_request) = server.await.unwrap();

        assert_eq!(resource.etag.as_deref(), Some("\"v2\""));
        assert!(get_request.starts_with("GET /sync.json HTTP/1.1"));
        let lowercase = propfind_request.to_ascii_lowercase();
        assert!(lowercase.starts_with("propfind /sync.json http/1.1"));
        assert!(lowercase.contains("depth: 0"));
    }

    #[tokio::test]
    async fn conditional_upload_sets_preconditions_and_maps_create_races() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut stream, request) = read_request(stream).await;
            write_response(&mut stream, "412 Precondition Failed", &[], b"").await;
            request
        });

        let error = test_client(address.port())
            .upload_conditionally("sync.json", b"{}", UploadCondition::CreateOnly)
            .await
            .unwrap_err();
        let request = server.await.unwrap().to_ascii_lowercase();

        assert_eq!(error, WebDavError::PreconditionFailed);
        assert!(request.starts_with("put /sync.json http/1.1"));
        assert!(request.contains("if-none-match: *"));
    }

    #[tokio::test]
    async fn conditional_upload_sets_if_match_and_recognizes_409_and_412() {
        for status in ["409 Conflict", "412 Precondition Failed"] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let status = status.to_string();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let (mut stream, request) = read_request(stream).await;
                write_response(&mut stream, &status, &[], b"").await;
                request
            });

            let error = test_client(address.port())
                .upload_conditionally(
                    "sync.json",
                    b"{}",
                    UploadCondition::IfMatch("\"v1\"".to_string()),
                )
                .await
                .unwrap_err();
            let request = server.await.unwrap().to_ascii_lowercase();

            assert_eq!(error, WebDavError::PreconditionFailed);
            assert!(request.contains("if-match: \"v1\""));
        }
    }

    /// Keeps tests that end in a refused connect from paying the production backoff.
    const FAST_RETRY: ConnectRetryPolicy = ConnectRetryPolicy {
        attempts: 2,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(20),
    };

    /// A loopback port with no listener: connecting to it is refused deterministically.
    fn closed_loopback_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("loopback address").port();
        drop(listener);
        port
    }

    fn test_client(port: u16) -> WebDAVClient {
        WebDAVClient::new(
            format!("http://127.0.0.1:{port}"),
            "user".to_string(),
            "password".to_string(),
            None,
        )
        .expect("client")
    }

    async fn read_request(mut stream: TcpStream) -> (TcpStream, String) {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "test HTTP client closed before sending headers");
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        (stream, String::from_utf8_lossy(&request).into_owned())
    }

    async fn write_response(stream: &mut TcpStream, status: &str, headers: &[&str], body: &[u8]) {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        for header in headers {
            response.push_str(header);
            response.push_str("\r\n");
        }
        response.push_str("\r\n");
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.write_all(body).await.unwrap();
        stream.shutdown().await.unwrap();
    }
}
