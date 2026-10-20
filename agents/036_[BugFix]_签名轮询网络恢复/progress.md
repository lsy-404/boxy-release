# Progress

- 确认当前分支为 `codex/service-selection-core`，上一个服务选择提交已完成，工作树干净。
- 读取现有 poll、签名 POST、session CAS 与 core loopback 测试结构；确认重试必须被限制在 `wait_for_writeback` 的签名 GET。
- Worker 修复契约已由主 agent 确认：receipt 前重复读取 writeback poll 可获得相同待写回内容；开始实现 poll-only bounded retry 和端到端回归。
- 在 `wait_for_writeback` 使用仅 poll 的有限重试 helper；不调整 start、session snapshot、任务、写回或 receipt 的 POST 调用。
- 对 call transport、响应 body reader IO、408/429/502/503/504做退避恢复；完整 body 的无效JSON和其他HTTP状态仍立即失败。任何成功解析的poll都会结束该 helper，本次失败计数不会带入下一次poll。
- 定向端到端回归通过，并运行 `cargo +1.98.0 test --locked --no-default-features`：48 library tests、6 host-block tests、9 launch-options tests、doctest无用例全部通过。
