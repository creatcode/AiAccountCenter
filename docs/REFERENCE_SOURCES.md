# 参考项目、固定提交与源码核验

核查日期：2026-10-13。本文记录方案证据，不表示 AiAccountCenter 已导入或运行相应源码。

## 1. 来源与采用策略

| 来源 | 固定提交 | 使用范围 |
| --- | --- | --- |
| [changexbc/workbuddy-switch](https://github.com/changexbc/workbuddy-switch) | d0badb47a9253f633e02888da9a62cff107809ec，v0.1.60 | 主代码基础，保留 Rust 核心、Tauri、React、HTTP 服务与既有业务 |
| [smart-open/TraeWorkAssistant](https://github.com/smart-open/TraeWorkAssistant) | af5c59e76fddb568cb58dcbb0855878174f56980 | Trae/Qoder 的协议、账号、活动、凭据及客户端快照参考 |
| [ardeyouxipianyi/workbuddy2api-hub](https://github.com/ardeyouxipianyi/workbuddy2api-hub) | 906ca6a097e3e002415db53ea84c9b89c1589bed | 仅 WorkBuddy 国际版补充协议与每日活跃技术会话，改为 Rust 实现 |

固定 SHA 是可复现起点，不代表最新版本。开发时不自动跟随 main，不凭 README 或演示确认真实集成。

在线核对：v0.1.60 的注解标签对象为 85dc204a23c144435b51485fad666d4d3e8d9d88，最终指向主仓库上述提交。标签对象显示 unsigned，不把它表述为经过签名验证的发布。

## 2. 本轮只读归档核验

初次核查读取了旧 Python 项目 .downloads/ 内既有 ZIP，没有修改、复制或解压旧项目文件。这些路径仅为历史证据；旧目录清理后不要求恢复。新 Rust 项目开发时从下列固定公开提交在自身 .downloads/ 重新准备资料，不能把旧目录作为运行或构建依赖。

| 归档文件名 | SHA-256 |
| --- | --- |
| workbuddy-switch-d0badb4.zip | c1e91fbf333057c256b94ee19f30a1d991478e06b399215d36fca7a1b2f4ee0c |
| TraeWorkAssistant-af5c59e7.zip | eae6c2b147e0b59d9c1a096a8434f0ee0be813a25fae294cc2bd16548b3239c7 |
| workbuddy2api-hub-906ca6a0.zip | 557cffb3579fdc4d7ef9d6b5f0316163e5dc09467c699f66c791ea0e7a30b612 |

ZIP 顶层目录均含对应完整 SHA。本轮校验了归档字节摘要与所需文本；尚未 git fetch、校验完整 Git 对象图、解析全部依赖或构建。GitHub 重新生成 ZIP 时归档字节可能变化，不能仅凭 ZIP 摘要不同就判定提交内容不同；必须同时核对提交、文件树与来源。

新项目核验记录只保存公开信息：仓库、提交、引用路径、文件摘要、许可和用途。不得把真实账号导出或客户端配置混入参考资料。

## 3. workbuddy-switch 关键源码

文件链接均固定到已核对提交。

| 路径 | 核对内容与影响 |
| --- | --- |
| [Cargo.toml](https://github.com/changexbc/workbuddy-switch/blob/d0badb47a9253f633e02888da9a62cff107809ec/Cargo.toml) | core/server/Tauri workspace；vendor-patched/indexmap 构建修复先保留，验证前不删除 |
| [crates/wb-switch-core/Cargo.toml](https://github.com/changexbc/workbuddy-switch/blob/d0badb47a9253f633e02888da9a62cff107809ec/crates/wb-switch-core/Cargo.toml) | 不依赖 Tauri；已有 reqwest/Tokio/rusqlite/AES-GCM；Linux 含 zbus 路径，不能由此假定无桌面运行已验证 |
| [crates/wb-switch-server/src/main.rs](https://github.com/changexbc/workbuddy-switch/blob/d0badb47a9253f633e02888da9a62cff107809ec/crates/wb-switch-server/src/main.rs) | serve/--port/--no-open；硬编码 loopback；启动任务循环、限额 watcher 与自动 Hook 安装 |
| [crates/wb-switch-server/src/api.rs](https://github.com/changexbc/workbuddy-switch/blob/d0badb47a9253f633e02888da9a62cff107809ec/crates/wb-switch-server/src/api.rs) | 同时暴露账号和桌面操作路由；需增加认证、运行权限与安全上传/导出 |
| [src/lib/api.ts](https://github.com/changexbc/workbuddy-switch/blob/d0badb47a9253f633e02888da9a62cff107809ec/src/lib/api.ts) | Tauri invoke/HTTP 双通道；API_BASE 固定为访问者本机，远程 Web 必须同源适配 |
| [config.rs](https://github.com/changexbc/workbuddy-switch/blob/d0badb47a9253f633e02888da9a62cff107809ec/crates/wb-switch-core/src/modules/config.rs) | WB_SWITCH_HOME 测试覆盖及 ~/.wb-switch；覆盖会连带影响 CLI 路径，不能当正式 AiAccountCenter 数据根设计 |
| [account.rs](https://github.com/changexbc/workbuddy-switch/blob/d0badb47a9253f633e02888da9a62cff107809ec/crates/wb-switch-core/src/modules/account.rs) | JSON load/save、id/uid 查询、脱敏元数据；新增平台身份与加密存储应从这些边界适配 |
| modules/oauth.rs、variant.rs、refresh.rs、credits.rs、credit_usage.rs、checkin.rs | 国内/国际认证、域名、刷新、额度和活动；实施前逐函数核对各路径，避免区域混用 |
| modules/codebuddy_cli.rs、codebuddy_ide.rs、codebuddy_cn_ide.rs、vscode_ext.rs、vscode_cn_inject.rs、jetbrains.rs、rotate.rs | 客户端身份、加密登录态、区域缓存、活动会话与 CLI 轮换 |
| [session.rs](https://github.com/changexbc/workbuddy-switch/blob/d0badb47a9253f633e02888da9a62cff107809ec/crates/wb-switch-core/src/modules/session.rs) | 约 281 行起能力探测：projects/ + workbuddy.db/sessions；国际版根可能不同构，不能只按 variant 开放 |
| modules/session_groups.rs、codebuddy_ide_session.rs、codebuddy_ide_session_sync.rs、vscode_session.rs、vscode_session_sync.rs、token_stats.rs | 会话命名空间、复制关联/增量同步与本地 Token 统计；不能用通用复制覆盖全部客户端 |
| src-tauri/、src/pages/、src/components/、vendor/agent-companion/ | 桌面 commands、账号和统计界面、Companion；保留有效组件并核对原生生命周期、Hook 和跳转 |
| package.json、package-lock.json、src-tauri/tauri.conf.json、scripts/ | 原脚本含 macOS 命令、上游签名路径与产品 ID；逐项改为新产品配置 |

除表内已给定的直接核对外，modules/ 下关联条目也来自旧方案的定位清单。正式改动前必须重新读取固定源码、核对真实调用链，不能仅凭本文行号编写实现。

当前基线的服务后台已有签到/轮换等循环，不得另起一个调度器与其同时执行。国际签到端点的“活动未开放/已过期”处理，不替代第三个项目的国际每日活跃会话流程。

## 4. TraeWorkAssistant 关键源码

固定源码入口：[af5c59e7 文件树](https://github.com/smart-open/TraeWorkAssistant/tree/af5c59e76fddb568cb58dcbb0855878174f56980)。

| 路径 | 参考内容 |
| --- | --- |
| src-tauri/src/commands/trae_apps.rs | Trae Work/Trae CN 的路径与账号发现；两种客户端的 UID 证据不同 |
| src-tauri/src/tasks/trae_checkin.rs | Trae 每日活动；核对设备上下文、状态与领取条件 |
| src-tauri/src/commands/switch.rs、switcher/profile.rs、icube.rs、copy.rs、authfile.rs | 白名单快照、日志 UID、退出/恢复及多文件状态；不能自动扩展设备标识功能 |
| src-tauri/src/commands/qoder/accounts.rs、common.rs、credits.rs、oauth.rs、checkin.rs | Qoder 账号、PAT/OAuth、额度、活动与状态核查 |
| src-tauri/src/commands/qoder/ide_store.rs | secret://aicoding.auth.userInfo；Local State/DPAPI/AES-GCM；真实 IDE 凭据与 PAT 用途区别 |
| src-tauri/src/tasks/qoder_checkin.rs、qoder_credits.rs | 定时查询/领取的条件、重复执行与失败处理 |

本轮直接核对了 qoder/oauth.rs、qoder/ide_store.rs、trae_apps.rs、switcher/icube.rs 的关键定义，其余条目是后续实施必须复读的定位。

Trae Work 原路径包含 TRAE SOLO CN，Trae CN 原路径为 Trae CN；名称和路径须按实际安装核实。Trae CN 的日志 UID 只是身份证据之一，不代表 JWT 能直接写入 IDE。

上游 Rust commands 耦合 AppHandle/State/AppState。选择所需协议与算法适配到 core，不能把第二套 AppState、调度器、React 组件树整体拼入主仓库。

## 5. workbuddy2api-hub 参考边界（国际版 + 国内版任务与福利）

固定源码入口：[906ca6a0 文件树](https://github.com/ardeyouxipianyi/workbuddy2api-hub/tree/906ca6a097e3e002415db53ea84c9b89c1589bed)。

**允许参考的范围**（2026-10-14 用户明确扩大，AGENTS.md 第 2 节同步更新；与旧表述冲突时以本节为准）：

- 国际版：wb_accounts.py（can_daily_chat、daily_chat_web、_daily_chat_web_upstream、daily_chat，参考国际任务资格与业务编排）、wb_webagent.py（沙箱连接、initialize、session/load、session/prompt、SSE/JSON-RPC 消息与 completed/failed 判断）、wb_activity.py（活动结果和奖励确认口径）。仅参考国际认证/刷新/额度的必要补充。
- **国内版任务与福利**：wb_tasks.py（成长任务查询/接取/领奖、`/v2/report` 事件上报、能量、连续打卡与补签/兑换、盲盒抽奖、猫猫旅行）及其实测约束，见 §5.1。

**仍不采用**：国内账号模块、网关路由、模型池、代理体系、Python 服务本体。

- 使用 Rust 的 reqwest/Tokio 实现必要流程；不承诺历史模型、奖励、路径一直有效。
- 远端 completed 证明会话执行结束，奖励完成还需相应活动证据。任务超时保留远端 ID 并先复查，不重复创建。

### 5.1 国内成长任务与福利（补核，2026-10-14）

用户明确要求把「福利任务」纳入范围（Buddy 旅行、盲盒抽取、任务领奖等），并指出本项目对应板块。本轮按固定提交 906ca6a0 重新拉取归档核对，**SHA-256 与本文 §3 记录逐字节一致**（`557cffb3579fdc4d7ef9d6b5f0316163e5dc09467c699f66c791ea0e7a30b612`）。

核对文件：`wb_tasks.py`（抬头自述「国内版成长任务与日常福利全自动完成引擎」）、`wb_activity.py`（活动结果与奖励确认口径）。

服务端基点：`copilot.tencent.com`（chat）、`www.workbuddy.cn`（web 回退）、`www.codebuddy.cn`（bill/兑换）。

| 能力 | 端点 | 备注 |
| --- | --- | --- |
| 成长任务列表 | `GET /v2/activity/growth/tasks` | 字段：task_code/title/description/jump_url/accept_status/task_type/progress{current,target}/reward_credit/reward_energy |
| 批量接取 | `POST /v2/activity/growth/tasks/accept` | 分片，参考实现按 20 个一批 |
| **任务领奖** | `POST /activity/growth/tasks/{code}/claim` | 失败回退 `www.workbuddy.cn/activity/growth/tasks/{code}/claim` |
| **事件上报点亮** | `POST /v2/report` | 进度靠上报事件推进；见下「不可伪造」约束 |
| 能量余额 | `GET /v2/activity/growth/energy` | |
| 连续打卡 | `GET /activity/growth/streak` | 配套 `POST /activity/growth/makeup-cards/use`（补签卡）、`POST /activity/growth/redeem`（兑换）、`GET /activity/growth/heatmap` |
| **盲盒/抽奖** | `GET /activity/growth/lottery/summary`、`POST /activity/growth/lottery/draw` | |
| **猫猫旅行** | `GET /activity/growth/buddy/travel/status`、`POST .../travel/depart`、`POST .../travel/claim`、`GET .../travel/config` | 与主仓库 `TRAVEL_API_PREFIX` 同族 |
| 成长中心资料 | `profile/growth-center` | 主仓库 travel.rs 已用到 |

必须沿用的实现约束（来自参考实现的实测注释，不是推测）：

- **不可伪造的任务**：`DESKTOP_ONLY_TASKS`（RichMeow_Chat / Library_read / Buddy_App / Buddy_App_QQ）上游只认桌面客户端真实行为信号（jump_url 为 `workbuddy://` 深链）。伪造 `/v2/report` 会被忽略或落到 heartbeat，进度恒为 0/1，claim 必然 400 `task not completed`。**正确处理是诚实跳过并给出深链，不是反复重试。**
- **夜间窗口**：`black_cat` 等只在 23:00–08:00 上报才计数，每天 1 次、累计 3 天。
- **事件去重**：专家/团队事件按 `(eventCode, id)` 去重，必须用**不同 id** 才能推进进度；重复同一 id 进度不动。参考实现备有已实测的专家/团队 id 池。
- **风控**：请求间隔 ≥1.0s，并使用稳定设备指纹；不得为提效并发打点。

复用状态：本节内容**已获授权复用**——用户 2026-10-14 明确要求把国内任务与福利纳入范围，AGENTS.md 第 2 节与本文 §5 已同步更新；协议、端点与实测约束可参考，实现仍须用 Rust 自行编写。

## 6. 许可与发布边界

已直接读取三个根 LICENSE，均为 MIT；主仓库 vendor/agent-companion/LICENSE 也是 MIT。这是已核对文件的事实，不是所有依赖、商标、图标和未来代码均已审计的声明。

开发导入时：

1. 保留原许可证正文与版权声明，不把 AiAccountCenter 的名称替换进第三方版权行。
2. 实际复用/改编的第二、第三来源代码在来源清单中记录原路径、提交、用途及许可；必要声明随发行包提供。
3. 核对 vendor、补丁、字体、图标、npm/Cargo 的实际锁定依赖及再分发要求。
4. 更新签名、公钥、GitHub owner/repo、应用 ID 与数据目录使用 AiAccountCenter 自有配置，不复用上游凭据。
5. 未取得自有发布源/密钥前，不向上游发布、不检查并安装上游程序更新。

## 7. 记录模板

每个新增证据记录：仓库 / commit / 原文件及函数 / 核查日期 / 业务用途 / 引入方式 / 许可 / 合成验证 / 真实验证 / 未决项。

## 8. Windows/WebView2 官方依据

2026-10-13 核对以下官方资料，支撑 DEPLOYMENT.md 第 4 节的发行设计，尚未据此实现或运行安装器：

| 官方资料 | 本项目采用内容 |
| --- | --- |
| [Tauri Windows 安装包](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options) | 内嵌引导程序的在线包、完整安装器的离线包；固定版为可选发行 |
| [Tauri WindowsConfig](https://v2.tauri.app/reference/config/#windowsconfig) | 按锁定 schema 核对 webviewInstallMode 和 minimumWebview2Version，安装后仍须复检 |
| [微软 Runtime 分发与检测](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution) | 机器/用户范围检测、Loader 可用性、预览通道区别、权限与分发处理 |
| [微软 Evergreen 与固定版](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/evergreen-vs-fixed-version) | 默认 Evergreen；采用固定版时由应用维护更新 |

原文示例的补丁版本、包体积和 Tauri 字段不能直接作为本项目最低要求或已验证配置。最低 Runtime 版本由实际所用 API、前端能力、锁定依赖及干净机器验收共同确定。

不要记录完整响应中的 Token、密码、私人提示词或会话正文。
