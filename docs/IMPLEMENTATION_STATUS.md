# AiAccountCenter 实施状态

更新日期：2026-10-14。

## 1. 当前结论

新项目已完成本轮最小 Windows 开发环境安装与验证，产品仍处于方案阶段。用户已确认继续使用 Rust + Tauri + React + TypeScript，基于 workbuddy-switch 扩展，综合另两个参考项目。正式名称 AiAccountCenter；当前已核对根目录 E:\phpstudy_pro\extraproject\AiAccountCenter，内容为新 Rust 项目，mycount 为历史暂存名。方案已补充 WebView2 兼容与验收要求，尚未实现。

当前交付为方案文档、环境入口/检查脚本及忽略规则，开发工具独立于项目。当前目录已有 Git；尚未引入产品源码或执行产品测试。本文件不继承旧 Python AiAccountCenter 的阶段、测试数量或可执行产物。用户已明确只补缺失环境、不随产品打包，并指定新增工具集中到 E:\rustenv；Rust/Cargo 和 MSVC 已到位且最小编译/链接/运行检查通过。目录改名由用户手动处理，Agent 未删除或移动项目目录。

## 2. 已进行的只读核对

- 初次建立方案时，暂存目录 E:\phpstudy_pro\extraproject\mycount 存在且为空，未发现其自身或上级 AGENTS.md；当时建立了本套 12 份文档。当时保留文件名与相对链接；本轮已核对实际根为新 Rust AiAccountCenter，Agent 未移动目录。
- 当前实际环境为 Windows 10 x64 Build 19045、PowerShell 7.6.5，Git 命令可解析。
- 安装前核对 Node v22.19.0、npm 10.9.3 可执行，Rust/Cargo/Rustup/Docker 未在当时命令解析中发现；本轮只补 Rust/MSVC，Node/npm 直接复用。
- 安装前核对未发现可用 MSVC，SDK 10.0.26100.0 和 WebView2 Runtime 154.0.4258.62 已有登记；本轮 SDK 实际链接通过，WebView/Tauri 环境创建及 Linux/Docker 仍未验证。
- 只读参考了旧项目方案、项目指令、参考记录与三个公开源码 ZIP，重新计算了归档 SHA-256。
- 读取了三个根 MIT LICENSE 及 vendor/agent-companion 的 MIT 声明，核对了关键结构/函数。
- 在线核对主仓库 v0.1.60 注解标签指向 d0badb47a9253f633e02888da9a62cff107809ec。
- 基线账号 JSON、固定 loopback API、自动 Hook/后台循环、国际会话结构限制已写入方案。
- 旧 Python 项目未被修改/删除，Agent 未执行项目目录改名；没有访问真实客户端或执行平台任务/通知；用户随后提供的 testdata 仅核对 JSON 格式、数量和字段存在，不输出凭据内容。
- 本次维护统一新 Rust 项目名称，明确用户手动目录切换；补充在线/离线安装包、便携运行、启动前正式 Runtime/最低版本检测及原生修复提示、权限/失败/干净机器验收矩阵。
- 已查阅 Tauri 与微软官方分发/检测文档；这是设计依据，不是安装、构建或兼容测试通过记录。

详细来源在 [REFERENCE_SOURCES.md](REFERENCE_SOURCES.md)，环境元数据在 [DEVELOPMENT.md](DEVELOPMENT.md)。

### 2.1 本轮环境安装与验证（2026-10-14）

- Rust 1.98.1 / Cargo 1.98.1 / rustup 1.29.1 / rustfmt / clippy 位于 E:\rustenv\rustup、cargo，自动自更新关闭。迁移后 rustc -Vv 与 cargo -V 正常。
- Visual Studio 2022 Build Tools 17.14.41 / MSVC 14.44.35207 位于 E:\rustenv\msvc。原项目 .tools\msvc 已通过官方卸载器卸载，退出码 0；新位置重装退出码 0，vswhere 返回新路径，不要求重启。
- scripts/check-env.ps1 已从项目根和环境根各运行通过，最终日志为 E:\rustenv\checks\AiAccountCenter\verification-20261014.log：复用 E:\Nodejs 的 Node 22.19.0 / npm 10.9.3，MSVC x64 与 E:\Windows Kits\10 的 10.0.26100.0 头文件/库完成最小 C 程序编译运行（msvc-sdk-ok），Rust 程序链接运行（rust-link-ok），rustfmt/clippy 版本调用成功。
- npm 实际缓存为 E:\rustenv\cache\npm，TEMP/TMP 为 E:\rustenv\tmp，Rust/Cargo homes 在指定环境根。入口加载和验证前后用户/系统持久 PATH 未变化。产品 CARGO_TARGET_DIR 动态指向项目 build/cargo。
- 官方微软安装器仍使用标准系统目录、注册表、用户 AppData 日志/状态；保留既有 E:\Microsoft Visual Studio\Packages / Shared 策略，不把这些不可避免影响宣称为完全目录隔离。详细来源、摘要、目录与失败处理见 DEVELOPMENT.md。
- 工具不提交、不随源码或发行包打包；项目 .gitignore 仅覆盖可重新生成的工具、缓存、临时数据、build/dist/node_modules，不排除重要参考资料或 testdata。误下载的 Node 便携包仅留在外部 downloads，未安装或使用；后续只补缺项。
- 用户提供 testdata/workbuddy国际.json（5 条）和 testdata/workbuddy国内.json（2 条），均可解析为 JSON 数组，含凭据相关字段。仅核对格式与数量，未执行账号请求；用户已明确当前为私有仓库的开发测试阶段，testdata 纳入版本管理并提交，保留原始内容，不当成合成数据，供后续隔离导入验证；源码提交与正式发行包内容分别管理。
- WebView2 已登记复用，Tauri/产品全量构建与真实 WebView 创建仍待 T01；Linux/Docker、真实平台、客户端写入和发行兼容仍未验收。

