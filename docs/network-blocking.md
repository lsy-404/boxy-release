# SV2 网络屏蔽

Boxy 的网络控制由本地客户端执行。浏览器发起状态读取或切换任务后，等待客户端回传实际结果；排队成功不代表规则已经生效。

## Windows

客户端使用 Windows Filtering Platform 为 SV2 主程序及其专属 WebView2 Runtime 建立 IPv4 和 IPv6 出站阻断规则。切换前先关闭 SV2。状态分开报告规则是否配置、四条规则是否存在，以及 WebView2 覆盖是否已验证。未能观察到未来进程的环境变量或 AUMID 时，不把已配置误报为已验证。

Windows 便携包只包含 Boxy 可执行文件。首次启用屏蔽时，客户端从 Microsoft 下载版本固定的 x64 WebView2 Fixed Runtime CAB，验证大小和 SHA-256 后解压到 Boxy 所在目录的 `WebView2Runtime`；后续复用已验证的目录。下载失败时不安装新的屏蔽规则。Boxy 目录需可写。

## macOS

客户端生成 LuLu 的 Process + Kids 导入规则。若未检测到 LuLu，生成规则时从 Objective-See 下载固定版本的安装镜像，并校验大小和 SHA-256；用户仍需安装 LuLu、批准系统扩展，并手动导入规则。导入前应建立并启用单独的 Boxy profile，确认其中没有其他用户规则，因为 LuLu 导入会替换当前 profile 的用户规则。生成文件和下载镜像都不代表屏蔽已经生效。切回 Default profile 可解除屏蔽，但会切换整台 Mac 的活动规则和设置。旧版 Boxy hosts 条目仅在明确清理操作中按标记移除，不修改其他 hosts 映射。

## 验证边界

Windows 规则状态与历史 hosts 清理可通过客户端测试和真实 Windows 环境核查；macOS 可核对规则文件、导入说明和旧条目清理。完整离线结果仍需在目标 SV2 安装和网络环境中观察实际连接。
