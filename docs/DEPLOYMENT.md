# 部署、发行与运行设计

日期：2026-10-13。当前只有文档，没有可执行程序、Dockerfile、Compose 或 systemd 文件。以下参数与布局是设计目标，T02/T10/T11 实现后必须以实际 --help 和产物更新本文，不能直接当现成命令使用。

## 1. 交付形态

| 产物 | 使用者需要 | 不应要求 |
| --- | --- | --- |
| Windows Tauri 桌面安装包/便携目录 | 支持的 Windows x64、实际 WebView2 运行条件 | 安装 Python、Node、Rust 或使用开发服务器 |
| Windows 独立 Web 服务 | 启动服务、设置数据/密钥、浏览器访问 | 安装桌面应用才能管理账号 |
| Linux 独立服务包 | 支持的 x86_64 系统及已声明运行库 | 图形桌面、目标 AI 客户端、桌面密钥环 |
| Linux Docker/Compose（可选） | 已有获授权 Docker 服务、明确卷与 secret | 为使用本项目强制安装 Docker/WSL |

浏览器访问无需安装 EXE。自动任务依赖后端常驻，浏览器关闭不影响任务；后端停止后任务停止。服务不得依赖浏览器的定时器或轮询。

## 2. 拟定服务入口契约

桌面可执行文件拟为 AiAccountCenter.exe；在既有 wb-switch-server 上适配对外服务名 ai-account-center-server，保留内部 crate 名减少上游差异。开发根目录已核对为新 Rust AiAccountCenter，mycount 为历史暂存名；部署参数和产物名称不绑定源码文件夹名。

| 参数/配置 | 目标语义 |
| --- | --- |
| serve | 仅启动 Web 服务和已启用的后台平台任务 |
| --host | 默认 127.0.0.1；显式绑定网络必须已完成管理面保护 |
| --port | 默认可延用上游 57890；冲突清晰报错，不悄悄换端口 |
| --data-dir | 显式 AiAccountCenter 数据根，解析为绝对路径并校验可写 |
| --secret-file | Linux/服务模式主密钥文件；不放 argv 明文密钥 |
| --no-open | 不打开本机浏览器；Linux 无桌面部署必须有效 |
| --public-url | 远程授权/反向代理实际需要时使用，校验允许 origin/回调 |
| --no-background-tasks | 调试与维护禁用真实账号任务，不改变模式权限 |
| --version / --help | 输出真实版本、支持参数与非敏感使用说明 |

只实现确有用途的参数，不先建立多层配置框架。首次引导、凭据和口令不可通过普通命令行参数传入。

上游目前已经存在 serve、--port、--no-open；其余是新增目标。旧前端固定 loopback URL 必须改成同源 /api，用户远程访问时才能连接正确服务。

## 3. 数据与安装布局

开发数据一律 .runtime/，正式安装不写代码仓库或程序目录。

- Windows 桌面默认：由新 Rust App ID 解析独立用户数据目录，命名空间例如 AiAccountCenter-Rust；WebView2 profile 使用该命名空间下的专用目录。产品显示名同为 AiAccountCenter 不代表复用旧 Python 产品的 AppData、数据库或密钥。
- Windows Web：建议显式 --data-dir，运行账号与 DPAPI 上下文固定；更换服务账号前处理密钥恢复。
- Linux：数据目录由服务配置明确指定，例如 /var/lib/ai-account-center；这是部署示例，不是所有主机的强制路径。
- Linux secret：位于独立只读配置位置，例如 /etc/ai-account-center/master.key；由服务用户读取，其他用户不可读，不写入 Git、镜像或日志。
- 备份输出位置显式配置，不与服务运行文件混淆；备份保留和清理操作遵循对应授权。

同一数据目录只能有一个业务实例。桌面与服务可使用不同库独立运行，但不会自动同步；首版只提供加密导入导出。

## 4. Windows 桌面发行与 WebView2 兼容

### 4.1 系统目标与产物

首版目标 Windows 10/11 x64。冻结发行前记录所支持的最低系统 build、实际锁定的 Tauri/Wry/WebView2 Loader、所用 Web API 和原生 API、最低 WebView2 Runtime、VC 运行库要求及验证环境。不得从 SDK 版本推断目标系统必须安装同版本 SDK，也不把开发机已装 WebView2 版本当最低要求。Windows 7/8、x86、ARM64 尚不纳入发行承诺。

Rust/Tauri 桌面运行需要 WebView2；用户不需要安装 Rust、Node、Python、MSVC 编译工具或开发 SDK。纯 Windows/Linux Web 服务不加载桌面运行时。

