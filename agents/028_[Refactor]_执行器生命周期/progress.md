# Progress

- 检查指定 worktree：分支 `codex/tauri-connector-core`，初始 HEAD `d604b29d18e54806f8606252c95d5c6d5a818d3e`，干净。
- 读取 agent-mode 技能、项目索引与任务索引。
- 只读定位现有main、session、launcher、network、tray和发布路径；尚未改业务代码。
- 合并 core 入口与生命周期控制；移除二进制入口，增加 `Config`、`ConnectorHost`、`prepare_config`、`run_connected` API。
- 每次运行在内存生成签名 key；用临时文件原子替换 session，CAS 哈希在临时文件同步后再次验证。
- 停止只在空闲/任务前生效；当前任务、session 写回和 signed receipt 完成后退出。shell取消会终止本次进程树并返回专用取消回执。
- shell输出以有界缓冲区读取；输出读线程等待使用命令 deadline 和 stop 检查，避免遗留子进程持有管道时阻塞关闭。
- 将 core 回归移至根 `/test`；`cargo +1.98.0 test --locked --no-default-features` 通过：43 library tests、6 host-block tests、6 launch-options tests，doctest 无用例。使用共享 `browser-only/target` 完成；本 worktree 默认 target 遇到并行构建的 linker LNK1104。
