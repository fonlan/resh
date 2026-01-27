# 日志系统设计方案 (2026-01-27)

## 概述
为 Resh 启用全栈日志功能，支持后端 (Rust) 和前端 (React) 的日志统一记录。日志保存在 `%AppData%\Resh\logs` 目录下，采用按天滚动和动态等级过滤机制。

## 核心架构

### 后端 (Rust)
- **库选择**: `tracing`, `tracing-appender`, `tracing-subscriber`。
- **存储路径**: `%AppData%\Resh\logs/`。
- **滚动策略**: 
  - 按天滚动 (`resh.YYYY-MM-DD.log`)。
  - 自动清理：保留最近 7 天的日志。
- **日志等级**:
  - 默认: `INFO`, `WARN`, `ERROR`。
  - 调试模式: `DEBUG`, `TRACE` (记录 SSH 握手细节)。
- **动态更新**: 使用 `tracing_subscriber` 的 `ReloadHandle`，在不重启应用的情况下动态切换日志等级。

### 前端 (React)
- **日志推送**: 暴露 Tauri 命令 `log_event(level, message)`，将前端日志同步至后端文件。
- **Logger 工具**: 封装 `src/utils/logger.ts`，自动附带组件上下文前缀。
- **全局捕获**: 自动记录未捕获的 JS 异常。

## 数据流与交互
1. 用户在“设置 -> 常规”中切换“启用调试日志”。
2. 前端调用 `save_config` 更新 `local.json`。
3. 后端接收到配置变更后，触发日志等级重载。
4. 后端及前端后续日志将按新等级记录。

## 安全与格式
- **格式**: 纯文本格式 `[Timestamp] [Level] [Context] Message`。
- **安全性**: 严禁记录 Master Password、私钥及会话凭据。
- **健壮性**: 日志系统初始化失败不应阻止应用启动。
