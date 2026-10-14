# AiAccountCenter 实施状态

更新日期：2026-10-14。

## 1. 当前结论

- 本机开发环境已在原有基础上按缺项补齐并完成最小验证：Rust 工具链、MSVC（C++ 生成工具）与 Windows SDK 均可用；Git、Node/npm、WebView2 Runtime 复用已有安装。
- 环境判断以“项目所需工具是否已安装且可用”为前提，**不约定、不要求、也不探测任何固定目录名或盘符**；检测到缺项时先询问使用者希望安装到哪个路径，再按官方流程安装。规则见 [AGENTS.md](../AGENTS.md) 第 4 节与 [DEVELOPMENT.md](DEVELOPMENT.md) 第 1 节。
- 产品仍处于方案阶段：尚未导入产品源码，未执行产品构建或产品测试。本文件不继承旧 Python AiAccountCenter 的阶段、测试数量或可执行产物。
- 正式名称 AiAccountCenter；当前根目录 E:\phpstudy_pro\extraproject\AiAccountCenter 是本机实际值，不是跨机器要求。mycount 为历史暂存名。目录改名由用户手动处理，Agent 未删除或移动项目目录。

## 2. 本轮只读核对

- 本机系统：Windows 10 x64 Build 19045，PowerShell 7.6.4，Git 2.41.0.windows.1。
- 初检结论：Node/npm 与 WebView2 Runtime 154.0.4258.62 可用；Rust/rustup/Cargo、MSVC、Windows SDK、vswhere 均缺失。注册表中只剩一条失效的 Visual Studio Installer 记录，`Windows Kits\Installed Roots` 不存在，D:\Microsoft Visual Studio 为空壳目录，故判定为不可复用。
- 目录核对：E:\phpstudy_pro\extraproject\AiAccountCenter 内容为新 Rust 项目；旧 Python 项目未被修改或删除，Agent 未执行项目目录改名。
- 只读参考了旧项目方案、项目指令、参考记录与三个公开源码 ZIP，重新计算了归档 SHA-256。
- 读取了三个根 MIT LICENSE 及 vendor/agent-companion 的 MIT 声明，核对了关键结构/函数。
- 在线核对主仓库 v0.1.60 注解标签指向 d0badb47a9253f633e02888da9a62cff107809ec。
- 基线账号 JSON、固定 loopback API、自动 Hook/后台循环、国际会话结构限制已写入方案。
- testdata/workbuddy国际.json（5 条）与 testdata/workbuddy国内.json（2 条）仅核对 JSON 格式、数量和字段存在，未输出凭据内容，未执行账号请求。
- 本次维护把环境规则改写为“能力检测 + 询问安装路径”，并同步 README、方案、指令、开发、交接与脚本；同时补充 WebView2 兼容与验收要求。

详细来源在 [REFERENCE_SOURCES.md](REFERENCE_SOURCES.md)，目录与复现步骤在 [DEVELOPMENT.md](DEVELOPMENT.md)。

### 2.1 本轮环境补齐与验证（2026-10-14）

| 组件 | 来源 | 结果 |
| --- | --- | --- |
| Rust 1.98.1（x86_64-pc-windows-msvc）、Cargo 1.98.1、rustfmt 1.9.0-stable、clippy 0.1.98、rustup 1.29.1 | rustup-init 1.29.1 官方安装器，SHA-256 `6f4bef…bdb7e` 已校验 | 安装退出码 0；自动自更新已关闭；版本调用通过 |
| Visual Studio 2022 生成工具 17.14.42 / MSVC 14.44.35207 | vs_BuildTools.exe 官方安装器，SHA-256 `985969…aa990` 已校验 | 提权静默安装退出码 0 |
| Windows SDK 10.0.26100.0 | 同上，`--add Microsoft.VisualStudio.Component.Windows11SDK.26100` | 头文件与库就位，编译链接通过 |
| Git / Node / npm / WebView2 Runtime | 复用已有安装，未重装 | 满足已核对基线 |

验证证据：E:\rustenv\checks\AiAccountCenter\verification-20261014.log、rustup-install.log、msvc-install.log。

