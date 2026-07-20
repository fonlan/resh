use crate::config::types::Proxy;
use reqwest::{
    header::{ETAG, IF_MATCH, IF_NONE_MATCH},
    Client, StatusCode,
};
use std::error::Error;
use std::fmt;

pub struct WebDAVClient {
    base_url: String,
    username: String,
    password: String,
    client: Client,
}

/// A downloaded WebDAV resource together with the entity tag observed during the GET.
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
        if status == StatusCode::PRECONDITION_FAILED {
            return Err(WebDavError::PreconditionFailed);
        }
        if !status.is_success() {
            tracing::error!(status = %status, "WebDAV upload returned a non-success status");
            return Err(WebDavError::Http(status));
        }

        Ok(response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned))
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

        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let content = response.bytes().await.map_err(|_| WebDavError::Request)?;
        Ok(Some(DownloadedResource {
            content: content.to_vec(),
            etag,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
