# 调研与决策

- 当前客户端版本为 2.0.0。过滤历史保留了旧的本地 v2.0.0 tag，其指向拆分前的旧提交；不能把该 tag 当作本轮源码。
- 原 sv2-boxy GitHub 仓库已有公开时间为 2026-09-19 的 v2.0.0 release；新独立客户端仓库目前没有 remote。
- macOS 本机可构建 arm64；Windows MSVC 交叉编译缺少 Windows SDK 头文件，需 Windows 构建环境。
- 2026-09-25 现行主仓库已改为仅在启用屏蔽时下载并校验 WebView2 Fixed Runtime；Windows 发行包应恢复为单 exe，旧固定运行时打包方案已过时。
- GitHub 的私有 boxy-release 远端目前为空；本地 main 有客户端源码，旧 v2.0.0 标签指向迁移前提交。
- 2026-09-25 源码已推到远端 main，建立 v2.0.0 草稿发行；首次跨平台构建在文件名规则变更后取消，正式产物必须从更新后的源码重新生成。
- 最终发行源码为 `905013753a745b86176b5bf8c2bbda6068fbcc15`，main 与 v2.0.0 标签一致。GitHub main 无分支保护或规则集；独立仓库现为公开且源码可匿名读取。
- GitHub Actions 构建 `36146974561` 全部成功：Windows 26 个 Rust 测试、macOS 19 个 Rust 测试通过，两个 runner 都完成 release 构建及上传。
- 远端归档下载核对：Windows ZIP 仅含目录和 `boxy.exe`，格式为 x86-64 PE；macOS ZIP 包含 arm64 Mach-O、Info.plist 和图标，归档检查、plist 检查、ad hoc 签名检查通过。两个远端文件 SHA-256 与 GitHub 资产 digest 一致。
- 已发布公开 v2.0.0 和 `SHA256SUMS`。运行时下载与真实 SV2/WFP/LuLu 生效路径未在用户设备上实测；macOS 包未公证，Windows exe 未签名，发行说明如实列出。
