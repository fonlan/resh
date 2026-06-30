# Resh macOS 支持实施方案

## 目标与范围

本方案用于将 Resh 从“具备跨平台源码基础”推进到“可正式发布和维护的 macOS
版本”。首个正式支持基线暂定为 macOS 10.15+，同时支持 Apple Silicon 和 Intel
Mac；最终产物至少包含 `.app` 与 `.dmg`。

首阶段以官网外直接分发为目标，暂不包含 Mac App Store 上架。若后续需要上架，
应另行补充 App Sandbox、权限声明、Provisioning Profile 和商店审核工作。

状态约定：

- `[ ]` 未开始或尚未通过验收
- `[x]` 已完成且有可复现的验证记录

## 完成定义

只有同时满足以下条件，才可在 README 和 Release 页面将 macOS 标记为“正式支持”：

- [ ] Apple Silicon 与 Intel 架构均有可安装、可启动的发布产物，或提供经过验证的 Universal Binary
- [ ] 核心 SSH、SFTP、代理、WebDAV、剪贴板和本地文件交互测试全部通过
- [ ] `.app` 和 `.dmg` 已使用 Developer ID Application 证书签名并完成 Apple 公证
- [ ] CI 能在干净环境中稳定构建、测试并保存 macOS 产物
- [ ] 至少在最低支持版本和一个当前主流 macOS 版本上完成验收
- [ ] 已发布安装、升级、卸载、数据目录和故障排查文档

## 阶段 0：建立基线与决策记录

目标：先固定支持边界和当前问题，避免后续实现反复变更。

- [x] 确认最低支持版本；维持 macOS 10.15+，已在 Tauri 配置中显式声明
- [x] 决定产物策略：分别发布原生 `aarch64`/`x86_64` 产物
- [x] 决定窗口策略：macOS 恢复系统标题栏，Windows 保留无边框窗口
- [x] 决定首发渠道：GitHub Releases
- [x] 记录一轮 Apple Silicon 开发构建结果和所有编译/运行错误

阶段 0 的书面结论见 [`docs/macos/phase-0-decisions.md`](docs/macos/phase-0-decisions.md)，
首次开发构建记录见
[`docs/macos/development-builds/2026-06-30-apple-silicon.md`](docs/macos/development-builds/2026-06-30-apple-silicon.md)。

阶段出口：支持版本、架构、窗口和分发策略已有书面结论，开发机能够进入依赖安装或编译阶段。

## 阶段 1：打通可复现构建

目标：在干净的 macOS 环境完成前端、Rust 和 Tauri 构建。

### 依赖与锁文件

- [x] 将开发环境统一到 Node.js 22.12.0、npm 10.9.0、Rust 1.88.0
- [x] 同步 `package.json`、`package-lock.json` 和 `bun.lock`，移除残留的 `smart-codebase`
- [x] 提交并维护 `src-tauri/Cargo.lock`，不再忽略应用程序的 Rust 锁文件
- [x] 验证固定 revision 的 `russh-sftp` Git 依赖可在干净 Cargo 缓存和离线目标检查中获取
- [x] 在干净依赖环境执行 `npm ci`、`npm run build` 和 `cargo test --locked`

### 图标与 Tauri 配置

- [x] 由 `logo.png` 生成并提交完整的 Tauri 图标集
- [x] 确认存在 `128x128.png`、`128x128@2x.png` 和 `icon.icns`
- [x] 在 `src-tauri/tauri.conf.json` 增加明确的 `bundle.macOS` 配置
- [x] 显式配置并验证 `minimumSystemVersion`
- [x] 分别完成 Debug 启动和 Release `.app` 构建
- [x] 完成 `.dmg` 构建，并检查应用名称、图标、Bundle Identifier 和版本号
- [x] 记录 Apple Silicon 构建命令及产物路径

阶段 1 Apple Silicon 验证记录（2026-06-30）：

