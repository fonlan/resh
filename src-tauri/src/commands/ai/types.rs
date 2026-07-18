use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionCall,
    /// Provider-issued signatures (for example Gemini thought signatures) that must accompany
    /// the original tool call when a provider requires round-tripping it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_signatures: Option<Vec<String>>,
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

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ToolRisk {
    ReadOnly,
    Mutating,
    Dangerous,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecution {
    Parallel,
    Sequential,
    Background,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    Ask,
    Agent,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ToolApproval {
    Auto,
    Countdown,
    AlwaysAsk,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ToolPolicy {
    pub risk: ToolRisk,
    pub execution: ToolExecution,
    pub allowed_modes: Vec<ToolMode>,
    pub approval: ToolApproval,
    pub idempotent: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
    #[serde(skip_serializing)]
    pub policy: ToolPolicy,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum ToolOutcomeStatus {
    Completed,
    BackgroundQueued,
    BackgroundRunning,
    Failed,
    Declined,
    Cancelled,
}

impl ToolOutcomeStatus {
    pub fn as_db_status(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::BackgroundQueued => "queued",
            Self::BackgroundRunning => "running",
            Self::Failed => "failed",
            Self::Declined => "declined",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_successful_observation(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::BackgroundQueued | Self::BackgroundRunning
        )
    }
}

#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub tool_call_id: String,
    pub status: ToolOutcomeStatus,
    pub content: String,
}

impl ToolOutcome {
    pub fn observation(&self) -> String {
        if self.status.is_successful_observation() {
            return self.content.clone();
        }
        serde_json::json!({
            "status": self.status.as_db_status(),
            "error": self.content,
        })
        .to_string()
    }
}

/// Stream text chunk for `ai-response-*` / `ai-reasoning-*` (request-scoped).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiStreamTextPayload {
    pub request_id: String,
    pub content: String,
}

/// Tool confirmation signal for `ai-tool-call-*`. Approval policy is supplied by the
/// backend so the UI can offer a countdown only for calls that are eligible for it.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiToolCallEventPayload {
    pub request_id: String,
    pub run_id: String,
    pub turn_index: i64,
    /// Persisted `ai_tool_invocations.id` values, aligned with `tool_calls`.
    pub item_ids: Vec<String>,
    /// Lifecycle status for these item projections. Kept explicit for incremental UI migration.
    pub status: String,
    pub tool_calls: Vec<ToolCall>,
    pub approval_ids: Vec<String>,
    pub approval_policies: Vec<ToolApproval>,
}

/// Latest non-terminal run for session restore. The frontend projects this state and does not
/// infer it from message order.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiRunSnapshot {
    pub run_id: String,
    pub request_id: String,
    pub status: String,
    pub turn_index: i64,
}

/// Batch of complete messages for `ai-message-batch-*`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiMessageBatchPayload {
    pub request_id: String,
    /// Present only for task updates that outlive an Agent run. These events are intentionally
    /// independent from requestId gating so a completed background task can update the UI while
    /// another run is streaming.
    pub background_task_id: Option<String>,
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
