# 调研记录

- 公共 OIDC discovery `https://account.dreamtonics.com.cn/realms/Dreamtonics/.well-known/openid-configuration` 返回 `issuer=https://account.dreamtonics.com.cn/realms/Dreamtonics`、`token_endpoint=https://account.dreamtonics.com.cn/realms/Dreamtonics/protocol/openid-connect/token`、`jwks_uri=https://account.dreamtonics.com.cn/realms/Dreamtonics/protocol/openid-connect/certs`。
- `connector/src/main.rs` 的 `refresh_access` 同时服务手动 refresh 与 401/403 后自动 refresh；现有 `refresh_client_id` 只接受 `.com` issuer，且 token endpoint 固定为 `.com`。
- 明确 issuer 到固定 endpoint 的映射可以支持 `.com` 与 `.com.cn`，并拒绝其他 issuer；不应从 token 的任意 `iss` 拼接 URL。
- 刷新 action 分支在调用 `refresh_access` 前返回；该分支设置的旧静态 endpoint 未实际发请求。
- 公开 Dreamtonics 文档没有发现 `my_licenses` 区域 API 映射；无认证 GET 对官方 `.com` 与 `.com.cn` 路由均返回 401，证明路径响应但不能证明凭据应发往哪个区域，因此维持现有 API host。
- 以 Rust 1.98.0 执行 `cargo test --locked -p boxy --bin boxy`，30 项通过；`cargo build --locked --release -p boxy` 成功。系统默认 Rust 1.92.0 不满足锁定依赖 `sysinfo 0.39.6` 的 1.95 MSRV，验证改用已安装的 1.98.0。
