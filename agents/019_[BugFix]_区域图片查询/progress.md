# 操作记录

- 检查指定 worktree `codex/regional-logo-api`，起始 commit `1ac38cb`，工作区干净。
- 确认历史区域 API 修复在 `connector/src/main.rs`，图片 fallback 位于 `connector/src/assets.rs`，并有 `fetch_product_logos_with` 许可证 loader 与图片 downloader 注入点。
- 在共享图片查询流程中，将 issuer endpoint resolver 放在 key normalization 与本地 metadata 覆盖检查之后；无缓存时才验证 issuer，解析错误直接返回且不会调用 HTTP transport。图片查询仍保持其许可证请求失败时返回已取到的本地图片的行为。
- 新增窄 transport 注入 wrapper，生产代码和回归测试共同使用。测试明确断言国际 `https://authr3.dreamtonics.com/api/v1/client/my_licenses`、中国 `https://authr3.dreamtonics.com.cn/api/v1/client/my_licenses`，恶意 issuer 在 transport 前失败，以及本地缓存覆盖时无效 token 也可成功。
- 认证 API 调用扫描确认：固定授权 action 统一在 `dreamtonics_request` 中按 token issuer 选区域 base；图片补全是唯一独立许可证 HTTP 调用，现已改为同一固定 issuer/base 映射。Refresh 使用独立的精确 issuer token endpoint 选择；`custom_request` 保持显式用户 URL 语义；未发现其它直接固定国际 auth API 请求入口。
- 已运行 `cargo +1.98.0 fmt --all` 与 `cargo +1.98.0 test --locked -p boxy`，指定复用 browser-only target：主 connector 32 项、`host_block_status` 6 项、`launch_options` 6 项，均通过。仅有现存 dead-code warnings。
- 尚未提交：先完成 diff / 敏感关键词自检，再本地提交到 `codex/regional-logo-api`，不推送。
