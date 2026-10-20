# 操作记录

2026-10-20：读取agent-mode及当前项目规则、核对main f7f2af8与v2.2.1。创建独立macos-blank与macos-render-smoke工作树，保留全部旧worktree。用户澄清启动即白屏。只读代理先错误假定tauri URL path为/，经明确要求实测后纠正为空字符串；静态源与锁定url2.5.8实测形成同一根因证据。临时Rust验证项目位于TEMP且代理清理被策略拒绝，未重试绕过；后续所有仓库测试严格放根/test。
最小负控：只把真实入口tauri://localhost加入原有test/desktop.rs，未改生产校验时，cargo +1.98.0 test --locked --bin boxy selection_navigation_only_accepts_the_local_entry_page明确FAILED，失败值正是该URL；其余测试未执行。随后只在原有同origin入口path集合加入空字符串，并补无路径query/hash/credentials拒绝用例；版本升为2.2.2。真实WK验证由独立子分支补充，不新增生产诊断功能。
修复后48库+5桌面+6host+9启动共68项Rust通过，独立安全复核确认未改变远端导航与原生命令隔离。合入macOS真实WK测试子分支e711faf：无新依赖，根/test example复用生产desktop模块、嵌入资产和原命令capability，读取真实DOM/CSS与控件矩形后触发原表单，断言loopback云UI/cookie/远端命令拒绝/签名回执/选择窗销毁/清理。test step限时3分钟；Windows只做example编译，不称为WK通过。