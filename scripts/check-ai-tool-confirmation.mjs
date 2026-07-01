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

const toolCall = (name, args = {}) => ({
  id: `${name}-call`,
  type: "function",
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

const ordinaryCommand = [toolCall("run_in_terminal", { command: "pwd" })]
assert.equal(shouldExecuteToolCallsWithoutConfirmation(ordinaryCommand, 0), true)
assert.equal(shouldExecuteToolCallsWithoutConfirmation(ordinaryCommand, 5), false)
assert.equal(shouldExecuteToolCallsWithoutConfirmation(ordinaryCommand, 30), false)

const sensitiveCommand = [toolCall("run_in_terminal", { command: "rm -rf /tmp/x" })]
assert.equal(shouldExecuteToolCallsWithoutConfirmation(sensitiveCommand, 0), false)

const readOnlyTool = [toolCall("read_file", { path: "README.md" })]
assert.equal(shouldExecuteToolCallsWithoutConfirmation(readOnlyTool, 30), true)

console.log("AI tool confirmation checks passed")
