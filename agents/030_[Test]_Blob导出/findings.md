# 调研与结论

- `connector/src/desktop.rs` 的 Tauri `on_download` 对请求下载调用原生 `rfd::FileDialog::save_file()`，由 `DownloadEvent::Requested` 提供建议文件名。
- 现有 Windows startup smoke 已验证真实 Tauri 远程窗口、Cookie、protocol-v3 签名任务、关闭清理；本任务只补 Blob 下载到原生保存对话框的实测。
- smoke 必须给 fixture 的页面主动生成固定内容的 Blob 并触发 `<a download>`，只由 UI Automation 操作所属进程的保存对话框，不使用全局键盘输入。
- 若 UIA 无法可靠定位保存控件，记录未验证结果，不引入新依赖或修改生产下载流程。
- 实际执行 `test/windows_export_smoke.ps1` 使用 `dist/boxy-windows-x64.exe`：UIA 只定位到原生窗口所有者 PID 与启动进程相同的保存框，通过 ValuePattern 写入测试专属路径并 Invoke 保存；Blob 文件内容匹配，WM_CLOSE 后进程 exit code 为 0，临时运行目录为空。
- 执行前后比较 AppData 签名键 hash 与 `app.boxy.bridge` 目录清单，均未变化。
- `node --check test/fixtures/tauri-webview-server.mjs`、`node --test test/tauri_webview_fixture.test.mjs`、`node --test test/package_portable.test.mjs`、PowerShell AST 解析及 `git diff --check` 均通过。
