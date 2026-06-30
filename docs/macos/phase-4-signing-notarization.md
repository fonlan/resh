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

## 待执行

- 准备 Apple Developer 账号和 Developer ID Application 证书。
- 在 GitHub Secrets 或本地环境中配置上述签名和公证凭据。
- 在 `v*` tag 或手动 release workflow 中执行一次真实签名、公证、stapling、Gatekeeper 验证和 SHA-256 生成。
- 在另一台未参与构建的 Mac 上下载、安装并启动产物。
