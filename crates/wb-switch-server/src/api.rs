//! HTTP API 层：把 wb-switch-core 暴露为 REST 接口，供同源 webui（浏览器）调用。
//!
//! 路由设计对应桌面端 commands.rs。默认只绑定回环地址；非回环绑定须等管理面
//! 认证与权限保护完成（见 `main.rs --host` 与 docs/SECURITY_AND_DATA.md）。
//!
//! **运行形态边界**：本服务是 Web 管理形态，本机客户端发现/切换、用户会话、
//! CLI/Hook 与本地统计等桌面专属接口在此一律 403 拒绝（[`reject_desktop_only`]）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Query, RawQuery, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde_json::{json, Value};

use wb_switch_core::modules::db::Db;
use wb_switch_core::modules::{
    account, auth, auth_file, backup, checkin, codebuddy_cli, codebuddy_cn_ide, codebuddy_ide,
    codebuddy_ide_session, codebuddy_ide_session_sync, config, credit_usage, credits,
    export_import, growth, jetbrains, limits, notifications, oauth, process, rate_limit_events,
    rate_limit_hook, refresh, rotate, session, session_groups, switch, token_stats, travel, update,
    variant::WbVariant, vscode_ext, vscode_session, vscode_session_sync,
};

/// WorkBuddy 运行状态缓存：Windows 上检测要跑 tasklist（慢），缓存几秒避免
/// 前端切 tab 频繁触发命令行导致卡顿/闪窗。按档位分别缓存。
#[cfg(target_os = "windows")]
static RUNNING_CACHE: Mutex<Option<(Instant, bool, WbVariant)>> = Mutex::new(None);

fn cached_workbuddy_running(variant: WbVariant) -> bool {
    #[cfg(target_os = "windows")]
    {
        let mut cache = RUNNING_CACHE.lock().unwrap();
        if let Some((t, v, cached_variant)) = cache.as_ref() {
            if *cached_variant == variant && t.elapsed() < Duration::from_secs(3) {
                return *v;
            }
        }
        let v = process::is_workbuddy_running(variant);
        *cache = Some((Instant::now(), v, variant));
        v
    }
    #[cfg(not(target_os = "windows"))]
    {
        process::is_workbuddy_running(variant)
    }
}

#[derive(RustEmbed)]
#[folder = "../../dist/"]
struct Assets;

/// 切换进度缓存：webui 通过 GET /api/switch/progress 轮询。
static SWITCH_PROGRESS: Mutex<Option<String>> = Mutex::new(None);
static SWITCH_RUNNING: Mutex<bool> = Mutex::new(false);

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/mode", get(api_mode))
        .route("/api/auth/status", get(api_auth_status))
        .route("/api/auth/init", post(api_auth_init))
        .route("/api/auth/login", post(api_auth_login))
        .route("/api/auth/logout", post(api_auth_logout))
        .route("/api/status", get(api_status))
        .route("/api/accounts", get(api_accounts))
        .route("/api/codebuddy-cli/status", get(api_codebuddy_cli_status))
        .route(
            "/api/codebuddy-cli/install-helper",
            post(api_codebuddy_cli_install_helper),
        )
        .route("/api/codebuddy-cli/switch", post(api_codebuddy_cli_switch))
        .route(
            "/api/codebuddy-cn-ide/status",
            get(api_codebuddy_cn_ide_status),
        )
        .route(
            "/api/codebuddy-cn-ide/switch",
            post(api_codebuddy_cn_ide_switch),
        )
        .route(
            "/api/codebuddy-cn-ide/detect",
            post(api_codebuddy_cn_ide_detect),
        )
        .route(
            "/api/codebuddy-cn-ide/sessions",
            get(api_codebuddy_cn_ide_sessions),
        )
        .route(
            "/api/codebuddy-cn-ide/session-links",
            post(api_codebuddy_cn_ide_session_links_preview),
        )
        .route("/api/codebuddy-ide/status", get(api_codebuddy_ide_status))
        .route("/api/codebuddy-ide/switch", post(api_codebuddy_ide_switch))
        .route("/api/codebuddy-ide/detect", post(api_codebuddy_ide_detect))
        .route(
            "/api/codebuddy-ide/sessions",
            get(api_codebuddy_intl_ide_sessions),
        )
        .route(
            "/api/codebuddy-ide/session-links",
            post(api_codebuddy_intl_ide_session_links_preview),
        )
        .route("/api/jetbrains/status", get(api_jetbrains_status))
        .route("/api/jetbrains/switch", post(api_jetbrains_switch))
        .route("/api/jetbrains/detect", post(api_jetbrains_detect))
        .route("/api/vscode-ext/status", get(api_vscode_ext_status))
        .route("/api/vscode-ext/sessions", get(api_vscode_ext_sessions))
        .route("/api/vscode-ext/switch", post(api_vscode_ext_switch))
        .route("/api/vscode-ext/detect", post(api_vscode_ext_detect))
        .route(
            "/api/vscode-ext/restart-precheck",
            post(api_vscode_restart_precheck),
        )
        .route(
            "/api/vscode-ext/session-links",
            post(api_vscode_ext_session_links_preview),
        )
        .route("/api/delete", post(api_delete))
        .route(
            "/api/update-account-display",
            post(api_update_account_display),
        )
        .route("/api/oauth/start", post(api_oauth_start))
        .route("/api/oauth/status", post(api_oauth_status))
        .route("/api/import-local", post(api_import_local))
        .route("/api/export-accounts", post(api_export_accounts))
        .route(
            "/api/export-accounts-to-path",
            post(api_export_accounts_to_path),
        )
        .route("/api/import/preview", post(api_preview_import))
        .route("/api/import", post(api_import))
        .route("/api/switch", post(api_switch))
        .route("/api/switch/progress", get(api_switch_progress))
        .route("/api/sessions", get(api_sessions))
        .route("/api/sessions/account", get(api_account_sessions))
        .route("/api/sessions/copy", post(api_copy_sessions))
        .route("/api/sessions/copy-cross", post(api_copy_sessions_cross))
        .route(
            "/api/session-links/preview",
            post(api_session_links_preview),
        )
        .route(
            "/api/session-links/preview-cross",
            post(api_session_links_preview_cross),
        )
        .route("/api/session-sync/cross", post(api_session_sync_cross))
        .route("/api/session-groups/list", post(api_list_session_groups))
        .route("/api/session-groups/detail", post(api_get_session_group))
        .route(
            "/api/session-groups/preview",
            post(api_preview_session_group_pair),
        )
        .route(
            "/api/session-groups/sync",
            post(api_sync_session_group_pair),
        )
        .route(
            "/api/session-groups/unify",
            post(api_sync_session_group_unify),
        )
        .route(
            "/api/session-groups/sync-safe",
            post(api_sync_session_group_safe_batch),
        )
        .route(
            "/api/session-groups/add",
            post(api_add_session_group_member),
        )
        .route(
            "/api/session-groups/unlink",
            post(api_unlink_session_group_member),
        )
        .route("/api/session-groups/delete", post(api_delete_session_group))
        .route(
            "/api/session-groups/copy-linked",
            post(api_copy_linked_sessions),
        )
        .route("/api/checkin/status", get(api_checkin_status))
        .route("/api/credits", post(api_credits))
        .route("/api/credits/stats", get(api_credit_statistics))
        .route("/api/token-stats", get(api_token_statistics))
        .route("/api/rate-limits", get(api_rate_limits))
        .route(
            "/api/rate-limits/hook-status",
            get(api_rate_limit_hook_status),
        )
        .route(
            "/api/rate-limits/install-hook",
            post(api_install_rate_limit_hook),
        )
        .route(
            "/api/rate-limits/uninstall-hook",
            post(api_uninstall_rate_limit_hook),
        )
        .route(
            "/api/rate-limits/config",
            get(api_rate_limit_config).post(api_save_rate_limit_config),
        )
        .route("/api/checkin", post(api_checkin))
        .route("/api/checkin/all", post(api_checkin_all))
        .route(
            "/api/checkin/config",
            get(api_checkin_config).post(api_save_checkin_config),
        )
        .route("/api/checkin/logs", get(api_checkin_logs))
        .route("/api/notifications", get(api_notifications))
        .route("/api/notifications/record", post(api_record_notification))
        .route("/api/notifications/clear", post(api_clear_notifications))
        .route("/api/travel/status", get(api_travel_status))
        .route(
            "/api/travel/config",
            get(api_travel_config).post(api_save_travel_config),
        )
        .route(
            "/api/rotate/config",
            get(api_rotate_config).post(api_save_rotate_config),
        )
        .route("/api/rotate/status", get(api_rotate_status))
        .route("/api/rotate/run", post(api_rotate_run))
        .route("/api/rotate/logs", get(api_rotate_logs))
        .route("/api/refresh-token", post(api_refresh_token))
        // 成长任务与福利（国内版）：查询 / 手动运行 / 历史 / 自动开关。
        .route("/api/growth/tasks", get(api_growth_tasks))
        .route("/api/growth/run", post(api_growth_run))
        .route("/api/growth/history", get(api_growth_history))
        .route(
            "/api/growth/config",
            get(api_growth_config).post(api_save_growth_config),
        )
        // 账本备份：创建与列表走 HTTP；**恢复不走 HTTP**——替换 app.db 需要停掉服务
        // （本进程自身持有打开的连接，Windows 上会因共享冲突失败），用 CLI
        // `ai-account-center-server restore` 在停机状态下做。
        .route("/api/backup/list", get(api_backup_list))
        .route("/api/backup/create", post(api_backup_create))
        .route("/api/update/check", get(api_update_check))
        .route(
            "/api/update/config",
            get(api_update_config).post(api_save_update_config),
        )
        // 运行形态边界：桌面专属接口在服务层拒绝，不依赖前端隐藏。
        .layer(middleware::from_fn(reject_desktop_only))
        // 管理面认证：/api 下除公开路由外，全部要求有效会话。
        .layer(middleware::from_fn({
            let state = state.clone();
            move |request: Request, next: Next| {
                let state = state.clone();
                async move { require_auth(state, request, next).await }
            }
        }))
        .fallback(static_handler)
        .with_state(state)
}

