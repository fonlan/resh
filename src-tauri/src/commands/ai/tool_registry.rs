use super::types::{
    FunctionDefinition, ToolApproval, ToolDefinition, ToolExecution, ToolMode, ToolOutcome,
    ToolOutcomeStatus, ToolPolicy, ToolRisk,
};
use super::ToolCall;

#[derive(Debug, Clone)]
pub(super) struct PreparedToolCall {
    pub call: ToolCall,
    pub policy: ToolPolicy,
}

#[derive(Debug, Clone)]
pub(super) enum ToolPreparation {
    Execute(PreparedToolCall),
    AwaitApproval(PreparedToolCall),
    Immediate(ToolOutcome),
}

/// Central policy evaluator for all AI tool calls. The order is deliberately fixed:
/// hard deny (unknown/mode/invalid) -> ask -> allow. UI countdowns only resolve an
/// existing `AwaitApproval` request and can never promote a hard-denied call.
pub(super) struct ToolPolicyEngine;

impl ToolPolicyEngine {
    pub fn prepare(
        call: ToolCall,
        is_agent_mode: bool,
        has_session_grant: bool,
    ) -> ToolPreparation {
        let Some(policy) = tool_policy(&call.function.name) else {
            return ToolPreparation::Immediate(ToolOutcome {
                tool_call_id: call.id,
                status: ToolOutcomeStatus::Failed,
                content: format!("Unknown tool '{}'.", call.function.name),
            });
        };

        if let Err(error) = validate_arguments(&call, &policy) {
            return ToolPreparation::Immediate(ToolOutcome {
                tool_call_id: call.id,
                status: ToolOutcomeStatus::Failed,
                content: error,
            });
        }

        let mode = if is_agent_mode {
            ToolMode::Agent
        } else {
            ToolMode::Ask
        };
        if !policy.allowed_modes.contains(&mode) {
            return ToolPreparation::Immediate(ToolOutcome {
                tool_call_id: call.id,
                status: ToolOutcomeStatus::Declined,
                content: format!(
                    "Execution denied: '{}' is not available in {} mode.",
                    call.function.name,
                    if is_agent_mode { "Agent" } else { "Ask" }
                ),
            });
        }

        let prepared = PreparedToolCall { call, policy };
        if prepared.policy.risk == ToolRisk::Dangerous {
            return ToolPreparation::AwaitApproval(prepared);
        }
        if prepared.policy.approval == ToolApproval::Auto
            || (has_session_grant && is_session_grant_eligible(&prepared.policy))
        {
            ToolPreparation::Execute(prepared)
        } else {
            ToolPreparation::AwaitApproval(prepared)
        }
    }
}

pub(super) fn is_session_grant_eligible(policy: &ToolPolicy) -> bool {
    policy.risk == ToolRisk::Mutating && policy.approval == ToolApproval::Countdown
}

pub(super) fn tool_policy(name: &str) -> Option<ToolPolicy> {
    tool_catalog()
        .into_iter()
        .find(|tool| tool.function.name == name)
        .map(|tool| tool.policy)
}

pub fn create_tools(is_agent_mode: bool) -> Vec<ToolDefinition> {
    let mode = if is_agent_mode {
        ToolMode::Agent
    } else {
        ToolMode::Ask
    };
    tool_catalog()
        .into_iter()
        .filter(|tool| tool.policy.allowed_modes.contains(&mode))
        .collect()
}

fn validate_arguments(call: &ToolCall, policy: &ToolPolicy) -> Result<(), String> {
    let tool = tool_catalog()
        .into_iter()
        .find(|tool| tool.function.name == call.function.name)
        .ok_or_else(|| format!("Unknown tool '{}'.", call.function.name))?;
    debug_assert_eq!(tool.policy, *policy);

    let arguments: serde_json::Value = serde_json::from_str(&call.function.arguments)
        .map_err(|_| format!("Invalid arguments JSON for '{}'.", call.function.name))?;
    let object = arguments.as_object().ok_or_else(|| {
        format!(
            "Arguments for '{}' must be a JSON object.",
            call.function.name
        )
    })?;
    let required = tool
        .function
        .parameters
        .get("required")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for required_name in required {
        let Some(name) = required_name.as_str() else {
            continue;
        };
        let Some(value) = object.get(name) else {
            return Err(format!(
                "Missing required argument '{}' for '{}'.",
                name, call.function.name
            ));
        };
        if value.is_null() || value.as_str().is_some_and(|value| value.trim().is_empty()) {
            return Err(format!(
                "Required argument '{}' for '{}' is empty.",
                name, call.function.name
            ));
        }
    }
    Ok(())
}

fn tool(
    name: &str,
    description: &str,
    parameters: serde_json::Value,
    policy: ToolPolicy,
) -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".to_string(),
        function: FunctionDefinition {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        },
        policy,
    }
}

fn read_only_parallel() -> ToolPolicy {
    ToolPolicy {
        risk: ToolRisk::ReadOnly,
        execution: ToolExecution::Parallel,
        allowed_modes: vec![ToolMode::Ask, ToolMode::Agent],
        approval: ToolApproval::Auto,
        idempotent: true,
    }
}

