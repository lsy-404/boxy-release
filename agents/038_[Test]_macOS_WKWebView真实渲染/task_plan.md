# macOS WKWebView 真实渲染 smoke

- [x] 复用生产 desktop module 构建单独的 macOS render smoke example。
- [x] 用 eval_with_callback 检查选择页 DOM、空输入、控件布局与样式，再触发真实服务选择命令。
- [x] 复用本地 protocol-v3 fixture，验证远程页面 JS、Cookie、权限拒绝与签名回执，正常退出并清理临时目录。
- [x] 接入 manual-build macOS workflow，并验证 example 编译与 shell 格式。
- [ ] 在 macOS runner 上运行真实 WKWebView smoke，记录负控和修复后的结果。
- [ ] 提交本分支供 root cherry-pick，不推送。
