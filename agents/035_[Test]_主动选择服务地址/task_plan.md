# 主动选择服务地址验证

- [x] 确认指定 worktree、agent-mode 和当前服务选择契约。
- [x] 扩展协议 fixture，检查远程页面无权调用 `select_service` 且不会创建第二个操作。
- [x] 新增 Windows UIA smoke，覆盖空白选择、无效 session 延迟读取、确认 loopback 后云页/签名回执和正常退出清理。
- [x] 更新 README 中普通发行版不再默认服务的说明。
- [x] 在 Windows release workflow 中运行新的真实 GUI smoke。
- [x] 运行脚本解析和 Node fixture 回归，并用真实 2.2.1 release exe 运行 GUI smoke。
- [ ] 完成最终审计并提交本分支供 cherry-pick。
