# 发现

用户明确启动即白屏，无服务地址框。Tauri2.12.4 manager/webview.rs对App(index.html)直接返回app_url，macOS生产URL为tauri://localhost，无尾斜线。url2.5.8实测该非特殊scheme的path为空字符串；http://tauri.localhost被规范化为/，所以Windows正常。当前local_selection_url仅接受/与/index.html，误拒绝Mac初始导航；已有测试只覆盖显式尾斜线，没有实际无斜线入口。最小修复只接受同一受限tauri localhost origin的空path，继续拒绝query/hash/credentials/port/任意其他路径，不扩展远端页面本地命令权限。

此前Mac CI只有单元测试/双架构构建/包结构检查，未证明WKWebView实际页面渲染。补一个根/test内的真实Tauri测试程序，复用生产desktop选择窗函数和嵌入资源，以Tauri既有eval_with_callback读取DOM/可见性并通过实际表单连接loopback。禁止新增生产探针、诊断命令、任意远端能力或持久数据。新增测试若无法在CI环境取得WindowServer必须如实报告，不能把单纯编译写成GUI通过。