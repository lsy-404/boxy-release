# 调研发现

- README.md 当前包含客户端/服务端边界、构建方式、平台实现、协议和数据处理说明；用户要求仅保留 Boxy 名称和“SV2 工具”说明。
- 保留并复核两条简洁的技术理由注释：`connector/src/desktop.rs:109` 说明 WebView2 环境变量可能绕开受控临时配置；`connector/src/product_database.rs:417` 说明数据库扫描兼容 SV2 平铺和版本子目录布局。
- 按任务要求跳过 `.github/workflows/manual-build.yml`、发行清理脚本和测试文件的修改；需将其中符合目标的问题交给主任务统一处理。