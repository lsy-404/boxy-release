# 操作记录

- 2026-10-16：本地合并 d355fc2；源码主 checkout 干净，无用户改动。新建交付分支 codex/regional-logo-release。
- 重新读取现有发行 workflow，沿用完整 SHA 对齐的 draft release 与 Windows/macOS 构建，不覆盖已有版本。
- 439e4e3 的版本和 Cargo.lock 更新为 2.0.7；fmt、打包测试、diff 检查通过，main 合并并推送。
- Actions 38993230873 的 Windows/macOS 全部成功；Windows 32+6+6 项 Rust 回归通过，打包启动日志确认浏览器 URL 已获取且没有 exe 相邻 WebView2 profile。
- Windows 资产 3371008 bytes，SHA256 7fdc1847e167f7eaa606a07570384c8235a25bbd897b2954ebd492fd6025ec41；macOS universal ZIP 3411065 bytes，SHA256 e5a06880d2770daff458566e7c0b3a7c612faafcb59a672b6d38e83d6ced18ff。
- 2026-10-17 02:05:34 UTC 公开 v2.0.7，复核 isDraft=false 且两个资产存在。最后 D1 复核最近半小时许可证任务仍为零，不能声称真实大陆产品数量已确认。
