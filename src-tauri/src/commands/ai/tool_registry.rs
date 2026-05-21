use super::types::{FunctionDefinition, ToolDefinition};

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
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "get_selected_terminal_output".to_string(),
                description: "Get the currently selected text in the terminal. Use this when the user asks to analyze or work with text they have highlighted/selected.".to_string(),
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
                name: "read_file".to_string(),
                description: "Read file content directly from the remote server over SFTP without using terminal commands. Useful for analyzing config/code/log files.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "remote_path": {
                            "type": "string",
                            "description": "Absolute path to the remote file (example: /etc/nginx/nginx.conf)"
                        }
                    },
                    "required": ["remote_path"]
                }),
            },
        },
    ];

    if is_agent_mode {
        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "run_in_terminal".to_string(),
                description: "Execute a command in the terminal. Always provide timeoutSeconds and estimate it with enough safety margin for the command. By default it waits for command completion or timeout and then returns output. Set wait_finish=false for interactive TUI programs (for example vim/top/htop) when you only need to launch without waiting.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        },
                        "timeoutSeconds": {
                            "type": "integer",
                            "description": "Required positive timeout in seconds. Estimate expected runtime in the current environment and add enough safety margin."
                        },
                        "wait_finish": {
                            "type": "boolean",
                            "description": "Whether to wait for command completion before returning (default: true). Set false for TUI/interactive programs that keep running."
                        }
                    },
                    "required": ["command", "timeoutSeconds"]
                }),
            },
        });

        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "run_in_background".to_string(),
                description: "Execute a command through a separate SSH exec channel without using the foreground terminal. Always provide timeoutSeconds and estimate it with enough safety margin for the command. Prefer run_in_terminal first. Use this only when the foreground terminal is blocked/busy or when an immediate parallel diagnostic/recovery command is required (for example process check/kill).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute in background channel"
                        },
                        "timeoutSeconds": {
                            "type": "integer",
                            "description": "Required positive timeout in seconds. Estimate expected runtime in the current environment and add enough safety margin."
                        }
                    },
                    "required": ["command", "timeoutSeconds"]
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
                description: "Send arbitrary characters or escape sequences to the terminal. Use this to send key presses like 'q' to quit a TUI program, or special keys like escape sequences. To press Enter, send '\\n' (newline), not literal '\\\\n'. Useful for dismissing prompts or navigating TUI applications.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string",
                            "description": "The characters or escape sequence to send. IMPORTANT: use '\\n' (newline) to send Enter; do not send literal '\\\\n' text. Example: ':wq\\n'."
                        }
                    },
                    "required": ["input"]
                }),
            },
        });

        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "sftp_download".to_string(),
                description: "Download a file or folder from the remote server to the local machine. If target local directory is not specified, it will use the default download path in settings.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "remote_path": {
                            "type": "string",
                            "description": "The absolute path of the file or folder on the remote server to download"
                        },
                        "local_path": {
                            "type": "string",
                            "description": "The local path where the file or folder should be saved. If omitted, the default download directory will be used."
                        }
                    },
                    "required": ["remote_path"]
                }),
            },
        });

        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "sftp_upload".to_string(),
                description: "Upload a local file or folder to the remote server.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "local_path": {
                            "type": "string",
                            "description": "The absolute path of the file or folder on the local machine to upload"
                        },
                        "remote_path": {
                            "type": "string",
                            "description": "The target absolute path on the remote server where the file or folder should be saved"
                        }
                    },
                    "required": ["local_path", "remote_path"]
                }),
            },
        });
    }

    tools
}
