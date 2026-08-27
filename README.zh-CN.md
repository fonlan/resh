<p align="center">
  <img src="logo.png" alt="Resh" width="120" />
</p>

<h1 align="center">Resh</h1>

<p align="center">一款基于 Tauri&nbsp;2 与 React&nbsp;19 构建的现代、快速、安全的 Windows &amp; macOS 多标签 SSH 客户端。</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://github.com/fonlan/resh/releases"><img src="https://img.shields.io/github/v/release/fonlan/resh?label=release" alt="最新版本" /></a>
  <a href="https://github.com/fonlan/resh/releases"><img src="https://img.shields.io/github/downloads/fonlan/resh/total" alt="下载量" /></a>
  <a href="https://github.com/fonlan/resh/actions/workflows/macos-ci.yml"><img src="https://github.com/fonlan/resh/actions/workflows/macos-ci.yml/badge.svg" alt="macOS CI" /></a>
  <img src="https://img.shields.io/badge/Windows-10%2B-0078d4?logo=windows&logoColor=white" alt="Windows 10+" />
  <img src="https://img.shields.io/badge/macOS-10.15%2B-black?logo=apple&logoColor=white" alt="macOS 10.15+" />
  <img src="https://img.shields.io/badge/Tauri-2-24c8db?logo=tauri&logoColor=white" alt="Tauri 2" />
</p>

<p align="center"><a href="README.md">English</a> | <strong>简体中文</strong></p>

---

Resh 是一款面向开发者的专业 SSH 客户端：多标签会话、分屏视图、内置 SFTP、远程文件编辑器、能直接操作服务器的 AI 助手，以及 WebDAV 配置同步——全部集成在一个轻量桌面应用中。配置完全可移植，支持跨机器同步。

## 目录