- 工具解析：git、node、npm、rustc、cargo、rustfmt、cl、link、rc 全部命中真实路径。
- MSVC + Windows SDK 最小 C 程序编译、链接、运行通过，输出 `msvc-sdk-ok`。
- rc.exe 资源编译通过，生成 .res。
- Rust 程序编译、链接、运行通过，输出 `rust-link-ok`。
- 只影响当前进程：未修改系统/用户持久 PATH、HOME、Shell 配置、执行策略或全局包管理器配置。

已知问题与处理：

- **rustup 代理 0 字节**：首次安装后 `cargo/bin` 下 13 个代理（rustc、cargo、rustfmt、clippy 等）为 0 字节，rustup.exe 本体正常，原因是代理的硬链接创建被拦截；表现为命令退出码 0 但无任何输出，容易误判为“通过”。已按 rustup 代理语义重新写入完整副本，修复后全部可用。其他机器复现时按同样方式处理，并在检查脚本里核对代理文件大小，不能只看退出码。
- **Windows SDK 落点**：SDK 安装到 `KitsRoot10` 注册表指向的目录（本机 D:\Windows Kits\10），未强制放进开发环境目录内，这是官方安装器的既有策略；文档不把它宣称为完全目录隔离或完全便携。
- **脚本端到端检查未在本会话完成**：`scripts/check-env.ps1` 经 `dev-env.ps1` 调用 `Enter-VsDevShell`，该步骤需要 `reg.exe`，而本会话沙箱把 `reg.exe` 列入程序黑名单且不可绕过。已改用显式设置 INCLUDE/LIB/PATH 的方式完成等价验证（上表证据）。请在普通 PowerShell 7 终端执行一次 `dev-env.ps1` + `check-env.ps1` 做最终确认。

### 2.2 是否具备完整开发、测试和打包条件

具备开始 Windows 开发与最小编译运行的条件，尚不具备完整流程就绪的证据。产品源码与锁定依赖尚未导入，产品 Rust/TypeScript 测试、前端生产构建、Tauri 实际运行、在线/离线安装包与便携目录打包均未执行；安装包工具与 Runtime 缺失场景仍待验证。Linux Web 服务还缺可验收的目标环境。testdata 只证明备份可解析，不能替代导入功能和真实平台验收。

### 2.3 T03 收尾：账本真源、备份恢复、登录限速（2026-10-14）

