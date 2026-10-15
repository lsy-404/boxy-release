# Boxy Release 任务追踪
> 准则版本: v0.2.2

## 任务列表

| 编号 | 任务名称 | 任务描述 | 变更动机 | 状态 |
| :--: | :------: | :------: | :------: | :--: |
| 001 | [Architecture] 独立发行仓库迁移 | 承接本地连接器，提供服务地址选择及独立构建 | 与云端 Worker 仓库分离并支持按文件名预填远程服务 | ✅ 已完成 |
| 002 | [Feature] 完成网络屏蔽模式 | 整合 Windows WFP、专属 WebView2 和 macOS LuLu 客户端实现 | 将 main 上的新功能纳入独立客户端并保持可构建 | ✅ 已完成 |
| 003 | [Release] 2.0.0 正式构建 | 在功能验证后构建 macOS 和 Windows 便携客户端 | 用户要求正式版 2.0.0 构建 | ✅ 已完成 |
| 004 | [Feature] 按需依赖与网络屏蔽迁移 | 移植运行时下载及后续网络屏蔽修复，保留独立客户端的服务选择 | Windows/macOS 依赖仅在启用时获取 | ✅ 已完成 |
| 005 | [Feature] 文件名直取服务地址 | 用有效 exe/app 文件名直接预填服务域名 | 用户取消文件名自动补全 boxy 前缀 | ✅ 已完成 |
| 008 | [BugFix] 移除 hosts 阻断方案 | 停止客户端写入系统 hosts 并保留受支持的进程级阻断 | 用户要求删除 host 阻断方案 | 🔄 进行中 |
| 009 | [PackageManagement] pnpm CLI migration | Migrate connector UI package to pnpm | Keep supported Node.js workflows consistently on pnpm | ✅ 已完成 |

| 010 | [Maintenance] Kit依赖升级 | 更新已使用的公共 kit 包和锁文件 | 使用最新已发布的共享组件 | ✅ 已完成 |
| 011 | [Feature] Dreamtonics授权请求 | 将授权服务请求、安全响应回传与过期 token 刷新放入本机 connector | Worker 直连 Dreamtonics 返回 403；离线写回需先读取许可信息 | ✅ 已完成 |
| 012 | [Feature] Session 持续监视与自动写回 | 监听本机会话变化、执行请求前 CAS，并在自动写回后持续服务 | 每次授权请求必须基于本机最新 session 且网页不再手动保存 | ✅ 已完成 |
| 013 | [Verify] 连接器回归 | 恢复一代路径选择、快速登录和自定义请求并运行完整本机回归 | 二代必须保持一代除阻断和门禁外的全部能力 | ✅ 本地验证通过；发行 CI 待验证 |
| 014 | [Release] 一代功能补齐发布 | 构建并公开发布 v2.0.4 Windows 与 macOS 包 | 分发路径选择及请求语义修复后的客户端 | 🔄 进行中 |
