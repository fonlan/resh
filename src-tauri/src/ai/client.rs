use serde::{Serialize, Deserialize};
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
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Content(String),
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

pub async fn stream_openai_chat(
    endpoint: &str,
    api_key: &str,
    model: &str,
    messages: Vec<ChatMessage>,
    tools: Option<Vec<ToolDefinition>>,
) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, String>> + Send>>, String> {
    tracing::debug!("[AI Client] Starting OpenAI API request to {}", endpoint);
    tracing::debug!("[AI Client] Model: {}, Messages: {}, Tools: {}", 
        model, messages.len(), tools.is_some());
    
    // Explicitly configure client for robustness
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .no_proxy() // Try bypassing system proxy first to see if that helps, or maybe we should NOT do this.
                    // Actually, if the user has a system proxy (e.g. Clash), they need it.
                    // "unexpected EOF" suggests the connection IS made but dropped.
                    // Let's NOT use no_proxy() by default, but rely on reqwest default env behavior.
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

    tracing::debug!("[AI Client] POST {}", url);

    let response = client.post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
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

/// Create tool definitions for agent mode
pub fn create_agent_tools() -> Vec<ToolDefinition> {
    vec![
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
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "run_in_terminal".to_string(),
                description: "Execute a command in the terminal. Use this to fix issues, install packages, or perform system operations.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
    ]
}
