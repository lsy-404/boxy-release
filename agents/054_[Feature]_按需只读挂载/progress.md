# 进展

- 聚焦提交完成通用 mount/unmount 任务及远端路径排除。
- 现有 Rust 58 library、4 main、5 host-block、9 launch-options 回归通过；desktop check 与格式检查通过。
- 本机独立数据根运行实际 connector，4 个只读挂载属于 2 个合成账户，未创建模型下载任务。
- 准备 v2.2.7 构建，既有 v2.2.6 draft 不发布。

- v2.2.7 draft 的 Windows build 报 E0596，修复后使用新未占用 v2.2.8 tag 重新完整构建，不移动既有 tag。旧 draft 不发布。
