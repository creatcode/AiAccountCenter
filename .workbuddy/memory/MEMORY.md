# AiAccountCenter 项目长期记忆

## 环境约定（用户明确要求，勿违背）

- 开发环境以「项目所需工具是否已安装且可用」为判断前提，**不约定、不要求、也不探测任何固定目录名或盘符**。`rustenv` 只是某台机器曾用过的目录名。
- 检测到缺项时：先给出最小环境清单，**询问用户希望安装到哪个路径**，确认后再按官方流程安装到该路径。
- 脚本接口：`scripts/dev-env.ps1`、`scripts/check-env.ps1` 通过 `-EnvironmentRoot` 参数或 `AI_ACCOUNT_CENTER_ENV_ROOT` 环境变量接收路径；都没有时若 PATH 已有 Rust/Cargo 则复用，否则提示先确认路径。
- 本机当前取值（记录，非要求，勿在其他机器硬套）：开发环境目录 E:\rustenv，项目根 E:\phpstudy_pro\extraproject\AiAccountCenter。
- 开发环境不提交、不随源码或发行包打包；只影响当前进程，不改持久 PATH/HOME/执行策略/全局包管理器配置。

## 环境事实（2026-10-14 验证）

- Rust 1.98.1 x86_64-pc-windows-msvc、Cargo 1.98.1、rustfmt 1.9.0-stable、clippy 0.1.98、rustup 1.29.1（自动自更新已关闭）。
- VS 2022 生成工具 17.14.42 / MSVC 14.44.35207；Windows SDK 10.0.26100.0 装在 D:\Windows Kits\10（KitsRoot10 注册表指向）。
- 复用：Git 2.41.0（E:\Git\cmd）、Node/npm（D:\node，22.20.0 / 10.9.3）、WebView2 Runtime 154.0.4258.62。
- 验证证据：E:\rustenv\checks\AiAccountCenter\verification-20261014.log。

## 已知陷阱

- **rustup 代理 0 字节**：在部分环境（拦截硬链接）安装后，`cargo/bin` 下 rustc/cargo/rustfmt/clippy 等代理文件为 0 字节，rustup.exe 本体正常；表现为退出码 0 但零输出，极易误判为通过。修复方式是按 rustup 代理语义重新写入完整副本；检查时必须核对文件大小，不能只看退出码。
- **本会话沙箱黑名单含 reg.exe**，`Enter-VsDevShell` 无法执行；需要 MSVC 环境时可显式设置 INCLUDE/LIB/PATH 绕过。用户本机普通终端不受此限制。
- 官方 MSVC 安装器必然写系统目录与注册表（C:\Program Files (x86)\Microsoft Visual Studio\Installer、C:\ProgramData\Microsoft\VisualStudio、用户 AppData），不能宣称完全目录隔离或完全便携。

## 工作方式

- 提交遵循 Conventional Commits（中文，标题 ≤50 字符）。
- 产品源码导入与功能开发需用户单独授权；环境准备不等于产品开发授权。
- 未经明确授权不创建子代理。

## 硬边界（用户明确要求，勿违背）

- **不要操作 git。** 禁止对该仓库执行 remote/fetch/merge/commit/branch/reset 等任何 git 操作（2026-10-14 曾因擅自关联上游仓库被当场制止并回滚）。需要纳入版本管理的动作一律先问用户。
- 上游参考源码以普通文件形式放在工作区，不建立任何 git 历史关联。

## 产品约定

- 本项目自有数据根：`~/.ai-account-center`（`config::STORE_DIR_NAME`）。与上游 `~/.wb-switch`、旧 Python 数据目录严格隔离，不自动导入。
- 远程更新/发布地址：https://github.com/creatcode/AiAccountCenter （开发测试阶段源码托管 Gitee，但更新源只用上面这个）。
- 隔离运行开关：env `AI_ACCOUNT_CENTER_NO_BACKGROUND_TASKS=1` 或 server `--no-background-tasks`，关闭全部后台副作用（签到/轮换/旅行/hook/更新）。
- 桌面产品名 AiAccountCenter，App ID `com.aiaccountcenter.app`；内部 crate 名保持 wb-switch-* 以减少上游合并成本；服务二进制名 `ai-account-center-server`。

## 数据层约定（T03 定稿，改动前先看清）