| 项 | 实现位置 | 验证 |
| --- | --- | --- |
| 账号账本切到 SQLite | `core/src/modules/account.rs`、`db.rs`（schema v2） | 账号读写落 `app.db`；`access_token`/`refresh_token`/`auth_raw`/`profile_raw` 拆进 `credentials` 加密列，明文不出现在盘上（`ledger_encrypts_credentials_at_rest_and_restores_them`） |
| 旧 `accounts.json` 一次性导入 | `account.rs::import_legacy_json` | 解析 → 一致备份(`.bak`) → 单事务写入 → 改名(`.migrated`)；损坏时保留原文件、账本为空、可重试（3 个用例） |
| 迁移失败回滚 | `db.rs::migrate` + 用例 | 目标表残留导致重建失败时 `user_version` 不提升、原数据保留、清障后可重试 |
| 主密钥语义 | `crypto.rs`、`account.rs::read_ledger` | 密钥缺失且账本非空时不新建密钥、凭据不回退明文；密文移位/篡改只丢该字段（3 个用例） |
| 备份与恢复 | `core/src/modules/backup.rs`（新增） | `头部(明文,含KDF参数与来源版本) + AES-256-GCM(主密钥+库快照)`；口令经 Argon2id 派生，换机可恢复主密钥，因此凭据仍可解。错口令/篡改/非备份文件一律拒绝且不动当前库；恢复前留 `pre-restore-*.{db,key}`（6 个用例） |
| 登录限速 | `server/src/api.rs`（`LoginThrottle`） | 连续失败超过阈值后指数退避（1s→60s 封顶），窗口内直接 429 + `Retry-After`，不比对密码；成功即清零（2 个用例） |
| 会话管理补充 | `db.rs` | `revoke_all_sessions`、`purge_expired_sessions`（供改密码/清理用） |
| 首次初始化引导码 | `server/src/api.rs`（`InitBootstrap`）、`core/src/modules/auth.rs` | 未设管理员密码时启动即签发 12 位一次性码并**只在本机控制台**打印；10 分钟有效、用后即失效、重启换新；错码不作废有效码；`/api/auth/init` 必须带码（密码格式校验之后才消费，避免一次误输逼用户重启）（4 个用例 + 冒烟） |
| 非回环监听开放 | `server/src/main.rs` | 管理面保护（会话鉴权 / CSRF 同源 / 登录限速 / 一次性初始化码）到位后允许 `--host` 非回环，并提示应由受控反向代理终止 TLS；本服务自身不提供 HTTPS |
| Windows 主密钥 DPAPI | `core/src/modules/crypto.rs` | 用户范围 `CryptProtectData`（UI 已关闭）；旧 32 字节明文密钥按**原值**就地升级为密文；备份包携带的是解开后的原值，再由口令封装 |
| 备份接 HTTP + CLI | `server/src/api.rs`（`/api/backup/list`、`/api/backup/create`）、`main.rs`（`backup` / `restore` 子命令） | 创建只写数据根 `backups/`，不接受请求方指定路径；恢复是停机操作（替换 `app.db` 需要无实例占用），CLI 先查实例租约再动手；Ctrl+C 优雅退出并释放租约（否则停止后 30 秒内无法重启/恢复） |
| 备份文件名校验 | `core/src/modules/backup.rs::backup_path_in` | 只接受 `backups/` 内的 `*.aacbak`，拒绝 `../`、路径分隔符与目录外规范化结果 |
| Web 管理面登录/初始化界面 | `src/components/auth-gate.tsx`、`src/App.tsx`、`src/lib/api.ts` | 仅 Web 形态：未登录整页换成登录表单；未初始化时要求本机控制台的一次性初始化码；业务请求收到 401 自动回到登录态（`aac:unauthorized` 事件）；侧栏加「退出登录」 |
| 备份界面入口 | `src/pages/SettingsPage.tsx::BackupCard`（仅 Web） | 备份口令 + 「立即备份」+ 已有备份列表（时间/大小/来源版本，损坏项标原因）；恢复仍是停机 CLI，页面上已写明命令 |
| 前端会话态接口 | `server/src/api.rs::api_auth_status` | `/api/auth/status` 增加 `authenticated`（只反映本次请求自带 Cookie 的会话），供门禁判断显示登录页还是应用 |

命令：`cargo test -p wb-switch-core --locked --lib -- modules::db modules::account modules::backup modules::crypto`、`cargo test -p wb-switch-core --locked --test cli_auth_sync_e2e`、`cargo test -p wb-switch-server --locked`。

冒烟（隔离 `WB_SWITCH_HOME`，`--host 0.0.0.0`）：启动打印初始化码；`/api/auth/status` 未初始化时 `initCodeRequired:true`；错码/缺码 401、正确码 200 → 已初始化、`initCodeRequired:false`；重复初始化 409；未登录调备份接口 401；登录后 `backup/create` + `backup/list` 正常；服务运行中 `restore` 被拒（实例租约），停机且租约过期后恢复成功并留下 `pre-restore-*.db`；错口令恢复被拒。

Web 界面验证（`npm run build` + `npx vitest run`）：tsc 通过、vite 构建通过、vitest 39/39。服务端冒烟：首页引用的 `assets/index-DMsO5v-a.js` 取回 200 且内容含新界面（确认嵌入产物与 `dist/` 一致）；未登录 `/api/auth/status` 为 `authenticated:false`/`initCodeRequired:true`；用初始化码登录后变 `true`；带会话 `/api/backup/list` 200、注销后 401 且状态回落 `authenticated:false`。

未完成／未验收：

