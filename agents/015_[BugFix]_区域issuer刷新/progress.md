# 操作记录

- 已在 `origin/main` 上建立独立分支 `codex/regional-issuer`，工作树位于指定目录。
- 已只读访问官方区域 OIDC discovery，并仅记录 issuer、token endpoint 和 JWKS endpoint。
- 已让手动刷新和 401/403 后自动刷新共用基于精确 issuer 的固定 endpoint 映射；未知 issuer 不会形成 URL。
- 已在根 `test/connector_regression.rs` 添加 `.com` / `.com.cn` 成功映射与未知域、HTTP、错误 realm、缺失/非法 client id 拒绝用例。
- 已将 connector package 与 Cargo.lock 更新至 2.0.5。
- `cargo +1.98.0 test --locked -p boxy --bin boxy`：30 passed。
- `cargo +1.98.0 build --locked --release -p boxy`：成功。
- 协调阶段完整 Cargo 回归 43 项通过，便携打包脚本测试通过。
- main 未保护；实现 5bc99e14740af3a63f8128cc09b38361287848e0 快进合并并推送，v2.0.5 标签固定该源码。
- 发行流水线 38686285436 针对标签完成 Windows/macOS 回归、构建、打包及 Windows 实际发行包启动检查，全部成功。
- v2.0.5 在 2026-10-15T05:26:21Z 公开；Windows exe SHA-256 为 9fd4a554632f8e49fd318d7610f4d6e0f1b3d68507a04ab31763309de2e3e45b，macOS universal ZIP 为 bdf1249c08e94eb7b8bcb3a4cffbf3e5e916d0da37e059d416c6e9f9f491b6f5。
