# 执行计划

- [x] fetch并核对当前main、remote refs、branch保护、最新Release和worktree状态。
- [x] 创建并verify完整Git bundle，保存远端refs和SHA256。
- [x] 独立子分支备份旧Release元数据与资产，再精简最新说明并删除旧Release。
- [x] 合并子分支审计并固定简短发行说明规则；保留完整代码快照。
- [x] 逐ref lease原子推送main/最新tag并删除旧tag和无独有改动的旧远端分支。
- [x] 校验公开提交数、refs、Release数量、说明、最新资产，记录实际结果。
