# Findings

- Windows `session_io::open_session_folder` 通过 `explorer.exe <directory>.status()` 同步等待并检查退出码。Explorer 会把目录请求转交给现有桌面进程；其启动器退出状态不等同于目录打开结果。
- `execute_directory_task` 使用 `?` 传播打开错误；`run_connected` 返回错误后，桌面 `run_worker` 显示错误并结束应用进程。因此非零/异常退出状态会呈现为 Operation 错误并导致 Boxy 关闭。
- `/usr/bin/open` 的 macOS 路径先保持原有行为，本次聚焦 Windows Explorer 启动语义。
- GUI 验证使用隔离临时目录，并以调用前 HWND 快照和精确目录 URL 识别新 Explorer 窗口；既有窗口保持打开。
- [Implementation] Windows Explorer is a single-instance shell entrypoint. Waiting for its process and requiring a successful process exit conflates request acceptance with the lifecycle of the Explorer window; use a detached spawn and report only spawn failures.
- [Regression] An isolated PowerShell process sleeps before returning nonzero. The test confirms the launcher call returns as soon as the child starts, and does not require the child to close successfully.
- [Validation] The Windows core-runner regression opened an isolated directory in Explorer and closed only its newly created window. Existing Explorer windows remained open.
- [Workflow review] If Explorer truly cannot be started, `spawn_directory_opener` still returns an error. `execute_directory_task` propagates it through `run_connected`; after the editor window is active, `desktop::run_worker` reports the error and finishes with a failure exit. The reported issue was the Explorer handoff process exit being mistaken for launch failure; no broader task error behavior change is required.
- [Initial test attempt] The first live core-runner attempt used a 9-byte session fixture and correctly failed session validation before operation start. The fixture was corrected to 8 bytes; the actual run then passed.
