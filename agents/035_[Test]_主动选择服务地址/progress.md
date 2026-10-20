# 操作记录

- 确认 `codex/service-selection-smoke` worktree 干净，基线为 `3633f54`；读取项目索引、发行说明、Windows workflow、fixture 和 Windows startup smoke。
- 已向 root 报告 UIA 的可行控件定位方案：PID 校验后按 AutomationId `service-url` / `connect` 定位输入框与按钮，选择页空值及错误元素由可访问树读取；真实窗口测试用于确认 HTML ID 映射行为。
- fixture 现验证远程 WebView 调用 `select_service` 被拒绝且服务端只收到一次 operation 创建；保持现有 cookie、protocol-v3 signed poll/read_files receipt 和 Blob export 覆盖。
- Windows UIA smoke 使用专用进程 PID 定位窗口及控件，通过 ValuePattern / InvokePattern 操作，没有全局键盘输入。无 `--server` 的普通文件名启动后显示空白选择页；一个被独占锁定的无效 session 在选择前不会触发读取或文件对话框；未选择时正常退出并清理 profile。
- 用 root 提供的 `tauri-build-cache/release/boxy.exe`（2.2.1）真实运行通过：选择 loopback 后 fake cloud 页面回传 cookie、完成 protocol-v3 签名 read_files 回执；远程选择命令拒绝、operation 数为 1、正常关闭后 profile 清理完成。
- 增补同一真实 GUI 测试：格式错误 URL 显示本地校验错误；独立 loopback start fixture 返回 503 后页面恢复可编辑；用户改选正常 fixture 后完成云页与签名回执，正常退出并清理 profile。失败 cleanup 等待正常关闭 30 秒，强制结束仅测试进程且记录为测试失败。
- 最终版临时目录清理先验证规范化路径位于专用 tempBase 下，且目录名符合本测试随机 GUID 命名，再进行递归删除。
- `node --check test/fixtures/tauri-webview-server.mjs`、`node --test test/tauri_webview_fixture.test.mjs`、PowerShell AST parse、`git diff --check` 通过。此前 startup 与 export GUI smoke 均通过。
