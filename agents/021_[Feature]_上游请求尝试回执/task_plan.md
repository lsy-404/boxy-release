# 上游请求尝试回执计划

- [x] 从当前 `main` 创建独占 worktree，检查请求、刷新、图片补全和任务完成协议。
- [x] 增加有界、无请求凭据的 attempt 数据结构与共享 HTTP 记录器，覆盖固定 API、refresh、retry、my_licenses 和图片下载。
- [x] 在逻辑任务完成回执中携带有序 attempt 列表，保留 legacy 最终 responseBody；失败路径也记录 attempt 并保留已刷新的凭据。
- [x] 在根目录 `/test` 添加主请求401→refresh→retry顺序/区域、网络失败、图片失败、刷新响应凭据剥离及截断测试。
- [x] 使用 Rust 1.98.0 和指定缓存运行完整 `cargo test --locked -p boxy`，检查差异并本地提交，不推送。
