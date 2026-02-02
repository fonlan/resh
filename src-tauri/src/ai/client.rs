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
    MessageBatch(Vec<ChatMessage>), // Support for multiple messages in one response (e.g., minimax)
    Done,
}

/// Extract thinking content from <thinking> tags in content
fn extract_thinking_tags(content: &str) -> (String, String) {
    if content.contains("<thinking>") && content.contains("</thinking>") {
        let thinking_pattern = regex::Regex::new(r"(?s)<thinking>(.*?)</thinking>").unwrap();
        if let Some(captures) = thinking_pattern.captures(content) {
            if let Some(thinking_match) = captures.get(1) {
                let thinking = thinking_match.as_str().trim().to_string();
                let cleaned_content = content
                    .replace("<thinking>", "")
                    .replace("</thinking>", "")
                    .trim()
                    .to_string();
                return (thinking, cleaned_content);
            }
        }
    }
    (String::new(), content.to_string())
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
    timeout: Option<u64>,
) -> Result<Vec<Model>, String> {
    tracing::debug!("[AI Client] Fetching models from {}", endpoint);

    let timeout_duration = Duration::from_secs(timeout.unwrap_or(30));

    let mut client_builder = Client::builder()
        .user_agent("Resh/0.1.0")
        .timeout(timeout_duration)
        .connect_timeout(Duration::from_secs(10));

    if let Some(p) = proxy {
        let scheme = if p.proxy_type == "socks5" { "socks5h" } else { "http" };
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

        // 忽略 SSL 证书校验（用于公司代理 MITM 场景）
        if p.ignore_ssl_errors {
            tracing::warn!("[AI Client] Ignoring SSL certificates for proxy {}", p.name);
            client_builder = client_builder.danger_accept_invalid_certs(true);
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
    timeout: Option<u64>,
) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, String>> + Send>>, String> {
    tracing::debug!("[AI Client] Starting OpenAI API request to {}", endpoint);
    tracing::debug!("[AI Client] Model: {}, Messages: {}, Tools: {}", 
        model, messages.len(), tools.is_some());
    
    let timeout_duration = Duration::from_secs(timeout.unwrap_or(120));

    // Explicitly configure client for robustness
    let mut client_builder = Client::builder()
        .user_agent("Resh/0.1.0")
        .timeout(timeout_duration)
        .connect_timeout(Duration::from_secs(10));

    if let Some(p) = proxy {
        let scheme = if p.proxy_type == "socks5" { "socks5h" } else { "http" };
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
                        if let Some(choices) = json["choices"].as_array() {
                            // Check for streaming format (delta) first
                            let first_choice = choices.first();
                            let has_delta = first_choice.and_then(|c| c.get("delta").map(|d| d.is_object())).unwrap_or(false);

                            if has_delta {
                                // Streaming response - use OpenAI-style delta processing
                                let delta = &json["choices"][0]["delta"];
                                if let Some(content) = delta["content"].as_str() {
                                    return Ok(StreamEvent::Content(content.to_string()));
                                } else if let Some(reasoning) = delta["reasoning_content"].as_str() {
                                    return Ok(StreamEvent::Reasoning(reasoning.to_string()));
                                } else if let Some(tool_calls) = delta["tool_calls"].as_array() {
                                    return Ok(StreamEvent::ToolCall(serde_json::Value::Array(tool_calls.clone())));
                                } else {
                                    return Ok(StreamEvent::Content("".to_string()));
                                }
                            } else {
                                // Check for message objects
                                let has_message_objects = choices.iter().any(|choice| {
                                    let message = &choice["message"];
                                    message.is_object()
                                });

                                if has_message_objects {
                                    // Check if this is a batch format (multiple messages with different roles)
                                    let roles: std::collections::HashSet<&str> = choices.iter()
                                        .filter_map(|choice| choice["message"].get("role")?.as_str())
                                        .collect();
                                    
                                    // If multiple unique roles or tool role present, treat as batch
                                    let is_batch = roles.len() > 1 || roles.contains("tool");

                                    if is_batch {
                                        // Batch response - process all messages
                                        let mut messages: Vec<ChatMessage> = Vec::new();
                                        for choice in choices {
                                            if let Some(message_obj) = choice["message"].as_object() {
                                                let role = message_obj.get("role")
                                                    .and_then(|r| r.as_str())
                                                    .unwrap_or("assistant")
                                                    .to_string();

                                                let raw_content = message_obj.get("content")
                                                    .and_then(|c| c.as_str())
                                                    .unwrap_or("")
                                                    .to_string();
                                                let (thinking_content, cleaned_content) = extract_thinking_tags(&raw_content);

                                                let reasoning = message_obj.get("reasoning_content")
                                                    .and_then(|r| r.as_str())
                                                    .map(|s| s.to_string())
                                                    .or(Some(thinking_content))
                                                    .filter(|s| !s.is_empty());

                                                let tool_calls = message_obj.get("tool_calls")
                                                    .and_then(|tc| tc.as_array())
                                                    .map(|arr| {
                                                        arr.iter().filter_map(|tc| {
                                                            Some(ToolCall {
                                                                id: tc.get("id")?.as_str()?.to_string(),
                                                                tool_type: tc.get("type")?.as_str()?.to_string(),
                                                                function: FunctionCall {
                                                                    name: tc.get("function")?.get("name")?.as_str()?.to_string(),
                                                                    arguments: tc.get("function")?.get("arguments")?.as_str()?.to_string(),
                                                                }
                                                            })
                                                        }).collect()
                                                    });

                                                if reasoning.is_some() || !cleaned_content.is_empty() || tool_calls.is_some() {
                                                    messages.push(ChatMessage {
                                                        role,
                                                        content: if cleaned_content.is_empty() { None } else { Some(cleaned_content) },
                                                        reasoning_content: reasoning,
                                                        tool_calls,
                                                        tool_call_id: None,
                                                    });
                                                }
                                            }
                                        }

                                        if !messages.is_empty() {
                                            return Ok(StreamEvent::MessageBatch(messages));
                                        }
                                    } else {
                                        // Single message response - treat as standard OpenAI format
                                        let message_obj = &choices[0]["message"];
                                        if let Some(content) = message_obj.get("content").and_then(|c| c.as_str()) {
                                            let (thinking_content, cleaned_content) = extract_thinking_tags(content);
                                            let reasoning = message_obj.get("reasoning_content")
                                                .and_then(|r| r.as_str())
                                                .map(|s| s.to_string())
                                                .or(Some(thinking_content))
                                                .filter(|s| !s.is_empty());

                                            if let Some(tc) = message_obj.get("tool_calls").and_then(|tc| tc.as_array()) {
                                                let tool_calls: Vec<ToolCall> = tc.iter().filter_map(|tc| {
                                                    Some(ToolCall {
                                                        id: tc.get("id")?.as_str()?.to_string(),
                                                        tool_type: tc.get("type")?.as_str()?.to_string(),
                                                        function: FunctionCall {
                                                            name: tc.get("function")?.get("name")?.as_str()?.to_string(),
                                                            arguments: tc.get("function")?.get("arguments")?.as_str()?.to_string(),
                                                        }
                                                    })
                                                }).collect();

                                                if !tool_calls.is_empty() {
                                                    return Ok(StreamEvent::MessageBatch(vec![ChatMessage {
                                                        role: "assistant".to_string(),
                                                        content: if cleaned_content.is_empty() { None } else { Some(cleaned_content) },
                                                        reasoning_content: reasoning,
                                                        tool_calls: Some(tool_calls),
                                                        tool_call_id: None,
                                                    }]));
                                                }
                                            }

                                            return Ok(StreamEvent::Content(cleaned_content));
                                        }
                                    }
                                }
                            }
                        }

                        // Standard OpenAI-style delta processing
                        let delta = &json["choices"][0]["delta"];
                        if let Some(content) = delta["content"].as_str() {
                            Ok(StreamEvent::Content(content.to_string()))
                        } else if let Some(reasoning) = delta["reasoning_content"].as_str() {
                            Ok(StreamEvent::Reasoning(reasoning.to_string()))
                        } else if let Some(tool_calls) = delta["tool_calls"].as_array() {
                            Ok(StreamEvent::ToolCall(serde_json::Value::Array(tool_calls.clone())))
                        } else {
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

        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "send_interrupt".to_string(),
                description: "Send Ctrl+C (ETX, character code 3) to interrupt a running program. Use this when a TUI program (like htop, vim, less, iftop) is blocking and needs to be terminated. Returns confirmation of the interrupt being sent.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        });

        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "send_terminal_input".to_string(),
                description: "Send arbitrary characters or escape sequences to the terminal. Use this to send key presses like 'q' to quit a TUI program, or special keys like escape sequences. Useful for dismissing prompts or navigating TUI applications.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string",
                            "description": "The characters or escape sequence to send (e.g., 'q' to quit, '\\x1b' for Escape, '\\n' for Enter)"
                        }
                    },
                    "required": ["input"]
                }),
            },
        });
    }

    tools
}