- 工具链：Node.js 22.12.0、npm 10.9.0、Rust/Cargo 1.88.0
- 干净安装与构建：`npm ci`、`npm run build`、`cargo test --manifest-path src-tauri/Cargo.toml --locked`
- Debug：`npm run tauri-dev`，Vite 与 `target/debug/resh` 均成功启动
- Release：`npm run tauri-build -- --target aarch64-apple-darwin`
- `.app`：`src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Resh.app`
- `.dmg`：`src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Resh_1.1.0_aarch64.dmg`
- 产物检查：原生 arm64、`com.fonlan.resh`、版本 1.1.0、图标存在；DMG 可挂载并包含 Applications 链接
- 最低版本：Info.plist 为 10.15；arm64 Mach-O 的部署目标为 11.0（Apple Silicon 系统起点）
- 签名状态：未签名开发产物，Developer ID 签名与公证留待阶段 4
- 依赖审计：兼容范围修复后剩余 3 项（1 low、2 moderate）；Monaco/UUID 修复需要主版本升级，留待独立依赖升级处理

阶段出口：新 Mac 按文档操作可从干净克隆稳定生成能启动的未签名 `.app` 和 `.dmg`。

## 阶段 2：平台行为与界面适配

目标：让应用在 macOS 上不仅“能启动”，还符合基本平台行为。

### 窗口与应用生命周期

- [x] 根据阶段 0 的决策实现 macOS 标题栏或左侧窗口控制区
- [ ] 验证拖动、最小化、全屏、关闭、重新激活和 Dock 点击行为
- [ ] 验证关闭最后一个窗口后的行为符合产品预期
- [ ] 验证单实例模式能显示、聚焦并恢复最小化窗口
- [x] 防止保存的窗口坐标在显示器变更后导致窗口落到屏幕外
- [ ] 验证深色/浅色系统主题切换和 Retina 缩放

### 键盘、剪贴板与菜单

- [ ] 验证并统一 `Command+C/V/A/W/Q` 等 macOS 快捷键
- [ ] 确保终端中的 `Control+C` 仍发送中断字符，不与复制逻辑冲突
- [ ] 验证 Monaco 编辑器保存、撤销、重做和选择快捷键
- [ ] 验证 Tauri 剪贴板插件、OSC 52 和右键复制/粘贴
- [x] 评估并补充标准应用菜单、About、Preferences 和 Quit 行为

### 文件与系统集成

- [ ] 验证应用数据目录为 `~/Library/Application Support/Resh/`
- [ ] 验证文件/目录选择器、终端日志导出和 SFTP 上传下载
- [ ] 验证默认应用打开文件以及自定义编辑器路径
- [x] 将 Windows 专用占位示例替换为按平台显示的路径示例
- [ ] 验证临时编辑文件的创建、监听、回传和清理
- [ ] 验证 Copilot 登录链接使用系统默认浏览器打开

阶段出口：平台行为检查表通过，不存在阻断日常使用的 Windows 专属交互。

阶段 2 首轮实现与待手工验收项见
[`docs/macos/phase-2-implementation.md`](docs/macos/phase-2-implementation.md)。

## 阶段 3：核心功能与稳定性验证

目标：证明 Mac 版本能够承担完整 SSH 客户端工作流。

### 自动化测试

- [x] 为 macOS 增加前端类型检查和构建任务
- [x] 在 macOS CI 上运行 Rust 单元测试
- [x] 将 SFTP 性能脚本迁移为跨平台入口，或为 `pwsh` 提供受支持的调用方式
- [x] 增加应用启动、主窗口可见性和基础 Tauri command smoke test
- [x] 对配置目录、路径分隔符和文件名编码增加回归测试

### 手工验收矩阵

