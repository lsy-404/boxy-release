# 调查结果

- [现象] `dreamtonics_request` 将固定许可与产品 action 发送到 `.com`；中国官网公开脚本按 `.cn` 页面域名选择 `https://authr3.dreamtonics.com.cn/api/v1`，且 `fetchMyLicenses` 调用 `/client/my_licenses` -> 固定 endpoint 需要按 issuer 分区。
- [约束] `refresh_authority` 已将 refresh JWT `iss` 精确映射到两条固定 token endpoint；相同的精确 allowlist 模式可用于 access JWT `iss`，不能用 claim 内容动态拼域名。
- [刷新语义] 401/403 后若刷新产生不同区域的 access token，必须将固定 endpoint 从旧 allowlist base 重映射到新 allowlist base；issuer 缺失或非法时禁止发送 retry，并回传已轮换凭据以保留 session 状态。
- [边界] `custom_request` 的 URL 由用户显式提供；本地 `product_logos` action 走单独本地处理分支，不要求区域 issuer。
- [证据] 官方中国站脚本 `https://my.dreamtonics.com.cn/assets/index-CsPzgODo.js` 暴露 `.com.cn` API base、`/client/my_licenses` 相对路径和 Keycloak 中国区 URL/realm/client ID。未调用真实许可证接口。
- [实现] Connector 按 JWT payload `iss` 精确映射到两个固定 API base，要求固定授权 action 的 access token 带受信 issuer；refresh 后重新映射并替换 endpoint base。非法的新 issuer 时不重试，仍回传已轮换凭据。
- [验证限制] 当前 HTTP helper 没有可注入的传输层；refresh 后实际网络重试未做模拟测试。endpoint base 区域切换由纯函数覆盖，完整 Windows 测试通过。实际大陆账户的在线许可证结果仍需在官方客户端环境验证。
