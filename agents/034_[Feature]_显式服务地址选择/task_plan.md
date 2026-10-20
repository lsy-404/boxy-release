# 显式服务地址选择

## 目标

- [x] 移除固定服务 URL，启动解析返回可选地址。
- [x] 导出 `LaunchOptions`，使桌面主线程可等待用户选择服务地址。
- [x] 仅在地址选择后构建并校验完整 `Config`，再解析 session 路径。
- [x] 更新根 `/test` 启动选项回归。
- [x] 完成格式、测试、自检并提交本地分支。

## 范围

只修改 `connector/src/launch.rs`、`connector/src/lib.rs`、根 `/test` 与本任务审计文件；不改桌面 shell、配置、构建脚本、版本或任务索引。
