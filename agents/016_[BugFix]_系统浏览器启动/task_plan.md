# 任务计划

- [x] 检查隔离 worktree 状态、connector UI/操作生命周期、发布 workflow 和现有 smoke 测试。
- [x] 用无 WebView 的启动流程替代本地选择窗口，保持 CLI、服务推断、session、授权后 browser URL 打开和持续操作监视。
- [x] 删除 Tao/Wry、内嵌 HTML/IPC、UI 包及仅供该 UI 使用的资源和依赖，更新 Cargo.lock 与项目索引。
- [x] 更新根目录 `/test` 覆盖默认服务、CLI/文件名推断以及授权后打开系统浏览器并不产生 `*.WebView2` 的 smoke 验证。
- [x] 运行 Rust/Node 检查和 Windows 发行构建检查，复核 diff 并提交本任务改动。