- [功能特性](#功能特性)
- [支持平台](#支持平台)
- [安装](#安装)
- [使用方法](#使用方法)
- [配置](#配置)
- [从源码构建](#从源码构建)
- [版本发布与内置更新](#版本发布与内置更新)
- [参与贡献](#参与贡献)
- [许可证](#许可证)

## 功能特性

- **多标签会话与分屏视图** —— 并排管理多个 SSH 会话；拖拽标签调整顺序，可在左右分屏、上下分屏、四宫格布局间切换，且不中断现有连接。
- **服务器与身份验证管理** —— 支持密码或 SSH 密钥认证（存储密钥*内容*而非路径，实现真正可移植），支持服务器分组、连接克隆，以及按连接配置的自动执行命令、环境变量和 keep-alive。
- **路由与端口转发** —— 支持 HTTP/SOCKS5 代理与 SSH 跳板机；每个连接可独立配置本地到远程的端口转发。
- **内置 SFTP** —— 文件浏览、收藏路径、传输队列与并发调优、服务器端复制、自定义命令，以及按服务器配置的编辑器关联。可用内置 Monaco 编辑器或本地编辑器编辑远程文件，远程文件变化时有冲突保护。
- **AI 助手** —— 与能执行命令、读取文件、通过 SFTP 传输文件、控制终端的智能体对话。支持 Anthropic、OpenAI 兼容与 GitHub Copilot 通道，可从 models.dev 目录自动填充模型信息，支持按服务器附加提示词、工具确认（含 YOLO 模式）与上下文压缩。
- **代码段** —— 保存可复用的命令片段，一键发送到任意会话。
- **终端体验** —— Canvas 或 WebGL 渲染器，可自定义字体/光标/回溯行数，支持选中复制+右键粘贴模式、终端会话录制（原始或纯文本）与会话导出。
- **WebDAV 同步** —— 跨机器同步服务器、身份验证、代理与代码段，支持逐项冲突解决。
- **内置更新** —— 自动检查更新（启动后每约 6 小时一次，也支持手动检查），SHA-256 校验下载，支持代理，安全重启并恢复标签页。
- **主题与多语言** —— 支持浅色/深色/跟随系统及落日橙、鼠尾草主题；UI 提供英文与简体中文。
- **桌面体验** —— 无边框窗口、单实例模式、更新后的窗口与标签恢复，以及带最近连接记录的欢迎页。

## 支持平台

| 平台 | 架构 | 发布格式 |
| --- | --- | --- |
| Windows 10+ | x64 | 便携版 `.exe`（未签名，无需安装） |
| macOS 10.15+ | Intel | `.dmg`（未签名 / 未公证） |
| macOS 11+ | Apple Silicon | `.dmg`（未签名 / 未公证） |

## 安装

所有构建产物均发布在 GitHub Releases：<https://github.com/fonlan/resh/releases>

### Windows

1. 下载 `Resh-<版本号>-windows-x86_64.exe`。
2. 放入任意可写目录直接运行——无需安装。配置保存在 `%AppData%\Resh\`。

### macOS

1. 下载对应架构的 `.dmg`（Apple Silicon 选 `-aarch64`，Intel 选 `-x86_64`）。
2. 打开 DMG，将 **Resh.app** 拖入 `/Applications`。
3. 由于 Release 包**未签名 / 未公证**，macOS Gatekeeper 可能在首次启动时拦截。清除隔离属性即可：

   ```bash
   xattr -d com.apple.quarantine /Applications/Resh.app
   ```

   也可以右键应用图标，选择**打开**。

   > 内置更新只会清除新安装且经校验的 `Resh.app` 上的 `com.apple.quarantine` 属性——不会修改系统 Gatekeeper 策略。

### 校验下载

```bash
sha256sum -c SHA256SUMS.txt
```

## 使用方法

### 连接

- 点击标签栏的 **+** 选择已配置的服务器，或使用**快速连接**输入 `ip`、`host` 或 `user@host`。
- 欢迎页会展示最近连接记录。

### 管理配置

打开**设置**（齿轮图标），各标签页可配置：

| 标签页 | 配置内容 |
| --- | --- |
| 服务器 | SSH 服务器、分组、路由、端口转发、keep-alive、自动执行命令、环境变量、SFTP 收藏路径 |
| 身份验证 | 密码与 SSH 密钥 |
| 代理 | HTTP/SOCKS5 代理（支持认证与 SSL 忽略） |
| 代码段 | 可复用的命令片段 |
| AI | AI 通道与模型、附加提示词、对话上下文、工具确认 |
| SFTP | 下载路径、传输并发与模式、编辑器关联、自定义命令 |
| 同步 | WebDAV 地址、代理、立即同步 |
| 常规 | 主题、语言、终端字体/光标/渲染器、标签宽度、录制、确认项、软件更新 |
| 关于 | 版本、仓库、许可证、技术栈 |

### SFTP 与编辑器

打开服务器的 SFTP 侧边栏浏览文件，将常用目录加入收藏，管理传输队列，或在内置编辑器中打开远程文件。编辑器关联与自定义 Shell 命令可按服务器或全局配置。

### AI 助手

1. 在 **AI 设置**中添加通道（Anthropic、OpenAI 兼容或 GitHub Copilot）与模型——可使用 models.dev 目录自动填充上下文大小。
2. 选择服务器并开始对话。智能体可以执行命令、读取文件、传输文件；危险操作需要确认。

### WebDAV 同步

1. 在设置 → 同步中配置 WebDAV 地址（可选代理）。
2. 点击**立即同步**手动同步；应用启动时也会自动同步。
3. 冲突可逐项解决（保留本地 / 采用远端）。

## 配置

Resh 将数据存储在特定于平台的应用程序数据目录：

```
Windows: %AppData%\Resh\
macOS:   ~/Library/Application Support/Resh/

Resh/
├── local.json        # 仅本地设置（主题、WebDAV、更新、确认项等）
├── sync.json         # 服务器 / 身份验证 / 代理 / 代码段（通过 WebDAV 同步）
├── config.db         # 本地应用数据库（AI 会话、最近连接等）
└── logs/             # 应用与连接日志
```

同步策略：`local.json` 中的条目会覆盖 `sync.json` 中相同 UUID 的条目；只有 `sync.json` 会与 WebDAV 服务端上传/下载。

## 从源码构建

### 环境要求

- **Node.js** 22.12.x 与 **npm** 10.9.x（见 `.nvmrc` / `package.json`）
- **Rust** 1.88+（见 `rust-toolchain.toml`）
- 平台构建依赖：
  - Windows：Microsoft C++ Build Tools 与 WebView2
  - macOS：Xcode Command Line Tools

### 快速开始

```bash
git clone https://github.com/fonlan/resh.git
cd resh
npm ci
npm run tauri-dev
```

### 常用脚本

| 命令 | 说明 |
| --- | --- |
| `npm run dev` | 仅启动 Vite 开发服务器 |
| `npm run tauri-dev` | 以开发模式运行 Resh |
| `npm run build` | 类型检查并构建前端 |
| `npm run tauri-build` | 构建生产应用（`src-tauri/target/release/Resh.exe`） |
| `npm run build:macos` | 构建 macOS DMG（`CI=true` 用于类似 CI 的未签名构建） |
| `npm run check:release-version -- vX.Y.Z` | 校验标签与所有版本来源一致 |
| `npm run check:updater-assets -- --tag vX.Y.Z --dir ./assets` | 校验更新器资产名称与校验和 |
| `npm run ci:pin-actions` | 校验第三方 GitHub Actions 已全 SHA 锁定 |
| `npm run test:updater-helpers` | 运行隔离的 Windows/macOS 更新辅助脚本测试 |
| `npm run sftp:baseline -- -ServerHost <host> -User <user>` | SFTP 性能测试（Windows PowerShell） |

## 版本发布与内置更新

推送版本标签（`v*`）是**唯一**的自动 GitHub Release 入口（`.github/workflows/release.yml`）。

打标签前需确保三个版本来源一致（semver，不带前导 `v`）：

| 文件 | 字段 |
| --- | --- |
| `package.json` | `"version"` |
| `src-tauri/Cargo.toml` | `package.version` |
| `src-tauri/tauri.conf.json` | `version` |

```bash
npm run check:release-version -- vX.Y.Z
git tag vX.Y.Z && git push origin vX.Y.Z
```

每次发布恰好包含四个资产——这些名称是**内置更新 API**，请勿重命名：

| 资产 | 说明 |
| --- | --- |
| `Resh-vX.Y.Z-windows-x86_64.exe` | Windows x64 便携版（未签名） |
| `Resh-vX.Y.Z-macos-aarch64.dmg` | Apple Silicon DMG（未签名 / 未公证） |
| `Resh-vX.Y.Z-macos-x86_64.dmg` | Intel DMG（未签名 / 未公证） |
| `SHA256SUMS.txt` | SHA-256 校验和（GNU `sha256sum` 格式） |

更新行为：默认开启自动检查（设置 → 常规 → 软件更新；启动后稍候检查一次，此后每约 6 小时），仅考虑稳定版，支持代理。Windows 会在原位置替换便携 EXE（请放在可写目录）；macOS 会替换已校验的 `Resh.app` 并清除其隔离属性。完整约定见 [scripts/checklists/updater-release-contract.md](scripts/checklists/updater-release-contract.md)。

## 参与贡献

欢迎贡献！请提交 Issue 或 Pull Request。

### 报告安全问题

请**私下**联系维护者报告安全漏洞，不要在公开 Issue 中披露。

## 许可证

[MIT](LICENSE) © fonlan
