# 调查结果

- [现象] `connector/src/main.rs` 的固定授权 actions 已按 `iss` 选择 API base，但 `connector/src/assets.rs::fetch_product_logos` 的缓存未命中分支直接把 access token 送到 `DREAMTONICS_LICENSES_ENDPOINT` 国际常量。
- [边界] `fetch_product_logos_with` 只在 key 非空且本地 metadata 未覆盖 key 时调用 license loader；该惰性边界应保留，issuer 解析放入实际 fallback 之前。
- [安全] 区域选择复用固定 issuer allowlist 和固定 API base，不采纳任意 token issuer 作为 hostname。非法 issuer 时在请求 closure 前返回错误。
- [测试] 生产图片查询 wrapper 可通过窄 transport closure 注入 HTTP GET，并共享同一 endpoint selection path；不需要网络或更改一般 HTTP 层。
