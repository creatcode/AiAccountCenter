//! AiAccountCenter CLI：独立 Web 管理形态的入口。
//!
//! ```bash
//! ai-account-center-server                       # 启动本地服务 + 打开浏览器
//! ai-account-center-server serve                 # 只起服务不开浏览器（--port / --no-open）
//! ai-account-center-server status                # 终端输出当前账号
//! ai-account-center-server version               # 版本号
//! ai-account-center-server backup --passphrase-file <f> [--out <path>]
//! ai-account-center-server restore --file <backup> --passphrase-file <f>
//! ```
//!
//! `restore` 是**停机操作**：替换 `app.db` 需要没有实例在用该数据目录（本进程自身持有
//! 打开的连接时，Windows 会拒绝改名）。命令会先检查实例租约，仍在运行则拒绝。
//!
//! `--no-background-tasks`（或环境变量 `AI_ACCOUNT_CENTER_NO_BACKGROUND_TASKS=1`）
//! 只启动服务、不启动任何后台任务循环，用于隔离运行与排查。
//!
//! `--host` / `--port`：默认 `127.0.0.1:57890`。非回环 host 允许（管理面已有会话鉴权、
//! CSRF 同源校验、登录限速与一次性初始化码）；本服务不提供 HTTPS，远程部署应由受控
//! 反向代理终止 TLS。
//!
//! 首次启动（尚未设置管理员密码）会在**本机控制台**打印一次性初始化码，`/api/auth/init`
//! 必须带上它；码 10 分钟内有效、用后即失效、重启即换新。

mod api;

use std::path::PathBuf;

use serde_json::json;

use wb_switch_core::modules::{
    account, auth_file, backup, checkin, config, db, growth, process, rotate, travel, update,
    variant::WbVariant,
};

fn default_port() -> u16 {
    57890
}

