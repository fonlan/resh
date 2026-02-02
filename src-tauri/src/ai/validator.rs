use std::collections::HashSet;

/// 消息验证结果
#[derive(Debug, Default)]
pub struct ValidationResult {
    /// 修复操作记录
    pub fixes: Vec<String>,
    /// 是否进行了修复
    pub was_fixed: bool,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            fixes: Vec::new(),
            was_fixed: false,
        }
    }

    pub fn add_fix(&mut self, fix: String) {
        self.fixes.push(fix);
        self.was_fixed = true;
    }
}

/// 验证并修复消息序列
///
/// 严格规则：
/// 1. 第一条必须是system消息
/// 2. 严格遵循 User -> Assistant -> User -> Assistant 顺序
/// 3. 严禁连续出现两条相同角色的消息
/// 4. 严禁以 Assistant 消息结尾（最后必须是 User 或 Tool）
/// 5. 只有当 role: "assistant" 且包含 tool_calls 时，content 可以为 null，否则不能为空
/// 6. role: "tool" 的消息中，tool_call_id 必须严格对应上一条 Assistant 消息中的 ID
/// 7. 如果 Assistant 发起了 tool_calls，下一条消息必须是 role: "tool"
/// 8. 如果 Assistant 一次调用了 N 个工具，下一轮必须连续回传 N 条 tool 消息
///
/// 正确序列示例：
/// - System → User → Assistant(content) → User → Assistant(content) → ...
/// - User → Assistant(tool_calls) → Tool → Tool → ... → Assistant(content) → User → ...
pub fn validate_and_fix_messages(messages: &mut Vec<MessagePayload>) -> ValidationResult {
    let mut result = ValidationResult::new();

    // 1. 合并连续同角色消息
    merge_duplicate_roles(messages, &mut result);

    // 2. 移除以Assistant结尾的空消息
    trim_trailing_empty_assistant(messages, &mut result);

    // 3. 验证tool_call_id对应关系
    validate_tool_call_ids(messages, &mut result);

    // 4. 验证消息序列逻辑
    validate_message_sequence(messages, &mut result);

    result
}

/// 从消息列表中提取所有tool_call_id
fn extract_tool_call_ids(messages: &[MessagePayload]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for msg in messages {
        if let Some(calls) = &msg.tool_calls {
            for call in calls {
                ids.insert(call.id.clone());
            }
        }
    }
    ids
}

/// 合并连续同角色消息
fn merge_duplicate_roles(messages: &mut Vec<MessagePayload>, result: &mut ValidationResult) {
    if messages.len() < 2 {
        return;
    }

    let mut i = 0;
    while i < messages.len().saturating_sub(1) {
        let current_role = messages[i].role.clone();
        let next_role = messages[i + 1].role.clone();

        if current_role == next_role {
            // 合并所有连续的相同角色消息
            let merged_content = merge_content(&messages[i].content, &messages[i + 1].content);

            // 对于 assistant 消息，还需要合并 reasoning_content
            let merged_reasoning = if current_role == "assistant" {
                merge_content(&messages[i].reasoning_content, &messages[i + 1].reasoning_content)
            } else {
                messages[i].reasoning_content.clone()
            };

            // 更新当前消息
            messages[i].content = merged_content;
            messages[i].reasoning_content = merged_reasoning;

            // 删除下一条消息
            messages.remove(i + 1);
            result.add_fix(format!(
                "Merged consecutive {} messages at positions {} and {}",
                current_role,
                i,
                i + 1
            ));
            // 不递增i，继续检查是否还有连续的
        } else {
            i += 1;
        }
    }
}

/// 合并两个content字段
fn merge_content(a: &Option<String>, b: &Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("{}{}", a, b)),
        (Some(a), None) => Some(a.clone()),
        (None, Some(b)) => Some(b.clone()),
        (None, None) => None,
    }
}

/// 移除结尾的空Assistant消息
fn trim_trailing_empty_assistant(
    messages: &mut Vec<MessagePayload>,
    result: &mut ValidationResult,
) {
    while let Some(last) = messages.last() {
        if last.role == "assistant" {
            let has_content = last
                .content
                .as_ref()
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            let has_tool_calls = last.tool_calls.is_some();
            let has_reasoning = last
                .reasoning_content
                .as_ref()
                .map(|s| !s.is_empty())
                .unwrap_or(false);

            if !has_content && !has_tool_calls && !has_reasoning {
                messages.pop();
                result.add_fix("Removed empty assistant message at end".to_string());
            } else {
                break;
            }
        } else {
            break;
        }
    }
}

