# 发现与验证

- 当前根因负控由 root 在真实生产解析器单测中确认：macOS 的 app URL `tauri://localhost` 没有尾斜线，当前 `local_selection_url` 只接受 `/` 或 `/index.html`，因此导航拒绝、窗口白屏；Windows URL 会规范化为带 `/`。
- 当前机器为 Windows，无法本机启动 WKWebView。Harness 需由 macOS CI 执行真实平台验证。
