# Session 持续监视与自动写回

- [x] 对齐 signed snapshot、request CAS、writeback conflict 与长驻轮询状态协议。
- [x] 实现本机会话变化上报、请求前 CAS 与冲突处理。
- [x] 写回 receipt 后恢复轮询并避免重复报告 connector 自己的写入。
- [x] 运行格式检查与 release build，检查变更并提交。