/// 桌面专属接口前缀（Web 形态一律拒绝）。
///
/// 与运行模式无关的请求参数、代理头与 UA 都不能改变这一边界；前端隐藏入口只是补充。
const DESKTOP_ONLY_PREFIXES: &[&str] = &[
    "/api/switch",
    "/api/sessions",
    "/api/session-sync",
    "/api/session-groups",
    "/api/session-links",
    "/api/codebuddy-cli",
    "/api/codebuddy-ide",
    "/api/codebuddy-cn-ide",
    "/api/jetbrains",
    "/api/vscode-ext",
    "/api/token-stats",
    "/api/import-local",
    "/api/export-accounts-to-path",
    "/api/rotate",
    "/api/rate-limits",
];

/// 精确匹配的桌面专属接口。
///
/// 不能按前缀收 `/api/update`：账号显示字段更新走 `/api/update-account-display`，Web 需要它。
const DESKTOP_ONLY_EXACT: &[&str] = &["/api/update/check", "/api/update/config"];

fn is_desktop_only(path: &str) -> bool {
    DESKTOP_ONLY_EXACT.contains(&path)
        || DESKTOP_ONLY_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
}

async fn reject_desktop_only(request: Request, next: Next) -> Response {
    if is_desktop_only(request.uri().path()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "error": "运行模式不支持",
                "message": "当前是 Web 管理形态：本机客户端发现/切换、用户会话、CLI/Hook 与本地统计只在 Windows 桌面版提供。",
            })),
        )
            .into_response();
    }
    next.run(request).await
}

/// GET /api/mode —— 运行形态与能力边界。
///
/// 前端据此隐藏入口；后端仍独立强制（前端隐藏不是安全边界）。
async fn api_mode() -> Response {
    json_ok(json!({
        "mode": "web",
        "os": std::env::consts::OS,
        "desktopFeatures": false,
        "capabilities": {
            "accounts": true,
            "credentials": true,
            "credits": true,
            "officialUsage": true,
            "checkin": true,
            "travel": true,
            "notifications": true,
            "scheduling": true,
            "clientSwitch": false,
            "sessions": false,
            "localTokenStats": false,
            "hooks": false,
            "cliRotate": false,
            "nativeWindow": false,
            "desktopUpdate": false,
        },
        "version": update::APP_VERSION,
    }))
}

/// 应用级共享状态：单实例 SQLite 连接（互斥锁保护）。
///
/// ponytail: 全局锁；若并发写成为瓶颈再换连接池。
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    pub init: Arc<InitBootstrap>,
}

/// 一次性初始化码的有效期。
pub const INIT_CODE_TTL_MS: i64 = 10 * 60 * 1000;

/// 首次初始化的引导码：短时有效、单次消费，只随服务进程在本机控制台出现。
///
/// 没有它，开放端口后第一个匿名访问者就能直接调 `/api/auth/init` 抢设管理员密码
/// （SECURITY_AND_DATA.md §5）。状态放在进程内存里：重启即换新码，这也正是「重启
/// 服务获取新码」这一提示的依据。
pub struct InitBootstrap {
    code: Mutex<Option<(String, i64)>>,
}

impl Default for InitBootstrap {
    fn default() -> Self {
        Self::new()
    }
}

impl InitBootstrap {
    pub fn new() -> Self {
        Self {
            code: Mutex::new(None),
        }
    }

    /// 生成并记住一个新的一次性初始化码（覆盖旧码）。
    pub fn issue(&self) -> String {
        let code = auth::generate_init_code();
        *self.code.lock().unwrap() = Some((code.clone(), config::now_ms() + INIT_CODE_TTL_MS));
        code
    }

    /// 是否还有未过期的初始化码。
    pub fn is_pending(&self) -> bool {
        let guard = self.code.lock().unwrap();
        matches!(guard.as_ref(), Some((_, expires)) if config::now_ms() <= *expires)
    }

    /// 校验并消费初始化码；成功即失效，失败给出可操作的原因。
    pub fn redeem(&self, candidate: &str) -> Result<(), String> {
        let mut guard = self.code.lock().unwrap();
        let Some((code, expires)) = guard.as_ref() else {
            return Err("初始化码已失效，请重启服务获取新码".to_string());
        };
        if config::now_ms() > *expires {
            *guard = None;
            return Err("初始化码已过期，请重启服务获取新码".to_string());
        }
        let code = code.clone();
        drop(guard);
        // 先比较再消费：比较失败不能顺手把码作废，否则一次误输就要重启。
        if !auth::constant_time_eq(code.as_bytes(), candidate.trim().as_bytes()) {
            return Err("初始化码不正确".to_string());
        }
        *self.code.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
impl InitBootstrap {
    /// 让当前码立即过期，用于覆盖过期分支（真实等待 10 分钟不现实）。
    fn expire_now_for_test(&self) {
        if let Some((_, expires)) = self.code.lock().unwrap().as_mut() {
            *expires = 0;
        }
    }
}


const SESSION_COOKIE: &str = "aac_session";
const SESSION_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// 连续失败多少次内不惩罚（正常输错不打扰）。
const LOGIN_FREE_ATTEMPTS: u32 = 3;
const LOGIN_BACKOFF_BASE_MS: i64 = 1_000;
/// 退避上限：再多的失败也不会把管理员长期锁在外面。
const LOGIN_BACKOFF_MAX_MS: i64 = 60_000;

/// 登录限速状态（单管理员、单实例，进程内即可）。
///
/// 重启会清零，但重启需要本机执行权限，攻击者能重启时已经绕过登录本身；
/// 这里的目标是挡住在线暴力破解，而不是对抗本地提权。
struct LoginThrottle {
    failures: u32,
    blocked_until: i64,
}

static LOGIN_THROTTLE: Mutex<LoginThrottle> = Mutex::new(LoginThrottle {
    failures: 0,
    blocked_until: 0,
});

/// 第 `failures` 次连续失败后的退避时长（指数增长、封顶）。
fn login_backoff_ms(failures: u32) -> i64 {
    if failures <= LOGIN_FREE_ATTEMPTS {
        return 0;
    }
    let step = (failures - LOGIN_FREE_ATTEMPTS - 1).min(6);
    (LOGIN_BACKOFF_BASE_MS << step).min(LOGIN_BACKOFF_MAX_MS)
}

/// 当前是否处于退避窗口；是则返回还需等待的秒数（不足 1 秒按 1 秒）。
fn login_retry_after_secs() -> Option<i64> {
    let throttle = LOGIN_THROTTLE.lock().unwrap();
    (throttle.blocked_until > 0)
        .then(|| throttle.blocked_until - config::now_ms())
        .filter(|left| *left > 0)
        .map(|left| (left + 999) / 1000)
}

fn login_record_failure() {
    let mut throttle = LOGIN_THROTTLE.lock().unwrap();
    throttle.failures = throttle.failures.saturating_add(1);
    let backoff = login_backoff_ms(throttle.failures);
    throttle.blocked_until = if backoff > 0 {
        config::now_ms() + backoff
    } else {
        0
    };
}

fn login_reset() {
    let mut throttle = LOGIN_THROTTLE.lock().unwrap();
    throttle.failures = 0;
    throttle.blocked_until = 0;
}

fn too_many_requests(retry_after_secs: i64) -> Response {
    let mut resp = json_err(
        format!("登录尝试过于频繁，请在 {retry_after_secs} 秒后重试"),
        StatusCode::TOO_MANY_REQUESTS,
    );
    if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        resp.headers_mut().insert(header::RETRY_AFTER, value);
    }
    resp
}

/// 无需登录即可访问的 /api 路由。
fn is_auth_public(path: &str) -> bool {
    matches!(
        path,
        "/api/mode"
            | "/api/auth/status"
            | "/api/auth/init"
            | "/api/auth/login"
            | "/api/auth/logout"
    )
}

fn cookie_value(cookie: Option<&HeaderValue>, name: &str) -> Option<String> {
    let s = cookie?.to_str().ok()?;
    s.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k.trim() == name).then(|| v.trim().to_string())
    })
}

fn origin_matches_host(origin: &str, host: &str) -> bool {
    let o = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .unwrap_or(origin);
    o == host
}

/// 管理面认证中间件：/api 下除公开路由外全部要求有效会话；写方法另校验 Origin 同源。
async fn require_auth(state: AppState, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    if !path.starts_with("/api/") {
        return next.run(request).await;
    }
    // CSRF：所有写方法（含登录/初始化等公开写接口）校验 Origin 与 Host 同源。
    // 无 Origin 头时放行（同源表单/脚本客户端），主要防线仍是 SameSite=Strict。
    if request.method() != Method::GET && request.method() != Method::HEAD {
        if let Some(origin) = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
        {
            let host = request
                .headers()
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !origin_matches_host(origin, host) {
                return json_err("跨站请求被拒绝".to_string(), StatusCode::FORBIDDEN);
            }
        }
    }
    if is_auth_public(&path) {
        return next.run(request).await;
    }
    let authenticated = match cookie_value(request.headers().get(header::COOKIE), SESSION_COOKIE) {
        Some(token) => {
            // 锁在 await 之前释放，避免 MutexGuard 跨 await 点。
            let db = state.db.lock().unwrap();
            db.session_valid(&auth::hash_token(&token)).unwrap_or(false)
        }
        None => false,
    };
    if authenticated {
        return next.run(request).await;
    }
    json_err("未登录或会话已过期".to_string(), StatusCode::UNAUTHORIZED)
}