- **Linux 主密钥承载未按 §3 收尾**：要求使用用户显式提供的只读 secret 文件，当前仍是程序在数据根自动生成的 0600 文件。
- 管理面只有单一管理员密码，没有多用户/权限分级（方案本身只要求单管理员）。
- 前端仍是客户端启发式（`isWebui()`）判断形态，尚未改成按 `/api/mode` 的 capabilities 驱动（T02 收尾项）。
- **WorkBuddy 福利任务只做到协议层**：`growth.rs` 已能用（10 个单测覆盖桌面专属跳过、夜间窗口边界、设备标识稳定性、事件去重键、限速间隔、任务分组），但**尚未接编排、后台循环与界面**，也**没有真实账号验收**——端点是否仍有效、领奖是否真到账，都必须拿真实账号跑一次才算数（需要授权）。
- 复用边界已按用户要求更新：AGENTS.md 第 2 节与 REFERENCE_SOURCES.md §5/§5.1 已把国内版任务与福利纳入允许参考范围。
- Windows DPAPI 只在代码层固定为用户范围，未在多用户/换机场景实测。
- 恢复是**停机操作**（设计如此），未做在线恢复；`Cargo.lock` 因启用 tokio `signal` 特性已更新（`signal-hook-registry` 已在锁内，离线可复现）。
- 真实平台凭据、客户端读写、干净机器打包与 Linux 验证均未执行。

### 2.4 T04 福利任务：协议层 + 编排 + 持久化（2026-10-14）

| 项 | 实现位置 | 验证 |
| --- | --- | --- |
| 任务查询/接取/领奖 | `core/src/modules/growth.rs` | `fetch_growth_tasks`（企业版短路空列表）、`accept_tasks`（每批 20，**失败与原因必须可见**）、`claim_task`（chat 侧优先，被拒降级 `www.workbuddy.cn`） |
| 事件上报 | 同上 | `build_event` 覆盖 canvas/template/expert/team/lighthouse/skill/automation/playbook/skin/chat/glmchat/cat，未知类型退化为 heartbeat |
| 福利 | 同上 | `lottery_chances` / `lottery_draw`（盲盒）、`fetch_streak` / `use_makeup_card` / `redeem_tier`（连登闭环） |
| 编排 | `plan_growth` / `codes_to_accept` / `report_progress` / `run_growth_for_account` / `run_streak_bonus` | 只补差上报（不超额打点）；已达标的直接领奖；桌面专属与业务不可伪造任务进 `manual` 并带原因+深链；夜间任务在窗口外进 `waiting_night`；未登记规格的任务只报告不猜事件；`first_buddy` 自动类不接取 |
| 约束实现 | 同上 | `MIN_REQUEST_INTERVAL=1s` 串行限速；`derive_id` 由 uid 稳定派生设备标识（SHA-256 截断，未引入 md5 依赖）；事件种子 `event_seed(uid, task, day, seq)` 按天稳定 |
| 运行记录与判重 | `db.rs` schema **v3**：`growth_runs`（运行历史 + 「今天是否已成功跑过」）、`growth_events`（上游判重键本地留痕） | 迁移 v2→v3 用例；`growth_runs_record_and_query`、`growth_event_dedupe_is_idempotent_and_prunable` |
| 桥接 | `growth.rs::record_run` / `recent_runs` / `ran_today` / `open_ledger` | 运行记录写在账号账本同一库（单一真源） |
| 两轮模式 | `GrowthMode::Daily` / `Night` | 白天与夜间分开、各自按天去重：夜猫子只在 23:00–08:00 计数，若与白天挤在同一次运行里，白天跑过之后当天就再也等不到窗口 |
| 后台循环 | `growth::run_growth_cycle`（`GROWTH_CYCLE_INTERVAL` 30 分钟） | 挂进**既有**调度（`server/src/main.rs`、`src-tauri/src/lib.rs`），不另起调度器；`ran_today` 做当天短路；自动运行受 `auto_growth_config.json` 的 `enabled` 控制，**默认关闭** |
| HTTP | `api.rs`：`GET /api/growth/tasks`（含 `ranToday`/`inNightWindow`）、`POST /api/growth/run`（`kind=daily|night|streak`）、`GET /api/growth/history`、`GET/POST /api/growth/config` | 全部要求会话（未登录 401）；手动触发不受自动开关限制 |
| 界面 | `src/pages/SettingsPage.tsx::GrowthCard`（Web + 桌面，演示模式不显示） | **按账号分组**（多账号各自一块，带展示名）：自动运行/夜间两个开关、每个账号三个手动入口、任务四分（可领奖/待完成/需人工/已领）、最近 5 条运行历史 |
| 桌面命令 | `src-tauri/src/commands.rs`：`growth_tasks` / `growth_run` / `growth_history` / `growth_config` / `growth_config_save`（已注册进 `lib.rs` 的 invoke_handler） | 与 HTTP 路由共用 core 的 `tasks_view` / `tasks_view_all` / `runs_json`，不做第二套实现；`growth_command_tests` 覆盖运行路径 |

