# Progress

- 读取 `agent-mode` skill 与公开仓库 `agents/project.md`、`agents/tasks.md` 和项目约束。
- 检查公开仓库初始工作区干净，并创建独立 worktree `codex/dreamtonics-request`。
- 检查 connector 已有 `directory_tasks` poll 和签名 `/complete` 回执逻辑，可复用现有通道。
- 扩展 connector task schema 以接收授权动作字段；固定 Dreamtonics endpoint，由本地 connector 构造 OAuth/授权请求，保留拒绝状态和有界响应。
- 更新手动构建 workflow，省略 cargo test 与 startup smoke 步骤，仅保留发行构建、打包与上传动作。
- `cargo fmt --all -- --check` 通过；使用 Rust 1.98.0 执行 `cargo build --locked --release -p boxy` 通过；未运行测试。
- `git diff --check` 通过；检查 Dreamtonics 请求错误路径不拼接上游异常文本/令牌，关闭重定向并限制响应大小，授权请求失败仍以签名 task complete 回执返回结构化失败。
- 为公开 v2.0.3 发行，将 connector package 与 Cargo.lock 版本号同步，并把 README 发布验证语句更新为 Worker protocol。