/// GET /api/auth/status —— 管理面入口状态。
///
/// 公开路由：`authenticated` 只反映本次请求自带 Cookie 的会话是否有效，不泄露密码状态
/// 以外的信息，也不需要额外接口供前端判断「该显示登录页还是应用」。
async fn api_auth_status(State(state): State<AppState>, request: Request) -> Response {
    let db = state.db.lock().unwrap();
    let initialized = db.admin_password_hash().ok().flatten().is_some();
    let authenticated = cookie_value(request.headers().get(header::COOKIE), SESSION_COOKIE)
        .map(|token| db.session_valid(&auth::hash_token(&token)).unwrap_or(false))
        .unwrap_or(false);
    json_ok(json!({
        "initialized": initialized,
        // 初始化需要本机控制台的一次性码；前端据此决定是否显示「输入初始化码」输入框。
        "initCodeRequired": !initialized && state.init.is_pending(),
        "authenticated": authenticated,
    }))
}

fn issue_session(db: &Db) -> Response {
    let token = auth::generate_session_token();
    let expires = config::now_ms() + SESSION_TTL_MS;
    if let Err(e) = db.create_session(&auth::hash_token(&token), expires) {
        return json_err(format!("创建会话失败: {e}"), StatusCode::INTERNAL_SERVER_ERROR);
    }
    let cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        SESSION_TTL_MS / 1000
    );
    let mut resp = json_ok(json!({ "ok": true }));
    let hv = HeaderValue::from_bytes(cookie.as_bytes())
        .unwrap_or_else(|_| HeaderValue::from_static("aac_session=; Path=/; Max-Age=0"));
    resp.headers_mut().insert(header::SET_COOKIE, hv);
    resp
}

async fn api_auth_init(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if let Some(secs) = login_retry_after_secs() {
        return too_many_requests(secs);
    }
    let db = state.db.lock().unwrap();
    if db.admin_password_hash().ok().flatten().is_some() {
        return json_err("已初始化，不能重复设置密码".to_string(), StatusCode::CONFLICT);
    }
    // 先校验密码格式再消费初始化码：码是一次性的，不能因为密码没填好就作废、逼用户重启。
    let password = body.get("password").and_then(Value::as_str).unwrap_or("");
    if password.chars().count() < 8 {
        return json_err("密码至少 8 位".to_string(), StatusCode::BAD_REQUEST);
    }
    // 本机控制台的一次性初始化码：没有它就拒绝，避免开放端口后被匿名访问者抢设密码。
    let code = body.get("code").and_then(Value::as_str).unwrap_or("");
    if let Err(e) = state.init.redeem(code) {
        login_record_failure();
        return json_err(e, StatusCode::UNAUTHORIZED);
    }
    let hash = match auth::hash_password(password) {
        Ok(h) => h,
        Err(e) => return json_err(e, StatusCode::INTERNAL_SERVER_ERROR),
    };
    if let Err(e) = db.set_admin_password(&hash) {
        return json_err(format!("保存密码失败: {e}"), StatusCode::INTERNAL_SERVER_ERROR);
    }
    issue_session(&db)
}

async fn api_auth_login(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    // 退避窗口内直接拒绝，不做密码校验：既限速，也不给比对结果。
    if let Some(secs) = login_retry_after_secs() {
        return too_many_requests(secs);
    }
    let db = state.db.lock().unwrap();
    let stored = match db.admin_password_hash() {
        Ok(Some(h)) => h,
        _ => return json_err("尚未初始化，请先设置密码".to_string(), StatusCode::PRECONDITION_REQUIRED),
    };
    let password = body.get("password").and_then(Value::as_str).unwrap_or("");
    if !auth::verify_password(password, &stored) {
        login_record_failure();
        return json_err("密码错误".to_string(), StatusCode::UNAUTHORIZED);
    }
    login_reset();
    issue_session(&db)
}

async fn api_auth_logout(State(state): State<AppState>, request: Request) -> Response {
    if let Some(token) = cookie_value(request.headers().get(header::COOKIE), SESSION_COOKIE) {
        let _ = state.db.lock().unwrap().revoke_session(&auth::hash_token(&token));
    }
    let mut resp = json_ok(json!({ "ok": true }));
    let hv = HeaderValue::from_static("aac_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    resp.headers_mut().insert(header::SET_COOKIE, hv);
    resp
}

fn json_ok(v: Value) -> Response {
    Json(v).into_response()
}

fn json_err(e: String, code: StatusCode) -> Response {
    (code, Json(json!({ "ok": false, "error": e }))).into_response()
}

/// 从 query string 解析档位（缺省国内版）。与 Tauri 命令的可选 `variant` 参数同义。
fn query_variant(query: Option<&str>) -> WbVariant {
    let raw = query.unwrap_or("").split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (key == "variant").then_some(value)
    });
    WbVariant::parse(raw)
}

/// 从请求体解析档位（缺省国内版）。与 Tauri 命令的可选 `variant` 参数同义。
fn body_variant(body: &Value) -> WbVariant {
    WbVariant::parse(body.get("variant").and_then(Value::as_str))
}

/// 从 query string 取指定键的原值（键值均为账号 id 这类 ASCII 串；
/// 与 `query_variant` 同一解析口径，不做百分号解码）。
fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    query.unwrap_or("").split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (name == key).then_some(value.to_string())
    })
}

// ---------------------------------------------------------------------------
// 状态 / 账号
// ---------------------------------------------------------------------------

async fn api_status(RawQuery(query): RawQuery) -> Response {
    let variant = query_variant(query.as_deref());
    let auth = auth_file::read_auth_file(variant);
    let current = auth.as_ref().map(|a| {
        let acct = a.get("account").cloned().unwrap_or_else(|| json!({}));
        json!({
            "uid": account::display_value(&acct, "uid"),
            "nickname": account::display_value(&acct, "nickname"),
            "email": account::display_value(&acct, "email"),
        })
    });
    json_ok(json!({
        "running": cached_workbuddy_running(variant),
        "authFile": auth_file::auth_file_path(variant).to_string_lossy(),
        "current": current,
        "appPath": auth_file::workbuddy_app_path(variant).to_string_lossy(),
        "version": update::APP_VERSION,
        "variant": variant.as_str(),
    }))
}

/// GET /api/accounts —— 返回全部档位的账号，`current` 取请求档位的登录态。
async fn api_accounts(RawQuery(query): RawQuery) -> Response {
    let variant = query_variant(query.as_deref());
    json_ok(json!({
        "accounts": account::load_accounts()
            .iter()
            .map(account::account_meta)
            .collect::<Vec<_>>(),
        "current": auth_file::read_auth_file(variant)
            .and_then(|a| a.get("account").and_then(|x| x.get("uid")).and_then(|x| x.as_str()).map(String::from)),
        "variant": variant.as_str(),
    }))
}

async fn api_codebuddy_cli_status() -> Response {
    json_ok(codebuddy_cli::status())
}

