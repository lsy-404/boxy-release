# Findings

- Tauri/Wry 在 Windows handler 中将自定义 HTTPS URI 修正为内部 boxy-local://localhost/...，所以 handler 校验 localhost，而不是 WebView 外部显示 URL 中的 boxy-local.localhost。
- 实际 WebView2 smoke 中，image 请求没有可用的 Sec-Fetch-Dest: image 值；强制该头会使允许路径图片失败。移除该条件后，选定服务 origin 中的测试图片加载成功，越界图片仍被拒绝。访问权限由 editor 标签、选定 origin、GET 与限定 SV2 图片路径共同约束。

- 下载进度合同为 `POST /api/v2/connector/operations/{operationId}/directory-tasks/{taskId}/progress`，复用 Connector nonce/签名，字段包含 stage、productId、单文件进度、产品计数及文件序数；updatedAt 由服务端生成。
- 进度上报 best-effort，普通采样至少间隔 1 秒，stage/当前文件变化时立即报告，单次请求 timeout 2 秒；报告失败不得影响下载。
- Tauri 自定义 URI scheme 在 Windows/Android 默认映射至 `http://{scheme}.localhost`，每个 WebviewWindowBuilder 可用 `use_https_scheme(true)` 将其映射至 HTTPS；macOS/Linux 使用 `{scheme}://localhost`。已对照[异步注册 API](https://docs.rs/tauri/2.12.4/tauri/struct.Builder.html#method.register_asynchronous_uri_scheme_protocol)和[WebviewWindowBuilder HTTPS scheme API](https://docs.rs/tauri/2.12.4/tauri/webview/struct.WebviewWindowBuilder.html#method.use_https_scheme)。
- 图片协议仅向 editor webview 的选定服务 origin 开放 GET image 请求；文件相对路径限定在 SV2 根下 `databases/meta/` 与 `server-product-logos/`，按文件字节判断 MIME 并拒绝符号链接、遍历及超大资源。
