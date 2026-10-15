# 操作记录

- 已在 `origin/main` 上建立独立分支 `codex/regional-issuer`，工作树位于指定目录。
- 已只读访问官方区域 OIDC discovery，并仅记录 issuer、token endpoint 和 JWKS endpoint。
- 已让手动刷新和 401/403 后自动刷新共用基于精确 issuer 的固定 endpoint 映射；未知 issuer 不会形成 URL。
- 已在根 `test/connector_regression.rs` 添加 `.com` / `.com.cn` 成功映射与未知域、HTTP、错误 realm、缺失/非法 client id 拒绝用例。
- 已将 connector package 与 Cargo.lock 更新至 2.0.5。
- `cargo +1.98.0 test --locked -p boxy --bin boxy`：30 passed。
- `cargo +1.98.0 build --locked --release -p boxy`：成功。