命令：`cargo test -p wb-switch-core --locked --lib -- modules::growth modules::db`、`cargo test -p wb-switch-server --locked`。

冒烟（隔离数据根，`--no-background-tasks`）：未登录三个成长接口均 401；`/api/growth/config` 默认 `{"enabled":false,"nightEnabled":false}`，写入后落盘 `auto_growth_config.json` 并可读回；无账号时 `tasks`/`run` 返回 400「账号不存在」；`history` 返回空；首页引用的 `assets/index-BfOAtj97.js` 与 `dist/` 同名同内容且含「成长任务与福利」。

#### 真实账号验收（2026-10-14，用户授权使用 `testdata/` 测试账号）

在**隔离数据根**（`WB_SWITCH_HOME` 指向临时目录，不碰真实数据目录）导入 `testdata/workbuddy国内.json` 的 2 个账号；凭据不落日志、不出现在接口响应里（`account_meta` 已剥离）。

| 验证项 | 结果 |
| --- | --- |
| 旧 JSON 一次性导入 | 2 个账号入库，生成 `.bak` / `.migrated`，接口不返回任何凭据字段 |
| 端点有效性 | `GET /v2/activity/growth/tasks`、`/activity/growth/streak` 均返回真实数据与业务码 —— 参考实现的端点仍有效 |
| **打点真的推进进度** | `template_5` 上报前 `in_progress 4/5` → 上报 1 次后变 `completed 5/5` |
| **领奖真的到账** | `{"claimed":1,"credit":100,"energy":5}`，`template_5` 由 `completed` 转 `claimed`，可领奖组清空 |
| 同一天不重复打点 | 立刻再跑一轮：`reported=0`（按天稳定 seed + 本地 `growth_events` 生效） |
| 夜间窗口门禁 | 17:xx 跑夜间模式：`不在夜间窗口（23:00–08:00），跳过`，`reported=0`，一个请求都没发 |
| 人工任务诚实跳过 | 桌面专属 3 个（服务端已标 `claimed`）+ 公众号任务正确进 `manual`，带原因与真实 `workbuddy://` 深链 |
| 业务失败不当异常 | 连登档位未解锁时 `ok=true` 并记录原因（"连续登录天数不足"），不误报失败 |
| 实例租约 | 服务被强杀后立刻重启被拒（残留 TTL 30 秒），30 秒后可接管 —— 与设计一致 |

**真实数据修正的两个语义错误**（单测没覆盖到，只有真数据能暴露）：

1. **`completed` ≠ `claimed`**：真实状态机是 `not_accepted → in_progress → completed（达标未领）→ claimed（已领到手）`。原实现把 `completed` 当"已领"跳过，导致达标任务的奖励**永远不会被领**、且任务在界面上消失。已改为只把 `claimed` 当已领，`completed` 进领奖清单。
2. **分组会静默丢任务 + 写死的状态值**：原 `tasks_payload` 把 `claimed`/`completed` 直接丢弃（真实账号上有 12 个任务因此看不见），且"可领奖"判定写死 `status == "accepted"`（真实值是 `in_progress`）。已加 `settled` 组（只有 `claimed` 算已领），可领奖判定改用 `is_claimable`，并补"总数守恒"用例。

副作用披露：上述验收在该测试账号上**真实上报 1 个事件、真实领取 1 次奖励（+100 积分 / +5 能量）**；未消耗补签卡、未消耗抽奖次数；只用了 2 个账号中的第一个。

多账号形状验证（真实账本，2 个账号）：不带 `accountId` 时 `GET /api/growth/tasks` 返回 `{accounts:[…]}`（两个账号各自一组、带展示名与各自 `ranToday`），带 `accountId` 时保持单组形状；此前不带账号会 400「账号不存在」，界面因此根本用不了（卡片调的就是不带账号那条）。

桌面端验证方式：`cargo check -p wb-switch-rust` 会被 build script 拦住——tauri 校验 `bundle.externalBin` 指向的 `src-tauri/binaries/agent-studio-runtime-x86_64-pc-windows-msvc.exe` 必须存在。**校验命令代码本身**用配置覆盖即可（不改任何文件）：

