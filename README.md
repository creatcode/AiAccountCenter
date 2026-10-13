# AiAccountCenter

项目名统一为 AiAccountCenter，继续采用 Rust 路线。基于 [changexbc/workbuddy-switch](https://github.com/changexbc/workbuddy-switch) 扩展的独立账号管理项目，采用 Rust + React + TypeScript，覆盖 WorkBuddy 国内/国际版、CodeBuddy 工具、Trae 和 Qoder。

文档日期：2026-10-13。当前阶段：方案与项目文档已建立，尚未引入上游源码、安装依赖或开始产品开发。

## 项目定位

- Windows x64 桌面版：沿用 Tauri，提供账号、客户端切换、支持的会话管理、统计、托盘和悬浮窗。
- Windows/Linux Web 版：运行独立 Rust 后端，通过浏览器管理账号、查询积分、设置与执行每日任务，使用者无需安装 EXE。
- Linux 无桌面运行：不依赖图形桌面、目标 AI 客户端或桌面密钥环；支持直接运行，Docker/Compose 为可选部署方式。
- 自动任务由持续运行的后端执行。浏览器关闭后任务继续；后端停止后任务停止，不承诺关机后继续执行。

本项目重新建立代码基础，保留并扩展 workbuddy-switch 的 Rust/Tauri/React 架构。旧 Python 项目不作为产品代码、运行时或数据库依赖。当前文档暂存于 `mycount`；方案确定、旧目录另行处理后，由用户手动把暂存目录改名为 `AiAccountCenter`。Agent 不执行项目目录删除或改名。目录约定见实施方案第 0 节。

Windows 桌面首版提供在线与离线安装包：检测并复用满足要求的 WebView2，缺失或版本过低时处理安装/升级；启动前检测失败须显示原生修复提示，避免白屏。Windows/Linux 独立 Web 服务不依赖 WebView2。详情见 [部署设计第 4 节](docs/DEPLOYMENT.md#4-windows-桌面发行与-webview2-兼容)。

`MYCOUNT-实施方案.md` 暂时沿用原文档文件名，内容与产品名称已统一为 AiAccountCenter；项目目录改名不影响相对链接。

## 三个上游的分工

| 项目 | 用途 |
| --- | --- |
| workbuddy-switch | 代码基础；复用账号、客户端切换、会话、统计、桌面和 WebUI 的有效实现 |
| TraeWorkAssistant | 参考并适配 Trae/Qoder 的认证、额度、任务、客户端快照与恢复 |
| workbuddy2api-hub | 仅参考 WorkBuddy 国际版认证补充、额度和每日活跃技术会话，使用 Rust 实现所需流程 |

具体提交、源码位置和复用边界见 [参考资料](docs/REFERENCE_SOURCES.md)。上游已有代码不等于 AiAccountCenter 已完成验收。

## 文档导航

| 文档 | 内容 |
| --- | --- |
| [实施方案](MYCOUNT-实施方案.md) | 完整目标、技术决策、架构、阶段任务和交付标准 |
| [项目指令](AGENTS.md) | 授权、操作目录、文件规则、编码和验证要求 |
| [参考资料](docs/REFERENCE_SOURCES.md) | 三个固定提交、校验记录、源码定位和许可处理 |
| [能力矩阵](docs/CAPABILITY_MATRIX.md) | 平台、区域、客户端、运行模式与不支持项 |
| [开发环境与上游维护](docs/DEVELOPMENT.md) | 项目内工具与缓存、基线导入、构建和上游更新 |
| [安全与数据](docs/SECURITY_AND_DATA.md) | 凭据、管理会话、迁移、并发、任务与恢复规则 |
| [界面要求](docs/UI_REQUIREMENTS.md) | 沿用上游界面、页面清单、状态和跨端行为 |
| [部署设计](docs/DEPLOYMENT.md) | Windows、Linux 直接运行、Docker、备份和更新 |
| [验收计划](docs/ACCEPTANCE.md) | 分层检查、异常矩阵和真实集成的证据要求 |
| [实施状态](docs/IMPLEMENTATION_STATUS.md) | 新项目真实进度、阻塞条件与下一步 |
| [下一位 Agent 交接](docs/NEXT_AGENT_HANDOFF.md) | 阅读顺序、授权边界和开发启动提示词 |

## 当前使用边界

当前目录只有文档，没有可运行的 AiAccountCenter 程序。文档中的接口、参数、目录结构和部署配置若标记为“设计目标”，须实现后才能使用。

本轮继续维护方案及配套文档。用户已明确暂缓环境准备，当前不下载、安装或启动开发工具；目录改名由用户手动处理。源码导入、依赖准备、开发、真实账号操作、客户端写入和发布分别遵循 [AGENTS.md](AGENTS.md) 的授权规则。