| 产物 | WebView2 策略 | 网络与使用条件 |
| --- | --- | --- |
| 在线安装包（首版必交付） | 使用 embedBootstrapper 随包携带官方 Evergreen 引导程序；先检测，满足版本则复用，缺失/过低时处理安装或升级 | 引导程序仍需联网获取 Runtime；网络失败时给离线包获取说明 |
| 离线安装包（首版必交付） | 使用 offlineInstaller 随包携带官方完整 x64 Evergreen 安装器；先检测并避免降级已有较新 Runtime | 在没有 WebView2 且断网的目标机器实际验证；离线安装成功不代表联网业务可离线执行 |
| 便携目录（首版必交付） | 默认复用机器/当前用户可用的正式 Evergreen Runtime；不暗中安装 | 明确运行时要求，缺失/过低时原生提示；不标为单 EXE 零依赖 |
| 随包固定版 Runtime（可选） | 只在明确需要无系统安装的受限部署时评估，运行时放应用目录并显式选择 | 包体增加；应用负责安全更新和发行维护，首版不默认增加此产物 |

在线与离线包共用同一产品、源码与最低版本策略，仅运行时携带方式不同。按实际锁定的 Tauri 配置 schema 验证 webviewInstallMode 和 bundle.windows.minimumWebview2Version；具体最低版本在 API 核对、干净系统测试后填写，不能留占位值发布。安装器的“尝试更新”不能代替应用启动复检。

### 4.2 启动前检测与失败处理

检测应在 Rust 入口创建任何 WebView、主窗口、悬浮窗以及启动真实后台任务之前完成，不依赖 React 页面正常加载才能显示错误。

1. 检查系统/架构是否在已声明范围内，使用微软支持的检测方式核对机器和用户范围的正式 Evergreen Runtime；不能只看 Edge 浏览器、某个安装目录或一个注册表项。
2. WebView2 Loader 的 GetAvailableCoreWebView2BrowserVersionString 可用于核对可用性，但可能返回 Edge Beta/Dev/Canary；结合正式 Runtime 的检测结果识别来源，不默认用预览通道替代正式运行时。版本使用受支持的版本比较方式，不能做字符串字典序比较。
3. 检查 Runtime 满足已确定的最低版本、应用专用 profile 可写；开发 profile 在 .runtime/，发行 profile 在独立用户数据目录。使用 Loader API 不等于实际环境创建一定成功，后续 WebView 创建错误也必须捕获。
4. 可用时继续创建窗口；缺失、版本过低、运行时损坏、profile 权限不足或环境创建失败时显示不依赖 WebView 的中文原生对话框，并记录脱敏错误代码和检测到的版本/来源。
5. 根据问题提供官方获取页面、离线安装包说明、明确的安装/修复操作、重新检测或退出。安装失败不能宣称桌面已就绪；退出和重试均保留用户数据，不启动任务、Hook 或客户端写入。
6. 修复结束后重新检测版本并重新创建环境，必要时提示重启应用。不要无限自动重试、反复提权、直接删除 profile 或默认修改系统注册表/GPU 设置。

### 4.3 安装权限、取消与更新

- 应用安装器先告知 WebView2 依赖；用户同意正常安装流程后可以调用官方安装器自动处理，无需用户另行手工搜索组件。主程序平时启动不未经选择下载/安装或申请提权。
- 优先按实际用户安装场景处理；Evergreen 官方安装器支持机器或用户范围，但已有 Edge Updater、组策略和目标权限会影响结果。不能承诺所有受限账户均可免管理员安装。
- 网络、代理、证书、磁盘空间、用户取消、权限不足和安装器错误分别显示。记录安装器退出码，处理实际需要的重启状态；结束后再查 Runtime，不能只用退出码判断成功。
- 安装组件只使用微软官方来源并核对数字签名；随包完整安装器记录版本、架构、摘要与来源。在线下载保持证书校验，不回退不可信镜像或忽略校验。
- Evergreen 由其更新机制维护；应用兼容当前稳定 Runtime，并对即将到来的版本进行必要回归。使用固定版时由本项目发行跟进更新，不把固定版描述为自动更新。
- 卸载 AiAccountCenter 默认保留账号数据，不卸载供其他应用共享的 Evergreen Runtime，不降级或替换其他应用的运行时。
- 这里描述的是最终产品安装流程。开发机环境准备已获用户授权，优先复用已有 SDK/Runtime，只补必要组件并记录目录外影响；发行设计本身不扩大开发机安装授权。

### 4.4 其他发行要求

锁定依赖在 Windows 实际构建，记录源码 SHA、工具链、打包参数与产物摘要；新产品 App ID、更新源、数据目录独立于 workbuddy-switch 及旧 Python 产品。检查 Companion/Hook 实际链路，必要的 Rust Hook 入口随包交付，不隐藏 Python 运行要求。

实际验证启动、登录、主窗口、托盘、关闭/明确退出、后台任务、悬浮窗、多窗口焦点及通知。安装/升级前备份，卸载默认保留数据；Windows 代码签名与 Tauri 更新签名分开记录，未签名测试包不得宣称已签名发行。

Windows/Linux 独立服务不因 WebView2 缺失而失败；桌面报错也不自动切换服务、开放端口或放宽 Web 权限。用户可主动按独立 Web 部署说明运行浏览器管理。

