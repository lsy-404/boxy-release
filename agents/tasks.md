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
| 008 | [BugFix] 移除 hosts 阻断方案 | 停止客户端写入系统 hosts 并保留受支持的进程级阻断 | 用户要求删除 host 阻断方案 | ✅ 评估完成，用户已确定实施范围 |
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

| 026 | [Research] Tauri控制页评估 | 核实内置控制页的结构、平台依赖及交付影响 | 用户考虑Tauri方案 | ✅ 评估完成，用户已确定实施范围 |

| 027 | [Feature] 内嵌云端界面 | Tauri承载云UI、临时运行数据与退出清理 | 用户要求完整内嵌和单文件 | ✅ v2.2.0两平台已公开，真实Windows启动/导出/清理验证通过 |
| 028 | [Refactor] 执行器生命周期 | 可取消core、内存签名密钥和原子写回 | 配合原生窗口关闭且不留下应用数据 | ✅ 已完成，本地回归通过 |
| 029 | [Test] 内嵌便携发行 | 单文件打包及真实页面启动关闭验证 | 验证交付与缓存清理 | ✅ Windows59项/macOS57项及两平台构建通过，v2.2.0已公开 |

| 030 | [Test] Blob导出 | 真实WebView导出保存与退出数据清理验证 | 确认云端导出在内嵌窗口正常工作 | ✅ 已完成，正式exe端到端验证通过 |

| 031 | [Maintenance] 发行与历史清理 | 备份后压缩公开历史，仅保留最新Release及简短说明 | 用户明确要求清理旧commit/Release | ✅ 已完成，单一root提交及最新Release/附件已核验 |
| 032 | [Maintenance] 旧发行清理 | 备份旧资产并精简公开Release列表 | 只保留最新版本 | ✅ 9个旧Release已备份删除，最新说明与附件已验证 |

| 033 | [BugFix] 主动选择服务地址 | 移除默认URL并恢复空白URL选择页 | 用户明确要求主动选择服务 | ✅ v2.2.1两平台已公开 |
| 034 | [Refactor] 显式地址启动 | 可选URL、来源优先级及延迟session解析 | 避免用户确认前读取/上传session | ✅ 已验证 |
| 035 | [Test] 地址选择运行验证 | 验证无默认地址、选择连接和退出数据清理 | 对真实发行程序验收 | ✅ 本地及发行CI真实Windows验收通过 |

| 036 | [BugFix] 轮询网络恢复 | 在可重放写回响应基础上仅重试临时GET错误 | 中国区旧版TLS EOF中断轮询 | ✅ v2.2.1已公开，配套Worker已部署 |

| 037 | [BugFix] macOS启动白屏 | 修复本地tauri入口空路径误拦截 | 用户启动即无地址选择页 | ✅ v2.2.2两平台已公开 |
| 038 | [Test] macOS真实页面验证 | 渲染地址页并执行真实表单服务连接 | 发行构建不能代替WK运行验证 | ✅ 真实WK运行及发行CI通过 |
