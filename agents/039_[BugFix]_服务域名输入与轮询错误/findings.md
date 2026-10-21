# 调查记录

- 2026-10-21：从 origin/main 65792d1 建立独立工作树。原工作区存在其他未提交审计文件，不混入本任务。
- 首页已使用 text 输入，不受浏览器URL输入校验限制；实际拒绝裸域名的是 launch.rs 的 Url::parse。
- 所选地址仍须为无凭证、路径、query及fragment的origin；HTTP仅允许loopback。未指定地址时继续显示空白首页，不持久化服务地址。
- GET轮询当前重试408、429、502、503、504及临时传输错误；500直接丢弃响应体并退出，导致用户只看到状态码。
- 用户规则要求不新增测试；复用现有Rust、Node及真实WK页面验证。
- 线上D1已取得7429实例内存超限reset证据。配套Worker将已知临时D1错误在GET轮询下映射503，当前客户端已有该状态的有界重试；因此客户端无需把所有500都当作可重试错误，保留原安全边界。
- 首轮发行CI39670699955：Mac全部通过，Windows编译、Rust/Node回归及正式exe启动通过，首页smoke失败。其Get-SelectionError将任何带HTTPS的说明文字都视作错误，新首页说明触发误判；应保留原“选服务前不读取session”的断言，修正错误区域读取的文本fallback为实际错误前缀Use HTTPS。
- 同一已有smoke还用not-a-service-url作为非法URL；新增省略协议后，该值是可解析的HTTPS单标签主机，原“非法”前提已改变。将该既有步骤输入改为空格分隔的not a service url，继续验证真正非法输入在本地被拒绝；不新增测试用例、不放宽断言。
- 已从首轮CI实际下载Mac universal包，SHA256与GitHub digest相同；真实正式包的空白首页接受裸域名boxy.voidcarve.com，并进入后续session-path校验。使用故意缺失的显式session路径，未读取或上传真实session。
