# 发现

- macOS 使用内置 NFS，Windows 使用 WinFsp；仅后端和 pinned helper 可用时公布能力。
- 使用官方 rclone 1.75.2，验证 archive SHA256，原始二进制随发行内嵌，MIT COPYING 与解出的 helper 并列保存。
- VFS cache off、buffer 0、固定 1MiB bounded Range；模型仅在线读取，小目录与图标可本地保存。
- 只操作 marker、路径、PID、启动时间与 executable 全部匹配的挂载；快照扫描跳过活动远端目录。
- 实际 macOS HTTP-backed mount 读取两份原生文件 SHA256 与源相同；Windows 编译与实际后端须分别计证。

- Windows release 编译发现 ReadDir closure 缺 mut；同时发现原 PowerShell 回归步骤会被后续成功命令覆盖失败状态。修正 closure 并开启 native-command fail-fast，避免虚假通过。
