# 操作记录

- 2026-10-21：检查现有worktree并fetch；远端main曾因先前用户授权的历史清理发生重写，本任务仅从当前main建立codex/service-link，不更改旧分支。
- 阅读launch.rs、bootstrap页面、GET轮询和既有启动选项/临时错误回归。
- 尚未启动服务器或创建浏览器标签。
- launch.rs在解析前为缺少协议的输入补全HTTPS；首页说明同步更新。显式HTTP的loopback限制及origin校验保持。
- GET轮询的终止错误现在保留最多4096字节响应正文，供定位服务端500；未将500当作网络错误重试。
- cargo fmt检查、66项既有Rust测试、2项Node测试与脚本语法检查通过；真实WKWebView smoke通过，原有fixture及临时运行目录由脚本trap清理，未留下监听进程。
- 准备2.2.3补丁发行，Cargo与Tauri版本一致；仅沿用现有跨平台发行工作流，公开说明按项目规则使用固定短句。
- ae4210d已签名提交并推送main，2.2.3草稿已建立；首轮两平台均编译，Windows首页smoke因既有文本fallback与新输入契约失败，完整日志已核对。仅调整同一现有smoke的错误前缀与非法输入，重新发行验证。
- 正式Mac下载包SHA256=8f64f85e398dbfe22d14e26845ab2a02cbc0942994c9aed179b3b2ab775d5c6b，ZIP8367326字节；双架构与plist验证通过。原生验证资源exec session78825，PID66142，开始时间2026-10-21 16:38:24 America/Toronto，工作目录本工作树，无监听端口；完成后须核对清理。
- 5fabbce仅调整既有Windows smoke；PowerShell原生Parser检查零语法错误，草稿目标改为该精确SHA，第二轮CI39672914480已启动。
- CUA读取关闭后的窗口状态时又启动了本任务包的空白实例PID68586；随后使用正常Quit退出。原session78825返回0，66142/68586均已不存在。未关闭用户原有应用或浏览器标签。
- 第二轮Windows job119475227302全部success：68项Rust、2项Node、正式exe内嵌云页/签名收据/窗口退出，以及空白首页/选定前session不读取/非法输入/拒绝后重试/正常退出清理均通过。
- Windows正式exe已实际下载，11,615,232字节，SHA256=d7401e7d6498f5a3acb6de27255eca48af8bd3a61ded1e308ec90f416779c726，与GitHub摘要核对通过；PE32+ GUI x86-64。Mac最终重跑仍在发行编译，当前草稿保持未公开。
- 运行中的Mac日志下载API返回BlobNotFound；临时创建隐藏IAB标签1查看实时日志，确认编译行继续新增。标签1已关闭，listTabs确认不再存在；未保留预览标签。
- 最终Mac job119475227243成功，CI整体success。重新下载最终Mac资产，两个SHA256检查均OK，ZIP检查无错误。本地lipo verify命令参数解析失败，改用lipo -archs与file确认双架构；plist lint及版本2.2.3通过。
- 2026-10-21 17:11公开v2.2.3并设latest；API确认draft=false、prerelease=false、两个asset uploaded、tag精确指向5fabbce。无本任务运行中的本地服务器、watcher、窗口或浏览器标签。
