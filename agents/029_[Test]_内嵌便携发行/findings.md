# 调研结果

- 当前 Windows smoke 仍验证系统浏览器；它会在结束前 Kill 连接器，不能作为 WebView 正常关闭证明，需要整体替换。
- `--server http://127.0.0.1:<port> --session <fixture>` 是生产支持的 loopback 接口，可供本地 HTTP fixture 使用。
- Smoke 必须通过真实 app 创建的 `editor` / `Boxy` 窗口加载 start 响应所给 URL；HTML 本身 fetch `/ready` 证明 renderer 实际执行。
- Fixture 的编辑器响应设置 HttpOnly cookie；页面必须在 `/ready` 请求中回传 cookie。页面同时探测 Tauri 内部 invoke，只有 API 缺失或拒绝窗口标题命令才继续。
- 生命周期数据在 `std::env::temp_dir()/boxy-runtime-<随机>/webview`，应用正常关闭应清除其唯一 runtime dir。Smoke 通过只给子进程设置系统 TEMP/TMP 到新建隔离目录，避免误判其他应用 runtime。
- 不用生产IPC、测试专用环境变量或 Kill 作为成功关闭证据；协议任务仅用受限 `read_files` 并检查已签名 receipt。失败时只为清理测试自启的进程作强制退出，且整个 smoke 仍判失败。
- Windows 产物目标为单个 exe，系统提供 WebView2，不包 Fixed Runtime 或旁置 loader DLL；macOS保留 universal `.app`/zip 及 `app.boxy.bridge`，CI检查系统 WebKit 链接。
- [测试调试] 初次 fixture 单测传入重复 Node 可执行参数，子进程提前退出；修正 spawn 参数后，打包及 fixture 两项 Node 测试通过。进程退出监听在 teardown 中先注册再发终止信号，避免退出事件竞争。
