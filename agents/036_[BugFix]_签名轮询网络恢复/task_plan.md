# 签名轮询网络恢复

## 目标

- [x] 仅对签名 operation poll GET 的传输错误与指定临时 HTTP 状态添加可取消、有限退避重试。
- [x] 每次尝试生成新的 nonce 和签名；HTTP/协议/JSON永久错误保持失败。
- [x] 回归覆盖 abrupt EOF、服务端重复提供 pending writeback、失败上限、取消、401/无效 JSON，以及 start/task/receipt POST 不重放。
- [x] Worker 保证首次回执前重复 poll 会返回相同 pending writeback 内容后，再提交客户端变更。
- [x] 完成检查并创建独立提交，等待主 agent 协调跨仓验收。

## 范围

只修改 `connector/src/lib.rs`、根 `/test` 与本任务审计文件；不修改桌面壳、其他请求方法或 Worker。