/// 验证tool消息的tool_call_id
fn validate_tool_call_ids(messages: &mut Vec<MessagePayload>, result: &mut ValidationResult) {
    let valid_ids = extract_tool_call_ids(messages);

    for (idx, msg) in messages.iter_mut().enumerate() {
        if msg.role == "tool" {
            if let Some(call_id) = &msg.tool_call_id {
                if !valid_ids.contains(call_id) {
                    result.add_fix(format!(
                        "Tool message at {} has unknown tool_call_id: {}, clearing it",
                        idx, call_id
                    ));
                    msg.tool_call_id = None;
                }
            }
        }
    }
}

/// 验证消息序列逻辑
fn validate_message_sequence(messages: &mut Vec<MessagePayload>, result: &mut ValidationResult) {
    if messages.is_empty() {
        return;
    }

    // 规则1: 第一条必须是system消息
    if let Some(first) = messages.first() {
        if first.role != "system" {
            result.add_fix(format!(
                "First message must be system, but found {}",
                first.role
            ));
        }
    }

    // 规则2-8: 验证消息序列
    let mut i = 0;
    while i < messages.len() {
        let msg = &messages[i];
        let role = msg.role.as_str();

        match role {
            "system" => {
                // System消息只能在第一条
                if i != 0 {
                    result.add_fix(format!(
                        "System message found at position {}, should only be at position 0",
                        i
                    ));
                }
            }
            "user" => {
                // User后必须是Assistant（除非是最后一条）
                if i < messages.len() - 1 {
                    if let Some(next) = messages.get(i + 1) {
                        if next.role != "assistant" {
                            result.add_fix(format!(
                                "User message at {} followed by {}, expected Assistant",
                                i, next.role
                            ));
                        }
                    }
                }
            }
            "assistant" => {
                let has_tool_calls = msg.tool_calls.as_ref().map(|calls| !calls.is_empty()).unwrap_or(false);
                let has_content = msg.content.as_ref().map(|s| !s.is_empty()).unwrap_or(false);
                let has_reasoning = msg.reasoning_content.as_ref().map(|s| !s.is_empty()).unwrap_or(false);

                // 规则5: 只有包含 tool_calls 时，content 可以为空，否则不能为空
                if !has_tool_calls && !has_content && !has_reasoning {
                    result.add_fix(format!(
                        "Assistant message at {} has no content and no tool_calls, this is invalid",
                        i
                    ));
                }

                // 规则4: 不能以空 Assistant 消息结尾
                if i == messages.len() - 1 {
                    if !has_tool_calls && !has_content && !has_reasoning {
                        result.add_fix(format!(
                            "Assistant message at {} (last message) is empty, should be removed",
                            i
                        ));
                    } else {
                        result.add_fix(format!(
                            "Last message is Assistant at {}, should end with User or Tool",
                            i
                        ));
                    }
                    break;
                }

                if let Some(next) = messages.get(i + 1) {
                    if has_tool_calls {
                        // 规则7: Assistant带tool_calls，下一条必须是Tool
                        if next.role != "tool" {
                            result.add_fix(format!(
                                "Assistant with tool_calls at {} followed by {}, expected Tool",
                                i, next.role
                            ));
                        }

                        // 规则8: 验证 N 个 tool_calls 对应 N 个 tool 消息
                        if let Some(calls) = &msg.tool_calls {
                            let num_calls = calls.len();
                            let mut num_tools = 0;
                            let mut j = i + 1;

                            // 收集所有的 tool_call_id
                            let call_ids: Vec<String> = calls.iter().map(|c| c.id.clone()).collect();
                            let mut found_tool_ids: Vec<String> = Vec::new();

                            while j < messages.len() && messages[j].role == "tool" {
                                num_tools += 1;
                                if let Some(tool_call_id) = &messages[j].tool_call_id {
                                    found_tool_ids.push(tool_call_id.clone());

                                    // 规则6: 验证 tool_call_id 是否属于上一条 Assistant 的 tool_calls
                                    if !call_ids.contains(tool_call_id) {
                                        result.add_fix(format!(
                                            "Tool message at {} has tool_call_id='{}' which doesn't match any tool_calls in Assistant at {}",
                                            j, tool_call_id, i
                                        ));
                                    }
                                }
                                j += 1;
                            }

                            if num_tools != num_calls {
                                result.add_fix(format!(
                                    "Assistant at {} has {} tool_calls but followed by {} tool messages, counts must match",
                                    i, num_calls, num_tools
                                ));
                            }

                            // 验证所有 tool_call_id 都被使用
                            for call_id in &call_ids {
                                if !found_tool_ids.contains(call_id) {
                                    result.add_fix(format!(
                                        "Tool call '{}' in Assistant at {} has no corresponding Tool message",
                                        call_id, i
                                    ));
                                }
                            }

                            // 跳过已验证的 tool 消息
                            i = j - 1;
                        }
                    } else {
                        // Assistant不带tool_calls，下一条必须是User
                        if next.role != "user" {
                            result.add_fix(format!(
                                "Assistant without tool_calls at {} followed by {}, expected User",
                                i, next.role
                            ));
                        }
                    }
                }
            }
            "tool" => {
                // 规则6: Tool消息必须有tool_call_id
                if msg.tool_call_id.is_none() {
                    result.add_fix(format!(
                        "Tool message at {} missing tool_call_id",
                        i
                    ));
                }

                // Tool消息可以是最后一条（这是允许的）
                if i == messages.len() - 1 {
                    // 这是允许的，不需要报错
                } else if let Some(next) = messages.get(i + 1) {
                    // Tool后面可以是另一个Tool（多工具调用），或者是Assistant
                    if next.role != "tool" && next.role != "assistant" {
                        result.add_fix(format!(
                            "Tool message at {} followed by {}, expected Tool or Assistant",
                            i, next.role
                        ));
                    }
                }
            }
            _ => {
                result.add_fix(format!(
                    "Unknown role '{}' at position {}",
                    role, i
                ));
            }
        }

        i += 1;
    }
}

