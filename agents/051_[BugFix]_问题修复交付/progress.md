# 操作记录
2026-10-23 建立Windows目录修复和下载元数据子工作树，各自实现并局部提交，父任务在main本地合并验证。读取现有发行及历史归一化工作流。源工作区初始干净。

已合并目录打开分支；真实Explorer+后续read_files签名回执通过。父cargo +1.98.0 test --locked -p boxy桌面全量通过，Node包装/fixture/历史/保留16项通过。客户端版本准备为2.2.5，待metadata与local image/progress两分支整合后完整构建与实际WebView图片验证。

最终客户端合并后完整桌面Rust测试58库+5桌面+6阻断+9启动+1目录通过；desktop_render_smoke例check通过，Node16项通过。2.2.5真实Debug EXE以IncludeOpenFolder+IncludeLocalImage组合运行通过：签名目录/读取、后续worker、隔离Explorer关闭、本地图可见及根外拒绝、WM_CLOSE清理。CI Windows已启用两flag，Mac WK真实例新增图片正/负控制，待远端运行。

main 5810c8c推送后，归一化Action39909045573成功；验证同树后远端根d00bdb907a2072e5430566838acddf45301aec91，与本地工作树完全相同。清洁且同树条件下用带旧SHA的update-ref对齐本地main，不重写文件。公开v2.2.5创建，说明仅“改进了一些内容”，构建39909092928 dispatch到该精确SHA。旧Release删除依赖两平台上传成功。
