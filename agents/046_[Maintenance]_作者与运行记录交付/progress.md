# 操作记录

- 读取当前脚本与工作流，核对 main clean，fetch 并建立两处隔离工作树。
- GitHub API 分页列出现有 27 个运行记录；artifact/cache 均为空。
- 查阅 GitHub 官方工作流运行删除限制，确认本次均满足 completed 可删除条件。
- 委派源作者保留及隔离 Git 测试；根代理处理外部清理与集成交付。
- 首次查询命令使用不兼容的 --slurp 与 --jq 组合，CLI 在任何删除前拒绝；改为 --slurp 后在 PowerShell 解析。分页核实后删除全部 27 个 completed 运行记录；GitHub API total_count=0。
- 合并子分支 334f6e6：通过 raw %an/%ae/%cn/%ce 读取源身份，仅改双时间，不固定或添加实际姓名邮箱。
- 根代理复验 6/6 集成测试通过；含不同源作者与提交者、环境身份覆盖负控制，脚本语法和 diff 检查通过。
- main 合并推送 4e0f560，Action 39746371124 自动运行成功，产生 36e767177329a4f6b2580a66293c40e36ea3d0a8；压缩前后 tree=2c25113d3c71e1d0498f9aef063d69f3f3bee973。
- 远端 main count=1、无 parent/gpgsig、双时间戳=0；author 与仓库配置身份完全一致，GitHub author.login=lsy-404，verification=unsigned。
- 删除本次验证 run 39746371124，API total_count 再次为 0；累计已清理 28 条。
- 保存最终审计记录会产生最后一次 push run；交付收尾在其完成后删除该运行，工具回执确认最终零记录，不通过新增清理 Action 产生递归运行。
