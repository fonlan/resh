use crate::config::types::{AiChannel, AiModel, Proxy};
use dashmap::DashMap;
use genai::adapter::AdapterKind;
use genai::chat::{
    ChatMessage as GenaiMessage, ChatOptions, ChatRequest, ChatStream, ReasoningEffort, Tool,
};
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{Client, ModelIden, ServiceTarget};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::Client as HttpClient;
use std::time::Duration;

pub struct CopilotToken {
    pub token: String,
    pub expires_at: u64,
}

pub struct AiManager {
    copilot_tokens: DashMap<String, CopilotToken>,
}

fn reasoning_effort_from_level(level: Option<&str>) -> Option<ReasoningEffort> {
    let normalized = level
        .map(str::trim)
        .filter(|level| !level.is_empty())
        .map(str::to_ascii_lowercase);

    match normalized.as_deref() {
        Some("minimal") => Some(ReasoningEffort::Minimal),
        Some("low") => Some(ReasoningEffort::Low),
        Some("medium") => Some(ReasoningEffort::Medium),
        Some("high") => Some(ReasoningEffort::High),
        Some("xhigh") => Some(ReasoningEffort::XHigh),
        Some("max") => Some(ReasoningEffort::Max),
        Some("none") | Some("off") | None => None,
        Some(other) => {
            tracing::warn!(
                "[AiManager] Unknown thinking level '{}', using no explicit reasoning effort",
                other
            );
            None
        }
    }
}

fn is_claude_anthropic_request(channel: &AiChannel, model: &AiModel) -> bool {
    let provider = channel.provider.to_ascii_lowercase();
    let model_name = model.name.to_ascii_lowercase();

    provider == "anthropic"
        || provider == "claude"
        || model_name.starts_with("claude-")
        || channel
            .endpoint
            .as_deref()
            .map(|endpoint| endpoint.to_ascii_lowercase().contains("anthropic"))
            .unwrap_or(false)
}

fn is_provider(channel: &AiChannel, provider: &str) -> bool {
    channel.provider.eq_ignore_ascii_case(provider)
}

fn default_endpoint_for_provider(provider: &str) -> &'static str {
    if provider.eq_ignore_ascii_case("anthropic") {
        "https://api.anthropic.com/"
    } else {
        "https://api.openai.com/v1/"
    }
}

fn endpoint_for_channel(channel: &AiChannel) -> &str {
    channel
        .endpoint
        .as_deref()
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty())
        .unwrap_or(default_endpoint_for_provider(&channel.provider))
}

fn adapter_kind_for_provider(provider: &str) -> AdapterKind {
    if provider.eq_ignore_ascii_case("anthropic") {
        AdapterKind::Anthropic
    } else {
        AdapterKind::OpenAI
    }
}

fn build_chat_options(
    channel: &AiChannel,
    model: &AiModel,
    thinking_level: Option<&str>,
) -> ChatOptions {
    let mut options = ChatOptions::default()
        .with_temperature(0.0)
        .with_capture_content(true)
        .with_capture_reasoning_content(true);

    if is_claude_anthropic_request(channel, model) {
        options = options.with_top_p(1.0);
    }

    // ponytail: string enum only; unknown values intentionally fall back to no explicit thinking.
    if let Some(reasoning_effort) = reasoning_effort_from_level(thinking_level) {
        options = options.with_reasoning_effort(reasoning_effort);
    }

    options
}

impl AiManager {
    pub fn new() -> Self {
        Self {
            copilot_tokens: DashMap::new(),
        }
    }

