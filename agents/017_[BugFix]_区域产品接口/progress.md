# 操作记录

- 检查指定 worktree 为 `codex/regional-api`，基于 `b772625`，开始时工作区干净。
- 阅读 `agent-mode`、仓库项目索引、任务索引及 connector 请求实现。仓库没有 `agents/local.instructions.md`。
- 在 `connector/src/main.rs` 增加两个固定区域 API base；对固定授权 action 解析 access token issuer 并选择对应 base，拒绝 issuer 缺失、格式错误或不受信的 token。
- 对 401/403 refresh 后返回的 access token 重新解析 issuer；有效的区域变化会重选 endpoint，不合法 issuer 或 endpoint 则阻止 retry 并将轮换凭据放入失败结果。
- 在 `test/connector_regression.rs` 添加端点逐项映射、允许/拒绝 issuer、网络前拒绝、区域重映射和本地 product logos 不需 JWT issuer 覆盖。
- 运行 `cargo +1.98.0 fmt --all` 和 `cargo +1.98.0 test --locked -p boxy`；34 个 unit、6 个 host-block、7 个 service-selection 测试通过。
- 执行 `git diff --check` 和禁用任务号/模型署名自检后，提交分支 `codex/regional-api`，commit `ae42cc2`。
