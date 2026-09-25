# Boxy Release 项目索引
> 最后更新：2026-09-24

## 项目目标

独立维护 Boxy 的本地 Rust 连接器、本机操作，以及 macOS/Windows 便携发行产物。云端 Worker、网页与 Rust/Wasm session codec 在 sv2-boxy 仓库中维护。

## 技术栈

- Rust 2021：本地连接器
- Node.js：便携应用打包脚本
- GitHub Actions：跨平台验证和便携发行构建

## 模块结构

- connector/：本机连接器、用户确认、设备签名、密文 session I/O、受限本地任务、命令批准。
- scripts/：便携版构建。
- test/：跨平台配置和协议验证测试。
- agents/：任务计划、调研和操作记录。

## 跨仓库边界

连接器与云端通过 HTTP JSON 协议及签名交互。本仓库不包含云端服务源码、Wasm codec 或云端签名私钥。
