# 调研与决策

- 独立仓库的 `main.rs` 和 `service_selection.rs` 提供服务地址选择，主仓库当前没有此功能；迁移时保留独立入口。
- 主仓库 `host_block.rs` 包含按需下载、WFP 内存/权限修复及 LuLu profile 说明；独立仓库仍为打包随附运行时的旧实现。
- 主仓库的便携打包脚本已不再要求固定 WebView2 Runtime；独立仓库工作流和 README 仍要求随包下载。
- [macOS Rust 编译报 E0308] -> 原样移植的 `macos_sv2_executable` 将 `PathBuf` 与 `Option<PathBuf>` 混入同一数组 -> 恢复首项的 `Some(...)`；上游当前源码也存在此问题。
- 本地 `cargo test --locked -p boxy` 通过 19/19；Windows 包样本测试通过。`cargo build --locked --release -p boxy` 与 macOS arm64 app/zip 打包、签名和归档检查通过。
- 源码快照和既有 Git 历史的 gitleaks 扫描无命中；包含编译输出的首次目录扫描有 2 个命中，未将编译输出纳入待发布源码。
- Objective-See 官方 v4.5.1 release 提供 `LuLu_4.5.1.dmg`，实际下载的 7,251,712 字节及 SHA-256 均与代码固定值一致。Microsoft CAB 地址 HEAD 返回 200，`Content-Length` 为 308,509,880 字节；本次未下载 308 MB CAB，Windows 安装路径需在目标平台进一步验证。
- 最终 Windows 与 macOS GitHub Actions runner 均通过 Rust 测试、release 编译和发行打包。Windows ZIP 内没有 WebView2 Runtime，符合启用屏蔽时按需下载的发行方式。
