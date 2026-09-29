# 操作记录

- 2026-09-28：获取发行仓库远端，检查工作树与未提交审计，建立独立工作树。读取 hosts 写入、WFP、LuLu、云端任务协议和测试。
- 2026-09-28：将 hosts 模块缩为精确标记清理，删除阻断域名列表与新增映射路径；Windows 使用进程防火墙，macOS 保留 LuLu 手动说明。连接器状态报告 `hostless`，任务仅接受状态、进程规则及旧规则清除。
- 2026-09-28：版本提升至 2.0.2，更新发行说明和网络阻断文档。`cargo fmt --check`、21 项 Rust 测试和 macOS `cargo check` 通过；Windows 交叉检查被当前 Mac 缺失的 SDK 标头阻断，尚未确认 Windows 构建。
- 2026-09-28：分别完成 arm64 与 x86_64 macOS release 构建，合并通用二进制并打包 `dist/boxy-macos-universal.zip`；Info.plist 显示 2.0.2，`codesign --verify --deep --strict` 通过，未替换用户安装的 Boxy。
- 2026-09-28：发行客户端提交 `6456ba2` 已推送到无保护规则的远端 `main`。创建不公开的 2.0.2 草稿发布并以该提交启动 GitHub Windows/macOS 发行构建；等待产物与 Windows 启动检查。
- 2026-09-28：发行构建 `36508223403` 完成且成功，macOS 通用 zip 与 Windows x64 exe 均在草稿中；Windows 启动窗口检查通过。已将草稿交给用户审阅，等待是否公开发布的明确答复。
