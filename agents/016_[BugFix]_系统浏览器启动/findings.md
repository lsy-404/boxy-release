# 调查结果

- 已发布 v2.0.5 使用 `tao` 与 `wry`，从 `main` 进入 `service_selection::run_window`，创建 `WebViewBuilder` 并把 UI HTML 注入内嵌窗口；Windows 后端未得到自定义 UDF，因而触发 WebView2 默认目录行为。
- 当前 `run_connected` 已在 `createOperation` 成功、校验 `browserUrl` 后调用 `webbrowser::open`；这段既有安全顺序可直接复用。
- `wait_for_writeback` 是保留 session 监视、签名回执及受限任务的主运行循环；实现需在远程操作终止状态返回，使后台进程自然退出。
- Worker 没有 browser operation close endpoint；网页 `closeOperation()` 只隐藏进度对话框。connector 继续按既有服务端状态运行，不发明 `closed` 协议状态。
- `windows_tray.rs` 读取系统通知区域的其他应用名称，用作设备报告字段，不是 Boxy 的托盘图标或退出入口。
- 固定 Dreamtonics 动作使用编译期固定 endpoint；custom request 只校验 HTTPS、host、userinfo、fragment 与长度，并无 `.com` 专属 host 白名单。本分支不改区域 API 常量或请求代码。
- 系统浏览器 smoke 使用 loopback 假服务和 8 字节测试 session，子进程设置不存在的 WebView2 runtime 路径，只观察假服务操作创建和浏览器请求计数。
- `agents/local.instructions.md` 在此发行仓库 worktree 不存在；本任务遵循根级说明及主任务明确范围。
- 本机未执行浏览器 smoke，以免打开交互式用户浏览器；PowerShell 脚本做语法解析，发行 CI workflow 会在隔离 Windows runner 执行它。
