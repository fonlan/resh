# macOS 阶段 4 签名、公证与供应链实现记录

- 日期：2026-06-30
- 状态：签名、公证和校验入口已实现；证书、Apple 账号和真实签名产物验收待执行

## 已实现

- 在 `src-tauri/tauri.conf.json` 显式启用 macOS hardened runtime。签名身份不写入配置文件，由 `APPLE_SIGNING_IDENTITY` 或 `APPLE_CERTIFICATE` 在构建环境中提供。
- 新增 `npm run macos:release`，封装签名和公证 Release 构建。该入口要求提供 `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_API_KEY`、`APPLE_API_ISSUER` 和 `APPLE_API_KEY_PATH`，并禁止 `SKIP_STAPLING=true`。
- 新增 `npm run macos:verify`，对 `.app` 和 `.dmg` 执行 `codesign --verify --deep --strict`、`spctl --assess` 和 `xcrun stapler validate`，随后生成 `.app.zip` 与 `SHA256SUMS.txt`。
- 新增 `npm run ci:pin-actions`，检查 GitHub Actions 是否固定到完整 commit SHA。
- 更新 `.github/workflows/macos-ci.yml`：
  - 默认权限收敛为 `contents: read`。
  - 所有 GitHub Actions 固定到完整 commit SHA。
  - 常规 macOS 自动化任务增加 Action pinning 审计。
  - `v*` tag 或手动勾选 `release_artifacts` 时，构建签名并公证的 `aarch64-apple-darwin` 与 `x86_64-apple-darwin` 产物，验证签名、公证票据和 Gatekeeper 状态，并上传 `.app.zip`、`.dmg`、`SHA256SUMS.txt`。
- 更新 `.gitignore`，避免提交本地构建产物、`.p12`、`.p8` 等敏感文件。

## CI Secrets

Release job 需要在 GitHub Secrets 中配置：

- `APPLE_CERTIFICATE`：Developer ID Application `.p12` 的 base64 内容
- `APPLE_CERTIFICATE_PASSWORD`：导出 `.p12` 时设置的密码
- `APPLE_API_KEY`：App Store Connect API Key ID
- `APPLE_API_ISSUER`：App Store Connect Issuer ID
- `APPLE_API_KEY_P8_BASE64`：`AuthKey_*.p8` 的 base64 内容

可选：

- `APPLE_SIGNING_IDENTITY`：需要强制指定证书 common name 时设置
- `APPLE_PROVIDER_SHORT_NAME`：Apple 账号存在多个 provider 时设置

## 本机验证

2026-06-30 已通过：

- `node --check scripts/macos_release_build.mjs`
- `node --check scripts/macos_release_verify.mjs`
- `node --check scripts/check-github-actions-pinned.mjs`
- `npm run ci:pin-actions`
- `npm run macos:release -- --help`
- `npm run macos:verify -- --help`
- `npm run tauri-build -- --config src-tauri/tauri.macos.conf.json --bundles app --no-sign`

## 未签名构建的副作用：本地网络权限（2026-09-21 实测）

在签名落地之前，Release 产物是 adhoc/linker 签名，这会让 **macOS 15 及以上的本地网络权限反复失效**。实证与机制如下（本机复现，日志来自 `~/Library/Application Support/Resh/logs/`）：

- 应用访问局域网地址（WebDAV 服务器在 NAS 上、或直连 `192.168.x` 的 SSH 目标）时，`EHOSTUNREACH (os error 65)` 瞬时失败，而同一时刻 `curl` 请求同一地址正常 —— 说明不是网络问题。
- 系统为应用记录了一条 “app rule”，但规则里绑定的是**签名派生的 UUID**。adhoc 签名每次构建都改变 cdhash，进程解析出的 UUID 随之变化，旧规则不再匹配，于是**拒绝且不再弹窗**（系统认为“已经有决定了”）。
- 实测形态：`nehelper` 缓存里仍显示该规则（`1 UUIDs for resh-<cdhash> are already in the cache`），但进程解析出的 UUID 与规则中的不一致；`/Library/Preferences/com.apple.networkextension.plist` 与 `...uuidcache.plist` 中**查不到当前进程 UUID**，而正常工作的应用（如 Foco）能查到。
- macOS **没有提供删除本地网络条目的 UI 或 CLI**（Apple 反馈报告 FB16270285 即是要求该能力；`tccutil` 不含此服务，状态存在 root 所属的 NetworkExtension 文件里，其中 uuidcache 是二进制 CUEN 格式）。因此用户侧无法自助恢复。

结论：这是**签名决策的后果**，也是本阶段待办工作（Developer ID + 公证）的直接理由之一。需要注意：用自签证书签名（DR 从 cdhash 变为基于证书）在本机实测**仍会失效**，所以“换证书即可解决”尚未被证实，需要在干净机器上用 Developer ID 实测（见下方「待执行」）。

在签名落地前，需要访问局域网的开发者可以启用本机回环中继：

```bash
npm run macos:local-network-relay -- --install --allow=your.webdav.host,lan
npm run macos:local-network-relay -- --status
npm run macos:local-network-relay -- --uninstall
```

原理：回环地址不属于 TN3179 定义的「局域网地址」（该定义要求绑定在支持广播的接口上），因此应用只连 `127.0.0.1` 就不再需要本地网络权限；真正需要该权限的那一跳由一个**已被系统放行的进程**完成。TLS 是盲转发，证书校验不受影响。注意中继必须由已被放行的解释器运行（本机 `node`/`curl` 已放行，`python3` 未放行），且**无法**通过“把中继写进应用内部”来绕过——LNP 按发起连接的 responsible process 判定。

中继默认拒绝一切目标（`--allow` 为空即 403），仅监听 `127.0.0.1`，并在上游 8 秒无响应时返回 504 而不是挂死。

## 待执行

- 准备 Apple Developer 账号和 Developer ID Application 证书。
- 在 GitHub Secrets 或本地环境中配置上述签名和公证凭据。
- 在 `v*` tag 或手动 release workflow 中执行一次真实签名、公证、stapling、Gatekeeper 验证和 SHA-256 生成。
- 在另一台未参与构建的 Mac 上下载、安装并启动产物。
- 在干净机器上验证「升级后本地网络权限是否仍有效」：安装当前 Release → 连局域网 WebDAV（应弹窗并可允许）→ 安装下一个版本（代码身份必然变化）→ 再连同一服务器，观察是否仍弹窗、是否仍可连接。若升级即失效，则所有局域网 WebDAV 用户都会遇到该问题，需要在此阶段解决（或在应用内把该失败渲染成可操作提示，而不是 `No route to host`）。
- 用 Developer ID 证书复现同一实验，确认基于证书的 designated requirement 能让授权跨版本保留（本机自签证书未能证实这一点）。