```bash
TAURI_CONFIG='{"bundle":{"externalBin":[]}}' cargo check -p wb-switch-rust --locked
```

实测该方式 `exit=0`（只剩 vendored 插件的告警），说明命令与注册都能编译。

#### 桌面端运行验收（2026-10-14）

externalBin 产出（等价 `scripts/prepare-agent-companion.mjs build`，但目标目录用被忽略的 `build/cargo`）：

```bash
cd vendor/agent-companion && npm ci && npm run build:embed
CARGO_TARGET_DIR=<root>/build/cargo cargo build --release --locked \
  --manifest-path vendor/agent-companion/Cargo.toml -p agent-studio-runtime --target x86_64-pc-windows-msvc
cp build/cargo/x86_64-pc-windows-msvc/release/agent-studio-runtime.exe \
   src-tauri/binaries/agent-studio-runtime-x86_64-pc-windows-msvc.exe
```

结果：

| 项 | 结果 |
| --- | --- |
| externalBin 就位后 `cargo check -p wb-switch-rust`（不带任何覆盖） | exit=0 —— 之前失败是因为 tauri build script 校验 `bundle.externalBin` 必须存在 |
| 桌面命令运行测试（`src-tauri/src/commands.rs::growth_command_tests`） | **通过**：隔离数据根下空账本返回 `{accounts:[]}`、历史为空、配置默认全关且能往返、未知账号/未知类型都返回可见错误 |
| `npx tauri build --no-bundle` | 产出 `build/cargo/release/wb-switch-rust.exe` |
| 启动应用（隔离 `WB_SWITCH_HOME` + 关后台任务） | **首次启动即 panic，见下** |

**启动阻塞（本次验收抓到的真问题）**：

```
panicked at src-tauri\src\lib.rs:331:
PluginInitialization("updater", "Error deserializing 'plugins.updater' within your
Tauri configuration: invalid type: null, expected struct Config")
```

`tauri.conf.json` 里 `"plugins": {}`，而 `tauri-plugin-updater`（2.10.1 `src/config.rs`）的 `Config` **要求 `pubkey: String`**（无默认值）；`update_service::download_package` 又是 `app.updater_builder()` 且未设 endpoints/pubkey，完全依赖插件配置。结论：**桌面应用在本项目里此前从未真正启动过**（与「外部运行时一直缺失」一致）。

用临时 `TAURI_CONFIG` 覆盖补一个占位 pubkey 重新构建后，应用**正常启动**：WebView2 155.0.4283.45 就绪、隔离数据根生成 `app.db` / `master.key` / `workbuddy_exe.json`、进程稳定存活 —— 证明这就是唯一的启动阻塞。

**该配置需要一个「自有签名公钥」，属于发布密钥决策，待用户确认后再改**（`AGENTS.md` 明确要求用自有密钥、不复用上游，且未取得自有发布源/密钥前不检查安装上游更新）。

已解决（2026-10-14）：按方案 A 补上 `plugins.updater`——endpoints 指向自有仓库、pubkey 为**自有 minisign 公钥**（`npx tauri signer generate` 生成；私钥在项目根 `tauri-signing.key`，开发测试阶段按用户要求暂随仓库跟踪；轮换策略见 `DEPLOYMENT.md` §4.4）。

未完成：
- **桌面 IPC 名称映射未做运行验收**：命令函数已被运行测试覆盖，但「前端 `invoke("growth_run", {kind, accountId})` → Rust `growth_run(kind, account_id)`」这条 camelCase→snake_case 链路没有在真实窗口里点击验证过。
- 未在**国际版**账号上验证（成长体系仅国内版，`testdata/workbuddy国际.json` 与本功能无关）。
- **桌面端还没接这套 Tauri 命令**：`GrowthCard` 目前只在 Web 形态显示。桌面接入需要改 `src-tauri/src/commands.rs`，而本机缺 `src-tauri/binaries/agent-studio-runtime-x86_64-pc-windows-msvc.exe` 占位文件、桌面 crate 无法完整构建验证，留到能验证时再做。
- 自动运行**默认关闭**是刻意的：它会向服务端上报客户端行为事件，必须用户显式开启。

