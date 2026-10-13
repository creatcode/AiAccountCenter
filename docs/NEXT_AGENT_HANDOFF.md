# 下一位 Agent 交接

日期：2026-10-14。项目正式名 AiAccountCenter，为基于 workbuddy-switch 的新 Rust 项目。根目录为本文件上级目录，与 AGENTS.md、MYCOUNT-实施方案.md 对应；方案文件暂留原文件名。

## 1. 先读哪些文件

1. ../AGENTS.md
2. ../MYCOUNT-实施方案.md
3. IMPLEMENTATION_STATUS.md
4. REFERENCE_SOURCES.md
5. CAPABILITY_MATRIX.md
6. DEVELOPMENT.md、SECURITY_AND_DATA.md、ACCEPTANCE.md
7. 涉及界面/部署时读 UI_REQUIREMENTS.md、DEPLOYMENT.md

运行前确认实际内容与绝对路径。当前已核对实际根目录为 E:\phpstudy_pro\extraproject\AiAccountCenter，内容为新 Rust 项目；mycount 为历史暂存名。改名由用户执行，Agent 不删除、移动、重命名或合并项目目录，不自动重建 mycount。相同路径曾属于旧 Python 项目，必须读文件内容确认新 Rust 版本；根目录、工具与缓存路径动态计算。

## 2. 已确定、不得误读的决策

- 放弃以旧 Python 项目作为代码基础，重新建立基于 workbuddy-switch 的 Rust 扩展项目。
- 沿用主仓库 React/TypeScript、Tauri、wb-switch-core 与 wb-switch-server，不重新用 Go/Python 实现整套业务。
- 第二个参考 TraeWorkAssistant 用于 Trae/Qoder；第三个 workbuddy2api-hub 仅用于 WorkBuddy 国际版补充及每日活跃技术会话，所需流程使用 Rust。
- Windows x64 桌面、Windows/Linux 独立 Web、Linux 无桌面都在完整目标内；Docker 可选。
- Windows 桌面提供 embedBootstrapper 在线包、offlineInstaller 离线包及明确 Runtime 要求的便携目录；创建任何 WebView 前检查正式 Runtime/最低版本，失败用原生修复提示。详见 DEPLOYMENT.md 第 4 节和 ACCEPTANCE.md 第 7.1 节，不能把本机已有 WebView2 作为兼容验证。
- 新增 Rust/Cargo 与 MSVC 已集中到 E:\rustenv，最小编译/链接/运行通过；现有 Node/Git/SDK/WebView2 复用，不重复安装或打包开发环境。先加载 scripts/dev-env.ps1，实际结果见 IMPLEMENTATION_STATUS.md。
- Web 使用者无需安装 EXE。自动任务由持续运行后端执行，浏览器关闭继续，服务停止则停止。
- 会话复制、关联、增量同步、CodeBuddy 工具、CLI 自动轮换、统计和悬浮窗都保留在范围内，按客户端实际能力实现。
- WorkBuddy 国际用户会话不因 README 声称支持就开放；先检查 projects 正文、数据库归属与实际版本。
- Trae/Qoder 不承诺通用跨账号会话复制，不增加网关、模型池、MITM、设备重置、未验证地区或其他平台。

## 3. 当前真实状态

尚无 AiAccountCenter 产品源码或产品测试，现有 Git 已核对，不重新初始化。T00 最小 Windows 环境已验证，T01–T11 未开始；工具状态以 IMPLEMENTATION_STATUS.md 为准。

已固定三份参考起点并核对本地归档摘要。旧项目的代码、数据库、EXE、测试数量与完成状态不能复制为新项目成果。旧项目不是新产品依赖；旧目录清理由用户另行处理，本轮没有删除。目录改名明确由用户手动完成；旧 Python 代码/数据库/指令和完成状态不因新目录同名而继承。

用户已提供 testdata/ 下国内 2 条、国际 5 条备份，均为可解析 JSON 数组且包含凭据相关字段。用户已明确这是私有仓库的开发测试阶段，测试备份和重要资料应纳入版本管理并提交，不得擅自忽略、删除或改写；正式发行包内容另按发行清单管理。后续导入功能使用隔离数据目录验证，不据此执行真实平台任务。

## 4. 当前授权与后续开发

