# 调查结果

- 当前 `DreamtonicsRequestResult` 只表示一个逻辑 action 的最终结果；task complete 回执走现有签名 HTTPS JSON 通道。签名覆盖方法、路径、nonce，不覆盖回执 body；本任务按要求不改签名协议。
- 普通 request、自动 refresh/retry、显式 refresh、图片内部 licenses 查询和 CDN 图片 GET 分别处于不同调用层。日志需在实际 HTTP 执行点记录，不能在最终结果中按 issuer 推导 endpoint。
- refresh 请求体包含 refresh token；该内容不得进入 attempt。刷新 response trace 需要先将 access_token/refresh_token 字段移除，原始响应仍只供本机解析。
- 图片回执不应把二进制 image body 塞入 trace，应记录 status、耗时、endpoint 和明确的 binary body 标记。
- 每次 attempt 正文单条上限64 KiB、总预算256 KiB；超出按 UTF-8 边界截断并置标志。attempt 元数据不受正文预算影响；任务内的图片下载由已有512 logo限额约束。
