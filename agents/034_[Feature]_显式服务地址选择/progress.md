# Progress

- 检查 `service-selection-core` worktree：分支 `codex/service-selection-core`，基于 `3633f54`，初始状态干净。
- 读取 agent-mode 技能、仓库指令、项目索引与任务索引。
- 已确认当前固定 URL 默认值和 `prepare_config` 内 session picker 的调用路径；准备拆分启动选项解析和最终配置解析。
- 移除固定服务 URL；CLI、环境变量、可识别可执行文件名仍按原优先级解析。没有可推断 host 时返回 `server_url: None`，命令行或环境变量提供的无效值直接报错。
- 导出 `LaunchOptions`。`prepare_config` 仅解析参数和默认 session 路径；`resolve_config` 在主线程收到用户选择后校验 URL 并检查/选择 session 文件。
- 更新根 `/test/launch_options.rs`；`cargo +1.98.0 test --locked --no-default-features` 通过：43 library tests、6 host-block tests、9 launch-options tests，doctest 无用例。