/// 后台任务：自动签到启动即核验，之后按 core 计算的下一轮延迟睡眠（未设置
/// 签到时间段时固定 30 分钟）；自动轮换按配置间隔执行；
/// 限额 hook 信号每秒轮询一次、启动时后台默认接入。
fn spawn_background_loops() {
    tokio::spawn(async move {
        if let Err(error) = config::compact_checkin_logs() {
            eprintln!("[签到] 历史日志整理失败: {error}");
        }
        let _ = checkin::run_checkin_cycle(checkin::CheckinCycleMode::StartupVerify).await;
        loop {
            tokio::time::sleep(checkin::next_cycle_delay()).await;
            let _ = checkin::run_checkin_cycle(checkin::CheckinCycleMode::PeriodicRecovery).await;
        }
    });

    tokio::spawn(async move {
        let mut last_cycle_at: i64 = 0;
        loop {
            let cfg = config::load_auto_rotate_config();
            if cfg.get("enabled").and_then(|v| v.as_bool()) == Some(true) {
                let interval_minutes = cfg
                    .get("check_interval_minutes")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(5)
                    .max(1);
                let now = config::now_ms();
                if now - last_cycle_at >= interval_minutes * 60_000 {
                    last_cycle_at = now;
                    let _ = rotate::run_rotate_cycle().await;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });

    // 派猫猫旅行：启动即派发，之后周期性补派（并重试 no-buddy / 瞬时错误）。
    tokio::spawn(async move {
        let _ = travel::run_travel_cycle().await;
        loop {
            tokio::time::sleep(travel::TRAVEL_RETRY_INTERVAL).await;
            let _ = travel::run_travel_cycle().await;
        }
    });

    // 旅行领取：启动立刻查一轮（避免重启后空等 15 分钟漏领），之后按周期检查。
    tokio::spawn(async move {
        let _ = travel::run_travel_claim_cycle().await;
        loop {
            tokio::time::sleep(travel::TRAVEL_CLAIM_INTERVAL).await;
            let _ = travel::run_travel_claim_cycle().await;
        }
    });

    // 成长任务与福利（国内版）：挂进既有后台循环，不另起调度器。
    // 真正的门禁是「当天是否已成功跑过」（core 内的 ran_today），这里只是检查频率；
    // 自动运行默认关闭，由 auto_growth_config.json 控制。
    tokio::spawn(async move {
        loop {
            let _ = growth::run_growth_cycle().await;
            tokio::time::sleep(growth::GROWTH_CYCLE_INTERVAL).await;
        }
    });

    // 限额 hook 信号：轮询 `~/.ai-account-center/hook-events.jsonl`，入账后由前端下次拉取可见。
    // webui 没有 Tauri 事件通道，因此不需要推送回调（桌面端见 src-tauri/src/lib.rs）。
    wb_switch_core::modules::rate_limit_events::spawn_watcher(|| {});

    // 默认接入：后台线程自动安装 hook（幂等、非阻塞、失败静默）；
    // 装上了就作废扫描缓存——扫描范围从全量收窄到「未注册的来源」。
    std::thread::spawn(|| {
        if wb_switch_core::modules::rate_limit_hook::auto_install_on_startup() {
            wb_switch_core::modules::limits::invalidate_scan_cache();
        }
    });
}

/// CLI 档位参数：`--variant ai` / `--variant=ai`；缺省国内版。
///
/// 与 Tauri 命令的可选 `variant` 参数、HTTP 路由的 query/body 字段同义。
fn variant_arg(args: &[String]) -> WbVariant {
    let raw = args.iter().enumerate().find_map(|(index, arg)| {
        if let Some(value) = arg.strip_prefix("--variant=") {
            return Some(value.to_string());
        }
        arg.eq("--variant")
            .then(|| args.get(index + 1).cloned().unwrap_or_default())
    });
    WbVariant::parse(raw.as_deref())
}

/// 子命令失败：打印可操作原因并返回非零退出码（不 panic、不静默）。
fn exit_on_err(result: Result<(), String>) {
    if let Err(error) = result {
        eprintln!("失败: {error}");
        std::process::exit(1);
    }
}

/// 取 `--key value` 形式的参数值。
fn arg_value(args: &[String], key: &str) -> Option<String> {
    let index = args.iter().position(|a| a == key)?;
    args.get(index + 1)
        .filter(|v| !v.starts_with("--"))
        .cloned()
}

/// 备份口令来源：优先 `--passphrase-file`（推荐，不进 shell 历史与进程列表），
/// 其次 `--passphrase`。
fn passphrase_from_args(args: &[String]) -> Result<String, String> {
    if let Some(path) = arg_value(args, "--passphrase-file") {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("读取口令文件失败（{path}）: {e}"))?;
        let passphrase = text.trim_end_matches(['\r', '\n']).to_string();
        if passphrase.is_empty() {
            return Err("口令文件为空".to_string());
        }
        return Ok(passphrase);
    }
    match arg_value(args, "--passphrase") {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err("缺少口令：请用 --passphrase-file <文件>（推荐）或 --passphrase <口令>".to_string()),
    }
}

fn run_backup(args: &[String]) -> Result<(), String> {
    let passphrase = passphrase_from_args(args)?;
    let out = arg_value(args, "--out").map(PathBuf::from);
    let (path, info) = backup::create_backup(&config::store_dir(), &passphrase, out.as_deref())?;
    println!("备份已生成: {}", path.display());
    println!(
        "格式版本 {}，来源版本 {}，大小 {} 字节",
        info.format_version,
        info.app_version,
        std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
    );
    Ok(())
}

fn run_restore(args: &[String]) -> Result<(), String> {
    let file = arg_value(args, "--file").ok_or("缺少 --file <备份文件>")?;
    let passphrase = passphrase_from_args(args)?;
    let src = PathBuf::from(&file);
    if !src.is_file() {
        return Err(format!("备份文件不存在: {}", src.display()));
    }
    let store_root = config::store_dir();
    // 停机校验：有实例在用这个数据目录时替换 app.db 会失败（且语义上不该做）。
    if let Ok(db) = wb_switch_core::modules::db::Db::open(&db::db_path_in(&store_root)) {
        if let Ok(Some(owner)) = db.active_instance_owner() {
            // 租约 TTL 30 秒：正常 Ctrl+C 退出会立刻释放；被强杀后会留下残留租约。
            return Err(format!(
                "另一个实例（{owner}）仍在使用数据目录，请先停止服务再恢复；\
                 若服务已经停止（例如被强制结束），等待最多 30 秒让租约过期后重试。"
            ));
        }
    }
    let info = backup::restore_backup(&store_root, &passphrase, &src)?;
    println!("已从 {} 恢复账号账本。", src.display());
    println!("格式版本 {}，来源版本 {}", info.format_version, info.app_version);
    println!("恢复前账本副本在 {}", backup::backup_dir_in(&store_root).display());
    Ok(())
}

fn print_status(variant: WbVariant) {
    let auth = auth_file::read_auth_file(variant);
    let current = auth.as_ref().map(|a| {
        let acct = a.get("account").cloned().unwrap_or_else(|| json!({}));
        json!({
            "uid": account::display_value(&acct, "uid"),
            "nickname": account::display_value(&acct, "nickname"),
            "email": account::display_value(&acct, "email"),
        })
    });
    let running = process::is_workbuddy_running(variant);
    println!("AiAccountCenter v{}", update::APP_VERSION);
    println!("WorkBuddy 运行中: {}", if running { "是" } else { "否" });
    match current {
        Some(c) => {
            let name = c
                .get("nickname")
                .and_then(|v| v.as_str())
                .or_else(|| c.get("email").and_then(|v| v.as_str()))
                .unwrap_or("未知");
            println!("当前账号: {name}");
        }
        None => println!("当前账号: 未登录"),
    }
    println!("账号数: {}", account::load_accounts().len());
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("serve");
    match cmd {
        "status" => print_status(variant_arg(&args)),
        "version" | "--version" | "-V" => {
            println!("AiAccountCenter {}", env!("CARGO_PKG_VERSION"));
        }
        "backup" => exit_on_err(run_backup(&args)),
        "restore" => exit_on_err(run_restore(&args)),
        _ => serve(&args).await,
    }
}

async fn serve(args: &[String]) {
    let mut port = default_port();
    if let Some(i) = args.iter().position(|a| a == "--port") {
        if let Some(p) = args.get(i + 1).and_then(|p| p.parse::<u16>().ok()) {
            port = p;
        }
    }

    // 打开版本化数据真源（账号/凭据/管理登录/会话）。失败即退出，不继续启动。
    let db = std::sync::Arc::new(std::sync::Mutex::new(
        match wb_switch_core::modules::db::Db::open_default() {
            Ok(db) => db,
            Err(e) => {
                eprintln!("数据初始化失败: {e}");
                std::process::exit(1);
            }
        },
    ));
    // 单实例互斥：同一数据目录只允许一个业务实例（SQLite 租约，崩溃后 TTL 到期可接管）。
    let lease_owner = format!("srv-{}-{}", std::process::id(), config::now_ms());
    {
        let mut guard = db.lock().unwrap();
        match guard.acquire_instance_lease(&lease_owner, 30_000) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("另一实例正在使用同一数据目录，已拒绝启动。");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("实例租约检查失败: {e}");
                std::process::exit(1);
            }
        }
    }
    // 心跳续期（与平台任务开关无关：实例保护常开）。
    {
        let db = db.clone();
        let owner = lease_owner.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let held = {
                    let db = db.lock().unwrap();
                    db.renew_instance_lease(&owner, 30_000).unwrap_or(false)
                };
                if !held {
                    eprintln!("实例租约已被接管，停止服务。");
                    std::process::exit(1);
                }
            }
        });
    }
    // 首次初始化引导码：只在未设置管理员密码时签发，并且只出现在本机控制台。
    // 没有它，远程访问者可以在开放端口后抢设管理员密码（SECURITY_AND_DATA.md §5）。
    let init = {
        let initialized = db
            .lock()
            .unwrap()
            .admin_password_hash()
            .ok()
            .flatten()
            .is_some();
        let init = std::sync::Arc::new(api::InitBootstrap::new());
        if !initialized {
            let code = init.issue();
            println!();
            println!("尚未设置管理员密码。请用下面的一次性初始化码完成首次设置：");
            println!("    初始化码: {code}");
            println!(
                "    有效期 {} 分钟，用后即失效；重启服务会生成新码。",
                api::INIT_CODE_TTL_MS / 60_000
            );
            println!("该码只在本机控制台输出，不会通过网络返回。");
            println!();
        }
        init
    };
    let shutdown_db = db.clone();
    let app = api::router(api::AppState { db, init });
    // 监听地址：默认回环。开放非回环需要管理面保护到位——会话鉴权、CSRF 同源校验、
    // 登录限速与一次性初始化码都已实现，因此允许远程监听（docs/SECURITY_AND_DATA.md 第 5 节）。
    let mut host = String::from("127.0.0.1");
    if let Some(i) = args.iter().position(|a| a == "--host") {
        if let Some(h) = args.get(i + 1).filter(|h| !h.starts_with("--")) {
            host = h.clone();
        }
    }
    let loopback = matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost");
    if !loopback {
        eprintln!("注意：正在监听非回环地址 {host}，管理面将可从网络访问。");
        eprintln!("建议只通过受控反向代理暴露，并在代理侧终止 TLS（本服务自身不提供 HTTPS）。");
    }
    let addr = format!("{host}:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("启动失败: 端口 {port} 被占用或不可用（{e}）。可用 --port 指定其他端口。");
            std::process::exit(1);
        }
    };

    println!("AiAccountCenter v{}", update::APP_VERSION);
    println!("webui: http://{addr}");
    println!("按 Ctrl+C 停止服务。");

    let no_open = args.iter().any(|a| a == "--no-open");
    if !no_open {
        open_browser(&addr);
    }

    // 隔离运行开关：只起服务，不起签到/轮换/旅行/hook/更新等主动副作用。
    let tasks_off = args.iter().any(|a| a == "--no-background-tasks")
        || config::background_tasks_disabled();
    if tasks_off {
        println!("后台任务循环已禁用：只提供账号管理接口，不执行自动任务与客户端写入。");
    } else {
        spawn_background_loops();
    }

    // Ctrl+C 优雅退出并释放实例租约：不释放的话崩溃/停止后租约要等 TTL 才过期，
    // 期间 `restore` 会误判「仍有个实例在用这个数据目录」。
    let shutdown_owner = lease_owner.clone();
    let shutdown = async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_db
            .lock()
            .unwrap()
            .release_instance_lease(&shutdown_owner);
        println!("已停止服务并释放实例租约。");
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .unwrap();
}

