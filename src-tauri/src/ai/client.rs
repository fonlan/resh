use serde::{Serialize, Deserialize};
use futures::Stream;
use std::pin::Pin;
use reqwest::Client;
use eventsource_stream::Eventsource;
use futures::StreamExt;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
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
) -> Result<Pin<Box<dyn Stream<Item = Result<String, String>> + Send>>, String> {
    let client = Client::new();
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

    let response = client.post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&req)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("API Error: {}", text));
    }

    let stream = response.bytes_stream().eventsource();
    
    let stream_mapped = stream.map(|event| {
        match event {
            Ok(event) => {
                if event.data == "[DONE]" {
                    return Ok("".to_string());
                }
                match serde_json::from_str::<serde_json::Value>(&event.data) {
                    Ok(json) => {
                        if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                            Ok(content.to_string())
                        } else {
                            Ok("".to_string())
                        }
                    },
                    Err(_) => Ok("".to_string()) // Ignore parse errors for keepalives etc
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
                name: "get_terminal_text".to_string(),
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