## 3. 阶段状态

| 阶段 | 状态 | 还缺什么 |
| --- | --- | --- |
| T00 范围与基线 | 部分完成 | 新 Rust 根及最小 Windows 环境已验证；产品基线复现随 T01，在开发授权后执行 |
| T01 上游导入与隔离 | 基本完成 | 源码以普通文件引入；数据根/品牌/启动副作用/更新源隔离完成；core+server 编译通过；桌面真实运行已验收（启动 + IPC 点击）；发行安装包 + 更新签名已产出（v0.1.1 + 新图标）；`latest.json` 发布与在线/离线包归 T11 |
| T02 三种运行形态 | 进行中 | 同源 API、`--host`（非回环暂拒）、`/api/mode`、Web 拒绝桌面接口均已实现并运行验证；前端入口隐藏沿用 isWebui；Linux 非桌面构建待验收 |
| T03 数据与管理安全 | 完成 | 账本真源、凭据加密、迁移回滚、备份/恢复（core + HTTP + CLI + 界面）、登录限速、一次性初始化码、非回环监听、Windows DPAPI 主密钥、Web 登录/初始化界面均已实现并验证；仅余 Linux secret 文件承载（§3，归 T10） |
| T04 WorkBuddy | 基本完成 | 福利任务全链路 + 桌面命令 + 真实验收 + IPC 运行验收全部完成（见 §2.4）；仅余：国际技术会话新增路径 |
| T05 Trae | 未开始 | 协议、身份、额度、任务与凭据维护 |
| T06 Qoder | 未开始 | OAuth/PAT/IDE 用途、额度、活动与刷新 |
| T07 调度与通知 | 未开始 | 持久去重/恢复、批量任务、后台与投递 |
| T08 Windows 客户端 | 未开始 | 发现/绑定/切换/恢复、真实身份验收 |
| T09 会话/统计/Companion | 未开始 | 上游能力保留、格式回归、Hook 与原生行为 |
| T10 Linux 与 Web 部署 | 未开始 | 真实无桌面 Linux、远程授权、可选 Docker |
| T11 整合发行与恢复 | 进行中 | NSIS 安装包 + 更新签名 `.sig` 已产出（`build.bat` 一键，v0.1.1 + 新图标）；剩余：`latest.json` 发布流程、MSI/离线包、干净 Win10/11 x64 验收、WebView2 兼容矩阵、便携运行、升级失败恢复 |

## 4. 待提供条件

- 产品源码导入与功能开发另待明确启动指令。
- 固定上游完整构建与 Tauri 运行待 T01 验证；Windows 桌面按上游经审查的 `npm run tauri -- build` 参数执行。
- 经授权的 Linux 目标环境；Docker 不是开始其他开发的前提。
- 真实平台凭据、客户端版本与相应读写/任务验收授权，留到需要时提出。
- 自有发布仓库、更新地址、公钥/签名流程与必要证书，留到发行阶段提供。

这些是后续阶段条件，不是本轮文档交付缺失。

## 5. 下一步

1. 补 T04 收尾：桌面端成长任务的 Tauri 命令（`GrowthCard` 目前仅 Web，见 §2.4 未完成项）；再按 T05→T09 推进其他平台（Trae/Qoder 等）。
2. 推进 T02 收尾：前端改成按 `/api/mode` 的 capabilities 驱动入口隐藏（当前是 `isWebui()` 启发式，后端已强制拒绝）；Linux 无桌面环境到位后再做非桌面构建/运行验收。
3. 按 T04→T09 推进各平台与客户端；账号/凭据读写一律走 `account::load_accounts*` / `save_accounts*` 账本接口，不要再直接读写 JSON。
4. 真实平台凭据、客户端版本与对应读写/任务验收授权，留到需要时提出。

## 6. 后续记录规则

每阶段填写：实现位置、提交、环境、真实/合成数据类型、命令/步骤、实际结果、未完成项与下一步。不要把“有参考源码”“页面能打开”“模拟接口返回成功”写成真实完成。

本轮通过的是开发工具最小编译/链接/运行与文档一致性检查；产品 Rust/前端/平台/客户端/部署测试仍全部未运行，不能混记。