- [ ] 密码、私钥和交互式认证 SSH 登录
- [ ] HTTP、SOCKS5 和 SSH Jumphost 连接
- [ ] 本地端口转发、Keep-Alive、断线重连和多标签会话
- [ ] 睡眠/唤醒、网络切换后的连接状态和恢复行为
- [ ] SFTP 浏览、上传、下载、重命名、删除、权限修改和冲突处理
- [ ] SFTP 大文件、批量小文件、取消任务和并发公平性
- [ ] 内置编辑器和外部编辑器往返保存
- [ ] WebDAV 同步、冲突合并和 Windows/macOS 跨机配置迁移
- [ ] AI 渠道、流式响应、工具确认和 Copilot 登录
- [ ] WebGL 终端渲染及上下文丢失后的回退/恢复
- [ ] 中文、Emoji、组合字符、宽字符和常用终端字体显示

### 测试环境

- [ ] Apple Silicon：最低支持版本或可获得的最接近版本
- [ ] Apple Silicon：当前主流 macOS 版本
- [ ] Intel Mac：原生设备或可信 CI runner
- [ ] 至少一次从全新用户账户启动，排除开发机残留配置影响

阶段出口：自动化任务稳定通过，手工验收没有 P0/P1 缺陷，性能无明显平台回退。

阶段 3 自动化实现与待验收项见
[`docs/macos/phase-3-implementation.md`](docs/macos/phase-3-implementation.md)。

## 阶段 4：签名、公证与供应链

目标：让用户下载后可以通过 Gatekeeper 正常安装和启动。

- [ ] 准备 Apple Developer 账号和 Developer ID Application 证书
- [ ] 在本地或 CI 密钥链中安全导入签名证书
- [ ] 配置 Tauri signing identity，禁止将证书或密码提交到仓库
- [ ] 配置 Apple 公证凭据并执行 notarization
- [ ] 将公证票据 stapling 到 `.app`/`.dmg`
- [ ] 使用 `codesign --verify --deep --strict` 验证签名
- [ ] 使用 `spctl --assess` 验证 Gatekeeper 接受状态
- [ ] 使用 `xcrun stapler validate` 验证公证票据
- [ ] 在另一台未参与构建的 Mac 上下载、安装并启动产物
- [ ] 生成并发布 SHA-256 校验值
- [ ] 审查 CI 权限、第三方 Action 固定版本和 Release 写入权限

阶段出口：外部下载的产物无需绕过系统安全设置即可安装和启动，签名与公证验证全部通过。

## 阶段 5：CI、发布与长期维护

目标：将一次性适配变成可持续维护的平台支持。

- [ ] 添加 macOS CI，覆盖前端构建、Rust 测试和 Tauri Release 构建
- [ ] 为 Apple Silicon 和 Intel/Universal 产物建立清晰的命名规则
- [ ] 仅在版本标签构建中启用签名、公证和 Release 上传
- [ ] 保存构建日志、测试结果、`.app`、`.dmg` 和校验值
- [ ] 增加发布前人工审批和失败回滚流程
- [ ] 更新 README，将 macOS 状态改为 Supported
- [ ] 编写 macOS 安装、升级、卸载、数据备份和日志位置文档
- [ ] 说明最低系统版本、支持架构和已知限制
- [ ] 建立每个正式版本的 Windows/macOS 双平台回归要求
- [ ] 将依赖升级纳入 Mac 构建回归，特别关注 Tauri、WebKit、窗口和签名变化

阶段出口：发布流程可由版本标签重复触发，文档、产物和验收记录完整，后续版本不会静默破坏 Mac 支持。

## 建议的首个 macOS 发布门禁

- [ ] 所有阶段出口已满足
- [ ] 没有未关闭的 P0/P1 macOS 缺陷
- [ ] CI 连续三次从干净环境成功构建并公证
- [ ] Apple Silicon 与 Intel/Universal 产物版本、配置格式和数据目录一致
- [ ] 从 Windows 同步而来的配置可以在 macOS 正常使用，且不会写入 Windows 路径默认值
- [ ] Release Notes 明确标注支持版本、架构、安装方式和已知限制
- [ ] 维护者完成最终发布批准
