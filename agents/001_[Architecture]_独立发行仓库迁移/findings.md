# 调研与决策

- 从原私有仓库路径过滤得到独立 Git 历史，再复制并核对未提交的连接器改动。
- 用户最终确认本仓库只包含客户端源码和发行流程；云端 Rust/Wasm codec 保留在原仓库。
- 启动服务地址需在任何设备数据或 session 密文上传前由用户确认；文件名只提供预填值。
- macOS 内部可执行文件仍叫 boxy，需读取外层 .app 目录名；Windows 读取 .exe 文件名。
- 签名公钥继续独立于服务 URL；更换地址不能降低云端写回验证强度。
- 新仓库重新从原仓库按客户端路径过滤历史，Git 历史及当前文件树均不包含 cloud/codec、Wasm 或云端构建脚本。
- 原工作树连接器目录与最初复制出的目录逐文件一致；用户的网络屏蔽改动迁入客户端仓库，原工作树未被修改。
- tinyfiledialogs 3.9.1 提供带默认值的跨平台输入框；用户确认前不读取 session 或发送请求。URL 校验只接受独立 HTTPS origin 或本机开发 HTTP origin。
- 本机 /etc/hosts 正在屏蔽 authr3.dreamtonics.com，导致现有下载 URL 正向测试失败；跳过该环境依赖测试后 15 个客户端单元测试及 3 个新测试通过。
- cargo check --locked -p boxy 与 macOS release build 通过；便携 .app 和 zip 已生成，Mach-O arm64、codesign 和 plist 校验通过。
- 在 macOS 上对 Windows MSVC 目标的交叉检查被 ring C 构建缺少 assert.h 阻断，不能据此判定 Windows 编译结果；发行工作流在 Windows runner 上执行该构建。
- 已尝试启动重命名的 .app 观察实际提示；Computer Use 无法绑定其模态窗口，随后终止测试进程。服务地址预填由纯函数测试验证，窗口视觉状态尚未确认。
