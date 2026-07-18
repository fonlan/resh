import assert from "node:assert/strict"
import fs from "node:fs/promises"
import path from "node:path"
import vm from "node:vm"
import ts from "typescript"

const helperPath = path.resolve("src/components/ai/helpers.ts")
const helperSource = await fs.readFile(helperPath, "utf8")
const { outputText } = ts.transpileModule(helperSource, {
  compilerOptions: {
    module: ts.ModuleKind.CommonJS,
    target: ts.ScriptTarget.ES2020,
  },
  fileName: helperPath,
})

const module = { exports: {} }
vm.runInNewContext(outputText, { module, exports: module.exports }, {
  filename: helperPath,
})

const { isMatchingAiRequest, shouldGenerateTitleAfterRun } = module.exports

// Production path: AISidebar gates stream/terminal events via isMatchingAiRequest.
assert.equal(isMatchingAiRequest("req-a", "req-a"), true)
assert.equal(isMatchingAiRequest("req-a", "req-b"), false)
assert.equal(isMatchingAiRequest(null, "req-a"), false)
assert.equal(isMatchingAiRequest("req-a", null), false)
assert.equal(isMatchingAiRequest("req-a", ""), false)
assert.equal(isMatchingAiRequest(undefined, "req-a"), false)

// After local stop, activeRequestId is cleared — late done/error/cancelled must not apply.
assert.equal(isMatchingAiRequest(null, "req-a"), false)
assert.equal(isMatchingAiRequest("req-b", "req-a"), false)
assert.equal(isMatchingAiRequest("req-a", "req-a"), true)

// Immediate resend: new active id rejects stale terminal events from previous run.
const activeAfterResend = "req-b"
for (const stale of ["req-a", null, "", undefined]) {
  assert.equal(
    isMatchingAiRequest(activeAfterResend, stale),
    false,
    `stale event requestId=${String(stale)} must not match active ${activeAfterResend}`,
  )
}
assert.equal(isMatchingAiRequest(activeAfterResend, "req-b"), true)

// Cancel / error never generate title; only normal done on "New Chat"
assert.equal(shouldGenerateTitleAfterRun("cancelled", "New Chat"), false)
assert.equal(shouldGenerateTitleAfterRun("error", "New Chat"), false)
assert.equal(shouldGenerateTitleAfterRun("pending_tools", "New Chat"), false)
assert.equal(shouldGenerateTitleAfterRun("done", "New Chat"), true)
assert.equal(shouldGenerateTitleAfterRun("done", "Existing title"), false)
assert.equal(shouldGenerateTitleAfterRun("done", null), false)

// Ensure the sidebar production gate is still the exported matcher.
const sidebarSource = await fs.readFile(
  path.resolve("src/components/AISidebar.tsx"),
  "utf8",
)
const turnSource = await fs.readFile(
  path.resolve("src-tauri/src/commands/ai/turn.rs"),
  "utf8",
)
const aiCommandSource = await fs.readFile(
  path.resolve("src-tauri/src/commands/ai/mod.rs"),
  "utf8",
)
assert.match(
  sidebarSource,
  /isMatchingAiRequest\(active,\s*requestId\)/,
  "AISidebar must gate events with isMatchingAiRequest",
)
assert.doesNotMatch(
  sidebarSource,
  /shouldAcceptAiTerminalEvent/,
  "unused terminal helper must not reappear in AISidebar",
)
// Terminal event listeners must exist for cancel as a normal terminal.
assert.match(sidebarSource, /ai-cancelled-\$\{sessionId\}/)
assert.match(sidebarSource, /ai-done-\$\{sessionId\}/)
assert.match(sidebarSource, /ai-error-\$\{sessionId\}/)
// Local stop must clear generating without waiting for cancel IPC.
assert.match(sidebarSource, /cancelRunLocally\(/)
assert.match(sidebarSource, /handleStopGeneration/)
assert.match(sidebarSource, /void aiService\.cancelMessage/)

// Rollback mode must not revive the recursive tool path or skip durable terminal cleanup.
assert.match(turnSource, /RESH_AI_AGENT_REACT_LOOP/)
assert.match(turnSource, /LegacySingleTurn/)
assert.match(turnSource, /run_legacy_single_turn/)
assert.match(turnSource, /finish_persisted_run\(&state, &context, &result\)\.await/)
assert.match(turnSource, /legacy_rollout_blocks_approval_resume/)
assert.match(turnSource, /persist_proposed_tools\(&state, &context, turn_index, &calls\)\.await\?;/)
const executeAgentToolsStart = aiCommandSource.indexOf("pub async fn execute_agent_tools")
const legacyApprovalGate = aiCommandSource.indexOf(
  "legacy_rollout_blocks_approval_resume",
  executeAgentToolsStart,
)
const persistedApprovalClaim = aiCommandSource.indexOf(
  "resume_agent_run(",
  executeAgentToolsStart,
)
assert.ok(
  legacyApprovalGate >= 0 && legacyApprovalGate < persistedApprovalClaim,
  "legacy rollback must reject approvals before claiming persisted invocation state",
)

console.log("AI cancel isolation checks passed")
