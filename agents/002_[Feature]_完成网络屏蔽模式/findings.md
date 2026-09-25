# 调研与决策

- 初始代码已迁入本仓库并提交；云端任务与 UI 在独立云端 worktree。
- 本机 /etc/hosts 当前有 Boxy 管理的 Dreamtonics 映射；现有下载地址单元测试依赖此实时状态而失败。
- Windows 11 虚拟机当前挂起。正式客户端构建需要独立验证 Windows 路径。
- 云端任务完成接口将执行失败作为 completed 任务的结果正文；状态读取页需显式处理 failed。
- 下载 URL 测试目前同时触发真实 hosts 屏蔽策略，导致同一输入在本机与 CI 上结果不同。将 URL 格式与 Dreamtonics 主机校验同实时 hosts 判定分开，保留产品下载时的屏蔽策略。
- 2026-09-25 用户确认新功能已提交远端 main。该 main 使用 Windows WFP 与私有 WebView2 运行时、macOS LuLu；旧客户端的 hosts/Windows Firewall 模块不应继续作为发行实现。
- 用户确认历史缓存主机扫描与勾选不纳入 2.0.0。Windows VM 已恢复原 suspended 与 Pause idle on 状态。
- 新版 main 已移除下载 URL 的本地 hosts 判断；此前为旧 hosts 测试分离解析函数的本地尝试被新版源码取代。
- 新版 main 在 macOS 构建时出现目录数组 PathBuf/Option<PathBuf 类型不一致；独立客户端修为两个 Option<PathBuf> 后，macOS 客户端全部 18 个 Rust 测试通过。
- Windows WebView2 打包样本测试通过；云端 Worker 31 个测试通过。
- 新 main 的 LuLu 实现未使用旧 hosts 规则作为运行时阻断；历史缓存主机勾选及 DNS 刷新任务不适用于 2.0.0 的进程级阻断范围。
- macOS 目录搜索数组的首项改为 Option<PathBuf> 后，客户端完整 Rust 测试 18/18 通过。随包图标由仓库资源读取，不依赖过滤历史中的旧 Tauri commit。
