# 操作记录

- 确认 `codex/tauri-release-packaging` 位于指定 worktree，工作区干净；HEAD 与整合提交 `aa3d2dcbb44dd064dce3363e2b5cf26d997bbe98` 一致。
- 确认 root 提供的 Windows release exe 存在；读取现有 fixture、startup smoke 与 Tauri 下载回调。
- 扩展 `test/fixtures/tauri-webview-server.mjs`，通过 `--enable-export` 为真实页面生成固定内容 Blob 并触发下载；fixture 仅记录是否触发，不保存下载正文或凭据。
- 新增 `test/windows_export_smoke.ps1`，以 UI Automation PID 所属校验限定至本次 Boxy 进程保存框，保存到唯一临时路径并核对文件内容；完成正常关闭和临时 profile/AppData 状态核对。
- 定向 fixture、打包 Node 测试及实际 Windows GUI Blob 导出 smoke 均通过。
