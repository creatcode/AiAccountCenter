# 开发环境、源码导入与上游维护

日期：2026-10-14。用户已明确优先补齐缺失开发环境，并指定本轮新增工具、安装包和工具缓存集中到 E:\rustenv；现有兼容环境直接复用。开发环境不随源码或发行包打包。产品源码导入与功能开发仍待明确启动；下文产品构建步骤在对应授权后执行。

## 1. 实际环境与兼容边界

当前已验证 Windows 10 x64 Build 19045、PowerShell 7.6.5、Git 2.39.1.windows.1、Node v22.19.0、npm 10.9.3。Git/Node/npm 复用原安装，没有升级或重装；SDK 10.0.26100.0 复用 E:\Windows Kits\10。WebView2 Runtime 154.0.4258.62 已登记安装，本轮没有重新安装，实际创建 WebView/Tauri 基线仍待产品阶段验证。

本轮补齐 Rust 1.98.1（x86_64-pc-windows-msvc）、Cargo 1.98.1、rustfmt 1.9.0-stable、clippy 0.1.98、rustup 1.29.1，以及 Visual Studio 2022 Build Tools 17.14.41 / MSVC 14.44.35207。Rust 为已核对的固定稳定版本，不自动跟随 stable；这不是已确定的产品 MSRV。最小 C 程序已使用 Windows API 编译/链接/运行，Rust 最小程序也已链接并运行，输出分别为 msvc-sdk-ok 与 rust-link-ok。

原环境缺少可用 Rust/MSVC，本轮先安装到项目 .tools；用户随后指定 E:\rustenv，Rust/Cargo 已迁入，项目内 MSVC 实例已通过官方卸载器卸载后重新安装到 E:\rustenv\msvc，两个最终安装器退出码均为 0。不得直接搬动或复制已登记的 MSVC 安装目录。

主基线锁文件：Vite 7.3.6 的 Node engines 为 ^20.19.0 || >=22.12.0；Vitest 5.0.2 为 ^22.12.0 || ^24.0.0 || >=26.0.0。本机 Node 满足已核对元数据，不等于 npm ci、产品测试与桌面打包已经通过。开发只补缺项；已有兼容版本不因搬迁便利重复下载或安装。

Python 3.13.16 仅复用为文本/JSON 分析工具，不是产品必装依赖。所有本机路径和版本都是环境证据；其他机器按实际兼容条件准备，不能把本机 SDK/WebView2 版本作为发行最低要求。Linux/Docker 未验证，本轮不安装 Docker、WSL 或 Linux 系统组件。

### 1.1 项目根与外部开发环境根

项目实际根已核对为新 Rust AiAccountCenter，mycount 仅为历史暂存名。项目根从脚本自身位置计算，不依赖当前工作目录；Agent 不执行项目目录删除、合并或改名。

开发环境根与项目根独立。本机默认 E:\rustenv，其他位置通过 AI_ACCOUNT_CENTER_ENV_ROOT 或 -EnvironmentRoot 指定；该默认值是本机约定，不是其他机器必须有 E 盘的要求。只设置当前进程/子进程变量，不改系统/用户 PATH、Shell 配置、执行策略、HOME/USERPROFILE 或全局 npm/Cargo 配置。

## 2. 最小环境清单与官方来源

| 工具/条件 | 用途 | 实际处理与状态 |
| --- | --- | --- |
| Git | 基线导入与本地回滚 | 复用 E:\Git\cmd\git.exe，2.39.1.windows.1；已有项目 Git 不重新初始化 |
| Node/npm | 前端依赖与构建 | 复用 E:\Nodejs，22.19.0 / 10.9.3；没有安装 Node 包或产品依赖 |
| Rust/rustup/Cargo | 核心、服务与桌面编译 | 新增到环境根 rustup/、cargo/；版本与最小编译检查通过 |
| rustfmt/clippy | 格式和静态检查 | 安装对应工具链组件，版本调用通过；尚未检查产品代码 |
| MSVC | Windows C/C++ 编译和 Rust 链接 | 新增到环境根 msvc/；只选择 Microsoft.VisualStudio.Component.VC.Tools.x86.x64，不含完整 IDE |
| Windows SDK | Windows 头文件、库与资源工具 | 复用 10.0.26100.0，实际 C/Rust 链接通过 |
| WebView2 Runtime | Windows 桌面运行 | 复用已有登记；创建环境和 Tauri 产品运行待验收 |
| Linux / Docker | 后续服务部署验证 | 未验证；Docker 可选，不作为当前开发前提 |

