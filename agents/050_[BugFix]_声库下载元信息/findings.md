# 调研发现

- 官方 SV2 产品目录缓存位于 `databases/meta`，共用的缓存 JSON 含 `name`, `vendor`, `genres`, `pitchRange`, `languages`, `profileImageUrl`, `audioSampleUrl`, `isTrialable`, `isDownloadAvailable`；对应图片以 JSON 文件 stem 命名为 PNG。
- 语音模型本体位于 `databases/<id>/<version>`，含 DNNI 与原生生成的 `m`。不复制或伪造 `m`。
- Boxy 当前 SV2 下载只落 DNNI，未写全局目录缓存。上游 JSON 的真实来源字段仍待验证。- 官方前端资源确认目录查询路由为 `/product/query?page=1&page_size=1000`；代码区服映射指向 `authr3.dreamtonics.com(.cn)/api/v1`。两区域无凭据 GET 均返回 HTTP 401；当前 Edge Dreamtonics 页面为未登录表单。没有读取或输出任何凭据。
- 当前缺少经真实授权响应确认的字段名、类型及图片 URL 格式，不能安全构造缓存对象或落盘。