官方依据：[Tauri Windows 安装包选项](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options)、[Tauri Windows 配置](https://v2.tauri.app/reference/config/#windowsconfig)、[微软 Runtime 分发与检测](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)、[Evergreen 与固定版](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/evergreen-vs-fixed-version)。核对日期：2026-10-13；字段与行为以实际锁定版本验证。

## 5. Windows 独立 Web

独立 Rust 服务可直接在 Windows 启动，不依赖 Tauri 窗口。默认回环地址，在保护齐备且获网络访问授权后可显式绑定局域网地址。

不默认注册 Windows 服务、开机任务或修改防火墙。需要后台常驻时提供经核对的运行说明；注册/启用必须有目标机器授权。关闭管理浏览器不会停止后台服务。

Windows Web 后端拒绝真实客户端、用户会话、Hook、CLI 轮换及原生通知接口，与 Linux Web 保持同样边界。

## 6. Linux 无桌面直接运行

- 在真实 Linux x86_64 环境构建/运行，明确最低发行版/glibc、系统库与目标 triple；不凭 Windows 构建推断可用。
- 生产包包含 server 与前端静态资源；最终运行无需 Node/npm/Vite。
- 不构建/运行 Tauri 窗口；没有 DISPLAY、Wayland、D-Bus session bus 和目标客户端时仍可启动并执行平台业务。
- 使用非 root 专用用户、受限目录权限、只读 secret、优雅退出和日志轮转策略。
- TLS 根证书、DNS、时区依赖按实际发布包处理，不因 Rust 就承诺零运行库。
- 给出 systemd 模板时设置明确 WorkingDirectory/ExecStart/数据路径/权限、停止宽限与 restart 行为；模板创建不等于获准注册/启用。
- 服务重启恢复任务先核查副作用，不因 systemd 自动重启重复领奖。
- musl 静态包、ARM64 与 Linux 桌面不是首版默认交付。

## 7. Docker/Compose 可选部署

开发实现时创建实际 Dockerfile 和 Compose 模板，目标如下：

1. 多阶段构建，锁定兼容稳定的基础镜像与依赖，不使用不受控的 latest。
2. 构建前端 dist/，仅编译 core/server 的依赖闭包；不为 server 安装 GTK/WebKit/Tauri 图形依赖。
3. 运行阶段非 root，仅包含所需二进制、静态资源/嵌入资源、证书与运行库。
4. 显式数据卷与只读 secret；密钥和账号不能复制进镜像层。
5. 容器内必须显式监听可转发地址；宿主端口默认只发布到回环地址，网络开放另行配置并保护。
6. 健康检查区分存活/就绪；设置停止宽限，让运行检查点持久化。
7. 说明 UID/GID、卷权限、备份、恢复与升级；不提供能掩盖权限问题的 root 默认运行。
8. 镜像构建成功与容器真实运行分别验收；缺 Docker 环境只保留待验收，不擅自安装。

Docker daemon 管理的镜像和缓存不完全位于源码目录，执行前说明实际影响与目标环境授权，不能声称仅设项目缓存就已隔离全部 Docker 写入。

## 8. 反向代理与远程授权

推荐通过已授权 HTTPS 反向代理暴露管理面，服务默认 loopback。不由程序自动申请域名、改防火墙或安装代理。

必须验证：同源 /api、Cookie Secure/SameSite、CSRF/Origin、可信代理、请求大小、超时、错误页与必要长连接。未明确需要不增加代理配置项。

平台设备流/二维码优先。需要回调时核对平台允许的 redirect URI 和 public URL；访问者的 127.0.0.1 与服务器不是同一台机器。不可用时提示并提供受保护的凭据/加密备份导入。

## 9. 备份、恢复与升级

- 一致数据库快照、加密凭据、必要客户端/会话元数据及密钥恢复方案一起考虑。
- 可移植加密导出不直接依赖原机器 DPAPI；导入错误密码/密文损坏不得影响现库。
- 实际在新数据目录恢复一次，验证账号身份、额度快照、计划、任务历史与可解密凭据。
- 更新来源和可信公钥由 AiAccountCenter 自有配置提供。未提供时禁用自动更新，不能回落上游。
- 下载安装前校验完整性/签名、备份程序和数据；迁移失败保留旧库，回滚程序时处理 schema 兼容。
- Linux server 更新通过明确的替换/重启流程，不能照搬桌面 Tauri updater 或承诺系统包管理集成。

## 10. 运维观察与故障分类

需要观察：服务就绪、单实例占位、计划启用状态、下一次执行、账号凭据状态、任务待核查、通知失败、磁盘/数据库错误。日志脱敏，不保留完整提示词。

权限/密钥错误、平台鉴权失败、活动未开放、网络超时、结果未知、客户端占用分别呈现。只有可恢复的明确错误才重试，不能以持续重启掩盖失败。

所有部署命令和示例最终用实际产物验证。当前没有部署完成项。
