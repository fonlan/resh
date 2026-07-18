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
vm.runInNewContext(outputText, { module, exports: module.exports }, { filename: helperPath })

const {
  clampAiToolConfirmationCountdown,
  shouldExecuteToolCallsWithoutConfirmation,
} = module.exports
const confirmationSource = await fs.readFile(
  path.resolve("src/components/ai/ToolConfirmation.tsx"),
  "utf8",
)

const toolCall = (name, args = {}, approvalPolicy = "Countdown") => ({
  id: `${name}-call`,
  type: "function",
  approval_policy: approvalPolicy,
  function: {
    name,
    arguments: JSON.stringify(args),
  },
})

assert.equal(clampAiToolConfirmationCountdown(0), 0)
assert.equal(clampAiToolConfirmationCountdown(5), 5)
assert.equal(clampAiToolConfirmationCountdown(30), 30)
assert.equal(clampAiToolConfirmationCountdown(-1), 0)
assert.equal(clampAiToolConfirmationCountdown(31), 30)

const terminalCommand = [
  toolCall("run_in_terminal", { command: "pwd" }, "AlwaysAsk"),
]
assert.equal(shouldExecuteToolCallsWithoutConfirmation(terminalCommand, 0), false)
assert.equal(shouldExecuteToolCallsWithoutConfirmation(terminalCommand, 5), false)

const countdownMutation = [
  toolCall("sftp_upload", { remote_path: "/tmp/test" }, "Countdown"),
]
assert.equal(shouldExecuteToolCallsWithoutConfirmation(countdownMutation, 0), true)
assert.equal(shouldExecuteToolCallsWithoutConfirmation(countdownMutation, 5), false)
assert.equal(shouldExecuteToolCallsWithoutConfirmation(countdownMutation, 30), false)

const sensitiveCommand = [
  toolCall("run_in_terminal", { command: "rm -rf /tmp/x" }, "AlwaysAsk"),
]
assert.equal(shouldExecuteToolCallsWithoutConfirmation(sensitiveCommand, 0), false)

const readOnlyTool = [toolCall("read_file", { path: "README.md" }, "Auto")]
assert.equal(shouldExecuteToolCallsWithoutConfirmation(readOnlyTool, 0), false)

// The confirmation UI is a projection of backend policy: users can decline/cancel,
// session grants stay restricted to Countdown calls, and AlwaysAsk never auto-confirms.
assert.match(confirmationSource, /onCancelRun/)
assert.match(confirmationSource, /onDecline/)
assert.match(confirmationSource, /onApproveForSession/)
assert.match(confirmationSource, /approval_policy === "Countdown"/)
assert.match(confirmationSource, /approval_policy === "AlwaysAsk"/)

console.log("AI tool confirmation checks passed")
