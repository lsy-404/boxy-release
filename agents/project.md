# Boxy Release 项目索引
> 最后更新：2026-10-20

## 项目目标

独立维护 Boxy 的本地 Rust 连接器、本机操作，以及 macOS/Windows 便携发行产物。云端 Worker、网页与 Rust/Wasm session codec 在 sv2-boxy 仓库中维护。

## 技术栈

- Rust 2021与Tauri 2：内嵌远程云界面、本地连接器、Windows防火墙和macOS LuLu规则导出
- Node.js：便携应用打包脚本
- GitHub Actions：跨平台验证和便携发行构建；Windows 和 macOS 依赖由客户端按需获取

## 模块结构

- connector/：本机后台连接器、空白服务地址选择页、Tauri远程窗口、单次运行的内存签名身份、密文session I/O、受限本地任务、原生命令批准。用户明确选定服务后才解析会话并连接。Windows使用系统WebView2及独立临时profile，macOS使用WKWebView非持久数据存储；正常退出释放浏览器并清理运行数据。远端页面不获原生命令权限，本地选择页仅能发起一次服务连接。
- HTTP请求由Worker下发完整描述，connector仅执行一次并回传原始结果；端点、参数、解析和刷新/写回/重试流程全部由Worker编排。通用文件I/O受SV2根路径和大小限额约束，会话替换保留独立CAS。
- scripts/：便携版构建。
- test/：跨平台配置和协议验证测试。
- agents/：任务计划、调研和操作记录。

## 跨仓库边界

连接器与云端通过 HTTP JSON 协议及签名交互。本仓库不包含云端服务源码、Wasm codec 或云端签名私钥。

项目额外指令见[local.instructions.md](local.instructions.md)。
