# 区域产品接口修复计划

- [x] 核对 worktree、现有区域 token issuer 逻辑、connector Dreamtonics 请求入口和根目录测试组织。
- [x] 用固定 issuer 映射选择授权 API 主机，覆盖所有固定产品/许可 action。
- [x] refresh 后基于新 access token issuer 重选区域 host；非法 issuer 时不发送重试请求并保留已轮换凭据回传。
- [x] 为区域映射、固定 endpoint 路径及非法 issuer 添加根目录回归测试。
- [x] 使用 Rust 1.98.0 执行 `cargo test --locked -p boxy`，检查 diff 并提交任务分支。
