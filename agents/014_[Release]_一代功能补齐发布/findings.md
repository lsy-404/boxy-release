# Findings

- v2.0.3 已公开，修复使用新版本 v2.0.4，不覆盖已发布版本。
- 本轮用户重新要求回归；Windows/macOS 均在发行前执行 Cargo 回归和打包测试，Windows 使用 fixture session/server 检查实际包启动。
- 云端许可证集合及过滤选项由 Worker 维护；公开客户端只负责受限请求、密文 session I/O 及本机操作。
