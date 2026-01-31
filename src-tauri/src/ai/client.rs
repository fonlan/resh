use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use futures::Stream;
use std::pin::Pin;
use reqwest::Client;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Content(String),
    Reasoning(String),
    ToolCall(serde_json::Value),
    Done,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
}

use crate::config::types::Proxy;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Model {
    pub id: String,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelListResponse {
    pub data: Vec<Model>,
}

pub async fn fetch_models(
    endpoint: &str,
    api_key: &str,
    extra_headers: Option<HashMap<String, String>>,
    proxy: Option<Proxy>,
) -> Result<Vec<Model>, String> {
    tracing::debug!("[AI Client] Fetching models from {}", endpoint);

    let mut client_builder = Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));

    if let Some(p) = proxy {
        let scheme = if p.proxy_type == "socks5" { "socks5" } else { "http" };
        let auth_part = if let (Some(u), Some(pass)) = (p.username, p.password) {
            format!("{}:{}@", u, pass)
        } else {
            "".to_string()
        };
        let proxy_url = format!("{}://{}{}:{}", scheme, auth_part, p.host, p.port);
        
        match reqwest::Proxy::all(&proxy_url) {
            Ok(proxy) => {
                client_builder = client_builder.proxy(proxy);
            },
            Err(e) => {
                tracing::warn!("[AI Client] Failed to create proxy: {}", e);
            }
        }
    }

    let client = client_builder.build().map_err(|e| e.to_string())?;

    let url = if endpoint.ends_with("/") {
        format!("{}models", endpoint)
    } else {
        format!("{}/models", endpoint)
    };

    let mut request_builder = client.get(&url)
        .header("Authorization", format!("Bearer {}", api_key));

    if let Some(headers) = extra_headers {
        for (k, v) in headers {
            request_builder = request_builder.header(k, v);
        }
    }

    let response = request_builder.send().await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("API Error: {}", text));
    }

    let list_response: ModelListResponse = response.json().await.map_err(|e| e.to_string())?;
    Ok(list_response.data)
}

pub async fn stream_openai_chat(
    endpoint: &str,
    api_key: &str,
    model: &str,
    messages: Vec<ChatMessage>,
    tools: Option<Vec<ToolDefinition>>,
    extra_headers: Option<HashMap<String, String>>,
    proxy: Option<Proxy>,
) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, String>> + Send>>, String> {
    tracing::debug!("[AI Client] Starting OpenAI API request to {}", endpoint);
    tracing::debug!("[AI Client] Model: {}, Messages: {}, Tools: {}", 
        model, messages.len(), tools.is_some());
    
    // Explicitly configure client for robustness
    let mut client_builder = Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10));

    if let Some(p) = proxy {
        let scheme = if p.proxy_type == "socks5" { "socks5" } else { "http" };
        let auth_part = if let (Some(u), Some(pass)) = (p.username, p.password) {
            format!("{}:{}@", u, pass)
        } else {
            "".to_string()
        };
        let proxy_url = format!("{}://{}{}:{}", scheme, auth_part, p.host, p.port);
        
        match reqwest::Proxy::all(&proxy_url) {
            Ok(proxy) => {
                client_builder = client_builder.proxy(proxy);
                // Log without auth details
                let safe_url = format!("{}://***:***@{}:{}", scheme, p.host, p.port);
                tracing::debug!("[AI Client] Using proxy: {}", safe_url);
            },
            Err(e) => {
                tracing::warn!("[AI Client] Failed to create proxy from URL, falling back to direct connection: {}", e);
            }
        }
    }

    let client = client_builder
        .build()
        .map_err(|e| format!("Failed to build client: {}", e))?;

    let req = ChatRequest {
        model: model.to_string(),
        messages,
        stream: true,
        tools,
    };

    let url = if endpoint.ends_with("/") {
        format!("{}chat/completions", endpoint)
    } else {
        format!("{}/chat/completions", endpoint)
    };

    let mut request_builder = client.post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json");

    if let Some(headers) = extra_headers {
        for (k, v) in headers {
            request_builder = request_builder.header(k, v);
        }
    }

    let response = request_builder
        .json(&req)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("[AI Client] Request failed: {}", e);
            e.to_string()
        })?;

    tracing::debug!("[AI Client] Response status: {}", response.status());
    
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        tracing::error!("[AI Client] API Error: {}", text);
        return Err(format!("API Error: {}", text));
    }

    let stream = response.bytes_stream().eventsource();
    
    let stream_mapped = stream.map(|event| {
        match event {
            Ok(event) => {
                if event.data == "[DONE]" {
                    return Ok(StreamEvent::Done);
                }
                match serde_json::from_str::<serde_json::Value>(&event.data) {
                    Ok(json) => {
                        let delta = &json["choices"][0]["delta"];
                        if let Some(content) = delta["content"].as_str() {
                            Ok(StreamEvent::Content(content.to_string()))
                        } else if let Some(reasoning) = delta["reasoning_content"].as_str() {
                            Ok(StreamEvent::Reasoning(reasoning.to_string()))
                        } else if let Some(tool_calls) = delta["tool_calls"].as_array() {
                            // Pass the whole tool_calls array to be processed by caller
                            Ok(StreamEvent::ToolCall(serde_json::Value::Array(tool_calls.clone())))
                        } else {
                            // Keepalive or empty delta
                            Ok(StreamEvent::Content("".to_string()))
                        }
                    },
                    Err(_) => Ok(StreamEvent::Content("".to_string())) // Ignore parse errors
                }
            },
            Err(e) => Err(e.to_string()),
        }
    });

    Ok(Box::pin(stream_mapped))
}

/// Create tool definitions based on the mode
pub fn create_tools(is_agent_mode: bool) -> Vec<ToolDefinition> {
    let mut tools = vec![
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "get_terminal_output".to_string(),
                description: "Get the current terminal output text to analyze errors, command results, or system state.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        },
    ];

    if is_agent_mode {
        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "run_in_terminal".to_string(),
                description: "Execute a command in the terminal. Waits for command to complete or timeout, then returns the command output. Use this to fix issues, install packages, or perform system operations.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        },
                        "timeoutSeconds": {
                            "type": "integer",
                            "description": "Timeout in seconds (default: 30). Maximum time to wait for command completion."
                        }
                    },
                    "required": ["command"]
                }),
            },
        });
    }

    tools
}
