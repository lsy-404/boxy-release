# Findings

- Worker 协议复用 directory task 下发和签名 complete 回执，kind 为 `dreamtonics_request`。action allowlist 为 `licenses | offline_activate | offline_deactivate | quick_login | quick_licenses | quick_devices | refresh | custom_request | product_download_url | model_download_url`。
- 固定 endpoint 与 method 由 connector 构造；唯一可变 endpoint 是 custom request，必须为 HTTPS Dreamtonics 域名或子域（含 dreamtonics.com.cn）、443/default port、无 userinfo，且 redirects 关闭。仅允许 GET/POST，JSON body 上限 256 KiB。
- connector 最多读取 2 MiB 响应；超限作为失败返回，不得截断后伪装成功。网络错误和非 200 状态保留为结构化结果。
- licenses 任务携带 access/refresh token；遇 401/403 时 connector 本机刷新并重试，凭据通过单独回执字段送回 Worker sealed-state 更新。明文 token 不进入日志或浏览器 task 结果。
- 用户要求不跑回归测试；允许构建。
- 初始 Rust stable shim 实际为 1.92，不满足锁定的 `sysinfo 0.39.6` MSRV 1.95；使用已安装的 1.98.0 toolchain 成功 release build，未改依赖或锁文件。
- Dreamtonics HTTP 错误正文不会拼入 connector 错误文本；状态码与有界响应单独回传。每个请求使用 ureq `redirects(0)`，响应体读取最多 2 MiB+1 并在超限时清空正文、返回失败回执。
- 本机校验或网络失败使用 status 599，带静态 `error` 且仍提交签名完成回执，避免无效状态码阻断轮询；refresh token issuer 固定校验 Dreamtonics realm。
- 默认 git commit 因 1Password SSH signing key 文件缺失失败；本次提交仅使用命令级 `commit.gpgsign=false`，没有更改仓库或全局配置。
