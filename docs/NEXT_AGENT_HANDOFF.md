# 下一位 Agent 交接

日期：2026-10-14。项目正式名 AiAccountCenter，基于 changexbc/workbuddy-switch（v0.1.60，d0badb47a9253f633e02888da9a62cff107809ec）扩展的新 Rust 项目。根目录为本文件上级目录 E:\phpstudy_pro\extraproject\AiAccountCenter。

## 0. 硬边界（先看，勿违背）

1. **不要操作 git。** 禁止 remote / fetch / merge / commit / branch / reset / push。2026-10-14 曾因 `git remote add upstream https://github.com/changexbc/workbuddy-switch.git` 并 `merge --allow-unrelated-histories` 被用户当场制止，已 `git reset --mixed 7b88ae7` 回滚（HEAD 回到 `7b88ae7 chore(git): 调整忽略规则并将测试备份纳入版本管理`，磁盘文件保留）。**需要纳入版本管理的动作一律先问用户。**
2. 上游源码当前以**普通文件**存在于工作区（`crates/ src/ src-tauri/ vendor/ vendor-patched/ npm/ scripts/ public/` 等），无 git 历史关联。仓库 `origin` 是用户自己的 `gitee.com:willrook/ai-account-center.git`，未动。
3. 不删除/移动/改名/合并项目目录（目录改名由用户手动）；不创建子代理；不读取 recommend-experts、不调用 suggest_plugin_install。
4. 不读取/修改真实客户端数据；不执行真实账号任务、真实外部通知、系统组件安装、服务注册、网络开放或发布。这些需对应授权时再提出。
5. 远程更新/发布源固定为 **https://github.com/creatcode/AiAccountCenter**（源码托管 Gitee 仅开发测试阶段）。桌面端更新签名已配好：`plugins.updater.{endpoints,pubkey}` 已写入 tauri.conf.json，自有密钥对在项目根 `tauri-signing.key` / `.key.pub`（无口令）；**开发测试阶段按用户要求暂随仓库跟踪**（未加入 .gitignore），正式发布前须移出仓库并轮换。轮换策略见 DEPLOYMENT.md §4.4。
6. 复用/改编代码保留原 LICENSE 与版权；不沿用上游发布密钥、更新地址、App ID、数据目录。

## 1. 当前真实状态

### 已完成（均有验证证据，非空口）

**T01 上游导入与隔离（基本完成）**

- 上游源码以普通文件引入，无 git 历史关联。
- 数据根改名 `~/.ai-account-center`（`config::STORE_DIR_NAME`），与上游 `~/.wb-switch`、旧 Python 数据隔离；`WB_SWITCH_HOME` 仍作为测试隔离入口。
- 品牌：tauri.conf.json `productName=AiAccountCenter`、`identifier=com.aiaccountcenter.app`；package.json name=`ai-account-center`；server 二进制名 `ai-account-center-server`。
- 副作用开关：`config::background_tasks_disabled()`（env `AI_ACCOUNT_CENTER_NO_BACKGROUND_TASKS=1`）；server `--no-background-tasks`。
- 更新源：update.rs / update.ts / index.html 指向 creatcode/AiAccountCenter。
- **WebView2 启动前检测**：`src-tauri/src/webview2_check.rs` 已接入 `lib.rs::run()` 最开头；最低版本下限未设（release-blocker，T11）。
- 编译：`cargo check -p wb-switch-core -p wb-switch-server`、`cargo check -p wb-switch-rust` 均通过（桌面 crate 需 `src-tauri/binaries/agent-studio-runtime-x86_64-pc-windows-msvc.exe` 存在）。真实桌面运行与在线/离线打包未做，归 T11。

**T02 三种运行形态（进行中）**

- 前端同源 API：`src/lib/api.ts` `API_BASE=""`；`vite.config.ts` 加 `/api` proxy。
- server `--host`（默认 127.0.0.1；非回环已允许，启动时提示应由受控反向代理终止 TLS）、`--port`。
- `api.rs` `/api/mode`（mode=web + capabilities）、`reject_desktop_only` 中间件对桌面专属接口 403。
- Linux 无桌面构建/运行未做（静态判断登记为待验收）。

