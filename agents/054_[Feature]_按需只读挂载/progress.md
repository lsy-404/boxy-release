# 进展

- 聚焦提交完成通用 mount/unmount 任务及远端路径排除。
- 现有 Rust 58 library、4 main、5 host-block、9 launch-options 回归通过；desktop check 与格式检查通过。
- 本机独立数据根运行实际 connector，4 个只读挂载属于 2 个合成账户，未创建模型下载任务。
- 准备 v2.2.7 构建，既有 v2.2.6 draft 不发布。

- v2.2.7 draft 的 Windows build 报 E0596，修复后使用新未占用 v2.2.8 tag 重新完整构建，不移动既有 tag。旧 draft 不发布。

- v2.2.8 Windows/macOS 均通过现有回归、release build、portable packaging 和已有界面/startup smoke；资产已发布，两个公开下载地址均 200。
- 实际 macOS 两个借入账户的包更新、旧挂载清理、退出无完整模型残留与离线撤销后重连清理均通过。临时进程、NFS、registry、监听和标签已清理。
- Windows WinFsp 实机挂载和 SV2 原生合成尚未验证，不以 build/smoke 代替该证据。
- 清理作业在 draft 阶段读取 release tag 返回 404，整个 run 标记 failure；两平台构建 job 均 success。未重跑删除旧公开版本的清理。仅删除本任务未发布的失败 draft，保留 tag 和既有公开版本。
