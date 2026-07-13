# Phase 1: macOS 中文 IME Shift 上档符号丢键 — 复现与根因分析

日期：2026-07-13  
范围：Resh 终端（xterm@5.3.0 + `useTerminal`）  
平台：macOS（Tauri WebView）

> **证据分级（Phase 1 诚实边界）**
>
> | 级别 | 含义 | 本阶段状态 |
> |------|------|------------|
> | **实测** | 在本机/CI 捕获的 `[resh-ime-debug]` 原始日志 | **未附**（本 agent 环境无 macOS GUI + 无 `node_modules`，无法现场打键） |
> | **代码确认** | 对 Resh / xterm 源码路径的静态对照 | **已完成** |
> | **推断** | 由代码路径 + 已知 IME 行为推出的最可能失败链 | **标为推断**，供 Phase 2 落地与 Phase 3 实测验收 |
>
> Phase 1 验收中「有明确失败用例 / 英文对照」在文档层以**可执行复现手册 + 日志判定标准**交付；**真正把日志贴进仓库**需人工在 macOS 开 `resh.debugImeKeys=1` 后粘贴一次，或在 Phase 3 回归时补录。当前**不**把「已 macOS 实测确认根因」写成既成事实。

路径说明：`.gitignore` 忽略整个 `docs/`，故本 RCA 放在可被 git 跟踪的 `notes/macos/`（非 `docs/`）。

---

## 1. 复现矩阵

### 1.1 环境记录（待实测填写）

| 项 | 值 | 证据级别 |
|----|-----|----------|
| 受影响平台 | macOS（Tauri WebView / Chromium 系） | 问题报告 + 代码路径指向 IME/WebView |
| 对照平台 Windows | **未在本阶段实测**；计划在有 Windows 机时对照中文 IME Shift+1 是否出 `!` | 待测 |
| 键盘布局 | 建议记录：ABC / ABC-扩展；数字行 `1-0` 与 `-=` 是否 US shift | 复现时填写 |
| 系统拼音 | 建议：简体拼音；「在键盘上直接输入标点」开/关 | 复现时填写 |
| 候选窗 | 未组字无候选；组字中有拼音候选 | 复现时填写 |
| Resh 快捷键 | 仅 `metaKey` + c/v/a，**不碰** Shift 符号 | **代码确认**（见 2.5） |

### 1.2 行为对比（失败定义 + 期望对照）

| # | 场景 | 操作示例 | 预期字符 | 判定「失败」的标准 | 英文 IME 期望 |
|---|------|----------|----------|-------------------|---------------|
| A | 英文输入法 | Shift+1 / 2 / \` / - / = | `!` `@` `~` `_` `+` | — | **有 onData** 对应字符 |
| B | 中文 IME，**未组字** | 同上 | 同上 | **无输出** 且无 onData | — |
| C | 中文 IME，组字中 | 先拼音再 Shift+1 | 依 IME | 是否进 composition*；与 B 区分 | — |
| D | 切回英文 | 同 A | 同 A | 应恢复 | 正常 |
| E | 无 Shift 数字 | `1` `2` | `1` `2` | 应仍有输出（可能走 229+diff 成功） | 正常 |

**明确失败用例（验收定义，待实测贴日志）**：

- macOS + 中文 IME（至少系统简体拼音）+ **未组字** + `Shift+1` → 终端无 `!`，`onData` 无 `!`。

**英文对照（验收定义）**：

- 同一终端切到 ABC（或英文）后 `Shift+1` → 有 `!` / 有 `onData: "!"`。

### 1.3 如何用埋点现场确认（可执行）

1. DevTools Console：
   ```js
   localStorage.setItem('resh.debugImeKeys', '1')
   ```
2. 刷新 / 重开终端标签（应看到 `listeners-attached`）。
3. 分别在英文 IME / 中文 IME 下按 `Shift+1`。
4. 观察前缀 `[resh-ime-debug]`。

**判定路径 A（229 + textarea-diff 无变化）— 日志标准**：

```
customKey / textarea.keydown:
  keyCode: 229
  key: "Process" | "Unidentified" | 偶发 "!"
  code: "Digit1"
  shiftKey: true
  isComposing: false
  path: "likely-xterm-composition-helper-229"
textarea.229-post-timeout-snapshot:
  unchanged: true
  grew: false
  diff: ""
textarea.input / beforeinput: 无有效 insertText data（或未出现）
composition*: 通常无 compositionstart
onData: 无 "!"
```

**英文 IME 正常 — 日志标准**：

```
keyCode: 非 229（例如 49）
key: "!"
code: "Digit1"
shiftKey: true
onData: "!"
（通常无 229-post-timeout-snapshot，或 snapshot 非丢键主路径）
```

**路径 B**：出现 `compositionstart` / `isComposing: true` 后再 keyup/finalize → 与「未组字丢上档符」主诉不同。  
**路径 C**：keydown `defaultPrevented` 且无 input/keypress 字符。

埋点：`src/hooks/useTerminal.ts`（`localStorage.resh.debugImeKeys === '1'`；phase 3 移除）。  
关闭调试时：`onData` **不**构造日志载荷；监听器不挂载。

### 1.4 实测日志粘贴区（人工 / Phase 3）

```
（在此粘贴一次中文 IME Shift+1 完整 [resh-ime-debug] 序列）
```

```
（在此粘贴一次英文 IME Shift+1 完整序列）
```

---

## 2. 事件路径与 xterm 5.3.0 对照（代码确认）

### 2.1 调用链（keyDown）

```
textarea keydown
  → Terminal._keyDown
      → customKeyEventHandler  (Resh: 仅 meta+c/v/a 拦截；Shift 符号 return true)
      → CompositionHelper.keydown
          · composing 中 + keyCode 229 → return false（继续组字）
          · 非 composing + keyCode 229 → _handleAnyTextareaChanges()；return false
          · 否则 return true
      → 若 CompositionHelper 返回 false：_keyDown 提前 return false
        （不再走 evaluateKeyboardEvent / triggerDataEvent）
      → 否则 evaluateKeyboardEvent → 可能 triggerDataEvent(result.key)
