# 核验发现

- 用户明确要求提交也由 Action 强制为单个 1970 年非签名提交；实施为每次 main 推送和手动运行后对 main 归一化。作者与提交者时间均固定 1970-01-01T00:00:00Z，使用无父快照，禁止签名，完整保留文件树。
- 旧 local.instructions.md 禁止未来随发布自动重写历史，与本轮新授权冲突，更新为当前用户确定的规则，无需重复请求授权。
- 当前 main a9f7053，工作区干净；main protected=false、适用 rules=[]。现有 tags v2.2.1/v2.2.2/v2.2.3 的语义不属于主分支提交归一化范围，保留现有发行标签。
- Git 官方 commit-tree 支持无 parent 与 --no-gpg-sign；指定 author/committer 环境日期。带具体旧 OID 的 force-with-lease 避免覆盖竞态推送。
- GitHub 官方说明 GITHUB_TOKEN 推送不再次触发 push 工作流，配合确定性快照和 concurrency 避免递归重写。
- 本地恢复备份 C:/Users/User/.codex/backups/boxy-release/epoch-history-20261022-035453/before-epoch-history.bundle，包含本次操作前全部 refs；不创建保留公开旧历史的远端备份分支。
- 初次 git log 使用签名状态格式触发本机 allowedSignersFile 未配置诊断；后续直接读取 commit 对象验证 gpgsig 头，避免把验签配置误当成有签名或无签名依据。
