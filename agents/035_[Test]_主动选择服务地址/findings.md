# 调研与发现

- 本次要求覆盖：普通发行文件名、无 `--server`、无 `BOXY_API_URL` 时本地选择服务；第一阶段传入一个内容无效且由 smoke 独占锁定的 session 文件，确认选择前应用没有打开它，也没有文件选择框或 connector 错误，关闭时 exit code 为 0。
- 选择页约定的 DOM selectors 是 `input#service-url`、`button#connect`、`#error`。Windows UIA 将先按 AutomationId 匹配并用 `GetWindowThreadProcessId` 确认窗口属于本次 Boxy PID，禁止全局键盘注入。
- 连接后复用现有本地 protocol-v3 fixture 的 Cookie、远程页面、签名 poll、read_files receipt 与 Blob 测试行为；远程 `select_service` 拒绝由 fixture probe 证明，并通过操作创建请求计数确认没有第二次创建。
# 验证结论

- Windows 2.2.1 release 的真实 smoke 观察到进程归属窗口，UI Automation 可定位并操作 `service-url` 与 `connect` 控件；UIA 操作限定于测试启动的 Boxy PID。
- 被独占锁定的无效 session 在 URL 确认前未引发文件读取或文件对话框；选择后云页握手、cookie、protocol-v3 签名回执通过。
- 远端页面尝试 `select_service` 被拒绝，且服务端只观察到一次 operation 创建。正常关闭返回 0 并清除本次 profile。
