# 操作记录

2026-10-20：读取agent-mode及当前项目规则、核对main f7f2af8与v2.2.1。创建独立macos-blank与macos-render-smoke工作树，保留全部旧worktree。用户澄清启动即白屏。只读代理先错误假定tauri URL path为/，经明确要求实测后纠正为空字符串；静态源与锁定url2.5.8实测形成同一根因证据。临时Rust验证项目位于TEMP且代理清理被策略拒绝，未重试绕过；后续所有仓库测试严格放根/test。
最小负控：只把真实入口tauri://localhost加入原有test/desktop.rs，未改生产校验时，cargo +1.98.0 test --locked --bin boxy selection_navigation_only_accepts_the_local_entry_page明确FAILED，失败值正是该URL；其余测试未执行。随后只在原有同origin入口path集合加入空字符串，并补无路径query/hash/credentials拒绝用例；版本升为2.2.2。真实WK验证由独立子分支补充，不新增生产诊断功能。