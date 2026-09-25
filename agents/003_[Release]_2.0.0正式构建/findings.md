# 调研与决策

- 当前客户端版本为 2.0.0。过滤历史保留了旧的本地 v2.0.0 tag，其指向拆分前的旧提交；不能把该 tag 当作本轮源码。
- 原 sv2-boxy GitHub 仓库已有公开时间为 2026-09-19 的 v2.0.0 release；新独立客户端仓库目前没有 remote。
- macOS 本机可构建 arm64；Windows MSVC 交叉编译缺少 Windows SDK 头文件，需 Windows 构建环境。
- 最新主仓库 Windows 发行包需要经 SHA-256 校验的 Microsoft WebView2 Fixed Runtime CAB；产物从单 exe 改为含私有运行时的 ZIP。正式构建必须使用更新后的打包脚本与固定运行时。
