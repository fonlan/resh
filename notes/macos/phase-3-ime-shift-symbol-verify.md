# Phase 3: 验证与收尾 — macOS 中文 IME Shift 上档符号

日期：2026-07-13  
范围：移除临时调试、回归清单、typecheck / 自检、独立审查

## 代码收尾

| 项 | 状态 |
|----|------|
| 移除 `localStorage.resh.debugImeKeys` 开关与 `[resh-ime-debug]` 监听 | **已完成**（`src/hooks/useTerminal.ts`；`src/` 内零命中） |
| 保留方案 A 注入 + composition 跟踪 + 40ms 去重 | **保留**（含 Phase 2 P2：不匹配/过期清 marker） |
| 保留 `src/utils/macOsImeShiftSymbol.ts` 映射与 load-time 自检 | **保留** |
| IME 特例注释 | **保留**（`useTerminal` + 映射模块头注释指向 phase-2 文档） |
| `npm run typecheck` | **通过** |
| 映射/守卫/去重 node 自检 | **通过**（21 个 `code` 映射；见下） |
| 独立 Review agent | **PASS**（无阻塞问题） |

历史 RCA（`phase-1-ime-shift-symbol-rca.md`）仍描述 phase-1 埋点用法，仅作根因文档；**生产代码已无调试开关**。phase-1 文中已注明埋点在 phase 3 移除。

## 自动化可验证（本 phase 已做）

1. **TypeScript**：`npm run typecheck`（`tsc --noEmit` 退出码 0）
2. **映射与守卫**：`assertMacOsImeShiftSymbolSelfCheck()`（模块加载时）+ 独立 node 脚本覆盖  
   - 229+Shift+Digit1 → `!`；Backquote → `~`；Minus → `_`  
   - 非 229 / 无 Shift / isComposing / meta / keyup / Shift 键 → `null`  
   - `imeComposing` 选项阻塞注入  
3. **去重逻辑（镜像 P2）**：紧邻匹配只丢一次；过期或不匹配清 marker，避免 `! → a → !` 误吞第二个 `!`
4. **残留搜索**：`src/` 无 `debugImeKeys` / `resh-ime-debug` / `logImeDebug` / `attachImeDebugListeners` / `imeDebugCleanup`
5. **composition 监听**：`compositionstart` / `compositionend` 挂载与 cleanup 配对完整

## 手工回归清单（真实 SSH 终端标签）

环境：macOS + 系统拼音（建议 ABC 键盘 / 半角标点）+ 已连接的 SSH 终端。

| # | 场景 | 期望 | 结果（发布前勾选） |
|---|------|------|-------------------|
| 1 | 中文 IME，**未组字**，Shift+1..0 | `!@#$%^&*()` | ☐ |
| 2 | 中文 IME，未组字，Shift+`-=[]\;',./` | `~_+{}|:"<>?` | ☐ |
| 3 | 中文 IME 组字：拼音 + 空格上屏 + 退格 | 正常候选/上屏，**不**被强行注入拉丁上档符 | ☐ |
| 4 | 英文 IME：字母数字 + Shift 符号 | 与系统一致，无双字符 | ☐ |
| 5 | macOS Cmd+C / Cmd+V / Cmd+A | 复制选区 / 粘贴 / 全选，不截获 Shift 符号 | ☐ |
| 6 | 分屏 ≥2 终端，切换焦点后继续输入 | 焦点终端可输入；组字状态不串台 | ☐ |
| 7 | 无 Shift 数字键 0–9 | 输出数字，非上档符 | ☐ |

### 本 agent 环境说明

- 宿主为 **Darwin arm64**，typecheck / 映射自检已在 isolated worktree 执行。  
- **完整 Tauri GUI + 中文 IME 物理打键**依赖人工在 App 窗口内操作；本 phase **不**伪称已完成表格全部勾选。  
- 若现场仍丢键：确认输入法是否「中文标点/全角」、键盘布局是否非 US（映射仅 US-QWERTY 半角）。

## 残余限制（ponytail，不阻塞本 phase）

- 仅 US 布局半角 ASCII 上档映射  
- 无法读取系统「中文标点」状态时优先半角  
- xterm 仍为 `xterm@5.3.0`；长期可评估 `@xterm/xterm`（方案 C）  
- 真实 GUI 回归为发版前人工一次勾选（上表）

## 结论

Phase 3 **代码与静态验收完成**：调试埋点清零、typecheck 与映射/去重自检通过、IME 特例注释保留、独立审查 PASS。  

**GUI 回归表**留给发版前人工一次勾选；逻辑与 Phase 1/2 RCA/实现文档一致。未提交 git（按计划由 Foco 在 worktree 提交）。
