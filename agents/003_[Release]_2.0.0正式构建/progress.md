# 操作记录

- 2026-09-24：读取客户端版本、旧 tag、发行工作流与可用构建环境。
- 2026-09-25：确认当前 main 的 WFP/LuLu/WebView2 源码和云端协议测试，客户端 Rust 测试 18/18、Windows 包样本和云端 31/31 测试通过。2.0.0 的历史 tag 指向旧源码，本轮构建需重新固定到新提交。
- 2026-09-25：在独立仓库迁移按需依赖后完成 macOS arm64 release 构建和 app/zip 打包；`codesign --verify`、`plutil -lint`、`unzip -t` 通过，本地 ZIP SHA-256 为 `4ebe0e4ce40602dfaf3baf00918d188f849d837d3cc9a612c905bf295659c1cb`。远端仍为空，Windows 正式构建待 GitHub Actions。
- 2026-09-25：将源码提交推到 GitHub main，建立 v2.0.0 标签和草稿发行并启动构建；因用户更新文件名规则，立即请求取消该轮构建，待新源码重跑。
- 2026-09-25：旧构建已取消，草稿发行中由旧构建上传的 macOS ZIP 已删除。按新文件名逻辑重新构建本地 macOS arm64 包，ZIP SHA-256 为 `c03d04c86e25b341d5f34822db3afaaa9d024a87e28c4a8cbf576c56cd1a6980`。
- 2026-09-25：将更新源码推送到 main，重置未公开的 v2.0.0 标签和草稿发行目标；将仓库设为公开，重跑 GitHub Actions 构建 `36146974561` 并确认三项工作均成功。
- 2026-09-25：下载并检查正式归档，Windows SHA-256 为 `3acbc861ed175ed3e491aa3a648631cb857500a5c8d7890b27d51f29f2786530`，macOS SHA-256 为 `80389f740dbb6d4202728d33f3ae83a79e3498dce3deba034e3d81cb65b2a223`；上传 `SHA256SUMS`，将草稿发布为正式 v2.0.0。匿名 GitHub API 显示 draft=false 和三个资产。
