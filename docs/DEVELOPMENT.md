# 开发环境、源码导入与上游维护

日期：2026-10-13。当前仅文档阶段。用户明确暂缓环境准备；本轮不下载/安装工具，不导入产品源码，不执行构建。以下步骤在用户恢复相应任务并具备对应授权后执行。

## 1. 原环境证据与可移植要求

本次会话只读核对：Windows 10 x64 Build 19045、PowerShell 7.6.5；Git 可解析；Node v22.19.0、npm 10.9.3 可执行。命令和常见位置未发现可用 Rust 工具链；常见 VS 安装位置、安装登记及实例目录未发现可用 MSVC。Windows SDK 10.0.26100.0 的头文件/库/工具目录存在，WebView2 Runtime 154.0.4258.62 已登记安装。尚未实际编译/链接或运行 Tauri，不能据此宣称完整环境已可用。Docker 未在命令中解析到，未作全机扫描。

已有 Python 3.13.16 仅用于只读 ZIP/文本分析，不是 AiAccountCenter 开发、部署或发行版的必装依赖。上述机器路径、版本都只是证据，不是跨机器硬要求。

主基线锁文件：Vite 7.3.6 的 Node engines 为 ^20.19.0 || >=22.12.0；Vitest 5.0.2 为 ^22.12.0 || ^24.0.0 || >=26.0.0。本机 Node 满足这些元数据，不等于 npm ci、测试与桌面打包已经通过。开发优先使用仍受维护且兼容锁文件的稳定 Node LTS。

Rust 先查锁文件、各 crate rust-version 与目标依赖，再选验证过的稳定版本。记录 rustc -Vv、cargo -V、目标 triple 和链接器。上游 edition=2021、注释里的本机 1.98.1 都不能替代实际 MSRV。

### 1.1 暂存目录与用户手动改名

项目正式名为 AiAccountCenter；当前在 E:\phpstudy_pro\extraproject\mycount 暂存，用户手动切换后的根为 E:\phpstudy_pro\extraproject\AiAccountCenter。按实施方案第 0 节执行边界：Agent 不删除/移动/重命名目录，不合并旧 Python 项目，不自动创建已废弃的暂存路径。

进入任务先按实际文档确定新 Rust 根目录。当前环境准备暂缓，尚无需迁移的私有 Rust 工具链；后续安装尽量局限于项目 .tools/，MSVC/SDK 等共享系统组件如有不可避免目录外写入，应先说明准确影响并按对应授权处理。

所有环境入口从自身所在项目根目录计算路径，仅设置当前进程/子进程变量。目录改名后重新计算 RUSTUP_HOME、CARGO_HOME、CARGO_TARGET_DIR、缓存/临时目录和构建路径；检查工具生成配置中的旧绝对路径并验证。原安装可复用不代表缓存/工具链保证跨机器可直接搬运。

## 2. 最小环境清单

| 工具/条件 | 用途 | 来源与处理 | 当前状态 |
| --- | --- | --- | --- |
| Git | 基线导入、上游差异和本地回滚 | 复用已安装 Git | 可解析，未在新目录初始化 |
| Node/npm | 前端安装、测试与生产构建 | 复用兼容版本；必要便携包放 .tools/ | 本轮版本已核对，未安装依赖 |
| Rust/rustup/cargo | core/server/桌面编译 | 官方稳定工具链，新增安装局限项目 | 未确认可用 |
| rustfmt/clippy | Rust 格式和静态检查 | 对应已选工具链组件 | 未确认 |
| MSVC/Windows SDK | Windows Rust/Tauri 链接 | 已有 SDK 优先复用；MSVC 如需新增按最小组件处理 | 未发现可用 MSVC；SDK 10.0.26100.0 目录存在，链接待验证 |
| WebView2 Runtime | Windows 桌面运行 | 复用满足发行最低版本的正式 Runtime；开发机不重复安装 | 本机 154.0.4258.62 已登记，实际环境创建待验证 |
| Linux 构建/运行环境 | 无桌面服务与 ABI 验证 | 经授权的本地/远程目标 | 未提供 |
| Docker | 可选容器构建/运行 | 仅复用已授权可用服务 | 命令未解析到 |

