# 操作记录

- 读取 agent-mode 与项目审计规则、发行流程、旧历史清理操作记录。
- 检查工作树/远端并 fetch；GitHub API 核实 main 未保护且规则为空。
- 为归一化实现和交付分别建立隔离工作树，保存全 refs bundle，委派脚本及真实本地 Git 测试。
- 查阅 Git commit-tree、push 和 GitHub Actions 工作流触发官方文档。
- 新增 normalize-history.yml：main push / workflow_dispatch、contents write、单并发不打断、main ref 限制；先执行真实 Git 测试再执行归一化脚本。
- 更新 local.instructions.md 旧历史规则和 project.md 行为索引；既有手动发包流程不变。
- actionlint v1.7.12 验证两个工作流通过；仓库 Actions enabled=true、allowed_actions=all。
- 整合子分支 f61c0b5，根代理真实 Git 集成复验 6/6 通过，覆盖 unsigned epoch root / 同树 / depth-1 / 幂等 / stale / 真实 push 竞态 / CLI。
- 审查移除不必要的手工 commit 序列化和重复 hash-object，以实际 commit-tree 对象直接判断幂等。
- 合并实现到 main 并推送 d83496c；首次按文件名查询工作流返回 404（新工作流索引尚未就绪），随后完整列表确认自动 push run 39744785227 已启动，无重复 dispatch。
- GitHub Action 39744785227 全步骤成功，含真实 Git 测试及归一化；远端 main 变为 45037b6ad2181b416cb1e7f8fdb5196691bd260c。
- git fetch 后验证 rev-list count=1、parent/gpgsig 头不存在、author/committer 时间戳均 0；GitHub API parents=0、verification=unsigned、双 date=1970-01-01T00:00:00Z。
- 压缩前后 tree 均为 41aa3f7f0b11b9ba8158f0fbaa354b7e781b2126；确认 checkout clean 后仅以带旧 OID 校验的 update-ref 同步本地 main，无 reset/stash。
- 保存完成记录后再次正常推送，由相同 Action 再次归一化最终审计树；最终远端核验以交付工具回执为准。