**T03 数据与管理安全（基本完成，本轮推进最多）**

- **账号账本以 SQLite 为唯一真源**：`core/src/modules/account.rs` 不再读写 `accounts.json`；`load_accounts()` / `load_accounts_in(store_root)` / `save_accounts()` / `save_accounts_in(store_root, ...)` 保持旧的调用语义（`Vec<Value>`），只换了持久化边界。
- schema v2（`db.rs`）：`accounts` 表重建（加 `data` JSON 列；`uid` 放开非空——缺 UID 的历史记录存 NULL，避免空串撞 `UNIQUE(platform,region,uid)` 而被误合并）。`credentials` 表沿用 v1：`nonce`/`ciphertext` 分列，`ON DELETE CASCADE`。
- **凭据加密落库**：`access_token` / `refresh_token` / `auth_raw` / `profile_raw` 拆进 `credentials`，用 AES-256-GCM 加密，AAD = `account_id|platform|region|purpose`（密文移位/换用途必失败）。加密信封对象（`{$wbEncrypted,…}`）原样往返，切换写回不受影响。
- 主密钥语义：`master.key` 位于数据根；**密钥缺失且账本非空时不新建密钥**，账号仍可读但凭据字段缺失并打印原因（提示重新授权），不回退明文。
- **旧 `accounts.json` 一次性导入**：解析 → 一致备份 `accounts.json.bak` → 单事务写入 → 改名 `accounts.json.migrated`。损坏/非数组时保留原文件、账本视为空（与改造前行为一致），下次启动可重试。
- **迁移失败回滚**：`migrate()` 每版本独立事务，失败回滚且 `user_version` 不提升。
- **备份与恢复** `core/src/modules/backup.rs`（新增）：文件 = `头部(明文: magic/格式版本/时间/KDF参数/salt/nonce/来源应用版本) + AES-256-GCM 密文`；密文里是自包含包 `bundle_version | has_key | master_key(32) | db 快照`。口令经 Argon2id 派生 → **换机可恢复**（主密钥随包，但被口令再包一层，不是机器绑定）。恢复在头部/解密/SQLite 校验全部通过后才替换当前库，替换前留 `backups/pre-restore-<ts>.{db,key}`。
- **登录限速**（`server/src/api.rs`）：`LoginThrottle` 连续失败超过 `LOGIN_FREE_ATTEMPTS`(3) 后指数退避 1s→60s 封顶；窗口内直接 429 + `Retry-After` 且**不比对密码**；成功即清零。
- 管理登录/会话：Argon2id + 32 字节会话令牌（只存 SHA-256 摘要）；`require_auth` 中间件、写方法 Origin↔Host 同源校验、Cookie `aac_session` HttpOnly+SameSite=Strict；`db.revoke_all_sessions` / `purge_expired_sessions` 备用。
- 单实例租约：`instance_lease` + `acquire/renew/release`，`main.rs` 启动获取 + 10s 心跳续期；**Ctrl+C 优雅退出会释放租约**（不释放则停止后 30 秒内既不能重启也不能恢复）。
- **首次初始化引导码**（`api.rs::InitBootstrap`）：未设管理员密码时启动即签发 12 位一次性码，**只在本机控制台**打印；10 分钟有效、用后即失效、重启换新、错码不作废有效码；`/api/auth/init` 必须带 `code`（在密码格式校验之后消费，避免一次误输逼用户重启）；`/api/auth/status` 返回 `initCodeRequired`。
- **非回环监听已开放**：`--host 0.0.0.0` 等不再 `exit(2)`；启动时打印「应由受控反向代理终止 TLS」提示。本服务自身不提供 HTTPS。
- **Windows 主密钥 DPAPI**：用户范围 `CryptProtectData`（`CRYPTPROTECT_UI_FORBIDDEN`，不加 `LOCAL_MACHINE`），换用户/换机解不开；旧 32 字节明文密钥按**原值**就地升级；备份包携带的是解开后的原值再由口令封装，所以换机仍可恢复。
- **备份/恢复**（`backup.rs` + `api.rs` + `main.rs`）：HTTP 只有 `GET /api/backup/list` 与 `POST /api/backup/create`（创建只写数据根 `backups/`，不接受请求方路径）；**恢复走 CLI**（`restore --file <备份> --passphrase-file <文件>`），先查实例租约再动手，替换前留 `pre-restore-<ts>.{db,key}`。CLI `backup` 同理。
- 验证：core 库测试 **784 通过**（唯一 4 个失败是 `process::tests::windows_registry_probe_*`，沙箱黑名单拦 `reg.exe`，与本轮改动无关）；`cli_auth_sync_e2e` 5/5；server 15/15。冒烟覆盖初始化码全流程、备份 HTTP、运行中恢复被拒、停机恢复成功、错口令被拒。

