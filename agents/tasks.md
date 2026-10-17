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
| 013 | [Verify] 连接器回归 | 恢复一代路径选择、快速登录和自定义请求并运行完整本机回归 | 二代必须保持一代除阻断和门禁外的全部能力 | ✅ 本地及跨平台发行 CI 通过 |
| 014 | [Release] 一代功能补齐发布 | 构建并公开发布 v2.0.4 Windows 与 macOS 包 | 分发路径选择及请求语义修复后的客户端 | ✅ 已公开发布 |
| 015 | [BugFix] 区域 issuer 刷新 | 按经确认的签发方选择固定区域 token endpoint 并保留 issuer 限制 | 支持 Dreamtonics 中国区签发的已验证会话 | ✅ 已公开发布 v2.0.5 |
| 016 | [BugFix] 系统浏览器启动 | 移除本地内嵌 WebView UI，创建授权操作后使用系统浏览器 | 避免 connector 初始化 WebView2 用户数据目录 | ✅ 本地验证通过 |
| 017 | [BugFix] 区域产品接口 | 按 access token 的精确 issuer 选择固定国际或中国区产品 API 主机，并在刷新后重新选择 | 中国官网使用 `authr3.dreamtonics.com.cn`，公开 connector 原先固定使用国际主机 | ✅ 已完成，本地验证通过 |
| 018 | [Release] 系统浏览器与区域 API | 验证并发布去内嵌浏览器与中国区 API 的新连接器 | 完成用户要求的可运行交付 | ✅ v2.0.6 已公开，两平台 CI 与 Windows 启动验证通过 |
| 019 | [BugFix] 区域图片查询 | 图片补全读取许可证时按 access token 精确 issuer 选择固定区域 endpoint | 图片本地缓存未覆盖时仍有代码路径将 access token 发往国际许可 API | ✅ 已完成，本地验证通过 |
| 020 | [Release] 区域图片请求 | 发布区域图片许可证请求修复 | 完成所有固定认证 API 的区域一致性 | ✅ v2.0.7 已公开，两平台 CI 与 Windows 启动验证通过 |
| 021 | [Feature] 上游请求尝试回执 | 将固定 API、刷新/重试及图片附属 HTTP 尝试逐条放入签名任务结果 | 服务端需要记录连接器发出的每次请求及其返回状态/正文 | ✅ 已完成，本地验证通过 |
| 022 | [Release] 本机请求尝试日志 | 发布逐次上游请求尝试回传 | 使服务端能区分下发、实际请求和收到回执 | ✅ v2.0.8已公开，两平台CI和Windows启动验证通过 |
| 023 | [Feature] 通用请求执行器 | 由服务端下发 protocol v3 通用 HTTP 与本地文件读取任务，客户端负责执行、日志和本地安装 | 移除客户端 Dreamtonics endpoint、issuer、参数及 refresh/retry 业务编排 | ✅ 已完成，本地验证通过 |
| 024 | [Release] 服务端编排协议 | 验证通用执行器并发布基础协议升级 | 普通业务调整不再要求更换客户端 | ✅ v2.1.0已公开，配套Worker已上线 |
| 025 | [BugFix] Session path identity aliases | 用文件身份保护会话并保存错误响应原文 | 关闭大小写别名和junction逃逸边界 | ✅ 已完成，50项本地测试通过 |
