# Phase 2: macOS 中文 IME Shift 上档符号 — 方案 A 实现

日期：2026-07-13  
范围：App 层最小补丁（不升级 xterm）

## 方案选择

采用 **方案 A**：macOS + `keyCode === 229` + Shift + 可映射 `event.code` 时，在 `attachCustomKeyEventHandler` 中直写字符并 `return false`，跳过 xterm CompositionHelper 的 textarea-diff 丢键路径。

未做方案 B（改 CompositionHelper 边界）与方案 C（升级 `@xterm/xterm`）。

## 代码

| 文件 | 作用 |
|------|------|
| `src/utils/macOsImeShiftSymbol.ts` | US-QWERTY `code`→上档符映射；`resolveMacOsImeDroppedShiftSymbol` 守卫；`assertMacOsImeShiftSymbolSelfCheck` |
| `src/hooks/useTerminal.ts` | macOS 注入 + composition 跟踪 + 40ms onData 去重；保留 phase-1 调试埋点 |

## 守卫与防重

1. 仅 `isMacOS()` + `keydown` + `keyCode === 229` + `shiftKey` 且无 meta/ctrl/alt  
2. `isComposing` 或 textarea `compositionstart…end` 活跃时不注入（真组字不碰）  
3. 映射只含常见上档符（`!@#$%^&*()_+{}|:"<>?~` 等），无 Shift / 非 229 / 字母键不改  
4. 注入走 `onDataRef`（不走 `term.paste`，避免自去重）；`return false` 阻止 xterm 再处理  
5. 若 IME 稍后仍经 textarea/input 产生同一字符，`onData` 在 40ms 内丢弃**紧邻的下一次匹配**重复  
6. **Review P2 修复**：`shouldDropDuplicateOnData` 在过期、字符不匹配时都会清除 marker，避免 40ms 窗口内「补发 `!` → `a` → `!`」误吞第二个合法 `!`

## 限制（ponytail）

- 按 **US 布局** 映射；非 US 键盘上档符需后续按布局扩展  
- 全角标点与系统「中文标点」状态无法读取时，优先半角 ASCII  
- Phase 1 调试开关仍在；Phase 3 回归后移除

## 验证（本阶段）

- `assertMacOsImeShiftSymbolSelfCheck()` 模块加载时自检  
- `npm run typecheck`  
- 真实 macOS 中文 IME 打键：Phase 3