**Web 管理面界面（已完成）**

- `src/components/auth-gate.tsx`：仅 Web 形态生效；未登录整页换登录表单，未初始化时要求**本机控制台一次性初始化码**；业务请求 401 派发 `aac:unauthorized` 事件把页面拉回登录态。
- `src/App.tsx`：门禁包在路由外（未登录不挂载页面，页面自身请求不会先撞 401）；侧栏加「退出登录」。
- `src/pages/SettingsPage.tsx::BackupCard`（仅 Web）：口令 + 立即备份 + 已有备份列表；恢复仍是停机 CLI，页面已写明命令。
- 验证：`npm run build`（tsc + vite）通过、`npx vitest run` 39/39；冒烟确认首页引用的 `assets/index-DMsO5v-a.js` 与 `dist/` 一致、`/api/auth/status` 的 `authenticated` 随登录/注销正确翻转、带会话 `/api/backup/list` 200、注销后 401。

### T04 真实验收结论（2026-10-14，用 `testdata/` 测试账号，隔离数据根）

用户授权后已跑通全链路：**上报 1 个事件真的把 `template_5` 从 4/5 推到 completed，再领奖真的到账 +100 积分 / +5 能量**，任务状态转为 `claimed`；同一天再跑一轮 `reported=0`（不重复打点）；夜间模式在窗口外 0 请求；桌面专属与公众号任务正确跳过并给深链。

**真数据揪出的两处语义（单测覆盖不到，改动时务必保留）**：

- **`completed` ≠ `claimed`**：真实状态机 `not_accepted → in_progress → completed（达标未领）→ claimed（已领到手）`。把 `completed` 当作已领会让奖励永远领不到；`tasks_payload` 的 `settled` 组只收 `claimed`。
- **状态值不是 `accepted`**：真实值是 `in_progress`；判定能否领奖一律走 `is_claimable()`，不要再写死字符串比较。分组要保证**总数守恒**（每个任务恰好出现在 claimable / manual / pending / settled 之一），早先静默丢任务导致真实账号上 12 个任务在界面里看不见。

冒烟写法（避免再踩路径坑）：数据根用 `mktemp -d /tmp/...` 后由 Git Bash 传给原生程序会被自动转成 Windows Temp 路径；**不要让 Windows python 去读 bash 的 `/tmp/xxx`**（它按 `C://tmp` 解释），需要落文件时写项目内 `.tmp/`。

### 未完成（下一步，新会话从这里开始）

**本次会话已把 T04 福利任务 + 桌面端 + 发行包收口**：协议/编排/持久化(schema v3)/后台循环/HTTP/界面/桌面命令全部完成并验证；真实账号验收通过（真上报 1 次、真领奖 +100 积分）；桌面启动阻塞（updater 配置缺失）已修；NSIS 安装包 + 更新签名工件已产出。证据见 docs/IMPLEMENTATION_STATUS.md §2.4 与 §3 状态表。

