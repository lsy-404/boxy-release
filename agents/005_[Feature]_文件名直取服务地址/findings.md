# 调研与决策

- 当前实现将 `a.b.exe` 和 `boxy.a.b.exe` 都预填为 `https://boxy.a.b`。用户要求去掉这个补全规则，有效域名样式的文件名应原样成为 HTTPS 主机名。
- 启动时仍需用户确认可编辑地址，且无效文件名沿用既有默认服务地址；现有 HTTPS、干净 origin 校验保持不变。
- 用户确认从文件名中提取最长有效域名片段，例如 `boxy.voidcarve.com (1).exe` 应预填 `https://boxy.voidcarve.com`，大小写不敏感。
- 实现按文件名中非域名字符分隔候选，逐个校验标签长度与字符，选择最长候选；相同长度保留先出现的候选。当前 `cargo test --locked -p boxy --test service_selection` 3/3 通过，macOS release 构建和打包通过。
