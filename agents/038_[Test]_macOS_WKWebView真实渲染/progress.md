# 操作记录

- 确认 `macos-render-smoke` worktree 基于 `f7f2af8` 且干净；读取 agent-mode、项目约定、macOS workflow、生产桌面模块、选择页及共享 HTTP fixture。
- 查阅本机锁定的 Tauri 2.12.4 `eval_with_callback` API：公开且跨平台，返回 JavaScript 结果序列化后的 JSON 字符串。
- root 已独立用生产解析器单测确认旧 macOS URL 负控失败，故此任务聚焦真实 WKWebView 集成 smoke，无需再次运行旧 GUI CI。
- 新增 `desktop_render_smoke` Cargo example，直接 include 生产 `desktop.rs` 并复用实际选择窗口构建、local command、worker 与 stop/cleanup 生命周期。WK DOM probe读取页面标题、控件存在性、输入空值、控件边界和 stylesheet border-radius；通过 `requestSubmit()` 触发生产选择 JS，再验证现有签名协议 fixture的cookie、远端IPC拒绝、signed poll/read_files receipt与清理。
- macOS manual-build workflow 启动本地 Node fixture 后运行真实 Tauri example，smoke step限制3分钟；未加生产probe或权限。
- `cargo +1.98.0 check --locked -p boxy --example desktop_render_smoke` 通过；`cargo +1.98.0 fmt --manifest-path connector/Cargo.toml -- --check`、Node fixture测试、Git Bash `bash -n` 和 `git diff --check` 通过。真实 WKWebView运行待 macOS CI。

根任务整合后CI39499486474真实macOS WKWebView smoke通过；首次测试过早eval回调在pending_scripts阶段被丢弃已修正，断言未弱化。66项Mac Rust测试、2项Node、双架构构建及v2.2.2附件校验通过并已发布。