/// 消息载荷结构（用于验证）
#[derive(Debug, Clone, PartialEq)]
pub struct MessagePayload {
    pub role: String,
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ToolCallPayload>>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallPayload {
    pub id: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_message(role: &str, content: Option<&str>) -> MessagePayload {
        MessagePayload {
            role: role.to_string(),
            content: content.map(|s| s.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn test_merge_duplicate_users() {
        let mut messages = vec![
            create_message("user", Some("Hello")),
            create_message("user", Some(" World")),
            create_message("assistant", Some("Hi")),
        ];

        let result = validate_and_fix_messages(&mut messages);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, Some("Hello World".to_string()));
        assert!(result.was_fixed);
    }

    #[test]
    fn test_trim_trailing_empty_assistant() {
        let mut messages = vec![
            create_message("user", Some("Hello")),
            create_message("assistant", None),
        ];

        let result = validate_and_fix_messages(&mut messages);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert!(result.was_fixed);
    }

    #[test]
    fn test_valid_conversation() {
        let mut messages = vec![
            create_message("system", Some("You are a helpful assistant")),
            create_message("user", Some("Hello")),
            create_message("assistant", Some("Hi there!")),
            create_message("user", Some("How are you?")),
        ];

        let result = validate_and_fix_messages(&mut messages);

        assert_eq!(messages.len(), 4);
        assert!(!result.was_fixed);
    }

    #[test]
    fn test_tool_call_sequence() {
        let mut messages = vec![
            create_message("system", Some("You are a helpful assistant")),
            create_message("user", Some("List files")),
            MessagePayload {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                tool_calls: Some(vec![ToolCallPayload {
                    id: "tc1".to_string(),
                    function: ToolCallFunction {
                        name: "run_in_terminal".to_string(),
                        arguments: r#"{"command": "ls"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
            },
            MessagePayload {
                role: "tool".to_string(),
                content: Some("file1\nfile2".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];

        let result = validate_and_fix_messages(&mut messages);

        assert_eq!(messages.len(), 4);
        assert!(!result.was_fixed);
    }

    #[test]
    fn test_invalid_sequence_assistant_to_user() {
        let mut messages = vec![
            create_message("system", Some("You are a helpful assistant")),
            create_message("assistant", Some("Answer 1")),
            create_message("assistant", Some("Answer 2")),
            create_message("user", Some("Hello")),
        ];

        let result = validate_and_fix_messages(&mut messages);

        // 连续的两个Assistant应该被合并
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].content, Some("Answer 1Answer 2".to_string()));
        assert!(result.was_fixed);
    }
}
