use crate::config::types::Proxy;
use reqwest::Client;
use std::error::Error;

pub struct WebDAVClient {
    base_url: String,
    username: String,
    password: String,
    client: Client,
}

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

            // 忽略 SSL 证书校验（用于公司代理 MITM 场景）
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

    pub async fn upload(&self, filename: &str, content: &[u8]) -> Result<(), Box<dyn Error>> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), filename);

        let response = self
            .client
            .put(&url)
            .basic_auth(&self.username, Some(&self.password))
            .body(content.to_vec())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "Could not read response body".to_string());
            tracing::error!("Upload failed: HTTP {} - {}", status, text);
            return Err(format!("Upload failed: HTTP {} - {}", status, text).into());
        }

        Ok(())
    }

    pub async fn download(&self, filename: &str) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), filename);

        let response = self
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Cache-Control", "no-cache")
            .header("Pragma", "no-cache")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "Could not read response body".to_string());
            tracing::error!("Download failed: HTTP {} - {}", status, text);
            return Err(format!("Download failed: HTTP {} - {}", status, text).into());
        }

        let content = response.bytes().await?;
        Ok(Some(content.to_vec()))
    }

    pub async fn exists(&self, filename: &str) -> Result<bool, Box<dyn Error>> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), filename);

        let response = self
            .client
            .head(&url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Cache-Control", "no-cache")
            .header("Pragma", "no-cache")
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    pub async fn test_connection(&self) -> Result<(), Box<dyn Error>> {
        self.exists("test").await?;
        Ok(())
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
}
