# 通用请求执行器计划

- [x] 从干净的 upstream-attempt-log worktree 创建隔离分支并核对现有调用/协议。
- [x] 在启动与轮询响应实施 protocolVersion=3 双向校验。
- [x] 用通用 http_request task 替代 Dreamtonics action、区域 endpoint、issuer 与 refresh/retry 业务逻辑。
- [x] 增加本地安全 read_files/write_files task，并删除产品图片的网络发现与下载逻辑。
- [x] 将产品包 URL 下载改为复用通用 HTTP 流式 primitive，同时保留本地安装校验与 attempt metadata。
- [x] 迁移根目录 /test 测试并覆盖协议拒绝、通用请求、binary、read_files、下载安装边界。
- [x] 运行 Rust 1.98.0 cargo test 与 Windows startup smoke，检查 diff 并本地提交，不推送或发布。
