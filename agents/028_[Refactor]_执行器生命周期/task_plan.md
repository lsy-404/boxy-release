# 执行器生命周期 Core

## 目标

- [x] 将连接器协议和本机任务逻辑抽到 library crate。
- [x] 固定公开 Config、prepare_config、elevated fastpath、ConnectorHost 和 run_connected API。
- [x] 支持停止轮询与等待；session 替换/receipt关键段完整结束后再停。
- [x] 将签名 key 限定在单次进程内，session替换使用临时文件原子替换。
- [x] 保留授权、generic HTTP、文件任务、CAS、安全审批和平台能力语义。
- [x] 在根 `/test` 添加生命周期、取消和写回回归。
- [x] 运行适当测试/构建并提交本地分支。

## 边界

不编辑 `main.rs`、Cargo/build/desktop/打包文件或 `agents/tasks.md`。Tauri host 实现由主 agent负责。
