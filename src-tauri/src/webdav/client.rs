use reqwest::Client;
use std::error::Error;

pub struct WebDAVClient {
    base_url: String,
    username: String,
    password: String,
    client: Client,
}

impl WebDAVClient {
    pub fn new(base_url: String, username: String, password: String) -> Self {
        WebDAVClient {
            base_url,
            username,
            password,
            client: Client::new(),
        }
    }

    pub async fn upload(&self, filename: &str, content: &[u8]) -> Result<(), Box<dyn Error>> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), filename);
        
        let response = self.client
            .put(&url)
            .basic_auth(&self.username, Some(&self.password))
            .body(content.to_vec())
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Upload failed: HTTP {}", response.status()).into());
        }

        Ok(())
    }

    pub async fn download(&self, filename: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), filename);
        
        let response = self.client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Download failed: HTTP {}", response.status()).into());
        }

        let content = response.bytes().await?;
        Ok(content.to_vec())
    }

    pub async fn exists(&self, filename: &str) -> Result<bool, Box<dyn Error>> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), filename);
        
        let response = self.client
            .head(&url)
            .basic_auth(&self.username, Some(&self.password))
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
        );
        
        assert_eq!(client.base_url, "https://example.com/webdav");
        assert_eq!(client.username, "user");
    }
}
