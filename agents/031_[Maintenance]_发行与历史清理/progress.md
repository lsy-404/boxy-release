# 操作记录

2026-10-19：核对公开仓库，无用户未提交修改；创建独立history-cleanup工作树。备份C:\Users\User\.codex\backups\boxy-release\20261019-history-cleanup\before-cleanup.bundle与local/remote refs文本。子任务负责发行备份及删除；根任务负责带lease原子历史重写。未触碰私有Worker仓库或旧本机worktree。

子分支ab312031已备份10份release元数据、9旧版18个资产及最新双资产验证副本；删除9个旧Release后，只剩417493925/v2.2.0。最新说明精确匹配用户要求的短句，资产ID、尺寸与digest均未变。合并子分支，新增简短发行说明规则，程序源码与已验证发行源码保持零diff。

一次审计命令使用PowerShell双引号内的中文弯引号导致parser error，整条脚本未执行；已改用literal here-string，未发生部分修改。

原子带lease推送已成功：main和v2.2.0均指向单一无父快照4bfc290，删除10个旧tag（含无Release的v2.0.0）及祖先远端分支codex/remote-bridge-neutral。GitHub commits API返回1，ls-remote仅main和v2.2.0两个refs；Release列表仅417493925/v2.2.0，body精确为用户短句，target metadata已更新。最新双资产ID/尺寸/digest均与操作前一致。

最后仅合入本段已完成记录，再以相同程序源码树重建无父快照并带当前ref lease推送；发布附件不重建、不重新上传。最终ref与核验回执保存于本机备份目录。主checkout仅在确认clean后切换至新快照并重设本地main；所有其他旧本机分支及worktree保持原状。
