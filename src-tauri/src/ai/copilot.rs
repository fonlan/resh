use reqwest::Client;
use serde::{Deserialize, Serialize};

const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98"; // Standard VSCode Copilot Client ID

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccessTokenResponse {
    pub access_token: Option<String>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CopilotTokenResponse {
    pub token: String, // The tid_... token
    pub expires_at: u64,
    pub endpoints: Option<CopilotEndpoints>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CopilotEndpoints {
    pub api: String,
    pub proxy: String,
}

pub async fn start_device_auth() -> Result<DeviceCodeResponse, String> {
    let client = Client::new();
    let res = client.post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": CLIENT_ID,
            "scope": "read:user"
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("Failed to request device code: {}", res.status()));
    }

    let body = res.json::<DeviceCodeResponse>().await.map_err(|e| e.to_string())?;
    Ok(body)
}

pub async fn poll_access_token(device_code: &str) -> Result<String, String> {
    let client = Client::new();
    let res = client.post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": CLIENT_ID,
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("Poll failed: {}", res.status()));
    }

    let body = res.json::<AccessTokenResponse>().await.map_err(|e| e.to_string())?;

    if let Some(error) = body.error {
        if error == "authorization_pending" {
            return Err("pending".to_string());
        } else if error == "slow_down" {
            return Err("slow_down".to_string());
        } else {
            return Err(format!("Auth error: {} - {}", error, body.error_description.unwrap_or_default()));
        }
    }

    if let Some(token) = body.access_token {
        Ok(token)
    } else {
        Err("No access token in response".to_string())
    }
}

pub async fn get_copilot_token(oauth_token: &str) -> Result<CopilotTokenResponse, String> {
    let client = Client::new();
    let res = client.get("https://api.github.com/copilot_internal/v2/token")
        .header("Authorization", format!("token {}", oauth_token))
        .header("User-Agent", "GithubCopilot/1.155.0")
        .header("Editor-Version", "vscode/1.85.1")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("Failed to get copilot token: {} - {}", status, text));
    }

    let body = res.json::<CopilotTokenResponse>().await.map_err(|e| e.to_string())?;
    Ok(body)
}
