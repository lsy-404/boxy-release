# 区域 issuer 刷新计划

- [x] 确认隔离 worktree 基于 origin/main 创建且目标路径未占用
- [x] 核对公开 OIDC discovery 的 issuer、JWKS 与 token endpoint
- [x] 记录连接器目前 issuer 检查和手动/自动刷新共用路径
- [x] 将两个精确受信 issuer 映射到各自固定 token endpoint
- [x] 在根目录 test 增加有效区域 issuer、默认 issuer、未知 issuer 的回归
- [x] 同步 connector package 与 Cargo.lock 至 v2.0.5
- [x] 运行连接器回归、release build 并复核 diff
- [x] 提交本地分支；不推送、不部署
- [x] 协调阶段合并推送主线，通过跨平台发行流水线并公开 v2.0.5