用户于 2026-10-13 明确恢复并优先要求安装环境、修改项目指令，允许必要环境文件与隔离验证，新增工具/安装包/工具缓存集中到用户后续指定的 E:\rustenv，MSVC 按官方流程重新安装到该根目录；现有兼容环境直接复用，先说明并记录系统登记等不可避免影响。开发环境不提交、不随产品打包。旧环境暂缓指令已撤销。尚未授权导入产品源码、开始功能开发或执行真实任务；交接文档本身不扩大这些授权。

用户后续明确说“按方案开始开发并允许项目内环境准备”后，可在项目内自主完成必要源文件、依赖、本地 Git 基线与隔离验证，不逐文件反复确认。系统组件、真实账号/客户端、外部通知、服务注册、网络开放和发布仍按对应授权执行。

未经用户明确授权，不创建子代理。

## 5. 开发获授权后的第一轮工作

1. 核对用户手动改名的实际结果、新 Rust 根目录、现有文件、Git 与必要工具；读本套文档，不先运行上游，不要求恢复旧目录。
2. 依据 DEVELOPMENT.md 建立本地文档基线，再安全引入固定上游历史。不要 clone 覆盖已有 README/AGENTS，不使用 hard reset/clean 解决冲突。
3. 加载已有外部开发环境（本机 E:\rustenv），按需配置项目调试数据目录；只补当前阶段真实缺项，不重装现成环境，不把工具打包进产品。
4. 改独立名称、App ID、数据根和更新配置，禁用上游启动时的自动 Hook/真实任务/发布副作用。
5. 复现前端/core/server 与 Windows 桌面基线，先接入启动前 Runtime/版本检测和原生失败提示。记录真实命令和问题；没有干净 Windows/Linux 环境时对应项保留待验收。
6. 按 T02/T03 完成同源 API、Web 桌面权限隔离、存储和管理保护，再推进各平台。
7. 更新 IMPLEMENTATION_STATUS.md，报告修改原因、范围、验证、未完成项和下一步，继续已有授权内的工作。

局部缺工具或目标环境时继续其他无关任务。不要静默改语言、减少完整范围或把合成验证当真实集成。

## 6. 可供用户发送的开发启动指令

以下提示词仅供用户后续开始产品开发时选择发送；当前环境准备已获授权，无需重复申请，本文件本身不构成产品开发授权：

~~~text
请在当前 AiAccountCenter 项目按 AGENTS.md 和 MYCOUNT-实施方案.md 开始开发。

我授权在已确认的项目目录内导入固定上游源码、建立本地 Git 基线、创建修改项目代码，并执行隔离构建和测试。复用已准备的外部开发环境，只安装真实缺项；遵循开发工具与产品数据分离的目录约束，开发环境不随产品打包；不要重复询问已授权的普通文件和依赖操作。

基于 workbuddy-switch 保留 Rust/Tauri/React 架构，综合 TraeWorkAssistant 的 Trae/Qoder 及 workbuddy2api-hub 的 WorkBuddy 国际版流程。按 T00 起的依赖顺序执行，保留完整功能范围。Windows 桌面按方案提供在线/离线安装包、便携运行要求和 WebView2 启动前检测/原生修复提示，不以本机安装状态代替兼容性验收。

当前根目录应已核对为新 Rust AiAccountCenter；目录改名由我手动处理，你不要执行项目目录删除、移动、改名或合并。不要访问/继承旧 Python 项目的代码、数据库或完成状态，不要修改全局配置或真实客户端，不执行真实账号任务、真实外部通知、系统组件安装、服务注册、网络开放或发布；这些需要对应授权时再具体提出。不要创建子代理。

每阶段更新实施状态，区分静态/合成/本机/真实集成和待验收，持续完成已授权工作。
~~~

## 7. 不能用来宣称完成的证据

- 上游 README、演示页面或旧项目记录。
- 空目录中的方案/接口设计与未实现参数。
- HTTP 200、Token 字段存在、收到首段 SSE、只创建国际会话。
- Windows 上 Web 启动代替 Linux 无桌面验证。
- 编译成功代替真实客户端切换/会话归属/原生悬浮窗验证。
- Dockerfile 存在代替容器运行，自有更新未配置却宣称签名升级通过。
