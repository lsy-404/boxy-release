# Findings

- 当前固定服务 URL 位于 `connector/src/launch.rs`；命令行、环境变量和可识别文件名推断优先于该默认值。
- 当前 `prepare_config` 同时解析 URL、校验/弹出 session 文件选择，因此无法让桌面 UI 先等待用户选择服务。
- Tauri shell 与版本/构建配置属于其他 agent 的工作范围；本变更只提供主线程可调用的 launch/config API。
