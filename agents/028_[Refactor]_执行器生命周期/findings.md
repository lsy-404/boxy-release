# Findings

- 连接器执行器目前位于二进制 `main.rs`，串行处理启动、Worker poll、任务和 session 写回。
- GUI能力只有启动时文件选择、系统浏览器打开及远程命令审批对话框；需要由宿主 trait提供GUI桥接。
- 本机 session 更新采用源哈希 CAS；取消不能打断替换文件与 signed receipt 之间的关键段。
- [待验证] device signing key目前持久化到用户数据目录；改为每次运行内存新建可能改变Worker端设备密钥身份绑定。
# Findings

- `run_connected` owns one remote operation and polls only while the host remains active. A stop observed between tasks exits without starting more work; an already-started task reaches completion and its receipt is sent before shutdown.
- The operation signing key is ephemeral for each run. The service binds operation public keys independently of persistent device identity.
- Session writeback checks the source hash before staging and again immediately before atomic replacement, then verifies the written ciphertext. Temporary files are removed by tempfile ownership on failure; no persistent backup is created.
- Shell output capture retains at most the configured output cap while draining pipes. Reader completion is deadline-bound and stop-aware, so descendants that retain inherited pipes cannot block connector shutdown indefinitely.
