# Progress

- 消费侧下载进度已接入三参 fetch 回调；发送用独立签名 POST、最多每秒一次并在文件/阶段切换时立即上报，单次请求 timeout 为 2 秒且错误忽略。
- 本地图片协议的 core 文件约束测试、progress 签名/节流测试、desktop feature 编译均通过。Windows WebView2 smoke 的第一轮因 fixture 在 document.body 创建前读状态而未发起图片请求；第二轮暴露实际 image 请求没有可用的 Sec-Fetch-Dest image 值；按 Wry handler URI 规范化校正 localhost host 并移除该头限制后，实际图片 HTTP 200 且成功解码，SV2 根外图片返回拒绝响应并触发 onerror。
- 进度接口与 downloader callback 已和 metadata 分支合并；该提交是当前分支祖先，不重复 cherry-pick。
- 有一次本地祖先调整操作误用 hard reset，清除了未提交的跟踪文件修改。已从已审阅代码重新恢复，按父线程要求不再重写祖先；后续测试覆盖恢复后的内容。

- 已完成 049 Windows open_folder 回归并提交；当前进入父任务追加的客户端资源与下载进度工作。
- 下载进度服务端合同与回调数据结构已确认。已起草签名 progress reporter 和 lib.rs dispatch 接入，metadata agent 负责流式 downloader callback 与阶段事件。
- Tauri 官方 API 已确认支持异步 URI protocol handler；Windows 的 `use_https_scheme(true)` 可将 scheme origin 固定为 HTTPS。选定服务 origin 和 editor webview 是访问边界。
