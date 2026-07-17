use serde::{Deserialize, Serialize};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
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

/// Stream text chunk for `ai-response-*` / `ai-reasoning-*` (request-scoped).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiStreamTextPayload {
    pub request_id: String,
    pub content: String,
}

/// Tool confirmation / auto-exec signal for `ai-tool-call-*`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiToolCallEventPayload {
    pub request_id: String,
    pub tool_calls: Vec<ToolCall>,
}

/// Batch of complete messages for `ai-message-batch-*`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiMessageBatchPayload {
    pub request_id: String,
    pub messages: Vec<ChatMessage>,
}

/// Provider / stream error for `ai-error-*`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiErrorEventPayload {
    pub request_id: String,
    pub error: String,
}

/// Reasoning stream end marker for `ai-reasoning-end-*`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiReasoningEndPayload {
    pub request_id: String,
    pub status: String,
}

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
