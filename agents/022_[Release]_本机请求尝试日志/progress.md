# 操作记录

- 2026-10-16：从干净主线 86b51fb 建立交付分支，独立实现位于 upstream-attempt-log worktree。
- 本地合并746844f，保留独立提交并解决任务索引相邻追加冲突；补齐单文件决策入口，版本更新为2.0.8。
- 复核GitHub main protected=false；合并后fmt、打包回归与diff关键字/空白检查通过。独立review无客户端blocker。
- a303d8f推送后，Actions 38994506662两平台全部通过；Windows 39+6+6测试、打包启动及无相邻WebView2 profile检查成功。
- Windows EXE 3405824 bytes，SHA256 b9ecc9e3216938b53eeba11f9fb797607929fb6adee8ec8c2208a85aa90cdf25；macOS universal ZIP3441068 bytes，SHA256 82b720fd2de2ea710ef8170e4712602022b4e8c41e6f4f9f86feca75c34a2b76。
- 配套服务端92项测试与构建通过，日志schema已应用，Worker1198e13f-24ca-4a91-9533-4ceb825abf96已部署；2026-10-16 22:25:06 Toronto公开v2.0.8，isDraft=false，两个资产均存在。
- 部署后暂未收到真实新回执。大陆设备旧会话两条quick_licenses仍pending，未宣称已确认实际端点或产品返回；旧版本内部HTTP尝试不可追补。
