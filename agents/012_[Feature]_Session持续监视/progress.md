# Progress

- 已完成 2 秒 bounded polling 的 session hash watch，并通过签名 snapshot endpoint 上报本地变更。
- Dreamtonics 请求发送前执行 expectedSessionSha256 CAS；不同步时先报告新快照，再提交 session_changed 回执。
- 自动写回支持源文件冲突回执与快照重同步；签名成功回执后持续轮询，不重复上报 connector 自己写入的 session。
- `cargo fmt --all -- --check` 与 `cargo build --locked --release -p boxy` 通过；未运行回归测试（按要求）。
- `git diff --check` 通过，变更敏感关键词检查无命中。
