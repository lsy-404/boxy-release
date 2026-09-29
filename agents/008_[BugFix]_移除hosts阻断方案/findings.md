# 调查记录

- [发行实现] 当前 `boxy-release` 的 `host_block.rs` 在 macOS 和 Windows 都会添加 Dreamtonics 域名到系统 hosts；`network_block.rs` 把该规则作为单独模式和 Windows 防火墙模式的前提。仅隐藏云端选项不会停止发行版连接器写 hosts。
- [清理边界] 既有设备可能仍留有 Boxy 标记的 hosts 条目。删除新增规则能力时仍需提供只删精确标记条目的清理操作，且保留用户自有映射。
- [编号] 发行仓库主 checkout 中已有尚未合并的 006 与 007 审计记录；本次使用 008 避免重复编号，不移动或覆盖原工作树内容。
- [工作树] 从已获取的 `origin/main` 建立独立 `codex/remove-host-blocking` 工作树；原主 checkout 的审计未提交内容保留。
- [协议门槛] 新版连接器状态报告 `hostless: true`。云端只对报告此能力的连接器开放 Windows 进程规则切换；旧版连接器只能接收 `hosts` 关闭任务以清除旧规则，不能再发起新的 hosts 或带 hosts 的防火墙阻断。
- [实现] 连接器不再内置待阻断域名列表，也不生成 hosts 映射。Windows 只设置 SV2 进程防火墙规则；macOS 继续提示在 LuLu 中手动配置。清理函数仅删除 Boxy 历史标记条目，保留用户映射。
- [验证边界] macOS `cargo test --locked -p boxy` 全部 21 项通过，`cargo check --locked -p boxy` 通过。macOS 上的 Windows MSVC 交叉检查因缺少 Windows SDK 的 `assert.h` 而停在 `ring` 构建脚本，尚未验证 Windows 产物；需 Windows runner 完成编译。
- [macOS 产物] `aarch64-apple-darwin` 与 `x86_64-apple-darwin` 的 release 构建均通过；合并为通用二进制并生成 Boxy 2.0.2 应用归档，Info.plist 版本与本地 codesign 验证通过。尚未安装到用户设备。
- [正式构建] GitHub Actions 运行 `36508223403` 的 Windows 与 macOS 作业均成功。Windows 作业完成 Rust 测试、MSVC release 构建、便携包打包与启动窗口检查；两个平台产物已上传至 2.0.2 草稿发布。草稿仍未公开，旧 2.0.1 仍为最新版。