fn open_browser(addr: &str) {
    let url = format!("http://{addr}");
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let mut c = std::process::Command::new("cmd");
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：开浏览器不闪 cmd 窗
        }
        let _ = c.args(["/C", "start", &url]).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::variant_arg;
    use wb_switch_core::modules::variant::WbVariant;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// 不传档位时必须仍是国内版（改造前行为）。
    #[test]
    fn cli_variant_defaults_to_cn() {
        assert_eq!(variant_arg(&args(&["status"])), WbVariant::Cn);
        assert_eq!(variant_arg(&args(&["status", "--debug"])), WbVariant::Cn);
        assert_eq!(variant_arg(&args(&["status", "--variant"])), WbVariant::Cn);
        assert_eq!(variant_arg(&args(&["status", "cn"])), WbVariant::Cn);
        assert_eq!(
            variant_arg(&args(&["status", "--variant=cn"])),
            WbVariant::Cn
        );
    }

    #[test]
    fn cli_variant_reads_space_and_equals_forms() {
        assert_eq!(
            variant_arg(&args(&["status", "--variant", "ai"])),
            WbVariant::Ai
        );
        assert_eq!(
            variant_arg(&args(&["status", "--variant=ai"])),
            WbVariant::Ai
        );
        assert_eq!(
            variant_arg(&args(&["status", "--variant", "AI", "--no-open"])),
            WbVariant::Ai
        );
        assert_eq!(
            variant_arg(&args(&["--variant=ai", "status"])),
            WbVariant::Ai
        );
    }
}