剩余（按建议顺序）：

- **T02 收尾**：前端入口隐藏改成按 `/api/mode` 的 capabilities 驱动（当前是 `isWebui()` 启发式）；Linux 无桌面构建/运行验收（缺环境则登记待验收）。
- **T03 剩余**：Linux 主密钥承载改「用户显式提供的只读 secret 文件」（现为自动生成 0600）——归 T10。
- **T04 剩余**：国际版技术会话新增路径（`wb_webagent.py` 沙箱连接/initialize/session、SSE 解析）未做。
- **T05 Trae**、**T06 Qoder**：协议、身份、额度、任务与凭据维护（参考 TraeWorkAssistant / REFERENCE_SOURCES §4）。
- **T07 调度与通知**：持久去重/恢复、批量任务、站内/桌面/Webhook 投递。
- **T08 Windows 客户端**：发现/绑定/切换/恢复、CLI 轮换（src-tauri 已有部分雏形）。
- **T09 会话/统计/Companion**：会话复制关联同步、Token 统计、Hook 生命周期。
- **T10 Linux 与 Web 部署**：真实无桌面 Linux、远程授权、数据卷/secret、可选 Docker。
- **T11 整合发行与恢复**：干净 Win10/11 x64 验收、在线/离线安装包（离线需带 WebView2）、便携运行、WebView2 兼容矩阵、升级/签名/备份恢复（签名密钥已就绪）。

## 2. 环境与构建命令（本机 + 沙箱）

开发环境在项目外 `E:\rustenv`（RUSTUP_HOME=`E:\rustenv\rustup`，CARGO_HOME=`E:\rustenv\cargo`，MSVC 14.44.35207，Windows SDK `D:\Windows Kits\10\...\10.0.26100.0`）。Node/npm 复用已有（22.x 满足 Vite 7 引擎要求）。

**沙箱限制（重要）**：

- `vcvarsall.bat` / `Enter-VsDevShell` 内部调 `reg.exe`，被沙箱程序黑名单拦截（不可绕过）。必须显式设置 `INCLUDE`/`LIB`/`PATH`（见下）。同理 `process::tests::windows_registry_probe_*` 4 个用例在沙箱里必然失败，**不是回归**。
- `npm ci` 的 esbuild postinstall 会 `spawnSync node.exe EBUSY`，**必须 `npm ci --ignore-scripts`**。
- 冒烟测试本机 curl 走系统代理，需 `curl --noproxy '*'`；服务用 `--no-open --no-background-tasks` 起，隔离数据根用 `export WB_SWITCH_HOME=<临时目录>`（注意 `export` 不能和 `./exe ... &` 写在同一条 `&&` 链里）。

最小可用构建（bash）：

```bash
cd E:/phpstudy_pro/extraproject/AiAccountCenter
export RUSTUP_HOME=E:/rustenv/rustup CARGO_HOME=E:/rustenv/cargo
export INCLUDE='E:\rustenv\msvc\VC\Tools\MSVC\14.44.35207\include;D:\Windows Kits\10\Include\10.0.26100.0\ucrt;D:\Windows Kits\10\Include\10.0.26100.0\shared;D:\Windows Kits\10\Include\10.0.26100.0\um'
export LIB='E:\rustenv\msvc\VC\Tools\MSVC\14.44.35207\lib\x64;D:\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64;D:\Windows Kits\10\Lib\10.0.26100.0\um\x64'
export PATH="/e/rustenv/msvc/VC/Tools/MSVC/14.44.35207/bin/HostX64/x64:/d/Windows Kits/10/bin/10.0.26100.0/x64:/e/rustenv/cargo/bin:$PATH"
export CARGO_TARGET_DIR=E:/phpstudy_pro/extraproject/AiAccountCenter/build/cargo
export npm_config_cache=E:/rustenv/cache/npm
# 前端
npm ci --ignore-scripts && npm run build   # 生成 dist/（server RustEmbed 依赖）
# Rust
cargo check -p wb-switch-core -p wb-switch-server --locked
cargo test  -p wb-switch-core --locked --lib -- modules::account modules::db modules::backup modules::crypto
cargo build -p wb-switch-server --locked      # 产出 build/cargo/debug/ai-account-center-server.exe
```