下载/安装前说明具体版本、来源、文件摘要、下载目录、安装目录、缓存/临时位置和目录外写入。禁止自动升级全部依赖来解决单一兼容问题。

## 3. 项目内目录与变量

沿用根 src/、src-tauri/、crates/、vendor/、package.json、node_modules/、dist/，不套一层无必要的 frontend/。

| 位置 | 内容 |
| --- | --- |
| .tools/rustup/ | 新增项目私有工具链与组件 |
| .tools/cargo/ | 项目私有 Cargo home；含 rustup shim/bin、registry/git 缓存 |
| .cache/npm/ | npm 缓存 |
| .cache/packaging/、.cache/tests/ | 打包/测试缓存与日志 |
| .downloads/ | 原始上游归档及安装包 |
| .tmp/ | 当前进程安装、编译和测试临时文件 |
| .runtime/ | 合成数据、隔离 home、调试数据与浏览器配置 |
| build/cargo/ | CARGO_TARGET_DIR |
| dist/ | Vite 生产静态文件，供 RustEmbed/Tauri 使用 |
| build/release/ | 整理后的可交付产物 |

Cargo home 的 registry/git 与 bin/toolchain 管理存在关联，采用 .tools/cargo/ 内统一布局以避免非标准符号链接。它是“Cargo 缓存位于 .cache/”规则的明确工具约束例外，实际下载仍完全在项目内；其他工具缓存放 .cache/。需要改变此布局时先核对 rustup/Cargo 实际写入，不能只改文档路径。

PowerShell 7 当前进程设置示例，须先切到已确认的项目根目录。所需目录在授权的实际安装/构建前按需创建：

~~~powershell
$projectRoot = (Get-Location).Path
$env:RUSTUP_HOME = Join-Path $projectRoot '.tools\rustup'
$env:CARGO_HOME = Join-Path $projectRoot '.tools\cargo'
$env:CARGO_TARGET_DIR = Join-Path $projectRoot 'build\cargo'
$env:npm_config_cache = Join-Path $projectRoot '.cache\npm'
$env:TEMP = Join-Path $projectRoot '.tmp'
$env:TMP = $env:TEMP
$env:TMPDIR = $env:TEMP
~~~

Linux 仅对当前 Shell/子进程设置对应绝对目录，不改 ~/.profile、系统 HOME 或全局包配置。新增 rustup 使用自定义 homes、--no-modify-path 和经核对的固定工具链，不运行未经审阅的管道安装脚本。

已有目录外 Rustup 默认可能写用户缓存/更新检查；优先核对并关闭非必要更新检查或使用获授权的私有工具链，不擅自改原安装。构建 SQLite bundled 等依赖仍可能需要 C 编译工具，不能因为主要语言是 Rust 就忽略链接器。

前端依赖执行项目 npm ci，不全局安装、不让 npx 自动下载工具。实际运行 npm.cmd/cargo.exe 等明确路径，避免 PowerShell 策略或 PATH 歧义。

## 4. 导入上游的可执行顺序

当前目录已有本套文档，不能直接 git clone 到“.”或用强制 checkout 覆盖。

开发授权后：

1. 重新读取目标文档、检查 Git 状态和所有重名文件，记录本套文档摘要。若已有 Git，沿用历史，不重新初始化。
2. 对尚无 Git 的目录建立独立本地仓库，先提交当前文档，形成可恢复基线；本地基线提交不包含凭据，不 push。
3. 添加只读用途的 upstream 远程，拉取 v0.1.60，核对标签最终提交为 d0badb47a9253f633e02888da9a62cff107809ec。GitHub origin/远程发布仓库待用户提供，不自行创建。
4. 将固定上游历史合入本地文档历史，可使用 git merge --allow-unrelated-histories --no-commit。导入属于已授权开发，不在本文档任务内执行。
5. 逐项处理重名冲突：保留 AiAccountCenter 的 README、AGENTS、方案和状态语义；保留上游原 README 到 docs/upstream/README.md，记录其来源；上游 LICENSE、vendor 声明和必要原注释保留。
6. 合并后先检查源文件、锁文件、工作流、子模块/gitlink 和 vendored 内容，再完成本地导入提交。不得因冲突使用 reset --hard/clean/强制覆盖。
7. 禁用未经审阅的发布工作流，配置 AiAccountCenter 名称、App ID、数据路径与更新源隔离。暂未配置更新源时禁用更新，不指向上游安装包。
8. 审查入口副作用，先建立安全的调试数据根与任务关闭机制，再运行任何原程序。
9. 记录主仓库导入 SHA、本地基线提交、上游差异及复现结果，更新实施状态。

