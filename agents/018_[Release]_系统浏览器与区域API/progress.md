# 操作记录

- 2026-10-16：确认 GitHub main protected=false 且分支规则为空；本地 source 主 checkout 无用户改动。
- 本地合并区域请求 3c50455 与系统浏览器 c57995a 两个子任务提交；保留独立历史并解决任务索引相邻追加冲突。
- 最终构建将复用 browser-only worktree 的 target 缓存，不写入其他工作区的业务源码。
- 最终构建复用 browser-only target，完成 Rust、Node、fmt、依赖树检查及 Windows release 构建；cargo tree 中没有 Tao、Wry 或 WebView2 crate。
- 使用新建的独立临时 fixture 执行 windows_startup_smoke.ps1，返回成功；通过浏览器工具确认系统 Edge 的测试页正文，再关闭测试页。原来的 Boxy 2.0.5 进程和目录没有被删除或替换。
