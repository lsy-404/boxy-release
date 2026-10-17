# 调查结果

- public main `4837cb8` 的连接器 task poll/complete 通道可承载新类型；目前 start request/response 与 poll response 均未校验协议版本。
- `DirectoryTask` 包含 Dreamtonics 专用 action、issuer token、endpoint/body 等字段；`dreamtonics_request_inner` 负责端点/参数构造和刷新重试，必须由服务端编排取代。
- HTTP 入口除主请求外还包括 `assets.rs` 的产品许可/图片 fallback 与 `product_database.rs` 的包下载；产品安装校验和文件落盘是本地职责。
- 本地图片能力需保留文件读取、metadata→data URL 格式和 translation catalogs；远程许可证匹配与图像 URL 获取应交给服务端。
- 通用文件读取必须限制在 SV2 数据根目录内，并拒绝路径遍历、symlink、超量条目和过大文件。
- 通用 write_files 可承接服务端选定的本地图片/metadata缓存，保持每批预校验、显式overwrite、单文件原子替换，并禁止命中session路径；不在客户端编码产品目录或流程。
- v3 wire 将 `contentBase64` 与 `responseBodyBase64` 定为 URL-safe、无填充 Base64；响应头用 `Record<string,string[]>` 保留重复头字段。
- `http_request` 是单次执行：允许七种常用方法，HTTPS、无userinfo/fragment、不跟随redirect，响应最多2MiB、请求body最多2MiB、超时最多120秒；测试只允许loopback HTTP。
- upstreamAttempts只记录单次请求metadata（phase/method/实际URL/status/duration/bodyKind/truncation/静态错误分类）；请求与回执凭据副本在传输后清零，不输出到stdout/stderr。
- read/write file任务限制64项、每项512KiB、总量1MiB，检查相对路径、重解析点/符号链接、Windows ADS/保留名及session别名；写任务先校验整批，再逐文件原子替换。
- 包下载继续落盘并检查签名头/长度/hash；URL与供应商host/path选择归服务端，客户端只保留HTTPS与扩展名边界。