async fn api_codebuddy_cli_install_helper() -> Response {
    match codebuddy_cli::install_helper() {
        Ok(result) => json_ok(result),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_codebuddy_cli_switch(Json(body): Json<Value>) -> Response {
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    // 入参 `closeRunningCli` 已废弃：后端一律先关闭正在运行的 CLI 再写状态，忽略该值。
    // 无头模式不投递系统通知，切号结果（含关闭数量）照常返回给调用方。
    match codebuddy_cli::switch_active_account(id) {
        Ok(result) => json_ok(result),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_codebuddy_cn_ide_status() -> Response {
    json_ok(codebuddy_cn_ide::status())
}

async fn api_codebuddy_cn_ide_switch(Json(body): Json<Value>) -> Response {
    let account_id = body
        .get("accountId")
        .or_else(|| body.get("account_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let restart = body
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    // 可选：切换前把勾选会话复制到目标账号（与 /api/vscode-ext/switch 同形）。
    // 任一条目非法即整包拒绝（与 Tauri 侧 `Option<Vec<CopyItem>>` 的 serde 整包报错同形），
    // 避免「部分成功 + 静默丢弃」让用户误以为全部复制成功。
    let copy_items: Vec<vscode_session::CopyItem> = match body
        .get("copySessions")
        .and_then(|v| v.as_array())
        .map(|array| {
            array
                .iter()
                .map(|item| serde_json::from_value::<vscode_session::CopyItem>(item.clone()))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
    {
        Ok(items) => items.unwrap_or_default(),
        Err(error) => {
            return json_err(
                format!("copySessions 条目非法：{error}"),
                StatusCode::BAD_REQUEST,
            )
        }
    };
    // 同步选择与桌面端同形（[{groupId, previewToken, mode}]），形状由 core 校验。
    let sync_selections = match session::parse_sync_selections(body.get("syncSelections")) {
        Ok(selections) => selections,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let result = if copy_items.is_empty() && sync_selections.is_empty() {
        codebuddy_cn_ide::switch_account(account_id, restart)
    } else {
        codebuddy_ide_session::switch_codebuddy_cn_ide_with_copy(
            account_id,
            restart,
            &copy_items,
            &sync_selections,
        )
    };
    match result {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/codebuddy-cn-ide/sessions —— 当前 IDE 账号可复制的会话（未登录返回空列表）。
async fn api_codebuddy_cn_ide_sessions() -> Response {
    let result =
        tokio::task::spawn_blocking(codebuddy_ide_session::list_current_codebuddy_ide_sessions)
            .await;
    match result {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// POST /api/codebuddy-cn-ide/session-links —— 预览当前 IDE 账号 → 目标账号的关联会话同步项。
///
/// 与桌面端 `codebuddy_ide_session_links_preview` 同形：直接返回 core 的只读预览。
async fn api_codebuddy_cn_ide_session_links_preview(Json(body): Json<Value>) -> Response {
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if target_account_id.trim().is_empty() {
        return json_err("缺少 targetAccountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let result = tokio::task::spawn_blocking(move || {
        let target = account::find_account(&target_account_id).ok_or("目标账号不存在")?;
        codebuddy_ide_session_sync::links_preview(&target)
    })
    .await;
    match result {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_codebuddy_cn_ide_detect() -> Response {
    match codebuddy_cn_ide::detect_current_account() {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_vscode_ext_status() -> Response {
    json_ok(vscode_ext::status())
}

/// GET /api/vscode-ext/sessions —— 当前 VS Code 扩展账号可复制的会话（未登录返回空列表）。
async fn api_vscode_ext_sessions() -> Response {
    let result = tokio::task::spawn_blocking(|| match vscode_ext::active_ext_uid() {
        Some(uid) => vscode_session::list_vscode_sessions(&uid),
        None => json!({ "sourceUid": null, "sessions": [], "skipped": 0 }),
    })
    .await;
    match result {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_vscode_ext_switch(Json(body): Json<Value>) -> Response {
    let account_id = body
        .get("accountId")
        .or_else(|| body.get("account_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // 默认重启（= 自动关闭并重开）：VS Code 运行时由后端先优雅退出再写入。
    // 显式传 restart=false 时退回「请先完全退出 VS Code」的手动模式。
    let restart = body
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    // 可选：切换前把勾选会话复制到目标账号（与 /api/vscode-ext/* 命名风格一致）。
    // 任一条目非法即整包拒绝（与 Tauri 侧 `Option<Vec<CopyItem>>` 的 serde 整包报错同形），
    // 避免「部分成功 + 静默丢弃」让用户误以为全部复制成功。
    let copy_items: Vec<vscode_session::CopyItem> = match body
        .get("copySessions")
        .and_then(|v| v.as_array())
        .map(|array| {
            array
                .iter()
                .map(|item| serde_json::from_value::<vscode_session::CopyItem>(item.clone()))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
    {
        Ok(items) => items.unwrap_or_default(),
        Err(error) => {
            return json_err(
                format!("copySessions 条目非法：{error}"),
                StatusCode::BAD_REQUEST,
            )
        }
    };

    // 同步选择与桌面端同形（[{groupId, previewToken, mode}]），形状由 core 校验。
    let sync_selections = match session::parse_sync_selections(body.get("syncSelections")) {
        Ok(selections) => selections,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };

    let result = if copy_items.is_empty() && sync_selections.is_empty() {
        vscode_ext::switch_account(account_id, restart)
    } else {
        vscode_session::switch_vscode_ext_with_copy(
            account_id,
            restart,
            &copy_items,
            &sync_selections,
        )
    };
    match result {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/vscode-ext/session-links —— 预览当前插件账号 → 目标账号的关联会话同步项。
///
/// 与桌面端 `vscode_session_links_preview` 同形：直接返回 core 的只读预览
/// （`supported` / `storeStatus` / `groups`），每组的 `defaultChecked` 与 `availableModes`
/// 是前端的勾选权限来源。
async fn api_vscode_ext_session_links_preview(Json(body): Json<Value>) -> Response {
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if target_account_id.trim().is_empty() {
        return json_err("缺少 targetAccountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let result = tokio::task::spawn_blocking(move || {
        let target = account::find_account(&target_account_id).ok_or("目标账号不存在")?;
        vscode_session_sync::links_preview(&target)
    })
    .await;
    match result {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_codebuddy_ide_status() -> Response {
    json_ok(codebuddy_ide::status())
}

/// POST /api/codebuddy-ide/switch —— 注入凭证到 CodeBuddy IDE（国际版），可选复制 / 同步会话。
///
/// `copySessions` / `syncSelections` 与国内版（`/api/codebuddy-cn-ide/switch`）同形：
/// 任一条目非法即整包拒绝，两者都为空时行为与纯切换逐字一致。
async fn api_codebuddy_ide_switch(Json(body): Json<Value>) -> Response {
    let account_id = body
        .get("accountId")
        .or_else(|| body.get("account_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let restart = body
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let copy_items: Vec<vscode_session::CopyItem> = match body
        .get("copySessions")
        .and_then(|v| v.as_array())
        .map(|array| {
            array
                .iter()
                .map(|item| serde_json::from_value::<vscode_session::CopyItem>(item.clone()))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
    {
        Ok(items) => items.unwrap_or_default(),
        Err(error) => {
            return json_err(
                format!("copySessions 条目非法：{error}"),
                StatusCode::BAD_REQUEST,
            )
        }
    };
    // 同步选择与桌面端同形（[{groupId, previewToken, mode}]），形状由 core 校验。
    let sync_selections = match session::parse_sync_selections(body.get("syncSelections")) {
        Ok(selections) => selections,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let result = if copy_items.is_empty() && sync_selections.is_empty() {
        codebuddy_ide::switch_account(account_id, restart)
    } else {
        codebuddy_ide_session::switch_codebuddy_intl_ide_with_copy(
            account_id,
            restart,
            &copy_items,
            &sync_selections,
        )
    };
    match result {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// GET /api/codebuddy-ide/sessions —— 当前国际版 IDE 账号可复制的会话（未登录返回空列表）。
async fn api_codebuddy_intl_ide_sessions() -> Response {
    let result =
        tokio::task::spawn_blocking(codebuddy_ide_session::list_current_intl_ide_sessions).await;
    match result {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// POST /api/codebuddy-ide/session-links —— 预览当前国际版 IDE 账号 → 目标账号的关联会话同步项。
///
/// 与桌面端 `codebuddy_intl_ide_session_links_preview` 同形：直接返回 core 的只读预览。
async fn api_codebuddy_intl_ide_session_links_preview(Json(body): Json<Value>) -> Response {
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if target_account_id.trim().is_empty() {
        return json_err("缺少 targetAccountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let result = tokio::task::spawn_blocking(move || {
        let target = account::find_account(&target_account_id).ok_or("目标账号不存在")?;
        codebuddy_ide_session_sync::links_preview_intl(&target)
    })
    .await;
    match result {
        Ok(Ok(value)) => json_ok(value),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_vscode_ext_detect() -> Response {
    match vscode_ext::detect_current_account() {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_codebuddy_ide_detect() -> Response {
    match codebuddy_ide::detect_current_account() {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_jetbrains_status() -> Response {
    json_ok(jetbrains::status())
}

async fn api_jetbrains_switch(Json(body): Json<Value>) -> Response {
    let account_id = body
        .get("accountId")
        .or_else(|| body.get("account_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let restart = body
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    // 可选：目标配置目录名列表（如 ["PyCharm2026.2"]）。缺省 / 空数组 = 全部装了插件的 IDE。
    let config_dirs: Option<Vec<String>> = body
        .get("configDirs")
        .and_then(|v| v.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|list| !list.is_empty());
    match jetbrains::switch_account(account_id, restart, config_dirs.as_deref()) {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_jetbrains_detect() -> Response {
    match jetbrains::detect_current_account() {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_delete(Json(body): Json<Value>) -> Response {
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    match account::delete_account(id) {
        Ok(()) => json_ok(json!({ "ok": true })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/update-account-display —— 更新账号本地展示字段（备注 / 显示选择）。
///
/// body：`{ accountId, patch }`，patch 可含 `note`（字符串或 null）与 `displayField`。
async fn api_update_account_display(Json(body): Json<Value>) -> Response {
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    let patch = body.get("patch").cloned().unwrap_or_else(|| json!({}));
    match account::update_account_display(id, &patch) {
        Ok(meta) => json_ok(json!({ "ok": true, "account": meta })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/import-local —— 导入本机当前账号（body 可选 `variant`，缺省国内版）。
///
/// body 允许缺失，保持改造前的调用方式可用。
async fn api_import_local(body: Option<Json<Value>>) -> Response {
    let variant = body
        .as_ref()
        .map(|Json(value)| body_variant(value))
        .unwrap_or_else(|| WbVariant::parse(None));
    match account::import_local(variant) {
        Ok(acc) => json_ok(json!({ "ok": true, "account": acc })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

// ---------------------------------------------------------------------------
// 导出 / 导入账号
// ---------------------------------------------------------------------------

async fn api_export_accounts(Json(body): Json<Value>) -> Response {
    let ids: Vec<String> = body
        .get("accountIds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    match export_import::export_accounts(&ids) {
        Ok(records) => json_ok(json!({ "ok": true, "accounts": records })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_export_accounts_to_path(Json(body): Json<Value>) -> Response {
    let ids: Vec<String> = body
        .get("accountIds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let path = body
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    match export_import::export_accounts_to_path(&ids, &path) {
        Ok(path) => json_ok(json!({ "ok": true, "path": path })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_preview_import(Json(body): Json<Value>) -> Response {
    let text = body
        .get("fileText")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    match export_import::preview_accounts(&text) {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_import(Json(body): Json<Value>) -> Response {
    let text = body
        .get("fileText")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let indexes: Vec<usize> = body
        .get("indexes")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64().map(|n| n as usize))
                .collect()
        })
        .unwrap_or_default();
    match export_import::import_accounts(&text, &indexes) {
        Ok(result) => json_ok(json!({
            "ok": true,
            "imported": result.imported,
            "skipped": result.skipped,
            "overwritten": result.overwritten,
        })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

// ---------------------------------------------------------------------------
// OAuth 登录
// ---------------------------------------------------------------------------

/// POST /api/oauth/start —— 发起扫码登录（body 可选 `variant`，缺省国内版）。
async fn api_oauth_start(body: Option<Json<Value>>) -> Response {
    let variant = body
        .as_ref()
        .map(|Json(value)| body_variant(value))
        .unwrap_or_else(|| WbVariant::parse(None));
    match oauth::oauth_start(variant).await {
        Ok(v) => json_ok(v),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/oauth/status —— 轮询采集结果（档位取发起时记录，无需传参）。
async fn api_oauth_status(Json(body): Json<Value>) -> Response {
    let login_id = body
        .get("loginId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    json_ok(oauth::oauth_poll(&login_id).await)
}

// ---------------------------------------------------------------------------
// 切换
// ---------------------------------------------------------------------------

async fn api_switch(Json(body): Json<Value>) -> Response {
    let account_id = body
        .get("accountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if account_id.trim().is_empty() {
        return json_err("缺少 accountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let restart = body
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let share_sessions = body
        .get("shareSessions")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let copy_ids: Vec<String> = body
        .get("copySessionIds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    // 同步选择与桌面端同形（[{groupId, previewToken, mode}]），形状由 core 校验。
    let sync_selections = match session::parse_sync_selections(body.get("syncSelections")) {
        Ok(selections) => selections,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };

    {
        let mut running = SWITCH_RUNNING.lock().unwrap();
        if *running {
            return json_err("已有切换任务进行中".to_string(), StatusCode::CONFLICT);
        }
        *running = true;
        *SWITCH_PROGRESS.lock().unwrap() = Some("开始切换账号…".to_string());
    }

    let progress: switch::ProgressFn = Box::new(|msg| {
        *SWITCH_PROGRESS.lock().unwrap() = Some(msg.to_string());
    });

    let result = tokio::task::spawn_blocking(move || {
        switch::switch_account(
            Some(&progress),
            &account_id,
            restart,
            share_sessions,
            &copy_ids,
            &sync_selections,
        )
    })
    .await;

    *SWITCH_RUNNING.lock().unwrap() = false;

    match result {
        Ok(Ok(v)) => json_ok(v),
        Ok(Err(e)) => json_err(e, StatusCode::BAD_REQUEST),
        Err(e) => json_err(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_switch_progress() -> Response {
    let p = SWITCH_PROGRESS.lock().unwrap().clone();
    let running = *SWITCH_RUNNING.lock().unwrap();
    json_ok(json!({ "running": running, "progress": p }))
}

// ---------------------------------------------------------------------------
// 会话
// ---------------------------------------------------------------------------

/// GET /api/sessions —— 当前账号的会话列表（query 可选 `variant`，缺省国内版）。
async fn api_sessions(RawQuery(query): RawQuery) -> Response {
    let variant = query_variant(query.as_deref());
    match session::current_user_uid(variant) {
        Some(uid) => json_ok(json!({
            "sessions": session::list_sessions_for_user(variant, &uid),
            "current": uid,
            "variant": variant.as_str(),
        })),
        None => json_ok(json!({ "sessions": [], "current": null, "variant": variant.as_str() })),
    }
}

/// GET /api/sessions/account —— 指定账号名下的会话列表（会话管理页的源账号视角）。
///
/// 与 `GET /api/sessions` 同形，但来源是显式账号（`accountId` query）：账号不存在
/// 返回 400；账号缺 uid 时返回空列表 + `current: null`（与现有容错一致）。
async fn api_account_sessions(RawQuery(query): RawQuery) -> Response {
    let account_id = query_param(query.as_deref(), "accountId").unwrap_or_default();
    let client = query_param(query.as_deref(), "client").unwrap_or_else(|| "workbuddy".to_string());
    let Some(account) = account::find_account(&account_id) else {
        return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let client = match session_groups::SessionClient::parse(&client) {
        Ok(client) => client,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match client {
        session_groups::SessionClient::VscodeExt => {
            let Some(uid) = account::get_str(&account, "uid")
                .map(|uid| uid.trim().to_string())
                .filter(|uid| !uid.is_empty())
            else {
                return json_err("账号缺少 uid".to_string(), StatusCode::BAD_REQUEST);
            };
            json_ok(vscode_session::list_vscode_sessions(&uid))
        }
        session_groups::SessionClient::Workbuddy => {
            json_ok(session::list_sessions_for_account(&account))
        }
        session_groups::SessionClient::CodebuddyIde => json_err(
            "当前客户端暂不支持列出账号会话".to_string(),
            StatusCode::BAD_REQUEST,
        ),
    }
}

async fn api_copy_sessions(Json(body): Json<Value>) -> Response {
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_ids: Vec<String> = body
        .get("sessionIds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let Some(target) = account::find_account(&target_account_id) else {
        return json_err("目标账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    // 档位取目标账号自身（源 uid 也从该档位的登录态读）。
    let variant = account::variant_of(&target);
    // 与桌面端同形：直接返回 core 的复制报告（copied / alreadyLinked / errors / needsRecovery）。
    let mut report = match session::copy_sessions_for_switch(&target, &session_ids) {
        Ok(report) => report,
        Err(error) => {
            return json_err(error, StatusCode::BAD_REQUEST);
        }
    };
    report["variant"] = json!(variant.as_str());
    json_ok(report)
}

/// POST /api/sessions/copy-cross —— 跨档复制：源与目标账号都显式给出（会话管理页用）。
///
/// 与 `POST /api/sessions/copy` 同形，多一个 `sourceAccountId`：源 uid 取自该账号，
/// 不再从目标档登录态读取，支持国内版 ↔ 国际版。报告与桌面端 `copy_sessions_cross`
/// 同形（含 `sourceVariant` / `targetVariant`）。
async fn api_copy_sessions_cross(Json(body): Json<Value>) -> Response {
    let source_account_id = body
        .get("sourceAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_ids: Vec<String> = body
        .get("sessionIds")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let Some(source) = account::find_account(&source_account_id) else {
        return json_err("源账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let Some(target) = account::find_account(&target_account_id) else {
        return json_err("目标账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let target_variant = account::variant_of(&target);
    let mut report = match session::copy_sessions_cross(&source, &target, &session_ids) {
        Ok(report) => report,
        Err(error) => {
            return json_err(error, StatusCode::BAD_REQUEST);
        }
    };
    // 与同档端点同形：`variant` 恒为目标档（前端按目标账号渲染）。
    report["variant"] = json!(target_variant.as_str());
    json_ok(report)
}

/// POST /api/session-links/preview —— 预览当前账号 → 目标账号的关联会话同步项。
///
/// 与桌面端 `session_links_preview` 同形：直接返回 core 的只读预览（`supported` /
/// `storeStatus` / `groups`），每组的 `defaultChecked` 与 `availableModes` 是前端的
/// 勾选权限来源。`variant` 缺省取目标账号自身档位。
async fn api_session_links_preview(Json(body): Json<Value>) -> Response {
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if target_account_id.trim().is_empty() {
        return json_err("缺少 targetAccountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let Some(target) = account::find_account(&target_account_id) else {
        return json_err("目标账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let variant = match body.get("variant").and_then(Value::as_str) {
        Some(raw) => WbVariant::parse(Some(raw)),
        None => account::variant_of(&target),
    };
    match session::session_links_preview(variant, &target) {
        Ok(report) => json_ok(report),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/session-links/preview-cross —— 预览「显式来源账号 → 显式目标账号」（跨档支持）。
///
/// 与 `POST /api/session-links/preview` 同形，多一个 `sourceAccountId`；
/// 成员内容按成员自身档位读取（跨档组的源读源档、目标读目标档）。
async fn api_session_links_preview_cross(Json(body): Json<Value>) -> Response {
    let source_account_id = body
        .get("sourceAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if source_account_id.trim().is_empty() {
        return json_err("缺少 sourceAccountId".to_string(), StatusCode::BAD_REQUEST);
    }
    let Some(source) = account::find_account(&source_account_id) else {
        return json_err("源账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let Some(target) = account::find_account(&target_account_id) else {
        return json_err("目标账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    match session::session_links_preview_cross(&source, &target) {
        Ok(report) => json_ok(report),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/session-sync/cross —— 把显式来源账号的新增同步到显式目标账号（跨档支持）。
///
/// `syncSelections` 与切号弹窗同形（core 校验缺 groupId / previewToken / mode）。
async fn api_session_sync_cross(Json(body): Json<Value>) -> Response {
    let source_account_id = body
        .get("sourceAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let target_account_id = body
        .get("targetAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let Some(source) = account::find_account(&source_account_id) else {
        return json_err("源账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let Some(target) = account::find_account(&target_account_id) else {
        return json_err("目标账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let selections = match session::parse_sync_selections(body.get("syncSelections")) {
        Ok(selections) => selections,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match session::sync_sessions_cross(&source, &target, &selections) {
        Ok(report) => json_ok(report),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

fn session_group_request(
    body: &Value,
) -> Result<(session_groups::SessionClient, Option<WbVariant>), String> {
    let client = body
        .get("client")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 client".to_string())?;
    let client = session_groups::SessionClient::parse(client)?;
    let scope =
        session_groups::parse_variant_scope(body.get("variantScope").and_then(Value::as_str))?;
    Ok((client, scope))
}

async fn api_list_session_groups(Json(body): Json<Value>) -> Response {
    let (client, scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    match session_groups::list(client, scope) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_get_session_group(Json(body): Json<Value>) -> Response {
    let (client, scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let group_id = body.get("groupId").and_then(Value::as_str).unwrap_or("");
    match session_groups::detail(client, scope, group_id) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_preview_session_group_pair(Json(body): Json<Value>) -> Response {
    let (client, scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let args = (
        body.get("groupId").and_then(Value::as_str).unwrap_or(""),
        body.get("sourceMemberId")
            .and_then(Value::as_str)
            .unwrap_or(""),
        body.get("targetMemberId")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    match session_groups::preview_pair(client, scope, args.0, args.1, args.2) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_sync_session_group_pair(Json(body): Json<Value>) -> Response {
    let (client, scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let args = (
        body.get("groupId").and_then(Value::as_str).unwrap_or(""),
        body.get("sourceMemberId")
            .and_then(Value::as_str)
            .unwrap_or(""),
        body.get("targetMemberId")
            .and_then(Value::as_str)
            .unwrap_or(""),
        body.get("previewToken")
            .and_then(Value::as_str)
            .unwrap_or(""),
        body.get("mode").and_then(Value::as_str).unwrap_or(""),
    );
    let restart = body
        .get("restart")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match session_groups::sync_pair(
        client, scope, args.0, args.1, args.2, args.3, args.4, restart,
    ) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_sync_session_group_unify(Json(body): Json<Value>) -> Response {
    let (client, scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let targets = match serde_json::from_value::<Vec<session_groups::GroupUnifyTarget>>(
        body.get("targets").cloned().unwrap_or(Value::Null),
    ) {
        Ok(value) => value,
        Err(error) => {
            return json_err(
                format!("目标副本参数无效：{error}"),
                StatusCode::BAD_REQUEST,
            )
        }
    };
    let group_id = body.get("groupId").and_then(Value::as_str).unwrap_or("");
    let source_member_id = body
        .get("sourceMemberId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let restart = body
        .get("restart")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match session_groups::sync_unify_batch(
        client,
        scope,
        group_id,
        source_member_id,
        &targets,
        restart,
    ) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_sync_session_group_safe_batch(Json(body): Json<Value>) -> Response {
    let (client, scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let group_id = body.get("groupId").and_then(Value::as_str).unwrap_or("");
    let restart = body
        .get("restart")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match session_groups::sync_safe_batch(client, scope, group_id, restart) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_add_session_group_member(Json(body): Json<Value>) -> Response {
    let (client, scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let args = (
        body.get("groupId").and_then(Value::as_str).unwrap_or(""),
        body.get("sourceMemberId")
            .and_then(Value::as_str)
            .unwrap_or(""),
        body.get("targetAccountId")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let restart = body
        .get("restart")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match session_groups::add_member(client, scope, args.0, args.1, args.2, restart) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_copy_linked_sessions(Json(body): Json<Value>) -> Response {
    let (client, _scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let source_account_id = body
        .get("sourceAccountId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let target_account_id = body
        .get("targetAccountId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let session_ids: Vec<String> = body
        .get("sessionIds")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let restart = body
        .get("restart")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match session_groups::copy_linked_sessions(
        client,
        source_account_id,
        target_account_id,
        &session_ids,
        restart,
    ) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

async fn api_vscode_restart_precheck(Json(body): Json<Value>) -> Response {
    let target_account_ids: Vec<String> = body
        .get("targetAccountIds")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    json_ok(session_groups::vscode_restart_precheck(&target_account_ids))
}

/// 取消关联：只解除管理关系，不触碰账号内的会话内容。
async fn api_unlink_session_group_member(Json(body): Json<Value>) -> Response {
    let (client, scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let group_id = body.get("groupId").and_then(Value::as_str).unwrap_or("");
    let member_id = body.get("memberId").and_then(Value::as_str).unwrap_or("");
    match session_groups::remove_member(client, scope, group_id, member_id) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// 删除会话组：组内所有成员一起解除关联，只解除管理关系，不触碰账号内的会话内容。
async fn api_delete_session_group(Json(body): Json<Value>) -> Response {
    let (client, scope) = match session_group_request(&body) {
        Ok(value) => value,
        Err(error) => return json_err(error, StatusCode::BAD_REQUEST),
    };
    let group_id = body.get("groupId").and_then(Value::as_str).unwrap_or("");
    match session_groups::delete_group(client, scope, group_id) {
        Ok(value) => json_ok(value),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

// ---------------------------------------------------------------------------
// 签到 / 保活
// ---------------------------------------------------------------------------

/// GET /api/checkin/status —— 传 accountId 时只查询该账号；缺省保留旧批量响应。
/// 两种形式都遵守单账号自动签到开关，避免展示状态时触发已关闭账号的请求。
async fn api_checkin_status(Query(query): Query<HashMap<String, String>>) -> Response {
    if let Some(id) = query.get("accountId") {
        let Some(acc) = account::find_account(id) else {
            return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
        };
        let status = checkin::get_checkin_status_for_display(&acc).await;
        return json_ok(checkin_status_item(&acc, status));
    }
    let list = account::load_accounts();
    let mut items = Vec::new();
    for acc in &list {
        let status = checkin::get_checkin_status_for_display(acc).await;
        items.push(checkin_status_item(acc, status));
    }
    json_ok(json!({ "accounts": items }))
}

fn checkin_status_item(account: &Value, mut status: Value) -> Value {
    status["accountId"] = account.get("id").cloned().unwrap_or(Value::Null);
    status["email"] = json!(account::account_display_name(account));
    status["variant"] = json!(account::variant_of(account).as_str());
    status
}

async fn api_credits(Json(body): Json<Value>) -> Response {
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    let Some(acc) = account::find_account(id) else {
        return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    json_ok(credits::get_credit_expiry(&acc).await)
}

fn query_flag_enabled(query: Option<&str>, name: &str) -> bool {
    query.unwrap_or("").split('&').any(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, "true"));
        key == name && matches!(value, "" | "1" | "true" | "yes")
    })
}

async fn api_credit_statistics(RawQuery(query): RawQuery) -> Response {
    json_ok(credit_usage::get_statistics(query_flag_enabled(query.as_deref(), "refresh")).await)
}

async fn api_token_statistics(RawQuery(query): RawQuery) -> Response {
    let days = query.as_deref().and_then(|value| {
        value
            .split('&')
            .find_map(|part| part.strip_prefix("days=")?.parse::<i64>().ok())
    });
    match tokio::task::spawn_blocking(move || token_stats::get_statistics(days)).await {
        Ok(statistics) => json_ok(statistics),
        Err(error) => json_err(
            format!("扫描 Token 统计失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// GET /api/rate-limits —— 模型限额台账（全部账号当前受限的模型与官方恢复时刻）。
///
/// 扫描本机日志文件，放 blocking 线程避免占用运行时线程；无受限模型时返回空数组。
async fn api_rate_limits() -> Response {
    match tokio::task::spawn_blocking(limits::get_rate_limits).await {
        Ok(payload) => json_ok(payload),
        Err(error) => json_err(
            format!("扫描模型限额失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// hook 状态 + 运行期字段（最近一次 hook 事件时刻）。
fn rate_limit_hook_status() -> Value {
    let mut status = rate_limit_hook::hook_status();
    status["lastEventAt"] = json!(rate_limit_events::last_event_at());
    status
}

/// GET /api/rate-limits/hook-status —— hook 安装状态（脚本 + 三处客户端配置逐项结果）。
async fn api_rate_limit_hook_status() -> Response {
    match tokio::task::spawn_blocking(rate_limit_hook_status).await {
        Ok(status) => json_ok(status),
        Err(error) => json_err(
            format!("查询限额 hook 状态失败: {error}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// POST /api/rate-limits/install-hook —— 安装 hook（幂等，写前备份；同时清除「卸载过」标记）。
async fn api_install_rate_limit_hook() -> Response {
    match tokio::task::spawn_blocking(|| {
        let result = rate_limit_hook::install_hook();
        // 扫描范围随安装结果变化（只对未注册的来源扫日志），缓存必须作废。
        limits::invalidate_scan_cache();
        result.map(|_| rate_limit_hook_status())
    })
    .await
    {
        Ok(Ok(status)) => json_ok(status),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// POST /api/rate-limits/uninstall-hook —— 卸载 hook（移除注册条目，尽量逐字节还原）。
///
/// 卸载即用户拒绝自动接入（`hookOptOut`），与安装逻辑同处 core，两个宿主共用同一语义。
async fn api_uninstall_rate_limit_hook() -> Response {
    match tokio::task::spawn_blocking(|| {
        let result = rate_limit_hook::uninstall_hook();
        limits::invalidate_scan_cache();
        result.map(|_| rate_limit_hook_status())
    })
    .await
    {
        Ok(Ok(status)) => json_ok(status),
        Ok(Err(error)) => json_err(error, StatusCode::BAD_REQUEST),
        Err(error) => json_err(error.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn api_rate_limit_config() -> Response {
    json_ok(config::load_rate_limit_config())
}

/// POST /api/rate-limits/config —— 保存限额监听配置。
///
/// 与桌面端同语义：`scanIdeLogs` 变化时作废扫描缓存，下一次按当前来源范围重算。
async fn api_save_rate_limit_config(Json(body): Json<Value>) -> Response {
    let submitted = body.get("config").unwrap_or(&body);
    match limits::save_rate_limit_config(submitted) {
        Ok(()) => json_ok(config::load_rate_limit_config()),
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

async fn api_checkin(Json(body): Json<Value>) -> Response {
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    let Some(acc) = account::find_account(id) else {
        return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    json_ok(checkin::checkin_account(&acc).await)
}

async fn api_checkin_all(body: Option<Json<Value>>) -> Response {
    // 缺省（无 body / 无 variant）= 全部档位，保持与桌面端 set 前的行为一致。
    let variant = body
        .as_ref()
        .and_then(|Json(value)| value.get("variant"))
        .and_then(|value| value.as_str())
        .map(|raw| WbVariant::parse(Some(raw)));
    // `respectWindow`（camelCase，与 `accountId` 先例一致）只由账号页「刷新并签到」
    // 传 true：窗口生效且当前不在时间段内时整轮跳过；缺省 false 保持设置页 /
    // 托盘 / 旧客户端的立即签到语义。
    let respect_window = body
        .as_ref()
        .and_then(|Json(value)| value.get("respectWindow"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    json_ok(checkin::run_checkin_all(variant, respect_window).await)
}

async fn api_checkin_config() -> Response {
    json_ok(config::load_checkin_config())
}

async fn api_save_checkin_config(Json(body): Json<Value>) -> Response {
    let submitted = body.get("config").unwrap_or(&body);
    match config::save_checkin_config(submitted) {
        Ok(()) => json_ok(config::load_checkin_config()),
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

/// GET /api/checkin/logs —— 签到日志（每行带 `variant`，便于前端按档位过滤）。
async fn api_checkin_logs() -> Response {
    json_ok(json!({ "logs": checkin::load_checkin_logs_with_variant() }))
}

async fn api_travel_status() -> Response {
    travel::reconcile_due_travel(None).await;
    let items = account::load_accounts()
        .iter()
        .map(|acc| {
            let id = acc.get("id").and_then(Value::as_str).unwrap_or("");
            let mut value = travel::travel_display(id);
            value["accountId"] = acc.get("id").cloned().unwrap_or(Value::Null);
            value["email"] = json!(account::account_display_name(acc));
            value
        })
        .collect::<Vec<_>>();
    json_ok(json!({ "accounts": items }))
}

async fn api_travel_config() -> Response {
    json_ok(config::load_travel_config())
}

async fn api_save_travel_config(Json(body): Json<Value>) -> Response {
    let submitted = body.get("config").unwrap_or(&body);
    match config::save_travel_config(submitted) {
        Ok(()) => {
            let saved = config::load_travel_config();
            if saved.get("enabled").and_then(Value::as_bool) == Some(true) {
                tokio::spawn(async {
                    let _ = travel::run_travel_cycle().await;
                    let _ = travel::run_travel_claim_cycle().await;
                });
            }
            json_ok(saved)
        }
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

async fn api_refresh_token(Json(body): Json<Value>) -> Response {
    let id = body.get("accountId").and_then(|v| v.as_str()).unwrap_or("");
    let Some(acc) = account::find_account(id) else {
        return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    json_ok(refresh::refresh_account_token(acc).await)
}

// ---------------------------------------------------------------------------
// 自动轮换（CodeBuddy CLI）
// ---------------------------------------------------------------------------

async fn api_rotate_config() -> Response {
    json_ok(config::load_auto_rotate_config())
}

async fn api_save_rotate_config(Json(body): Json<Value>) -> Response {
    match config::save_auto_rotate_config(&body) {
        Ok(()) => json_ok(json!({ "ok": true, "config": config::load_auto_rotate_config() })),
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

async fn api_rotate_status() -> Response {
    json_ok(rotate::rotate_status())
}

async fn api_rotate_run() -> Response {
    json_ok(rotate::run_rotate_cycle().await)
}

async fn api_rotate_logs() -> Response {
    json_ok(json!({ "logs": rotate::rotate_logs() }))
}

// ---------------------------------------------------------------------------
// 更新
// ---------------------------------------------------------------------------

async fn api_update_check() -> Response {
    json_ok(update::update_check(None, false).await)
}

// ---------------------------------------------------------------------------
// 成长任务与福利（国内版）
// ---------------------------------------------------------------------------

/// GET /api/growth/tasks —— 任务列表（按 `claimable` / `manual` / `pending` / `settled` 分组）。
///
/// 带 `accountId` 返回该账号一组；不带则返回所有支持成长中心的账号（`{accounts:[…]}`）。
/// 界面不该在「没指定账号」时拿到 400——单账号场景走的就是这条路径。
async fn api_growth_tasks(Query(query): Query<HashMap<String, String>>) -> Response {
    let Some(id) = query.get("accountId").map(String::as_str).filter(|v| !v.is_empty()) else {
        return json_ok(growth::tasks_view_all().await);
    };
    let Some(account) = account::find_account(id) else {
        return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    match growth::tasks_view(&account).await {
        Ok(payload) => json_ok(payload),
        Err(e) => json_err(e, StatusCode::BAD_GATEWAY),
    }
}

/// POST /api/growth/run —— body `{ "accountId": "...", "kind": "daily" | "night" | "streak" }`。
///
/// 手动触发：不受 `auto_growth_config.json` 的 `enabled` 限制（开关只管后台自动运行）。
async fn api_growth_run(Json(body): Json<Value>) -> Response {
    let account_id = body
        .get("accountId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(account) = account::find_account(account_id) else {
        return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST);
    };
    let uid = account.get("uid").and_then(Value::as_str).unwrap_or("");
    match body.get("kind").and_then(Value::as_str).unwrap_or("daily") {
        "daily" => {
            let report = growth::run_growth_for_account(&account, growth::GrowthMode::Daily, "manual")
                .await;
            json_ok(growth::report_json(&report))
        }
        "night" => {
            let report = growth::run_growth_for_account(&account, growth::GrowthMode::Night, "manual")
                .await;
            json_ok(growth::report_json(&report))
        }
        "streak" => {
            let report = growth::run_streak_bonus(&account).await;
            growth::record_run(uid, "streak", "manual", &report);
            json_ok(growth::report_json(&report))
        }
        other => json_err(format!("未知的运行类型: {other}"), StatusCode::BAD_REQUEST),
    }
}

/// GET /api/growth/history —— 运行历史（新的在前）。
///
/// 入参是账号记录 id，但成长状态按 **uid** 记录（与事件判重同一身份），所以先解析再查。
async fn api_growth_history(Query(query): Query<HashMap<String, String>>) -> Response {
    let uid_filter = match query.get("accountId").map(String::as_str).filter(|v| !v.is_empty()) {
        Some(id) => match account::find_account(id) {
            Some(acc) => acc.get("uid").and_then(Value::as_str).map(str::to_string),
            None => return json_err("账号不存在".to_string(), StatusCode::BAD_REQUEST),
        },
        None => None,
    };
    let limit = query
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        .clamp(1, 200);
    match growth::recent_runs(uid_filter.as_deref(), limit) {
        Ok(runs) => json_ok(json!({ "runs": growth::runs_json(&runs) })),
        Err(e) => json_err(e, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// GET /api/growth/config —— 自动运行开关（默认关闭）。
async fn api_growth_config() -> Response {
    json_ok(config::load_growth_config())
}

/// POST /api/growth/config —— body `{ "enabled": bool, "nightEnabled": bool }`。
async fn api_save_growth_config(Json(body): Json<Value>) -> Response {
    match config::save_growth_config(&body) {
        Ok(()) => json_ok(config::load_growth_config()),
        Err(e) => json_err(format!("保存成长任务配置失败: {e}"), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// ---------------------------------------------------------------------------
// 备份（创建 / 列表；恢复见 CLI）
// ---------------------------------------------------------------------------

fn backup_info_json(info: &backup::BackupInfo) -> Value {
    json!({
        "formatVersion": info.format_version,
        "appVersion": info.app_version,
        "createdAt": info.created_at_ms,
    })
}

/// GET /api/backup/list —— 数据根 `backups/` 下的备份文件（只解析头部，不解密）。
async fn api_backup_list() -> Response {
    match backup::list_backups(&config::store_dir()) {
        Ok(entries) => json_ok(json!({
            "backups": entries
                .iter()
                .map(|entry| json!({
                    "fileName": entry.file_name,
                    "size": entry.size,
                    "info": entry.info.as_ref().map(backup_info_json),
                    "error": entry.error,
                }))
                .collect::<Vec<_>>(),
        })),
        Err(e) => json_err(e, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// POST /api/backup/create —— body `{ "passphrase": "..." }`。
///
/// 只写入数据根 `backups/`，不接受请求方指定路径：Web 请求不能变成任意服务端写入。
/// 备份内容受口令保护，但口令本身经网络传输，远程使用请在反向代理侧终止 TLS。
async fn api_backup_create(Json(body): Json<Value>) -> Response {
    let passphrase = body
        .get("passphrase")
        .and_then(Value::as_str)
        .unwrap_or("");
    match backup::create_backup(&config::store_dir(), passphrase, None) {
        Ok((path, info)) => json_ok(json!({
            "fileName": path.file_name().map(|n| n.to_string_lossy().to_string()),
            "info": backup_info_json(&info),
        })),
        Err(e) => json_err(e, StatusCode::BAD_REQUEST),
    }
}

async fn api_update_config() -> Response {
    json_ok(update::load_github_config())
}

async fn api_save_update_config(Json(body): Json<Value>) -> Response {
    match update::save_github_config(&body) {
        Ok(()) => json_ok(json!({ "ok": true, "config": update::load_github_config() })),
        Err(e) => json_err(e.to_string(), StatusCode::BAD_REQUEST),
    }
}

// ---------------------------------------------------------------------------
// 静态前端
// ---------------------------------------------------------------------------

fn content_type(path: &str) -> &'static str {
    if path.ends_with(".js") || path.ends_with(".mjs") {
        "text/javascript"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    }
}

async fn static_handler(uri: Uri) -> Response {
    let mut path = uri.path().trim_start_matches('/').to_string();
    if path.is_empty() || path == "index.html" {
        path = "index.html".to_string();
    }
    // 前端路由回退到 index.html。Content-Type 必须按**实际命中的文件**给：按请求路径算
    // 会把 `/sessions` 这类前端路由标成 application/octet-stream，浏览器直接当下载。
    let (data, served) = match Assets::get(&path) {
        Some(f) => (f, path),
        None => match Assets::get("index.html") {
            Some(f) => (f, "index.html".to_string()),
            None => {
                return Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("not found"))
                    .unwrap()
            }
        },
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type(&served))
        .body(Body::from(data.data.into_owned()))
        .unwrap()
}

// ---------------------------------------------------------------------------
// 通知存档（toast 事后可查）
// ---------------------------------------------------------------------------

/// GET /api/notifications —— 最近的应用内提示（新的在前，最多 100 条）。
async fn api_notifications() -> Response {
    match notifications::list() {
        Ok(items) => json_ok(json!({ "items": items })),
        Err(error) => json_err(error, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// POST /api/notifications/record —— 记录一条提示（前端 toast 同步写一份）。
async fn api_record_notification(Json(body): Json<Value>) -> Response {
    let level = body.get("level").and_then(|v| v.as_str()).unwrap_or("info");
    let title = body.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let description = body.get("description").and_then(|v| v.as_str());
    match notifications::record(level, title, description) {
        Ok(()) => json_ok(json!({ "recorded": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

/// POST /api/notifications/clear —— 清空通知存档。
async fn api_clear_notifications() -> Response {
    match notifications::clear() {
        Ok(()) => json_ok(json!({ "cleared": true })),
        Err(error) => json_err(error, StatusCode::BAD_REQUEST),
    }
}

#[cfg(test)]
mod tests {
    use super::{body_variant, checkin_status_item, query_variant, static_handler};
    use axum::http::{header, StatusCode, Uri};
    use serde_json::json;
    use wb_switch_core::modules::variant::WbVariant;

    /// SPA 回退必须按**命中的文件**给 Content-Type：`/sessions` 这类前端路由曾按请求路径
    /// 被标成 application/octet-stream，浏览器直接当文件下载、页面打不开（真机验收发现）。
    #[tokio::test]
    async fn spa_fallback_serves_html_content_type() {
        for path in ["/", "/sessions", "/accounts"] {
            let resp = static_handler(Uri::from_static(path)).await;
            assert_eq!(resp.status(), StatusCode::OK, "{path}");
            let content_type = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            assert!(
                content_type.starts_with("text/html"),
                "{path} 的 Content-Type 应为 text/html，实际 {content_type}"
            );
        }
    }

    /// 缺省档位必须与改造前一致（不传 variant 即国内版）。
    #[test]
    fn variant_query_defaults_to_cn() {
        assert_eq!(query_variant(None), WbVariant::Cn);
        assert_eq!(query_variant(Some("")), WbVariant::Cn);
        assert_eq!(query_variant(Some("refresh=true")), WbVariant::Cn);
        assert_eq!(
            query_variant(Some("refresh=true&variant=cn")),
            WbVariant::Cn
        );
        assert_eq!(query_variant(Some("variant=ai")), WbVariant::Ai);
        assert_eq!(
            query_variant(Some("variant=ai&refresh=true")),
            WbVariant::Ai
        );
        assert_eq!(query_variant(Some("variant=unknown")), WbVariant::Cn);
    }

    #[test]
    fn variant_body_defaults_to_cn() {
        assert_eq!(body_variant(&json!({})), WbVariant::Cn);
        assert_eq!(body_variant(&json!({"variant": null})), WbVariant::Cn);
        assert_eq!(body_variant(&json!({"variant": "ai"})), WbVariant::Ai);
        assert_eq!(body_variant(&json!({"accountId": "x"})), WbVariant::Cn);
    }

    #[test]
    fn web_checkin_status_keeps_account_identity() {
        let item = checkin_status_item(
            &json!({"id": "account-1", "email": "user@example.com"}),
            json!({"ok": true, "todayCheckedIn": true}),
        );

        assert_eq!(item["accountId"], "account-1");
        assert_eq!(item["email"], "user@example.com");
        assert_eq!(item["todayCheckedIn"], true);
        assert_eq!(item["variant"], "cn");
    }

    #[test]
    fn web_checkin_status_preserves_failure_state() {
        let item = checkin_status_item(
            &json!({"id": "account-2"}),
            json!({"ok": false, "todayCheckedIn": false, "error": "status failed"}),
        );

        assert_eq!(item["accountId"], "account-2");
        assert_eq!(item["ok"], false);
        assert_eq!(item["error"], "status failed");
    }

    #[test]
    fn web_checkin_status_preserves_exclusion_without_inventing_today_status() {
        let item = checkin_status_item(
            &json!({"id": "excluded"}),
            json!({"ok": false, "result": "skipped", "reason": "auto_checkin_disabled"}),
        );
        assert_eq!(item["accountId"], "excluded");
        assert_eq!(item["reason"], "auto_checkin_disabled");
        assert_eq!(item["result"], "skipped");
        assert!(item.get("todayCheckedIn").is_none());
    }

    #[test]
    fn web_checkin_status_row_carries_variant() {
        let item = checkin_status_item(
            &json!({"id": "ai-1", "variant": "ai"}),
            json!({"ok": false, "statusUnsupported": true}),
        );

        assert_eq!(item["variant"], "ai");
        assert_eq!(item["statusUnsupported"], true);
    }

    /// 退避曲线：前几次失败不惩罚，之后指数增长并封顶。
    #[test]
    fn login_backoff_grows_exponentially_then_caps() {
        assert_eq!(super::login_backoff_ms(0), 0);
        assert_eq!(super::login_backoff_ms(1), 0);
        assert_eq!(super::login_backoff_ms(super::LOGIN_FREE_ATTEMPTS), 0);
        assert_eq!(super::login_backoff_ms(4), super::LOGIN_BACKOFF_BASE_MS);
        assert_eq!(super::login_backoff_ms(5), super::LOGIN_BACKOFF_BASE_MS * 2);
        assert_eq!(super::login_backoff_ms(6), super::LOGIN_BACKOFF_BASE_MS * 4);
        // 封顶，且不随失败次数溢出。
        assert_eq!(super::login_backoff_ms(20), super::LOGIN_BACKOFF_MAX_MS);
        assert_eq!(super::login_backoff_ms(u32::MAX), super::LOGIN_BACKOFF_MAX_MS);
    }

    /// 初始化码：错码不作废、正确的码一次性消费。
    #[test]
    fn init_code_is_single_use_and_wrong_code_does_not_burn_it() {
        let bootstrap = super::InitBootstrap::new();
        let code = bootstrap.issue();

        assert!(bootstrap.redeem("").is_err(), "空码必须拒绝");
        assert!(bootstrap.redeem("WRONGCODE123").is_err());
        assert!(bootstrap.is_pending(), "错码不应把有效码作废");
        assert!(bootstrap.redeem(&code).is_ok(), "正确码应通过");

        assert!(!bootstrap.is_pending(), "用后即失效");
        let err = bootstrap.redeem(&code).unwrap_err();
        assert!(err.contains("已失效"), "二次使用应报失效：{err}");
    }

    /// 没有签发过码时直接拒绝（重启后未签发、或已初始化后码被清掉）。
    #[test]
    fn init_code_missing_is_rejected() {
        let bootstrap = super::InitBootstrap::new();
        assert!(!bootstrap.is_pending());
        assert!(bootstrap.redeem("ANYTHING").is_err());
    }

    /// 过期的码即便内容正确也必须拒绝。
    #[test]
    fn expired_init_code_is_rejected() {
        let bootstrap = super::InitBootstrap::new();
        let code = bootstrap.issue();
        bootstrap.expire_now_for_test();

        assert!(!bootstrap.is_pending(), "过期后不应再报「有待用码」");
        let err = bootstrap.redeem(&code).unwrap_err();
        assert!(err.contains("已过期"), "过期码的提示应可操作：{err}");
    }

    /// 重新签发会换新码，旧码随即失效。
    #[test]
    fn reissued_init_code_invalidates_the_previous_one() {
        let bootstrap = super::InitBootstrap::new();
        let first = bootstrap.issue();
        let second = bootstrap.issue();
        assert_ne!(first, second);
        assert!(bootstrap.redeem(&first).is_err(), "旧码应失效");
        assert!(bootstrap.redeem(&second).is_ok());
    }

    /// 连续失败进入退避、成功即清零。
    #[test]
    fn login_throttle_blocks_after_repeated_failures_and_resets_on_success() {
        super::login_reset();
        for _ in 0..super::LOGIN_FREE_ATTEMPTS {
            super::login_record_failure();
        }
        assert!(
            super::login_retry_after_secs().is_none(),
            "少量失败不应触发退避"
        );

        super::login_record_failure();
        let retry = super::login_retry_after_secs().expect("超过阈值应进入退避");
        assert!(retry >= 1, "Retry-After 至少 1 秒，实际 {retry}");

        super::login_reset();
        assert!(super::login_retry_after_secs().is_none(), "成功后应清零");
    }
}