- **账号账本 = 数据根下 `app.db`（SQLite），唯一真源。** `accounts.json` 已废弃，只在首次打开账本时被导入一次（`.bak` 备份 + `.migrated` 收尾改名）。新增账号/凭据相关代码一律走 `account::load_accounts*` / `save_accounts*` 或 `load_accounts_in(store_root)` / `save_accounts_in(store_root, …)`，不要再引入 JSON 账号库或第二份凭据副本。
- 凭据字段集合 `SECRET_FIELDS`（`account.rs`）= `access_token` / `refresh_token` / `auth_raw` / `profile_raw`，写库时拆进 `credentials` 表加密（AES-256-GCM，AAD = `account_id|platform|region|purpose`），读库时按 AAD 校验后重新注入账号对象。
- 主密钥 `master.key` 在数据根；**账本非空且密钥缺失时不新建密钥**，账号仍可读但凭据字段缺失并打印原因。
- 改 schema 一律在 `db.rs` 加新版本号 + 独立事务，并配「失败回滚、版本不提升」用例。`uid` 用 NULL 表示缺失（空串会撞 `UNIQUE(platform,region,uid)`）。
- 备份/恢复 `backup.rs`：口令（Argon2id）封装的 `主密钥 + 库快照` 自包含包，换机可恢复；恢复前必须全部校验通过再替换当前库。
- 备份/恢复、迁移、限速的验证命令见 `docs/IMPLEMENTATION_STATUS.md` §2.3。
- 主密钥：Windows 用用户范围 DPAPI（`master.key` 存密文，远长于 32 字节；恰好 32 字节=DPAPI 之前的旧明文，按原值就地升级）；其他平台为 0600 明文文件（Linux 按 §3 应改用户显式提供的 secret 文件，未做）。**备份包必须带解开后的密钥原值再由口令封装**，否则换机恢复出来的凭据全是解不开的密文。
- 管理面：首次初始化需**本机控制台一次性码**（10 分钟、单次消费、重启换新）；`--host` 非回环已允许，本服务不提供 HTTPS，应放在受控反向代理后。备份恢复是**停机操作**，走 CLI `restore --file <备份> --passphrase-file <文件>`（服务自身的打开连接会让 Windows 改名失败）；先查实例租约。
- 实例租约：Ctrl+C 优雅退出会释放；被强杀则残留最多 30 秒——这期间既不能重启也会让 `restore` 误判。租约 TTL 30s、心跳 10s。
- 加依赖特性后首次构建要去掉 `--locked`（会更新 `Cargo.lock`），可用 `--offline`。本轮为 tokio `signal` 更新过一次。
- **桌面应用启动阻塞（已解决）**：`tauri.conf.json` 的 `"plugins": {}` 缺 `updater.pubkey` 导致启动 panic；已补 `plugins.updater.{endpoints,pubkey}`（自有仓库端点 + 自有 minisign 公钥）。私钥在项目根 `tauri-signing.key`（无口令；开发测试阶段按用户要求随仓库跟踪、不加 .gitignore，发布前须移出并轮换）；发布走 CI secret + `createUpdaterArtifacts: true`；轮换策略见 DEPLOYMENT.md §4.4。上游 workbuddy-switch 是本项目基线，其「自带更新」= tauri-plugin-updater + 自有公钥 + 自建 latest.json（检查走 core manifest、下载走插件）。
- **忽略名单决定（用户 2026-10-14 拍板）**：`/release/`（安装包+签名，可再生产物）**忽略**；`src-tauri/binaries/agent-studio-runtime-*.exe`（4.4MB）**刻意不忽略、随仓库跟踪**（新克隆无需先跑 prepare 即可 `cargo check/build` 桌面 crate）；`tauri-signing.key` / `.key.pub` 开发测试阶段也随仓库跟踪（发布前须移出并轮换）。`vendor/agent-companion/{node_modules,dist-embed}` 必须显式忽略（根锚定 `/node_modules/` 覆盖不到，约 166MB）。上述均已在 `.gitignore` 写注释固定，勿当成"漏忽略"再加回去。
- **桌面构建前置**：`src-tauri/binaries/agent-studio-runtime-<target>.exe` 必须存在，否则 tauri build script 直接失败（`resource path ... doesn't exist`）。产出方式见 docs/IMPLEMENTATION_STATUS.md §2.4。`tauri build` 的 beforeBuildCommand 会跑 `npm ci`，本环境会因 safe-delete 超时删空 vendor 依赖，用 `--config '{"build":{"beforeBuildCommand":"echo skipped"}}'` 跳过。
- **改完前端必须让 server 重新嵌入**：`Assets` = `#[folder = "../../dist/"]` 的 RustEmbed，`cargo build` 可能报 Finished 却仍嵌旧包。可靠做法 `cargo clean -p wb-switch-server` 后再 build，并**用 HTTP 取回 asset 与 `dist/` 比对**确认；`grep 二进制` 不可作准。
- 管理面界面（仅 Web 形态）：`src/components/auth-gate.tsx` 登录/初始化门禁 + `SettingsPage::BackupCard` 备份 + `SettingsPage::GrowthCard` 成长任务；`isWebui()` 是客户端启发式，T02 要改成按 `/api/mode` 的 capabilities 驱动。
- 成长任务自动运行由 `auto_growth_config.json` 的 `enabled`/`nightEnabled` 控制，**默认全关**（上报客户端行为事件必须用户显式开启）；后台循环挂进既有调度（server main.rs / src-tauri lib.rs），不另起调度器。
- **福利任务（T04）**：协议层 + 编排在 `core/src/modules/growth.rs`；状态在 `db.rs` schema v3（`growth_runs` 运行历史 + `growth_events` 判重）。**事件 id 按天稳定派生**（`event_seed(uid,task,day,seq)`），同一天重跑不重复打点，不照抄参考实现的随机时间戳。`unforgeable_reason` 是「桌面专属 + 业务不可伪造」的统一出口（含关注公众号满 24h、真实捐款）。**上游有两套前缀**：任务列表/接取用 `/v2/activity/growth/tasks...`，其余用 `/activity/growth/...`；领奖走 chat 侧、失败降级 `www.workbuddy.cn`。约束：桌面专属任务不可伪造（诚实跳过给深链）、夜猫子任务只在 23:00–08:00、事件按 `(eventCode,id)` 去重且对话类事件没有 `id`（去重键回落到 requestId）、请求间隔 ≥1s、设备标识由 uid 稳定派生。端点证据见 `docs/REFERENCE_SOURCES.md` §5.1。
- **T04 真实账号验收（2026-10-14，用 `testdata/` 真实账号，隔离数据根）**：全链路通过——上报真推进进度、领奖真到账（+100 积分）。**两处只有真数据能暴露的语义**：① `completed` ≠ `claimed`（状态机 `not_accepted → in_progress → completed 达标未领 → claimed 已领到手`），把 `completed` 当已领会让奖励永远领不到；② 真实状态值是 `in_progress` 而非 `accepted`，判定一律走 `is_claimable()`；任务分组必须总数守恒（claimable/manual/pending/settled 四组不许丢任务）。
- **`testdata/` 是真实凭据**（国内 2 / 国际 5）。用户 2026-10-14 明确授权用它做真实平台验收；AGENTS.md §5.11 原写「不据此执行真实平台任务」，冲突时以用户最新指令为准。验收务必用隔离 `WB_SWITCH_HOME`，凭据不落日志。
- **复用边界（用户 2026-10-14 拍板）**：workbuddy2api-hub 的**国内版任务与福利**已正式纳入允许参考范围（AGENTS.md §2、REFERENCE_SOURCES §5/§5.1 已改）。仍不采用其国内账号模块、网关路由、模型池、代理体系与 Python 服务本体。**用户的新要求与方案冲突时以用户为准。**

## 构建环境（本机 + 沙箱限制）

- Rust/Cargo/rustfmt/clippy：E:\rustenv（RUSTUP_HOME=E:\rustenv\rustup，CARGO_HOME=E:\rustenv\cargo）。
- **vcvarsall.bat 在沙箱里不可用**（内部调 reg.exe 被黑名单拦截）。必须显式设置 INCLUDE/LIB/PATH 指向 MSVC 14.44.35207 与 D:\Windows Kits\10\Include|Lib\10.0.26100.0。
- **npm ci 需加 `--ignore-scripts`**（esbuild postinstall 会 spawnSync node.exe 报 EBUSY）。
- 构建顺序：先 `npm ci --ignore-scripts` + `npm run build` 生成 dist/，再 cargo check/build（server 的 RustEmbed 依赖 dist/）。
