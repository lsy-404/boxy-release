# 核验发现

- 用户新增要求删除 Actions 记录，随后明确 Git commit author 保持本人。现有脚本固定为 Boxy 的元数据需要替换为预期源提交内的原作者与提交者身份。
- 不在代码中写入用户姓名或邮箱；直接读取原 commit 元数据，继续将双时间固定 0 +0000、禁用签名且不传父提交。
- 当前仅 main；远端 root fda72af，checkout clean。已列出 27 个现有 Actions run，全部 completed；artifact/cache 总数均为 0。
- GitHub 官方删除运行文档限定运行已完成或超过两周；Action 不能在本次尚未完成时删除自身记录。本轮删除完成记录和本次验证新增记录，不引入会持续产生自身记录的清理工作流，也不擅自新建仓库外自动化。
- 删除运行记录不改动 Release、代码、Git refs 或既有附件；当前不需要中断运行。