    pub fn build_http_client(
        &self,
        proxy: Option<Proxy>,
        provider: Option<&str>,
    ) -> Result<HttpClient, String> {
        let mut builder = HttpClient::builder()
            .user_agent("Resh/0.1.0")
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10));

        // Add default headers for Copilot provider
        if provider == Some("copilot") {
            let mut headers = HeaderMap::new();
            headers.insert("Editor-Version", HeaderValue::from_static("vscode/1.85.1"));
            headers.insert(
                "Copilot-Integration-Id",
                HeaderValue::from_static("vscode-chat"),
            );
            builder = builder.default_headers(headers);
        }

        if let Some(p) = proxy {
            let scheme = if p.proxy_type == "socks5" {
                "socks5h"
            } else {
                "http"
            };
            let auth_part = if let (Some(u), Some(pass)) = (&p.username, &p.password) {
                format!("{}:{}@", u, pass)
            } else {
                "".to_string()
            };
            let proxy_url = format!("{}://{}{}:{}", scheme, auth_part, p.host, p.port);

            match reqwest::Proxy::all(&proxy_url) {
                Ok(proxy_obj) => {
                    builder = builder.proxy(proxy_obj);
                }
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

    async fn get_copilot_token(
        &self,
        oauth_token: &str,
        http_client: &HttpClient,
    ) -> Result<String, String> {
        if let Some(token) = self.copilot_tokens.get(oauth_token) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| format!("系统时间错误: {}", e))?
                .as_secs();
            if token.expires_at > now + 60 {
                return Ok(token.token.clone());
            }
        }

        let mut auth_val = HeaderValue::from_str(&format!("token {}", oauth_token))
            .map_err(|e| format!("Invalid auth header: {}", e))?;
        auth_val.set_sensitive(true);

        let res = http_client
            .get("https://api.github.com/copilot_internal/v2/token")
            .header(AUTHORIZATION, auth_val)
            .header(USER_AGENT, "GithubCopilot/1.155.0")
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| format!("Token request failed: {}", e))?;

        let status = res.status();
        let body_text = res.text().await.map_err(|e| e.to_string())?;

        if !status.is_success() {
            return Err(format!(
                "Failed to get copilot token: {}. Body: {}",
                status, body_text
            ));
        }

        let body: serde_json::Value =
            serde_json::from_str(&body_text).map_err(|e| e.to_string())?;
        let token = body["token"]
            .as_str()
            .ok_or("No token in response")?
            .to_string();
        let expires_at = body["expires_at"]
            .as_u64()
            .ok_or("No expires_at in response")?;

        self.copilot_tokens.insert(
            oauth_token.to_string(),
            CopilotToken {
                token: token.clone(),
                expires_at,
            },
        );

        Ok(token)
    }

    pub async fn stream_chat(
        &self,
        channel: &AiChannel,
        model: &AiModel,
        messages: Vec<GenaiMessage>,
        tools: Option<Vec<Tool>>,
        proxy: Option<Proxy>,
        thinking_level: Option<&str>,
    ) -> Result<ChatStream, String> {
        let http_client = self.build_http_client(proxy, Some(&channel.provider))?;
        let mut chat_req = ChatRequest::new(messages);
        if let Some(t) = tools {
            chat_req = chat_req.with_tools(t);
        }

        let (endpoint, auth, adapter_kind) = if is_provider(channel, "copilot") {
            let oauth_token = channel.api_key.as_deref().unwrap_or_default();
            let session_token = self.get_copilot_token(oauth_token, &http_client).await?;

            let auth = AuthData::Key(session_token);
            let endpoint = Endpoint::from_owned("https://api.githubcopilot.com/");

            // Copilot chat APIs are OpenAI-compatible regardless of model family.
            // Using Anthropic adapter here sends Anthropic-style routes and can 404.
            (endpoint, auth, AdapterKind::OpenAI)
        } else {
            let api_key = channel.api_key.as_deref().unwrap_or_default();
            let auth = AuthData::Key(api_key.to_string());
            let endpoint_url = endpoint_for_channel(channel);

            let endpoint_url = if endpoint_url.ends_with('/') {
                endpoint_url.to_string()
            } else {
                format!("{}/", endpoint_url)
            };
            let endpoint = Endpoint::from_owned(endpoint_url);

            (endpoint, auth, adapter_kind_for_provider(&channel.provider))
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

        let options = build_chat_options(channel, model, thinking_level);

        Ok(genai_client
            .exec_chat_stream(&model.name, chat_req, Some(&options))
            .await
            .map_err(|e| e.to_string())?
            .stream)
    }

    pub async fn fetch_models(
        &self,
        channel: &AiChannel,
        proxy: Option<Proxy>,
    ) -> Result<Vec<String>, String> {
        if is_provider(channel, "anthropic") {
            return Err(
                "Anthropic Message channels do not support model fetching via /models; add the model name manually."
                    .to_string(),
            );
        }

        let http_client = self.build_http_client(proxy, Some(&channel.provider))?;

        if is_provider(channel, "copilot") {
            let oauth_token = channel.api_key.as_deref().unwrap_or_default();
            let session_token = self.get_copilot_token(oauth_token, &http_client).await?;

            let res = http_client
                .get("https://api.githubcopilot.com/models")
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
                let is_chat = m["capabilities"]["type"].as_str() == Some("chat")
                    || m["type"].as_str() == Some("chat");
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
            let endpoint = endpoint_for_channel(channel);

            let url = if endpoint.ends_with("/") {
                format!("{}models", endpoint)
            } else {
                format!("{}/models", endpoint)
            };

            let res = http_client
                .get(&url)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(provider: &str, endpoint: Option<&str>) -> AiChannel {
        AiChannel {
            id: "channel".to_string(),
            name: "Channel".to_string(),
            provider: provider.to_string(),
            endpoint: endpoint.map(str::to_string),
            api_key: None,
            proxy_id: None,
            is_active: true,
            synced: true,
            updated_at: "now".to_string(),
        }
    }

    fn model(name: &str) -> AiModel {
        AiModel {
            id: "model".to_string(),
            name: name.to_string(),
            channel_id: "channel".to_string(),
            enabled: true,
            synced: true,
            updated_at: "now".to_string(),
        }
    }

    #[test]
    fn chat_options_map_thinking_and_claude_defaults() {
        let options = build_chat_options(
            &channel("openai", Some("https://api.anthropic.com/v1")),
            &model("claude-sonnet-4"),
            Some("high"),
        );

        assert_eq!(options.temperature, Some(0.0));
        assert_eq!(options.top_p, Some(1.0));
        assert_eq!(
            options
                .reasoning_effort
                .as_ref()
                .map(|effort| effort.variant_name()),
            Some("high")
        );
    }

    #[test]
    fn unknown_thinking_level_is_no_explicit_effort() {
        let options = build_chat_options(&channel("openai", None), &model("gpt-4o"), Some("fast"));

        assert_eq!(options.temperature, Some(0.0));
        assert!(options.top_p.is_none());
        assert!(options.reasoning_effort.is_none());
    }
    #[test]
    fn anthropic_provider_uses_anthropic_adapter_and_endpoint() {
        let anthropic = channel("anthropic", None);
        let openai = channel("openai", None);

        assert!(matches!(
            adapter_kind_for_provider(&anthropic.provider),
            AdapterKind::Anthropic
        ));
        assert_eq!(
            endpoint_for_channel(&anthropic),
            "https://api.anthropic.com/"
        );

        assert!(matches!(
            adapter_kind_for_provider(&openai.provider),
            AdapterKind::OpenAI
        ));
        assert_eq!(endpoint_for_channel(&openai), "https://api.openai.com/v1/");
    }

    #[tokio::test]
    async fn anthropic_model_fetch_returns_clear_error() {
        let err = AiManager::new()
            .fetch_models(&channel("anthropic", None), None)
            .await
            .unwrap_err();

        assert!(err.contains("Anthropic Message"));
        assert!(err.contains("/models"));
    }
}
