# SV2 网络屏蔽

Boxy 的网络控制由本地客户端执行。浏览器发起状态读取或切换任务后，等待客户端回传实际结果；排队成功不代表规则已经生效。

## Windows

客户端使用 Windows Filtering Platform 为 SV2 主程序及随 Windows 便携包提供的专属 WebView2 Runtime 建立 IPv4 和 IPv6 出站阻断规则。切换前先关闭 SV2。状态分开报告规则是否配置、四条规则是否存在，以及 WebView2 覆盖是否已验证。未能观察到未来进程的环境变量或 AUMID 时，不把已配置误报为已验证。

Windows 便携包必须完整包含 Microsoft WebView2 Fixed Runtime。构建时需固定官方 CAB 的 URL、四段版本和 SHA-256，校验后解压打包。解压 ZIP 后保留 exe 与 WebView2Runtime 的相邻目录关系。

## macOS

客户端生成 LuLu 4.5+ 的 Process + Kids 导入规则，由用户在 LuLu 中导入。生成规则文件不代表拦截已生效。旧版 Boxy hosts 条目仅在明确清理操作中按标记移除，不修改其他 hosts 映射。

## 验证边界

Windows 规则状态与历史 hosts 清理可通过客户端测试和真实 Windows 环境核查；macOS 可核对规则文件、导入说明和旧条目清理。完整离线结果仍需在目标 SV2 安装和网络环境中观察实际连接。