fn tool_catalog() -> Vec<ToolDefinition> {
    vec![
        tool(
            "get_terminal_output",
            "Get the current terminal output text to analyze errors, command results, or system state.",
            serde_json::json!({"type": "object", "properties": {}, "required": []}),
            read_only_parallel(),
        ),
        tool(
            "get_selected_terminal_output",
            "Get the currently selected text in the terminal. Use this when the user asks to analyze or work with text they have highlighted/selected.",
            serde_json::json!({"type": "object", "properties": {}, "required": []}),
            read_only_parallel(),
        ),
        tool(
            "read_file",
            "Read file content directly from the remote server over SFTP without using terminal commands. Useful for analyzing config/code/log files.",
            serde_json::json!({
                "type": "object",
                "properties": {"remote_path": {"type": "string", "description": "Absolute path to the remote file (example: /etc/nginx/nginx.conf)"}},
                "required": ["remote_path"]
            }),
            read_only_parallel(),
        ),
        tool(
            "run_in_terminal",
            "Execute a command in the terminal. Always provide timeoutSeconds and estimate it with enough safety margin for the command. By default it waits for command completion or timeout and then returns output. Set wait_finish=false for interactive TUI programs (for example vim/top/htop) when you only need to launch without waiting.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to execute"},
                    "timeoutSeconds": {"type": "integer", "description": "Required positive timeout in seconds. Estimate expected runtime in the current environment and add enough safety margin."},
                    "wait_finish": {"type": "boolean", "description": "Whether to wait for command completion before returning (default: true). Set false for TUI/interactive programs that keep running."}
                },
                "required": ["command", "timeoutSeconds"]
            }),
            ToolPolicy {
                risk: ToolRisk::Dangerous,
                execution: ToolExecution::Sequential,
                allowed_modes: vec![ToolMode::Agent],
                approval: ToolApproval::AlwaysAsk,
                idempotent: false,
            },
        ),
        tool(
            "run_in_background",
            "Execute a command through a separate SSH exec channel without using the foreground terminal. Always provide timeoutSeconds and estimate it with enough safety margin for the command. Prefer run_in_terminal first. Use this only when the foreground terminal is blocked/busy or when an immediate parallel diagnostic/recovery command is required (for example process check/kill).",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to execute in background channel"},
                    "timeoutSeconds": {"type": "integer", "description": "Required positive timeout in seconds. Estimate expected runtime in the current environment and add enough safety margin."}
                },
                "required": ["command", "timeoutSeconds"]
            }),
            ToolPolicy {
                risk: ToolRisk::Dangerous,
                execution: ToolExecution::Background,
                allowed_modes: vec![ToolMode::Agent],
                approval: ToolApproval::AlwaysAsk,
                idempotent: false,
            },
        ),
        tool(
            "send_interrupt",
            "Send Ctrl+C (ETX, character code 3) to interrupt a running program. Use this when a TUI program (like htop, vim, less, iftop) is blocking and needs to be terminated. Returns confirmation of the interrupt being sent.",
            serde_json::json!({"type": "object", "properties": {}, "required": []}),
            ToolPolicy {
                risk: ToolRisk::Dangerous,
                execution: ToolExecution::Sequential,
                allowed_modes: vec![ToolMode::Agent],
                approval: ToolApproval::AlwaysAsk,
                idempotent: false,
            },
        ),
        tool(
            "send_terminal_input",
            "Send arbitrary characters or escape sequences to the terminal. Use this to send key presses like 'q' to quit a TUI program, or special keys like escape sequences. To press Enter, send '\\n' (newline), not literal '\\\\n'. Useful for dismissing prompts or navigating TUI applications.",
            serde_json::json!({
                "type": "object",
                "properties": {"input": {"type": "string", "description": "The characters or escape sequence to send. IMPORTANT: use '\\n' (newline) to send Enter; do not send literal '\\\\n' text. Example: ':wq\\n'."}},
                "required": ["input"]
            }),
            ToolPolicy {
                risk: ToolRisk::Dangerous,
                execution: ToolExecution::Sequential,
                allowed_modes: vec![ToolMode::Agent],
                approval: ToolApproval::AlwaysAsk,
                idempotent: false,
            },
        ),
        tool(
            "sftp_download",
            "Download a file or folder from the remote server to the local machine. If target local directory is not specified, it will use the default download path in settings.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "remote_path": {"type": "string", "description": "The absolute path of the file or folder on the remote server to download"},
                    "local_path": {"type": "string", "description": "The local path where the file or folder should be saved. If omitted, the default download directory will be used."}
                },
                "required": ["remote_path"]
            }),
            ToolPolicy {
                risk: ToolRisk::Mutating,
                execution: ToolExecution::Sequential,
                allowed_modes: vec![ToolMode::Agent],
                approval: ToolApproval::Countdown,
                idempotent: false,
            },
        ),
        tool(
            "sftp_upload",
            "Upload a local file or folder to the remote server.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "local_path": {"type": "string", "description": "The absolute path of the file or folder on the local machine to upload"},
                    "remote_path": {"type": "string", "description": "The target absolute path on the remote server where the file or folder should be saved"}
                },
                "required": ["local_path", "remote_path"]
            }),
            ToolPolicy {
                risk: ToolRisk::Dangerous,
                execution: ToolExecution::Sequential,
                allowed_modes: vec![ToolMode::Agent],
                approval: ToolApproval::AlwaysAsk,
                idempotent: false,
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ai::FunctionCall;

    fn call(name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            tool_type: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: arguments.to_string(),
            },
        }
    }

    #[test]
    fn denies_agent_execution_tool_in_ask_mode_before_approval() {
        let prepared = ToolPolicyEngine::prepare(
            call("run_in_terminal", r#"{"command":"pwd","timeoutSeconds":5}"#),
            false,
            true,
        );
        assert!(matches!(
            prepared,
            ToolPreparation::Immediate(ToolOutcome {
                status: ToolOutcomeStatus::Declined,
                ..
            })
        ));
    }

    #[test]
    fn dangerous_tool_never_uses_session_grant() {
        let prepared = ToolPolicyEngine::prepare(
            call(
                "sftp_upload",
                r#"{"local_path":"/tmp/a","remote_path":"/tmp/a"}"#,
            ),
            true,
            true,
        );
        assert!(matches!(prepared, ToolPreparation::AwaitApproval(_)));
    }
}
