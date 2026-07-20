use crate::config::types::Proxy;
use reqwest::{
    header::{ETAG, IF_MATCH, IF_NONE_MATCH},
    Client, Method, StatusCode,
};
use std::error::Error;
use std::fmt;

pub struct WebDAVClient {
    base_url: String,
    username: String,
    password: String,
    client: Client,
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
    Request,
    Http(StatusCode),
    PreconditionFailed,
}

impl fmt::Display for WebDavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request => f.write_str("WebDAV request failed"),
            Self::Http(status) => write!(f, "WebDAV request failed: HTTP {status}"),
            Self::PreconditionFailed => f.write_str("WebDAV resource changed concurrently"),
        }
    }
}

impl Error for WebDavError {}

impl WebDAVClient {
    pub fn new(base_url: String, username: String, password: String, proxy: Option<Proxy>) -> Self {
        let mut client_builder = Client::builder().user_agent("Resh/0.1.0");

        if let Some(proxy_config) = proxy {
            let scheme = match proxy_config.proxy_type.as_str() {
                "socks5" => "socks5h",
                _ => "http",
            };
            let proxy_url = format!("{}://{}:{}", scheme, proxy_config.host, proxy_config.port);

            if let Ok(mut p) = reqwest::Proxy::all(&proxy_url) {
                if let (Some(u), Some(pass)) = (proxy_config.username, proxy_config.password) {
                    if !u.is_empty() {
                        p = p.basic_auth(&u, &pass);
                    }
                }
                client_builder = client_builder.proxy(p);
            } else {
                tracing::warn!("Invalid WebDAV proxy configuration: {}", proxy_url);
            }

            // Ignore certificate validation only for the explicitly configured proxy scenario.
            if proxy_config.ignore_ssl_errors {
                tracing::warn!(
                    "WebDAV: Ignoring SSL certificates for proxy {}",
                    proxy_config.name
                );
                client_builder = client_builder.danger_accept_invalid_certs(true);
            }
        }

        WebDAVClient {
            base_url,
            username,
            password,
            client: client_builder.build().unwrap_or_else(|_| Client::new()),
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
        let mut request = self
            .client
            .put(&url)
            .basic_auth(&self.username, Some(&self.password))
            .body(content.to_vec());
        request = match condition {
            UploadCondition::CreateOnly => request.header(IF_NONE_MATCH, "*"),
            UploadCondition::IfMatch(etag) => request.header(IF_MATCH, etag),
        };

        let response = request.send().await.map_err(|_| WebDavError::Request)?;
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
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Cache-Control", "no-cache")
            .header("Pragma", "no-cache")
            .send()
            .await
            .map_err(|_| WebDavError::Request)?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let status = response.status();
        if !status.is_success() {
            tracing::error!(status = %status, "WebDAV download returned a non-success status");
            return Err(WebDavError::Http(status));
        }

        let etag = response_etag(response.headers());
        let content = response.bytes().await.map_err(|_| WebDavError::Request)?;
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
        );

        assert_eq!(client.base_url, "https://example.com/webdav");
        assert_eq!(client.username, "user");
    }

    #[test]
    fn webdav_errors_do_not_include_credentials() {
        assert_eq!(WebDavError::Request.to_string(), "WebDAV request failed");
        assert_eq!(
            WebDavError::PreconditionFailed.to_string(),
            "WebDAV resource changed concurrently"
        );
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

    fn test_client(port: u16) -> WebDAVClient {
        WebDAVClient::new(
            format!("http://127.0.0.1:{port}"),
            "user".to_string(),
            "password".to_string(),
            None,
        )
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
