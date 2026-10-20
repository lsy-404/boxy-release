# 操作记录

2026-10-20：读取agent-mode、项目说明和极简Release规则；fetch核对main无差异，保留所有旧worktree，创建独立服务选择实现及core/smoke工作树。公开历史此前清理属于一次性授权，本次采用正常提交，不重写历史。

2026-10-20：合入显式地址core提交；prepare_config只解析启动选项，不检查session。实现内嵌空白选择页及唯一select_service命令，使用Tauri生成的allow-select-service权限、本地窗口capability和真实caller URL检查；远程窗口导航限制保持。连接前失败回到选择页，未连接关闭不等待不存在的worker，已连接关闭沿原有任务回执与销毁窗口路径清理。

本地完整Rust测试先通过43库+4桌面+6host+9启动，新增精确本地选择页URL测试后桌面5项通过，2.2.1 Windows release编译通过。真实UIA验收与跨平台构建待完成。

用户补充中国区旧版TLS握手EOF截图；代码定位为Boxy operation轮询GET传输错误直接退出，并非上游区域API响应。只读协议审查发现写回GET并非幂等，新增独立Worker修复及客户端安全有限GET重试任务，禁止重发start/task/receipt POST。真实大陆网络尚未复验，不把重试机制等同于跨境可达性证明。