Rust 使用[官方 rustup 安装方式](https://rust-lang.org/tools/install/)及[自定义工具目录](https://rust-lang.github.io/rustup/installation/index.html)。固定安装器为 https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-pc-windows-msvc/rustup-init.exe ，SHA-256 为 6f4bef66261261fcb43131be8720bab817d403a09edec7455c371974b90bdb7e，已与同源 .sha256 校验。

MSVC 使用[微软官方 Build Tools 安装器](https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio?view=vs-2022)。本轮固定下载地址为 https://download.visualstudio.microsoft.com/download/pr/bc92e2cb-33de-4a0c-995d-efa817f16b16/985969f472caad75d993a5cb4c35a6a4271460cc12b343e2433b994d173aa990/vs_BuildTools.exe ，SHA-256 为 985969f472caad75d993a5cb4c35a6a4271460cc12b343e2433b994d173aa990，Authenticode 为 Valid / Microsoft Corporation。

MSVC 主体虽在环境根，官方安装器仍写 C:\Program Files (x86)\Microsoft Visual Studio\Installer、C:\ProgramData\Microsoft\VisualStudio、用户 AppData 下安装日志/状态及系统登记。沿用原有 CachePath=E:\Microsoft Visual Studio\Packages、SharedInstallationPath=E:\Microsoft Visual Studio\Shared、KeepDownloadedPayloads=0；未修改这些既有全局配置。不能声称环境完全位于 E:\rustenv 或完全便携。初次项目缓存路径超过[微软安装器 50 字符限制](https://learn.microsoft.com/en-us/visualstudio/install/configure-policies-for-enterprise-deployments?view=vs-2022)，实际安装因此保留既有策略；静默卸载须以管理员身份启动。

误额外下载的 Node 22.23.3 便携包仅保留在环境根 downloads/，没有解压、安装、使用或打包；后续不再为已有兼容环境重复准备工具。

## 3. 目录、入口与其他机器复现

| 位置 | 内容 |
| --- | --- |
| 环境根 rustup/ | 新增 Rust 工具链与组件 |
| 环境根 cargo/ | Cargo bin 与 registry/git 缓存；rustup 自动自更新已关闭 |
| 环境根 msvc/ | 官方登记的 C++ Build Tools 安装主体 |
| 环境根 downloads/ | 官方安装器、校验文件及本轮下载记录，不随产品打包 |
| 环境根 cache/npm/ | npm 缓存，实际 npm config get cache 已核对 |
| 环境根 tmp/、checks/AiAccountCenter/ | 安装临时文件、最小编译验证源码与产物 |
| 项目 node_modules/、dist/ | 后续前端依赖与静态构建结果 |
| 项目 build/cargo/、build/release/ | 后续编译缓存与仅含产品的最终发行包 |
| 项目 .cache/、.tmp/、.runtime/、.downloads/ | 后续产品测试、隔离数据与参考资料，按需使用 |

在 PowerShell 7 的项目根执行：

~~~powershell
. .\scripts\dev-env.ps1
.\scripts\check-env.ps1
~~~

指定另一处已准备的环境根：

~~~powershell
. .\scripts\dev-env.ps1 -EnvironmentRoot 'D:\devtools\rustenv'
~~~

dev-env.ps1 只加载当前进程环境，固定使用本轮验证的 Rust 1.98.1，并加载 x64 MSVC/已有 SDK；不下载或安装任何组件。check-env.ps1 检查实际命令路径、版本、C/Windows API 编译运行、Rust 链接运行和 npm 缓存路径，验证文件只写环境根 checks/。源码根由脚本定位，环境不随项目目录改名移动；移动已登记的 MSVC 必须按官方安装流程处理。

其他机器先核对 Git、Node/npm、Rust、MSVC、SDK、Runtime，兼容项直接复用。只在 Rust 缺失且已获安装授权时，设置选定环境根的 RUSTUP_HOME=rustup/、CARGO_HOME=cargo/、TEMP/TMP=tmp/，下载并校验上述官方安装器后执行：

~~~powershell
& "$environmentRoot\downloads\rustup-init-1.29.1.exe" -y --no-modify-path --default-host x86_64-pc-windows-msvc --default-toolchain 1.98.1 --profile minimal --component rustfmt,clippy
& "$environmentRoot\cargo\bin\rustup.exe" set auto-self-update disable
~~~

示例变量 $environmentRoot 必须指向事先确认并按需创建的目录，不能把本机 E:\rustenv 写成其他机器的硬要求。已有 Rust 时不重复安装；按实际 homes 与已安装稳定版本设置当前进程变量、重新验证，不盲目运行上述缺项安装步骤。当前脚本是本轮固定布局的环境入口，不是通用自动安装器。

只有 MSVC 缺失时才使用上述已校验安装器，以 --installPath 指定环境根 msvc/，仅 --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64、--addProductLang en-US、--quiet --wait --norestart；已有 SDK 优先复用，不加 --includeRecommended/Optional。系统组件的实际共享路径和权限需按目标机器核对；缺 SDK/Runtime 时按实际缺口和对应授权另行处理。Linux 另用适配目标的工具链和环境变量，Windows 编译结果不能替代 Linux 验证。

前端开发后执行项目 npm ci，不全局安装、不让 npx 自动下载工具。使用 npm.cmd 避免 PowerShell 策略歧义。测试真实客户端前继续遵守第 5 节隔离约束，不能用修改 HOME/USERPROFILE 的方式改变真实客户端路径。

## 4. 导入上游的可执行顺序

当前目录已有本套文档，不能直接 git clone 到“.”或用强制 checkout 覆盖。

开发授权后：

1. 重新读取目标文档、检查 Git 状态和所有重名文件，记录本套文档摘要。若已有 Git，沿用历史，不重新初始化。
2. 对尚无 Git 的目录建立独立本地仓库，先提交当前文档，形成可恢复基线；本地基线纳入用户授权提交的测试备份和重要资料，不擅自加入其他账号数据，不 push。
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
- 普通调试默认使用合成数据；用户提供的 testdata/ 可用于隔离导入/解析验证，不当成合成数据，也不据此执行真实账号任务。自动任务、自动 Hook 安装和自动更新关闭。
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

当前为私有仓库的开发测试阶段，.gitignore 仅忽略项目根的可重新生成工具、依赖、缓存、临时数据和构建产物：.tools、.cache、.tmp、.runtime、node_modules、build、dist。源码、项目文档、配置、Cargo.lock、package-lock.json、必要来源/补丁、部署模板、重要参考资料和用户提供的 testdata/ 纳入版本管理；不按凭据字段、文件类型或目录名笼统忽略重要文件，不把整个 vendor 或 .downloads 目录排除。工具安装包位于外部环境根 downloads/；需保留的测试证据放在项目文档中提交，不混入临时缓存目录。

本轮按用户明确要求取消 testdata/ 和 .downloads/ 的忽略，测试备份作为原始资料提交到本地私有仓库，不改写其内容。Git 提交范围与正式发行包范围分别管理，当前不执行发布。

当前允许创建环境准备所需的工具目录和脚本，按需维护 .gitignore；不创建产品源码或产品依赖锁文件，不重新初始化已有 Git。所有新增环境、检查命令、实际写入路径与目录外限制写入实施状态。