改依赖特性后首次构建要去掉 `--locked`（会更新 `Cargo.lock`）；可用 `--offline` 避免联网。本轮为启用 tokio `signal` 做过一次，`signal-hook-registry` 已在锁内。

离线运维命令（`WB_SWITCH_HOME` 指向数据根，恢复前必须先停止服务）：

```bash
BIN=build/cargo/debug/ai-account-center-server.exe
$BIN backup  --passphrase-file /path/to/pass.txt [--out /path/to/out.aacbak]
$BIN restore --file /path/to/x.aacbak --passphrase-file /path/to/pass.txt
```

## 3. 关键代码位置

| 位置 | 内容 |
| --- | --- |
| crates/wb-switch-core/src/modules/account.rs | 账本读写唯一入口：`load_accounts*` / `save_accounts*` / `accounts_db_in` / `split_account` / `import_legacy_json` |
| crates/wb-switch-core/src/modules/db.rs | schema v1→v2、`AccountRow`/`CredentialRow`、`read_accounts`/`write_accounts`、`backup_to`(VACUUM INTO)、租约、管理登录与会话 |
| crates/wb-switch-core/src/modules/crypto.rs | AES-256-GCM `encrypt_parts`/`decrypt_parts`、`key_path_in`/`load_or_create_in`、`write_key_at`、Windows DPAPI `seal`/`unseal` |
| crates/wb-switch-core/src/modules/auth.rs | Argon2id、会话令牌、`generate_init_code`、`constant_time_eq` |
| crates/wb-switch-core/src/modules/backup.rs | 口令封装备份 `create_backup` / `restore_backup` / `read_backup_info`（新增） |
| crates/wb-switch-core/src/modules/growth.rs | 国内版成长任务与福利（新增）：协议层 `fetch_growth_tasks` / `accept_tasks` / `claim_task` / `report_events` / `lottery_*` / `fetch_streak` / `use_makeup_card` / `redeem_tier`；编排 `plan_growth` / `codes_to_accept` / `report_progress` / `run_growth_for_account` / `run_streak_bonus`；约束 `task_spec` / `unforgeable_reason` / `in_night_window` / `derive_id` / `event_seed` / `MIN_REQUEST_INTERVAL`；桥接 `record_run` / `recent_runs` / `ran_today` |
| crates/wb-switch-core/src/modules/config.rs | `STORE_DIR_NAME`、`background_tasks_disabled()`、`store_dir()`（`accounts_file()` 已删除） |
| crates/wb-switch-server/src/api.rs | 路由、`/api/mode`、`reject_desktop_only`、`require_auth`、`/api/auth/*`、`/api/backup/{list,create}`、`/api/growth/{tasks,run,history,config}`、`LoginThrottle`、`InitBootstrap`、`AppState` |
| crates/wb-switch-server/src/main.rs | `--host/--port/--no-background-tasks`、`backup`/`restore` 子命令、初始化码打印、单实例租约 + Ctrl+C 释放、后台循环（rotate/travel/**growth**） |
| crates/wb-switch-core/src/modules/config.rs | `WORKBUDDY_CHAT_ENDPOINT`（copilot.tencent.com）/ `WORKBUDDY_WEB_ENDPOINT` / `GROWTH_API_PREFIX`；`load_growth_config` / `save_growth_config`（`auto_growth_config.json`，默认关闭） |
| src/pages/SettingsPage.tsx | `GrowthCard`（成长任务与福利，Web + 桌面）+ `BackupCard`（备份，仅 Web） |
| src-tauri/src/commands.rs | 成长任务桌面命令（新增）：`growth_tasks` / `growth_run` / `growth_history` / `growth_config` / `growth_config_save`，与 HTTP 路由共用 core 的 `tasks_view` / `tasks_view_all` / `runs_json` |
| src/lib/api.ts / vite.config.ts | 同源 API / dev proxy |
| src-tauri/src/webview2_check.rs + lib.rs::run() | WebView2 启动前检测 + 原生提示 |

## 4. 已踩坑（供下一位避免）

1. axum 0.7 中间件：`from_fn_with_state` + `State` 提取器在此签名下 trait bound 失败 → 改用 `from_fn` 闭包捕获 `AppState`。
2. `MutexGuard<Db>` 跨 `await` 不 Send → 锁作用域收敛到 `next.run().await` 之前。
3. `Connection::transaction()` 需 `&mut self`；用 `transaction_with_behavior(Immediate)` 做租约串行化。
4. `PRAGMA foreign_keys` 在事务内是 no-op；`DROP TABLE` 会按外键级联删除子表行。
5. **SQLite UNIQUE 与空值**：`UNIQUE(platform, region, uid)` 下，缺 UID 的记录若写空串会互相冲突；必须存 NULL（NULL 彼此不算相等）。schema v2 为此重建了 `accounts` 表。
6. **WAL 与 mtime**：账本提交先落 `app.db-wal`，主库 mtime 要等检查点才更新；「账号库最近是否被写过」要取两者较新者（`codebuddy_cli::account_store_written_within`）。
7. `VACUUM INTO` 的目标文件不能已存在；直接复制主库会漏掉 WAL 中已提交内容，备份必须走它。
8. `cargo test` 编译期较长，短超时的前台 bash 调用会在编译阶段被 SIGTERM；用后台任务跑。
9. `git restore --source=<旧提交> --worktree` 可只恢复工作区、不动索引。
10. **DPAPI 与主密钥**：Windows 上 `master.key` 存的是 DPAPI 密文，长于 32 字节；代码用「文件恰好 32 字节」识别 DPAPI 之前的旧明文密钥并按原值就地升级。备份包必须带**解开后**的密钥原值再由口令封装，否则换机恢复出来的凭据全是解不开的密文。
11. **恢复替换 `app.db` 是停机操作**：服务进程自己持有打开的连接，Windows 上改名会因共享冲突失败。CLI `restore` 用实例租约先拒绝掉「服务仍在跑」的情况；租约 TTL 30 秒，被强杀后要等它过期。
12. `--locked` 会在依赖特性变化时直接失败（本轮加 tokio `signal` 时遇到）；写文档里的命令时注意留出这个口子。
13. 冒烟脚本里别用 Windows 的 `python` 去读 Git Bash 的 `/tmp/...`（它按 `C:\tmp` 解释，会 FileNotFound）；用 shell 自带命令取文件名。
14. **改完前端必须让 server 重新嵌入**：`Assets` 是 `#[folder = "../../dist/"]` 的 RustEmbed，靠 `include_bytes!` 跟踪，`cargo build` 可能直接报 Finished 而嵌的还是旧包。可靠做法是 `cargo clean -p wb-switch-server` 后再 build，并**用 HTTP 取回 asset 与 `dist/` 同名文件比对**来确认；不要用 `grep 二进制` 判断（嵌入内容可能被压缩，命中与否不能作准）。

## 5. 下一步建议顺序

1. T02 收尾（按 capabilities 隐藏桌面入口）+ 各平台（T05 Trae → T06 Qoder，协议参考 TraeWorkAssistant）。
2. T07 调度与通知 → T08 Windows 客户端 → T09 会话/统计/Companion。
3. T10 Linux（真实无桌面环境到位后）→ T11 整合发行（签名密钥已就绪，见 DEPLOYMENT.md §4.4）。

全程遵守：**不要操作 git**；新增账号/凭据代码一律走 `account::load_accounts*`/`save_accounts*`，新表按 `db.rs` 版本化迁移追加并配「失败回滚」用例；真实平台/客户端/外部通知/发布等副作用须有对应授权再做。