若 git merge 因实际仓库状态不适用，先读 Git 证据选最小保留历史的方法，不删除现有文件或静默丢掉文档。已经存在的初始文档不作为必须重写的上游业务代码。

## 5. 安全启动前必须完成

- 新 Rust AiAccountCenter 数据命名空间与旧 Python 数据、上游 ~/.wb-switch、用户真实客户端目录隔离，不因同名目录自动导入旧库。
- 所有调试凭据为合成数据，自动任务、自动 Hook 安装和自动更新关闭。
- WB_SWITCH_HOME 只可作为审核后的上游基线测试覆盖，因为它会同时改变 CLI 等路径；正式新参数应区分应用存储与真实客户端。
- Windows WebView2 数据目录与浏览器测试 profile 在 .runtime/，测试 TEMP 在 .tmp/。创建 WebView 前的运行时/版本检测与原生失败提示按 DEPLOYMENT.md 第 4 节实现；本机已安装 Runtime 不代替干净机器缺失场景验收。
- 查看测试代码是否使用系统 home、系统临时目录或真实客户端发现；必要时先修复路径注入再运行。
- server 中的后台轮换、Hook watcher/安装与旅行任务逐个审核，Web 不启动客户端循环。
- 不读取真实账号、客户端配置或私人会话作为“自动发现测试”。

## 6. 构建检查顺序

导入后先读取真实 package.json 和 workspace，命令以实际脚本为准。以下是基线已有命令，不代表当前空项目能执行：

~~~text
npm ci
npm run build
npm test
cargo fmt --all -- --check
cargo check -p wb-switch-core -p wb-switch-server --locked
cargo test -p wb-switch-core -p wb-switch-server --locked
cargo clippy -p wb-switch-core -p wb-switch-server --locked -- -D warnings
~~~

执行时使用实际已确认的工具路径与项目变量。RustEmbed 依赖 dist/，先构建前端。测试和 clippy 的上游问题应记录基线并做必要修复，不全局压掉 warning。

Windows 桌面按上游经审查的 npm run tauri -- build 参数执行，并核对自有签名/资源配置；不得执行包含 macOS Shell 或上游密钥路径的 build:app 脚本。T11 从同一产品配置构建 embedBootstrapper 在线包、offlineInstaller 离线包与明确 Runtime 要求的便携目录；验证最低版本检测、原生失败路径和实际锁定 Tauri schema，不把文档配置当已可执行脚本。

Linux 仅选择 core/server 包构建，检查 cargo tree 的依赖闭包与运行库，不用整个 workspace 构建推断 server 需要 GTK/WebKit。实际发行命令、二进制名与输出目录在实现后更新部署文档。

## 7. 上游维护

- 工作代码在本项目独立维护，upstream 仅用于拉取和对比，不向参考库直接 push。
- 开发阶段先固定基线，升级时阅读 release/commit 差异，检查登录/会话格式、原生依赖、构建补丁和安全修复。
- 先在本地可回滚分支/提交上合并，重跑受影响能力、权限与恢复用例，再更新固定 SHA、锁文件和来源记录。
- 避免大范围改名、搬目录、格式化未触及文件，以降低未来合并成本。
- 不复用上游自动更新公钥、发布源、签名私钥、应用 ID、浏览器数据目录和正式用户数据目录。

## 8. 忽略与提交

代码开发开始后，按实际生成目录维护 .gitignore，忽略 .tools、.downloads、.cache、.tmp、.runtime、node_modules、build、dist、凭据与 secret。保留 Cargo.lock、package-lock.json、必要来源/补丁和部署模板；不把整个 vendor 目录笼统忽略。

仅文档阶段不创建工具目录、锁文件、Git 仓库或安装脚本。所有新增环境、检查命令、实际写入路径与目录外限制写入实施状态。
