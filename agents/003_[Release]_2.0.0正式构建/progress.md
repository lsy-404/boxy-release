# 操作记录

- 2026-09-24：读取客户端版本、旧 tag、发行工作流与可用构建环境。
- 2026-09-25：确认当前 main 的 WFP/LuLu/WebView2 源码和云端协议测试，客户端 Rust 测试 18/18、Windows 包样本和云端 31/31 测试通过。2.0.0 的历史 tag 指向旧源码，本轮构建需重新固定到新提交。
- 2026-09-25：在独立仓库迁移按需依赖后完成 macOS arm64 release 构建和 app/zip 打包；`codesign --verify`、`plutil -lint`、`unzip -t` 通过，本地 ZIP SHA-256 为 `4ebe0e4ce40602dfaf3baf00918d188f849d837d3cc9a612c905bf295659c1cb`。远端仍为空，Windows 正式构建待 GitHub Actions。
