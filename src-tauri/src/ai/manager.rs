use dashmap::DashMap;
use genai::chat::{ChatMessage as GenaiMessage, ChatRequest, ChatStream, Tool};
use genai::adapter::AdapterKind;
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{Client, ServiceTarget, ModelIden};
use crate::config::types::{AiChannel, AiModel, Proxy};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT, ACCEPT};
use reqwest::Client as HttpClient;
use std::time::Duration;

pub struct CopilotToken {
    pub token: String,
    pub expires_at: u64,
}

pub struct AiManager {
    copilot_tokens: DashMap<String, CopilotToken>,
}

impl AiManager {
    pub fn new() -> Self {
        Self {
            copilot_tokens: DashMap::new(),
        }
    }

    pub fn build_http_client(&self, proxy: Option<Proxy>, provider: Option<&str>) -> Result<HttpClient, String> {
        let mut builder = HttpClient::builder()
            .user_agent("Resh/0.1.0")
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10));

        // Add default headers for Copilot provider
        if provider == Some("copilot") {
            let mut headers = HeaderMap::new();
            headers.insert("Editor-Version", HeaderValue::from_static("vscode/1.85.1"));
            headers.insert("Copilot-Integration-Id", HeaderValue::from_static("vscode-chat"));
            builder = builder.default_headers(headers);
        }

        if let Some(p) = proxy {
            let scheme = if p.proxy_type == "socks5" { "socks5h" } else { "http" };
            let auth_part = if let (Some(u), Some(pass)) = (&p.username, &p.password) {
                format!("{}:{}@", u, pass)
            } else {
                "".to_string()
            };
            let proxy_url = format!("{}://{}{}:{}", scheme, auth_part, p.host, p.port);

            match reqwest::Proxy::all(&proxy_url) {
                Ok(proxy_obj) => {
                    builder = builder.proxy(proxy_obj);
                },
                Err(e) => {
                    tracing::warn!("[AiManager] Failed to create proxy: {}", e);
                }
            }

            if p.ignore_ssl_errors {
                builder = builder.danger_accept_invalid_certs(true);
            }
        }

        builder.build().map_err(|e| e.to_string())
    }

    async fn get_copilot_token(&self, oauth_token: &str, http_client: &HttpClient) -> Result<String, String> {
        if let Some(token) = self.copilot_tokens.get(oauth_token) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            if token.expires_at > now + 60 {
                return Ok(token.token.clone());
            }
        }

        let mut auth_val = HeaderValue::from_str(&format!("token {}", oauth_token))
            .map_err(|e| format!("Invalid auth header: {}", e))?;
        auth_val.set_sensitive(true);

        let res = http_client.get("https://api.github.com/copilot_internal/v2/token")
            .header(AUTHORIZATION, auth_val)
            .header(USER_AGENT, "GithubCopilot/1.155.0")
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("Token request failed: {}", e))?;

        let status = res.status();
        let body_text = res.text().await.map_err(|e| e.to_string())?;

        if !status.is_success() {
            return Err(format!("Failed to get copilot token: {}. Body: {}", status, body_text));
        }

        let body: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| e.to_string())?;
        let token = body["token"].as_str().ok_or("No token in response")?.to_string();
        let expires_at = body["expires_at"].as_u64().ok_or("No expires_at in response")?;

        self.copilot_tokens.insert(oauth_token.to_string(), CopilotToken {
            token: token.clone(),
            expires_at,
        });

        Ok(token)
    }

    pub async fn stream_chat(
        &self,
        channel: &AiChannel,
        model: &AiModel,
        messages: Vec<GenaiMessage>,
        tools: Option<Vec<Tool>>,
        proxy: Option<Proxy>,
    ) -> Result<ChatStream, String> {
        let http_client = self.build_http_client(proxy, Some(&channel.provider))?;
        let mut chat_req = ChatRequest::new(messages);
        if let Some(t) = tools {
            chat_req = chat_req.with_tools(t);
        }

        let (endpoint, auth, adapter_kind) = if channel.provider == "copilot" {
            let oauth_token = channel.api_key.as_deref().unwrap_or_default();
            let session_token = self.get_copilot_token(oauth_token, &http_client).await?;
            
            let auth = AuthData::Key(session_token);
            let endpoint = Endpoint::from_owned("https://api.githubcopilot.com/");
            
            let is_claude = model.name.to_lowercase().contains("claude");
            let adapter_kind = if is_claude {
                AdapterKind::Anthropic
            } else {
                AdapterKind::OpenAI
            };

            (endpoint, auth, adapter_kind)
        } else {
            let api_key = channel.api_key.as_deref().unwrap_or_default();
            let auth = AuthData::Key(api_key.to_string());
            let endpoint_url = channel.endpoint.as_deref().unwrap_or("https://api.openai.com/v1/");
            
            let endpoint_url = if endpoint_url.ends_with('/') {
                endpoint_url.to_string()
            } else {
                format!("{}/", endpoint_url)
            };
            let endpoint = Endpoint::from_owned(endpoint_url);
            
            (endpoint, auth, AdapterKind::OpenAI)
        };

        let model_name = model.name.clone();
        let resolver = ServiceTargetResolver::from_resolver_fn(move |_| {
            Ok(ServiceTarget {
                endpoint: endpoint.clone(),
                auth: auth.clone(),
                model: ModelIden::new(adapter_kind, model_name.clone()),
            })
        });

        let genai_client = Client::builder()
            .with_reqwest(http_client)
            .with_service_target_resolver(resolver)
            .build();

        let options = genai::chat::ChatOptions::default().with_capture_content(true);

        Ok(genai_client.exec_chat_stream(&model.name, chat_req, Some(&options)).await.map_err(|e| e.to_string())?.stream)
    }

    pub async fn fetch_models(
        &self,
        channel: &AiChannel,
        proxy: Option<Proxy>,
    ) -> Result<Vec<String>, String> {
        let http_client = self.build_http_client(proxy, Some(&channel.provider))?;
        
        if channel.provider == "copilot" {
            let oauth_token = channel.api_key.as_deref().unwrap_or_default();
            let session_token = self.get_copilot_token(oauth_token, &http_client).await?;
            
            let res = http_client.get("https://api.githubcopilot.com/models")
                .header(AUTHORIZATION, format!("Bearer {}", session_token))
                .header("Copilot-Integration-Id", "vscode-chat")
                .header(USER_AGENT, "GithubCopilot/1.155.0")
                .send()
                .await
                .map_err(|e| e.to_string())?;

            if !res.status().is_success() {
                return Err(format!("Failed to fetch Copilot models: {}", res.status()));
            }

            let body: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
            let models = body["data"].as_array().ok_or("Invalid models response")?;
            
            let mut ids = Vec::new();
            for m in models {
                let is_chat = m["capabilities"]["type"].as_str() == Some("chat") || 
                             m["type"].as_str() == Some("chat");
                let picker_enabled = m["model_picker_enabled"].as_bool().unwrap_or(false);
                
                if is_chat && picker_enabled {
                    if let Some(id) = m["id"].as_str() {
                        ids.push(id.to_string());
                    }
                }
            }
            ids.sort();
            Ok(ids)
        } else {
            let api_key = channel.api_key.as_deref().unwrap_or_default();
            let endpoint = channel.endpoint.as_deref().unwrap_or("https://api.openai.com/v1/");
            
            let url = if endpoint.ends_with("/") {
                format!("{}models", endpoint)
            } else {
                format!("{}/models", endpoint)
            };

            let res = http_client.get(&url)
                .header(AUTHORIZATION, format!("Bearer {}", api_key))
                .send()
                .await
                .map_err(|e| e.to_string())?;

            if !res.status().is_success() {
                return Err(format!("Failed to fetch models: {}", res.status()));
            }

            let body: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
            let models = body["data"].as_array().ok_or("Invalid models response")?;
            
            let mut ids = Vec::new();
            for m in models {
                if let Some(id) = m["id"].as_str() {
                    ids.push(id.to_string());
                }
            }
            ids.sort();
            Ok(ids)
        }
    }
}