### 2.2 是否具备完整开发、测试和打包条件

当前具备开始 Windows 开发和最小编译运行的基础条件，尚不能确认完整流程就绪。产品源码/锁定依赖尚未导入安装，因此产品 Rust/TypeScript 测试、前端生产构建、Tauri 实际运行、在线/离线安装包与便携目录打包均未执行；安装包工具与运行时缺失场景仍待验证。Linux Web 服务还缺可验收的目标环境。testdata 只证明备份可解析，不能替代导入功能和真实平台验收。

后续从固定源码导入与副作用隔离开始，按锁文件准备项目依赖，先跑 Windows 完整构建/测试/打包，再补真实 Linux 环境及发行兼容验证；按实际缺项处理，不重装已有兼容工具，不以最小探针通过宣称全流程完成。

## 3. 阶段状态

| 阶段 | 状态 | 还缺什么 |
| --- | --- | --- |
| T00 范围与基线 | 部分完成 | 新 Rust 根及最小 Windows 环境已验证；产品基线复现随 T01，在开发授权后执行 |
| T01 上游导入与隔离 | 未开始 | 产品开发授权、Git 导入、品牌/数据/启动副作用隔离；环境已准备 |
| T02 三种运行形态 | 未开始 | 同源 API、运行权限、server 编译与运行 |
| T03 数据与管理安全 | 未开始 | 账号/凭据/账本、认证、迁移、备份与互斥 |
| T04 WorkBuddy | 未开始 | 主仓库回归及国际技术会话新增路径 |
| T05 Trae | 未开始 | 协议、身份、额度、任务与凭据维护 |
| T06 Qoder | 未开始 | OAuth/PAT/IDE 用途、额度、活动与刷新 |
| T07 调度与通知 | 未开始 | 持久去重/恢复、批量任务、后台与投递 |
| T08 Windows 客户端 | 未开始 | 发现/绑定/切换/恢复、真实身份验收 |
| T09 会话/统计/Companion | 未开始 | 上游能力保留、格式回归、Hook 与原生行为 |
| T10 Linux 与 Web 部署 | 未开始 | 真实无桌面 Linux、远程授权、可选 Docker |
| T11 整合发行与恢复 | 未开始 | 干净 Windows 10/11 x64、在线/离线包、便携运行、WebView2 兼容矩阵、签名/更新与恢复验收 |

## 4. 待提供条件

- 本轮最小 Windows 开发环境已完成；源码导入和产品功能开发另待明确启动指令。
- 固定上游完整构建/Tauri 运行仍待 T01 验证；当前 Rust/MSVC/SDK 的最小编译链接检查已通过。
- 经授权的 Linux 目标环境；Docker 不是开始其他开发的前提。
- 真实平台凭据、客户端版本与相应读写/任务验收授权，留到需要时提出。
- 自有发布仓库、更新地址、公钥/签名流程与必要证书，留到发行阶段提供。

这些是后续阶段条件，不是本轮文档交付缺失。

## 5. 下一步

环境准备已完成，后续会话先加载 scripts/dev-env.ps1，必要时运行 scripts/check-env.ps1，不重复安装已有兼容工具。待明确产品开发指令后进入 T01 安全导入，后续导入功能使用 testdata 的备份在隔离数据目录验证。先隔离上游副作用，再运行程序；先保护管理面，再开放远程 Web。

## 6. 后续记录规则

每阶段填写：实现位置、提交、环境、真实/合成数据类型、命令/步骤、实际结果、未完成项与下一步。不要把“有参考源码”“页面能打开”“模拟接口返回成功”写成真实完成。

本轮通过的是开发工具最小编译/链接/运行与文档一致性检查；产品 Rust/前端/平台/客户端/部署测试仍全部未运行，不能混记。
