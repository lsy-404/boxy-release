# 操作记录

- 2026-10-17：核对git worktree list和干净状态，在既有专用worktree从main4837cb8创建codex/server-driven-delivery集成分支。两仓库main均无保护。
- 合并通用执行器54afa2a，在索引冲突中保留022/023/024全部记录。README与project已按唯一服务端编排边界更新。
- 客户端48项Rust测试通过；实际Windows fixture验证protocol3 startup/poll、系统浏览器URL访问、无旁置WebView2目录。服务端选择PATCH、URL/query/body/header，401不刷新、带Location302不重定向，签名原始回执、file路径/预算/会话保护均有回归。
- 版本更新2.1.0，Cargo offline locked metadata和便携打包测试通过。新增执行器未残留供应商endpoint、OAuth参数/refresh-token解析或业务action；禁止关键词和diff whitespace检查通过。

- 第一轮发行CI39002753017两平台成功，但最终文件边界审查在草稿公开前发现session文件别名/junction与无效UTF8原文丢失；同一2.1.0草稿改用修复后的新HEAD重建，避免发布后要求额外升级。
- 合并33aeb3d/5dcdc53补充文件身份与reparse拒绝、原始binary错误载体；Windows50项测试/build通过。该分支审计编号与既有发行记录冲突，仅在集成时将新增审计目录归为025，保留全部记录。