```

### 2.2 CompositionHelper 关键逻辑（xterm 5.3.0 语义）

```ts
// 非组字 + 229：依赖 hidden textarea 后续变化 diff 猜字符
if (ev.keyCode === 229) {
  this._handleAnyTextareaChanges();
  return false;
}

private _handleAnyTextareaChanges(): void {
  const oldValue = this._textarea.value;
  setTimeout(() => {
    if (!this._isComposing) {
      const newValue = this._textarea.value;
      const diff = newValue.replace(oldValue, '');
      if (newValue.length > oldValue.length) {
        this._coreService.triggerDataEvent(diff, true);
      } else if (/* 变短 / 等长但内容变 */) { /* DEL 或整值 */ }
      // 若 newValue === oldValue → 无任何 triggerDataEvent → 丢键
    }
  }, 0);
}
```

埋点用 `textarea.229-post-timeout-snapshot` **近似** 该 setTimeout(0) 前后值（同 0 延迟），用于支持/反驳「diff 无变化」；**不是** hook 进 xterm 私有方法，故仍标为**推断支持证据**，而非内部状态证明。

### 2.3 主因假设（路径 A）— 推断，待实测日志验证

1. macOS 中文 IME 对 Shift+符号常上报 **`keyCode === 229`**，且 **`isComposing === false`**。  
2. xterm 因此走 `_handleAnyTextareaChanges`，**完全依赖** hidden textarea 是否变长。  
3. 若 IME **不写入** textarea、也不发可靠 `input`，则 `oldValue === newValue`。  
4. 结果：无 `triggerDataEvent` → 终端无输出。

### 2.4 正常键路径（英文 IME）

`evaluateKeyboardEvent` default 分支（`Keyboard.ts` 语义）：

```ts
} else if (ev.key && !ev.ctrlKey && !ev.altKey && !ev.metaKey
           && ev.keyCode >= 48 && ev.key.length === 1) {
  result.key = ev.key;  // 例如 "!"
}
```

- 英文 IME 下 `keyCode` 非 229、`key === "!"` 时可直接 `triggerDataEvent`。  
- **`keyCode >= 48` 对 186（`;`）等也成立**；是否进入该分支还取决于 `ev.key` 长度、修饰键等，**不能**写成「186 不满足 `>= 48`」。  
- **229 时 CompositionHelper 已 `return false`**，根本进不了该分支。

### 2.5 `_inputEvent` 兜底有限

仅在 `insertText` 且 `(!composed || !_keyDownSeen)` 等条件触发。有 keydown 且 composed 时，IME 直出常被跳过；无 `input`/`data` 则无兜底。

### 2.6 Resh 自有逻辑排除（代码确认）

`useTerminal` 中 `attachCustomKeyEventHandler`：

- 条件：`isMacOS() && metaKey && !ctrl && !alt`，仅处理 `c` / `v` / `a`。  
- Shift+符号：`metaKey` 为 false → **立即 `return true`**，不拦截。  
- 埋点 `customKey.path`：`keyCode===229` → `likely-xterm-composition-helper-229`。

---

## 3. 结论（供 Phase 2）

| 问题 | 结论 | 证据级别 |
|------|------|----------|
| 失败用例定义 | macOS 中文 IME 未组字 + Shift+数字/符号 → 无上档字符 | 验收定义；实测待贴 |
| 英文对照定义 | 切英文 IME 后应正常 | 验收定义；实测待贴 |
| 最可能失败点 | xterm CompositionHelper **229 + textarea-diff 无变化**（路径 A） | **推断** + 代码路径确认 |
| 非 Resh meta | handler 不碰 Shift 符号 | **代码确认** |
| 推荐修复 | 方案 A：App 层 macOS + 229 + shift + `code` 可映射且未真正 composing 时补发单字符并防重 | Phase 2 |

---

## 4. 本阶段交付物

1. 本文档：`notes/macos/phase-1-ime-shift-symbol-rca.md`（可提交；非 `docs/`）。  
2. 临时埋点：`src/hooks/useTerminal.ts`  
   - `localStorage.resh.debugImeKeys === '1'` 才挂监听 / 打日志  
   - `onData` 调试载荷仅在开关开启时构造  
   - `beforeinput`/`input` 标签为 `textarea.${ev.type}`  
   - keyCode 229 后 `setTimeout(0)` 快照 textarea old/new/diff  
3. **不改**业务输入逻辑；Phase 2 再落地补丁。  
4. 关闭调试：无额外 DOM 监听；`onData` 无调试分配。
