# Progress

- 初始化指定独立任务审计目录；未改共享 `agents/tasks.md`。
- 初始分支 `codex/windows-folder`，HEAD `1ab1a49`，工作区干净。
- 已确认错误路径为 `explorer.exe.status()` 非零 -> `open_folder` 任务返回错误 -> `run_connected` 结束 -> 桌面 `run_worker` 弹出错误并退出。
# Progress

- 修复 `connector/src/session_io.rs`：Windows 用 `Command::spawn()` 将规范化目录交给 Explorer 后立即返回；macOS `/usr/bin/open` 仍检查启动状态；不可用平台仍返回明确错误。
- 新增 `/test/windows_folder_launch.rs` 与 manifest target。测试启动隐藏 PowerShell 进程，模拟启动器接受目录后延迟 3 秒并以退出码 7 结束，验证调用端不等待后续进程退出；未触碰用户 Explorer/Boxy 窗口。
- `cargo +1.98.0 fmt --all -- --check` 通过。
- `cargo +1.98.0 test --locked --no-default-features` 通过：51 library、6 host block、9 launch options、1 Windows folder launch 测试；doc tests 0。
- 专项回归重跑通过（1/1）。初版测试脚本将路径附加到 PowerShell 命令后触发解析错误，调整脚本注释后测试验证了延迟退出场景。
- 没有对真实 Explorer 打开目录做交互验证，避免在用户桌面留下额外窗口；自动回归在当前 Windows 主机真实运行，验证了进程提交与退出码隔离。

## 端到端补充验证

- 按父任务要求取消 Explorer 零窗口前置条件。实际回归记录调用前 ShellWindows HWND；仅匹配临时测试目录的精确 `LocationURL` 且 HWND 不属于原集合的窗口，回归结束关闭该窗口。
- Windows core runner 真实端到端测试通过：`run_connected` -> 本机 HTTP mock -> 实际 `open_session_folder`/Explorer -> 校验签名 `open_folder` 回执 -> 第二次签名 poll -> 校验 `read_files` 内容、哈希、大小和回执 -> 停止并返回。测试结束正常关闭新建临时目录窗口。
- 扩展 `tauri-webview-server.mjs` 可选 `--include-open-folder` 模式，随后仍派发原有 `read_files`；状态记录回执有效及 open_folder 后再次 poll。默认 fixture 路径不变。
- 扩展 `windows_startup_smoke.ps1 -IncludeOpenFolder`，按调用前 HWND 快照和临时目录 LocationURL 关闭唯一新窗口；默认 smoke 参数保持原行为。
- `node --test test/tauri_webview_fixture.test.mjs` 通过（2/2）；PowerShell Parser 对扩展后的 Windows smoke 脚本无语法错误；cargo fmt 检查通过。
- 实际执行了 core runner Explorer 场景；未构建/运行便携桌面 EXE smoke。现有五个 Explorer 窗口在调用前被记录并保留，测试仅关闭新增临时目录窗口。
