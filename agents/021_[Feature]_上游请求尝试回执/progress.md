# 操作记录

- 从干净 `main` (`86b51fb`) 创建独占 worktree `codex/upstream-attempt-log`。
- 确认固定 API 调用集中在 `dreamtonics_request`，refresh 经 `refresh_access`，图片许可证及 CDN GET 在 `assets.rs`；最终 result 经 directory task `/complete` 回传。
- 新增 `request_trace.rs`，将尝试数据结构、正文预算、刷新响应 token 字段剥离和记录包装器集中在独立模块。
- 主请求、refresh、401/403后的refresh与retry记录实际调用时使用的endpoint；图片补全licenses失败和CDN下载失败也保留attempt metadata。
- attempt正文最多64 KiB/条、256 KiB/任务，UTF-8边界截断；请求内容不入日志，refresh响应移除access/refresh/id token字段，图片正文只记录binary元数据。
- 完成6项新增回归测试，包括中国区请求顺序、网络失败、刷新凭据保留、图片失败、正文预算及JSON序列化大小；完整 `cargo +1.98.0 test --locked -p boxy` 通过：39 connector + 6 host block + 6 launch tests。
- 序列化边界测试在2 MiB主响应和256 KiB attempt正文下测得JSON约14至15 MB；服务端complete route需设置相应有界输入上限，不能沿用约1.4 MB的现有上限。
