# Findings

- 初始握手已读取并发送 session ciphertext 与 SHA-256；后续无本机文件 watcher。现有 connector poll 周期为 2 秒，因此采用 bounded polling 检测 hash 变化，不引入新依赖。
- Worker 协议：签名 POST `/api/v2/connector/operations/:id/session-snapshot`，请求 `{ciphertext,sourceSha256}`；成功确认 `{state:'accepted',sourceSha256}`。`writeback_pending` 时不更新本机已上报 hash。
- Dreamtonics task 携带 `expectedSessionSha256`。发送上游前本机重读并 hash CAS；冲突时先上报当前快照，再 complete `{action,status:409,responseBody:'',error:'session_changed'}`。
- 自动写回继续使用 `ready_for_writeback`；本机源 hash 冲突时签名回执 `{sourceSha256,error:'source_changed',observedSha256}`，Worker 清除 pending writeback 并恢复 editor_active。成功写入后 receipt ack 为 `writeback_completed`，connector 更新 last reported hash 并继续 poll/watch。
- Worker 每次 action 前检查本机最新快照的 access expiry；接近到期则先下发 refresh，接收新 token 后自动加密写回，等待签名 receipt 后才派原 action。
- 用户明确禁止回归测试；允许格式检查和构建。
