# AI 流式取消 — 端到端回归清单

本清单配合 Phase 1–3 的 requestId 级取消生命周期，用于真实 Tauri UI 或可控慢速 mock 验收。
路径放在 `scripts/checklists/`（非 `docs/`，避免被 `.gitignore` 忽略）。

完整的 ReAct faux provider、跨渠道、SSH、回退与非目标验收见 [ai-agent-react-e2e-checklist.md](./ai-agent-react-e2e-checklist.md)。

自动化覆盖（无需 UI）：

```bash
cd src-tauri && cargo test --lib ai_cancel_race_async_tests -- --nocapture
cd src-tauri && cargo test --lib commands::config::tests -- --nocapture
node scripts/check-ai-cancel-isolation.mjs
node scripts/check-ai-tool-confirmation.mjs
npm run typecheck
npm run build
```

自动化已覆盖：

- `AiRunRegistry` 同 session 原子替换、旧 guard 不删新 token、匹配/不匹配 cancel、并发 register 压力
- 慢速 mock stream：正文/reasoning/tool 混流中途取消且**不** flush 尾部；首包等待取消；停止后立即重发（旧流不污染新 request）
- 工具 await 可中断：`await_or_cancel` 对阻塞 future 在 token 取消后立即返回，不等待 SFTP/历史/SSH 自然完成
- 前端 `isMatchingAiRequest` 门控 + 标题仅在正常 `done` + `New Chat` 时生成
- `execute_agent_tools` 的 session bind / `load_history` 与只读工具、终端命令轮询路径已 cancellation-first
- 终端取消路径对 `stop_command_recording` 使用 fire-and-forget，避免 SSH 清理阻塞 AI task 退出
- 取消与“无待执行工具”竞态：`plan_ai_run_finish_with_token` 在 token 已取消时优先 `ai-cancelled-*`，不 emit 裸 `ai-done`
真实 Tauri UI / 多渠道协议仍须手工走下方矩阵。

## 前置

1. `npm run tauri-dev` 启动应用。
2. 配置至少一个可用流式模型（OpenAI 兼容 / GitHub Copilot / Anthropic Message 各至少验证一条路径；无真实 key 时可用兼容 mock endpoint 慢速 SSE）。
3. 打开 AI 侧边栏，新建会话（标题为 `New Chat`）。

## 场景矩阵

| # | 场景 | 操作 | 期望 |
|---|------|------|------|
| 1 | 流式正文中途停止 | 发送长回复提示 → 生成中点击停止 | UI **立即**退出 generating；之后无新字符；不弹 CANCELLED 错误；**不**自动生成标题 |
| 2 | reasoning 中途停止 | 使用支持 thinking 的模型 → 思考流中点击停止 | 同 #1；之后无新 reasoning 块 |
| 3 | 首包等待停止 | 对极慢/人工阻塞 endpoint 发请求 → 首 token 前停止 | UI 立即停止；后端在下一调度点退出；registry 无残留 |
| 4 | 自动只读工具 | 触发 `read_file` 自动执行中途停止 | 停止后无后续工具结果注入当前气泡；可立刻重发 |
| 5 | 待确认工具 | 出现 `ToolConfirmation` 时点停止（若仍 generating）或取消工具 | 不误发标题；不把旧 tool batch 写入新请求 |
| 6 | 终端命令执行 | `run_in_terminal` 长时间命令中停止 | 命令可中断或不再回传后续输出到当前 run；UI 已非 generating |
| 7 | 停止后立即重发 | #1 后立刻再发一条 | 新 request 正常完成；旧 `done`/`error`/`cancelled` **不**终止新请求 |
| 8 | 切换会话 | 生成中切换到另一会话再回来 | 旧会话迟到事件不污染新会话；各 session 独立 activeRequestId |
| 9 | 正常完成标题 | 不点停止，等流结束 | 标题从 `New Chat` 变为生成标题 |
| 10 | 渠道覆盖 | 对 OpenAI 兼容、Copilot、Anthropic 各跑 #1 + #7 | 协议差异下取消语义一致 |

## 时延建议目标

- **UI**：点击停止 → generating 关闭 ≈ 同步（同一事件循环）。
- **后端**：点击停止 → 日志出现 `cancel requested` / `run cancelled` 且 token 唤醒后，任务在下一次 `select!`/await 点退出；建议 < 1s（网络挂起时以取消 token 唤醒为准，不依赖 HTTP 读超时）。

## 日志核对

取消成功时预期日志形态（级别 info/debug）：

- `[AI] cancel requested session_id=… request_id=…`
- `[AI] run cancelled session_id=… request_id=…`

不匹配 requestId 的取消应为 debug：`cancel ignored (no matching run)`。

registry：同 session 新请求启动后旧 token 已 cancel；旧 guard Drop 后 `current_request_id` 仍为新 request；运行结束 `clear_if_matches` 后该 session 无条目。

## 结果记录（手工）

| 场景 | 渠道 | 通过 | 停止→最后 accepted event | 备注 |
|------|------|------|--------------------------|------|
| 1 正文 | | | | |
| 2 reasoning | | | | |
| 3 首包 | | | | |
| 4 只读工具 | | | | |
| 5 待确认工具 | | | | |
| 6 终端命令 | | | | |
| 7 立即重发 | | | | |
| 8 切换会话 | | | | |
| 9 正常标题 | | | | |
| 10 多渠道 | | | | |

真实 Tauri 界面验收需在本机有配置的模型/凭据时完成；CI 以 `cargo test --lib` 与 `check-ai-cancel-isolation.mjs` 为主。
