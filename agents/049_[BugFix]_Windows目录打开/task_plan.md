# Windows 目录打开修复计划

- [x] 读取 agent-mode、项目说明和本地指令，确认隔离分支工作区状态。
- [x] 追踪 `open_folder` 从 Explorer 启动、任务结果到桌面连接器退出的错误传播。
- [x] 调整 Windows Explorer 启动语义，使目录请求提交后不等待 Explorer 窗口进程退出。
- [x] 在 `/test` 添加回归，覆盖 opener 接收目录请求后以非零状态退出的成功语义。
- [x] 运行相称的 Rust 检查；评估可安全执行的 Windows 验证。
- [x] 检查差异和禁用关键词，提交基础修复，不推送或合并。

## 端到端补充验证

- [ ] 在 `run_connected` HTTP mock 中真实调用 Windows `open_session_folder`，验证签名 `open_folder` 回执后 worker 继续轮询并完成 `read_files`。
- [ ] 扩展现有本地 WebView fixture 和 Windows smoke 的可选场景，保持默认 smoke 任务序列不变。
- [ ] 运行 Windows core runner 实际 Explorer 测试、Node fixture 测试及 PowerShell 语法检查；按实际证据记录环境限制。
- [ ] 审阅 Explorer 真正无法启动时的错误传播，检查差异并提交补充 commit。
- [x] 在 `run_connected` HTTP mock 中真实调用 Windows `open_session_folder`，验证签名 `open_folder` 回执后 worker 继续轮询并完成 `read_files`。
- [x] 扩展现有本地 WebView fixture 和 Windows smoke 的可选场景，保持默认 smoke 任务序列不变。
- [x] 运行 Windows core runner 实际 Explorer 测试、Node fixture 测试及 PowerShell 语法检查；按实际证据记录环境限制。
- [x] 审阅 Explorer 真正无法启动时的错误传播，确认本次只修复 handoff 退出误判。
- [x] 检查补充差异和禁用关键词，提交当前分支，不推送或合并。
