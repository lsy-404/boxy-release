# 区域图片查询修复计划

- [x] 检查指定 worktree、现有固定 issuer/API base 映射、所有授权 API 调用入口与图片查询测试注入点。
- [x] 在本地 key/cache 命中之后才解析 issuer，并让图片许可证查询使用与其它固定授权 action 相同的区域 API base。
- [x] 通过共享生产请求路径的注入测试覆盖中国端点、国际端点、非法 issuer 网络前拒绝及本地缓存无 issuer 成功。
- [x] 使用指定已有 target 和 Rust 1.98.0 运行 `cargo test --locked -p boxy`，复扫认证请求入口并提交当前分支。
