# Phase 4: macOS 中文 IME 空格双输入 — 跨路径去重

日期：2026-09-05  
范围：`useTerminal` 输入交付层（承接 `19bcc6a` 的 beforeinput 同步接管）

## 现象

macOS + 中文输入法激活时，终端里**每按一次空格上屏两个空格**（确定性，不是偶发竞态）。

## 根因

一次物理按键可以经**两条独立路径**抵达 SSH 会话：

| 路径（`InputDeliveryPath`） | 来源 |
|---|---|
| `beforeinput` | 我们在 helper textarea 上同步接管 IME 文本插入（Scheme B，见 `19bcc6a`） |
| `terminal` | xterm 自己的 keydown / keypress / textarea-diff / paste，经 `Terminal.onData` |
| `manual` | 我们自己直发（Scheme A 上档符、Scheme C 229-Backspace） |

IME 激活时两条路径**同时**为同一次按键交付：

1. keydown 上报 `keyCode 229`（Process）→ Scheme B `return false`，xterm 不再调度 diff；
2. IME 把字符写进 helper textarea → 我们的 `beforeinput` 交付该字符；
3. 但**空格的 keypress 仍带 `charCode 32`**（不像字母在 229 下 charCode 为 0），xterm `_keyPress`（`Terminal.ts:1100`）于是也 `triggerDataEvent(" ")` → 第二个空格。

`19bcc6a` 之前的旧去重只覆盖「注入后 xterm 经 textarea 再发一次」，方向单一（`markImeInjected` → 丢弃**紧邻的下一次 `onData`**），无法同时容纳 beforeinput 与 keypress 两侧，于是空格稳定翻倍。

## 修复

新增 `src/utils/imeInputDelivery.ts`：按**路径**标记的短窗去重，`useTerminal` 所有交付点（`onData` 包装、`beforeinput`、Scheme A/C 直发）统一走 `deliverInput(data, path)`。

规则（`createInputDeliveryTracker`，窗口 30ms）：

- 仅当**同一文本**来自**不同路径**且在上一次交付后 30ms 内 → 丢弃（并消费标记，使随后同一字符的合法重复仍能通过）；
- **同一路径的连续交付永不吞**（用户快速连按同一键、按键自动重复都不受影响）；
- 空数据一律不交付。

30ms 依据：两条路径同源于一次 DOM 事件，实际间隔只有几毫秒；最快的人手同键重复（含 macOS 最快自动重复 ≈15ms）远大于此。

## 验证

| 项 | 结果 |
|----|------|
| `npm run typecheck` | 通过 |
| `npm run build` | 通过 |
| `assertInputDeliveryTrackerSelfCheck()`（模块加载 + `node --experimental-strip-types`） | 通过（空格两条路径正反顺序、同路径快重复、异文本、超窗、manual→beforeinput、空数据） |
| 手工回归：中文 IME 未组字按空格 | 待真机确认 → 期望 1 个空格 |
| 手工回归：英文输入法字母/数字/符号、快速连打 | 待真机确认 → 与修复前一致 |
| 手工回归：中文 IME 组字 + 空格上屏候选 | 待真机确认 → 正常上屏（`isComposing` 时 beforeinput 直接返回，不参与去重） |

## 限制（ponytail）

- 去重是**跨路径**的启发式：若某引擎对同一次按键在**同一条路径**上交付两次（例如 diff 与 `_inputEvent` 同属 `terminal`），本机制不覆盖；当前结构下该组合不可达，因为 229 字符键的 keydown 已被 Scheme B 阻断。
- 窗口常量与路径枚举集中在 `src/utils/imeInputDelivery.ts`，若新增交付路径需一并登记 `InputDeliveryPath`。
