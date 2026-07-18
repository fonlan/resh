# AI Agent ReAct — 渐进发布与端到端验收清单

本清单覆盖 ReAct 运行时发布前的自动化验证、真实渠道手工矩阵、SSH 兼容性与回退操作。真实 API 凭据不进入 CI；CI 只运行 faux provider / 纯状态机测试和静态门禁。

## 自动化门禁

在仓库根目录执行：

```bash
npm run test:ai-agent-loop
npm run check:ai-cancel
npm run check:ai-tool-confirmation
cargo test --manifest-path src-tauri/Cargo.toml --lib commands::ai -- --nocapture
npm run typecheck
git diff --check
```

`test:ai-agent-loop` 不启动 Tauri、不访问网络，也不连接 SSH。它使用脚本化 faux provider 验证：

- 自然完成和单一 terminal event。
- 连续只读工具与每批次屏障。
- 混合只读/待审批批次，下一模型轮必须等待审批调用收敛。
- 拒绝、取消、模型轮次预算耗尽。
- 相同工具与相同参数超过限制。
- 崩溃恢复将未完成副作用调用标为 `interrupted`，不自动重放。

## 渐进发布与回退

默认值为新 ReAct 循环。短期内部 kill switch 为环境变量：

```bash
RESH_AI_AGENT_REACT_LOOP=legacy npm run tauri-dev
```

`legacy`、`0` 或 `false` 启用安全的单轮回退模式：仍注册 requestId、仍写入 run 的终态、仍支持取消，但不向 provider 声明工具，也不恢复已删除的递归工具执行路径。既有待审批项只能在恢复默认 ReAct 模式后批准；拒绝或取消会安全地写入对应终态，不会执行工具。它用于在跨渠道问题出现时先停用 Agent 工具循环。

回退后核对：

- 普通无工具对话可完成，`ai-done-*` 只出现一次。
- 点击停止后仍收到 `ai-cancelled-*`，不会把旧事件写入下一 request。
- 模型没有收到工具定义；若异常返回 tool call，每个调用都有拒绝的 terminal observation，run 受控失败且不会执行副作用。
- 取消环境变量或设置为任意非上述值后恢复 ReAct。

旧会话兼容检查：打开在 `ai_messages` 尚无 `run_id` / `turn_index` 的历史会话，确认消息可以只读加载且不会自动创建、执行或重放旧 tool call。历史查询仅按 `session_id` / 时间读取消息，运行投影缺失时应显示普通历史而非伪造待审批状态。

## 真实 Provider 手工矩阵

前置条件：使用独立测试会话和非生产 SSH 主机；每个渠道使用其自己的可用模型与凭据。开始每个场景前记录 channel、model、requestId 和 runId；结束后记录 HTTP 状态、终态事件、数据库 run/invocation 状态及 UI 现象。

| 场景 | OpenAI-compatible | GitHub Copilot | Anthropic Message | 通过条件 |
|---|---:|---:|---:|---|
| 无工具回答 | □ | □ | □ | 回复完成，恰好一个 `done`，无待审批项 |
| 两个只读工具 | □ | □ | □ | 两个 invocation 都完成；并行安全工具的结果按模型原始 call 顺序回传 |
| 工具确认 | □ | □ | □ | `awaitingApproval` 可恢复；批准一次后从同一 run 继续且预算未重置 |
| 拒绝工具 | □ | □ | □ | 每个被拒调用都有结构化 terminal result；模型可收到结果并继续/完成 |
| 取消 | □ | □ | □ | UI 立即停止，后端为匹配 request 发送 `cancelled`，无晚到输出污染新请求 |
| 压缩后续轮 | □ | □ | □ | 长对话/大 observation 后仍可继续；完整 tool batch 不被拆散 |

渠道特定核对：

- Anthropic：同一 assistant tool-use turn 的全部 `tool_result` 位于紧邻的一条 user message，且在文本前；确认 API 不返回 HTTP 400。
- OpenAI-compatible：对不声明 reasoning replay 能力的 endpoint，确认请求历史不含 reasoning / thought signature 字段，避免兼容端拒绝。
- Copilot：确认工具调用 ID 与工具结果匹配；取消后的旧 request 不关闭新 run。

## 真实 SSH 兼容性

在测试 SSH 服务器上完成以下场景，避免使用生产路径或不可逆命令：

1. 用 `read_file` 和终端快照触发连续只读工具；确认工具批次和运行预算正常累计。
2. 用受限、可观察的长命令（例如 sleep）触发 `run_in_terminal` 超时；确认超时恢复/重连路径仍可用，失败结果有 terminal outcome。
3. 在模型流、只读 SFTP、前台终端命令各阶段取消一次；确认 UI 可立即重发，未完成普通调用被取消，已脱离 run 的后台任务不会被错误取消。
4. 通过重复相同 read-only call 和多轮工具调用触达测试预算边界；确认不再请求下一模型轮，run 状态为 `budgetExceeded`。
5. 在待审批副作用工具出现后退出并重新启动应用；确认启动恢复只显示/保留权威待审批状态，不自动执行副作用。

## 第一阶段明确非目标

本次发布不引入以下能力，也不应在验收中以任何形式自动启用：

- 子 Agent、任务树或 agent-to-agent 委派。
- 插件市场、通用 hook 配置 UI。
- 模型自动审批 reviewer。
- 将真实 provider 凭据、SSH 地址或敏感 observation 写入测试日志。

## 结果记录

| 日期 | 构建版本 | 渠道/模型 | SSH 场景 | 自动化门禁 | 手工矩阵 | 回退演练 | 备注 |
|---|---|---|---|---|---|---|---|
| | | | | | | | |
