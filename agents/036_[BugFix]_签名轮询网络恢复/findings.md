# Findings

- 原轮询调用在 `wait_for_writeback` 直接使用 `get_signed`；传输错误和所有非2xx状态都会立即退出连接器。
- `get_signed` 也服务于其他只读请求，本任务应通过新的 poll-only helper 限定重试范围；签名 POST、任务、写回和 receipt 保持原样。
- Worker 一度在首次返回 ready-for-writeback 时消费一次性 payload；该状态下网络重试不安全。Worker 当前修复契约为 receipt 前将 `writeback_delivered` 继续映射到相同 pending payload。集成回归模拟服务端在首次已处理 poll 后断开响应，并在重试时重复返回相同 payload。
- ureq `Response::into_json` 会将截断JSON包装为 `InvalidData`，无法区分传输中断与完整但语法错误的JSON。poll helper 改为先完整读取响应字节：reader IO 错误按传输故障有限重试；完整读取后 JSON decode 失败仍立即报错。
- 重试最多再发5次（总共最多6次 poll 请求）；退避为250、500、1000、2000毫秒并封顶2秒。每次调用生成新的 nonce/signature，等待阶段每50毫秒检查停止状态。临时HTTP状态仅为408、429、502、503、504。
- Loopback 端到端测试覆盖首次 abrupt EOF、一次 product_inspect task、已经处理但响应截断的 ready-for-writeback、服务端重复返回同一 payload、单次session写回及单次receipt；断言 start、task receipt、writeback receipt各一次且poll签名nonce不同。
