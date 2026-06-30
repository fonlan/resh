# macOS 阶段 3 自动化实现记录

- 日期：2026-06-30
- 状态：自动化测试入口已实现；手工验收矩阵和测试环境验收待执行

## 已实现

- 新增 `.github/workflows/macos-ci.yml`，在 `macos-15` 和 `macos-15-intel` 上运行前端类型检查、前端生产构建、Rust 单元测试、SFTP PowerShell 入口检查、macOS `.app` 构建和启动可见性 smoke test。
- 新增 `npm run typecheck`，并将 `npm run build` 拆为类型检查加 Vite production build。
- 将 SFTP 性能脚本入口迁移到 `scripts/run-pwsh.mjs`，自动选择 `pwsh` 或 Windows PowerShell；CI 会在缺失时安装 PowerShell。
- 新增 `scripts/check-pwsh-sftp-entry.mjs`，用于验证 SFTP performance suite PowerShell 参数绑定可用。
- 新增 `scripts/macos_tauri_smoke.mjs`，构建后启动 `.app`，通过 CoreGraphics 检查主窗口已出现在屏幕上，并在结束时关闭应用。
- 新增 `backend_smoke_check` Tauri command，作为稳定的后端命令 smoke surface。
- 抽出 app data dir 解析逻辑到 `src-tauri/src/app_paths.rs`，并补充 macOS app data dir、路径分隔符、Unicode 文件名/配置内容回归测试。
- 补充 SFTP 编辑相关文本编码、二进制拒绝、远端 Unicode 文件名临时路径回归测试。

## 本机验证

2026-06-30 已通过：

- `npm run typecheck`
- `npm run build`
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`：37 项通过，1 项文档测试忽略。
- `npm run tauri-build -- --config src-tauri/tauri.macos.conf.json --bundles app`
- `npm run smoke:macos -- --skip-build`
- `node --check scripts/run-pwsh.mjs`
- `node --check scripts/check-pwsh-sftp-entry.mjs`
- `node --check scripts/macos_tauri_smoke.mjs`

## 待执行

- 本机未安装 PowerShell，未在本机执行 `npm run sftp:perf-suite:check`；该检查已放入 macOS CI，并在 CI 中确保 `pwsh` 可用。
- 阶段 3 手工验收矩阵仍待执行，包括 SSH 登录、代理、端口转发、SFTP 全矩阵、WebDAV、AI、WebGL 终端渲染和宽字符显示。
- 测试环境验收仍待执行，包括最低 macOS 版本、当前主流版本、Intel Mac/可信 runner 和全新用户账户启动。
