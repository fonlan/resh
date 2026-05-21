use reqwest::Client;

use super::types::{AccessTokenResponse, DeviceCodeResponse};

const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

#[tauri::command]
pub async fn start_copilot_auth() -> Result<DeviceCodeResponse, String> {
    let client = Client::new();
    let res = client
        .post("https://github.com/login/device/code")
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

    let body = res
        .json::<DeviceCodeResponse>()
        .await
        .map_err(|e| e.to_string())?;
    Ok(body)
}

#[tauri::command]
pub async fn poll_copilot_auth(device_code: String) -> Result<String, String> {
    let client = Client::new();
    let res = client
        .post("https://github.com/login/oauth/access_token")
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

    let body = res
        .json::<AccessTokenResponse>()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(error) = body.error {
        if error == "authorization_pending" {
            return Err("pending".to_string());
        } else if error == "slow_down" {
            return Err("slow_down".to_string());
        } else {
            return Err(format!(
                "Auth error: {} - {}",
                error,
                body.error_description.unwrap_or_default()
            ));
        }
    }

    if let Some(token) = body.access_token {
        Ok(token)
    } else {
        Err("No access token in response".to_string())
    }
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        Command::new("cmd")
            .args(["/C", "start", "", &url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
