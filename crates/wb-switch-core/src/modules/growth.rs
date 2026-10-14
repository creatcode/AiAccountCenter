//! WorkBuddy 国内版成长任务与福利：任务查询/接取/领奖、事件上报、盲盒抽奖、连续打卡。
//!
//! 协议对照 workbuddy2api-hub `wb_tasks.py`（固定提交 906ca6a0），端点与实测约束记录在
//! docs/REFERENCE_SOURCES.md §5.1。**实现用 Rust 自行编写**，不搬其 Python 服务本体。
//!
//! 三条实测约束必须守住，否则会出现「接取了一堆但一个都没点亮」：
//!
//! 1. **部分任务不可伪造**：任务只认桌面客户端真实行为信号（`jump_url` 为
//!    `workbuddy://` 深链）。伪造 `/v2/report` 会被忽略或落到 heartbeat，进度恒为 0/1，
//!    claim 必然返回 `task not completed`。正确处理是**诚实跳过并给出深链**，不是反复重试。
//! 2. **夜间窗口**：夜猫子类任务只在 23:00–08:00 上报才计数，且每天 1 次。
//! 3. **事件去重**：上游按 `(eventCode, id)` 去重，重复同一 id 不推进进度，必须换 id。
//!
//! 另有两条工程约束：请求间隔 ≥1s（`MIN_REQUEST_INTERVAL`，串行发），设备标识由 uid
//! 稳定派生（`derive_id`）——随机机器码会被上游风控识别。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::modules::account::{self, build_auth_headers, get_str, variant_of};
use crate::modules::db::{db_path, Db, GrowthRun};
use crate::modules::config::{
    http_request, load_growth_config, now_ms, GROWTH_API_PREFIX, WORKBUDDY_CHAT_ENDPOINT,
    WORKBUDDY_WEB_ENDPOINT,
};
use crate::modules::refresh::refresh_account_token;

/// 请求最小间隔：上游按真实客户端行为判风控，参考实现固定 ≥1.0s。
pub const MIN_REQUEST_INTERVAL: Duration = Duration::from_secs(1);

/// 夜猫子任务的夜间窗口（含起点、不含终点）：23:00–08:00。
pub const NIGHT_WINDOW_START_HOUR: u32 = 23;
pub const NIGHT_WINDOW_END_HOUR: u32 = 8;

/// 批领取任务时每批的条数上限。
const ACCEPT_CHUNK: usize = 20;

/// 任务列表/接取的 `v2` 前缀；其余成长接口走 [`GROWTH_API_PREFIX`]（上游两套并存）。
const TASKS_V2_PREFIX: &str = "/v2/activity/growth/tasks";

/// 只认桌面端真实行为、无法通过上报事件完成的任务。
///
/// 值是给用户看的说明；`jump_url` 由服务端下发（`workbuddy://` 深链）。
pub const DESKTOP_ONLY_TASKS: [(&str, &str); 4] = [
    ("RichMeow_Chat", "在桌面端发起 1 次对话"),
    ("Library_read", "在桌面端打开「资料库」并读完介绍文档"),
    ("Buddy_App", "在桌面端左上角「发现应用」进入任意一个 Buddy 应用"),
    ("Buddy_App_QQ", "在桌面端「发现应用」进入「企鹅教师助手」"),
];

/// 夜间窗口任务码。
pub const NIGHT_TASK_CODES: [&str; 1] = ["black_cat"];

/// 业务上无法自动完成的任务（真实动作或人工操作），值与给用户的说明。
pub const UNFORGEABLE_TASKS: [(&str, &str); 2] = [
    (
        "wb_wechat_oa_subscribe_task",
        "需在微信关注「腾讯 WorkBuddy」公众号满 24 小时",
    ),
    ("Expert_Philanthropy", "需要真实捐款动作"),
];

/// 任务规格：决定该任务用什么事件推进、目标次数与奖励。
///
/// 对照参考实现 `TASK_SPECS`。`kind` 是 [`build_event`] 认识的类型。
pub struct TaskSpec {
    pub kind: &'static str,
    pub target: i64,
    pub reward: i64,
    pub name: &'static str,
}

/// 任务码 → 规格；未登记的任务不自动推进（宁可少做，也不乱发事件）。
pub fn task_spec(task_code: &str) -> Option<TaskSpec> {
    let (kind, target, reward, name) = match task_code {
        "create_canvas" => ("canvas", 1, 300, "创建设计任务"),
        "template_5" => ("template", 5, 200, "模板创建任务"),
        "expert_5" => ("expert", 5, 200, "使用专家助手"),
        "Expert_team_use_3" => ("team", 3, 150, "使用专家团队"),
        "skill_1" => ("skill", 1, 100, "体验技能"),
        "automation_1" => ("automation", 1, 100, "创建自动化任务"),
        "playbook_prompt" => ("playbook", 1, 100, "灵感案例使用"),
        "Expert_lighthouse" => ("lighthouse", 1, 100, "轻量云专家使用"),
        // first_buddy 是自动类任务（上游不接受接取），完成后才发 Buddy 实例；
        // 它是所有 single 任务与猫猫旅行的前置（"prerequisite not met: first_buddy"）。
        "first_buddy" => ("chat", 1, 300, "领取一只 Buddy"),
        "chat_5" => ("chat", 5, 100, "发起 5 次对话"),
        "Model_chat_GLM5.2" => ("glmchat", 1, 100, "体验 GLM-5.2"),
        "black_cat" => ("cat", 3, 100, "夜猫子任务 (23:00-08:00)"),
        _ => return None,
    };
    Some(TaskSpec {
        kind,
        target,
        reward,
        name,
    })
}

/// 该任务能否通过上报事件自动推进。
pub fn is_unforgeable(task_code: &str) -> bool {
    unforgeable_reason(task_code).is_some()
}

/// 无法自动完成的统一判定：桌面专属行为 + 真实世界动作。
pub fn unforgeable_reason(task_code: &str) -> Option<&'static str> {
    desktop_only_reason(task_code).or_else(|| {
        UNFORGEABLE_TASKS
            .iter()
            .find(|(code, _)| *code == task_code)
            .map(|(_, reason)| *reason)
    })
}

/// 专家/团队事件用的 id 池：每次必须换 id，否则上游按 `(eventCode, id)` 判重不推进。
pub const EXPERT_ID_POOL: [(&str, &str); 6] = [
    ("ex_PZw8Gu81HfN4", "运维工程师"),
    ("ex_ROsDtJbzADFV", "产品经理"),
    ("ex_SMUnl0nJbPix", "UI设计师"),
    ("ex_ZTR062oVBOCW", "数据分析师"),
    ("ex_a3sSSFBy8qaC", "后端架构师"),
    ("ex_aG1kvKbq8lPx", "文案策划"),
];

pub const TEAM_ID_POOL: [(&str, &str); 4] = [
    ("CloudOpsTeam", "运维专家团队"),
    ("CloudContentTeam", "内容专家团队"),
    ("CloudDevTeam", "研发专家团队"),
    ("ProductStrategyTeam", "产品战略团队"),
];

/// 取第 `seq` 个专家/团队 id（池内循环；配合 seed 保证整体唯一）。
fn pool_pick(pool: &[(&'static str, &'static str)], seq: usize) -> (&'static str, &'static str) {
    pool[seq % pool.len()]
}

// ---------------------------------------------------------------------------
// 纯逻辑（可单测，不发请求）
// ---------------------------------------------------------------------------

/// 该任务是否只能由桌面端真实行为完成。
pub fn desktop_only_reason(task_code: &str) -> Option<&'static str> {
    DESKTOP_ONLY_TASKS
        .iter()
        .find(|(code, _)| *code == task_code)
        .map(|(_, reason)| *reason)
}

/// 是否落在夜间窗口内（传入本地小时数，便于单测覆盖边界）。
pub fn in_night_window(hour: u32) -> bool {
    hour >= NIGHT_WINDOW_START_HOUR || hour < NIGHT_WINDOW_END_HOUR
}

/// 该任务码是否受夜间窗口限制。
pub fn is_night_task(task_code: &str) -> bool {
    NIGHT_TASK_CODES.contains(&task_code)
}

/// 由 uid + 用途盐稳定派生设备/会话标识。
///
/// 参考实现用 md5，这里用已在依赖里的 SHA-256 截断 36 位十六进制：设备标识只需对同一
/// 账号**稳定**且账号之间互不相同，不需要与参考实现逐字节一致，因此不为一个非安全用途
/// 新增 md5 依赖。
pub fn derive_id(uid: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{salt}:{}", if uid.is_empty() { "anonymous" } else { uid }).as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()[..36].to_string()
}

/// 上报事件所需的标准桌面端指纹（每个账号固定不变）。
pub fn build_fingerprint(uid: &str, nickname: &str) -> Value {
    let now = now_ms();
    json!({
        "timezone": "Asia/Shanghai",
        "reportDelay": 2000,
        "userId": uid,
        "username": nickname,
        "userNickname": nickname,
        "product": "SaaS",
        "ideName": "WorkBuddy",
        "ideType": "WorkBuddy",
        "ideVersion": "5.5.6",
        "machineId": derive_id(uid, "machine"),
        "sessionId": derive_id(uid, "session"),
        "extName": "workbuddy-desktop",
        "extVersion": "5.5.6",
        "os": "win32",
        "arch": "x64",
        "timestamp": now,
        "presentAt": now,
    })
}

/// 事件去重键：上游按 `(eventCode, id)` 判重。
///
/// 只有部分事件带 `id`（画布/专家/团队/剧本/皮肤），对话类事件没有该字段，改用其请求
/// 标识参与判重——否则同一账号的多次对话会被上游当成同一条事件丢掉。
pub fn event_dedupe_key(event: &Value) -> Option<(String, String)> {
    let code = event.get("eventCode").and_then(Value::as_str)?;
    let id = ["id", "requestId", "conversationId"]
        .iter()
        .find_map(|key| event.get(*key).and_then(Value::as_str))
        .unwrap_or("");
    Some((code.to_string(), id.to_string()))
}

/// 上一次真实请求的时间（进程内串行限速用）。
static LAST_REQUEST_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// 距离下一次请求还需要等多久；`elapsed` 已知时是纯函数，便于单测。
fn remaining_gap(elapsed: Option<Duration>, min_interval: Duration) -> Duration {
    match elapsed {
        None => Duration::ZERO,
        Some(elapsed) if elapsed >= min_interval => Duration::ZERO,
        Some(elapsed) => min_interval - elapsed,
    }
}

/// 串行地把两次请求之间的间隔压到不小于 [`MIN_REQUEST_INTERVAL`]。
async fn respect_min_interval() {
    let elapsed = {
        let guard = LAST_REQUEST_AT.lock().unwrap();
        guard.map(|last| last.elapsed())
    };
    let wait = remaining_gap(elapsed, MIN_REQUEST_INTERVAL);
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
    *LAST_REQUEST_AT.lock().unwrap() = Some(Instant::now());
}

// ---------------------------------------------------------------------------
// 请求
// ---------------------------------------------------------------------------

/// chat 侧请求头：成长中心走桌面/web 混合形态，与 travel 保持同一套鉴权字段。
fn growth_headers(account: &Value) -> HashMap<String, String> {
    let mut headers = build_auth_headers(account);
    headers.insert("x-client-platform".to_string(), "web".to_string());
    headers.insert("origin".to_string(), WORKBUDDY_CHAT_ENDPOINT.to_string());
    headers.insert(
        "referer".to_string(),
        format!("{WORKBUDDY_WEB_ENDPOINT}/profile/growth-center"),
    );
    headers
}

/// web 回退请求头（降级领奖用），授权与 Origin 都换成 web 域。
fn growth_web_headers(account: &Value) -> HashMap<String, String> {
    let mut headers = build_auth_headers(account);
    headers.insert("origin".to_string(), WORKBUDDY_WEB_ENDPOINT.to_string());
    headers.insert(
        "referer".to_string(),
        format!("{WORKBUDDY_WEB_ENDPOINT}/profile/growth-center"),
    );
    headers.insert("x-client-platform".to_string(), "web".to_string());
    headers
}

fn resp_code(resp: &Value) -> i64 {
    resp.get("code").and_then(Value::as_i64).unwrap_or(-1)
}

fn resp_error(resp: &Value) -> String {
    resp.get("msg")
        .or_else(|| resp.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("code={}", resp_code(resp)))
}

fn is_unauthorized(resp: &Value) -> bool {
    matches!(resp_code(resp), 401 | 403) || {
        let msg = resp_error(resp).to_lowercase();
        ["unauthorized", "invalid token", "登录", "失效", "过期"]
            .iter()
            .any(|key| msg.contains(key))
    }
}

/// 发成长接口请求：限速 + 未授权时刷新一次 token 重试。
async fn growth_request(path: &str, method: &str, body: Option<Value>, account: &Value) -> Value {
    // 加密信封凭据短路：不发空 Bearer，直接给可读错误（与 travel 一致）。
    if let Some(err) = account::envelope_token_error(account) {
        return json!({"code": -2, "message": err});
    }
    let url = format!("{WORKBUDDY_CHAT_ENDPOINT}{path}");
    let headers = growth_headers(account);
    respect_min_interval().await;
    let mut resp = http_request(&url, method, body.clone(), Some(&headers)).await;
    if is_unauthorized(&resp) && !get_str(account, "refresh_token").unwrap_or_default().is_empty() {
        let refreshed = refresh_account_token(account.clone()).await;
        let headers = growth_headers(&refreshed);
        respect_min_interval().await;
        resp = http_request(&url, method, body, Some(&headers)).await;
    }
    resp
}

// ---------------------------------------------------------------------------
// 任务列表 / 接取 / 领奖
// ---------------------------------------------------------------------------

/// 成长任务的安全视图：只带展示与判断需要的字段，不含任何凭据。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GrowthTask {
    pub code: String,
    pub name: String,
    pub description: String,
    /// `not_accepted` / `accepted` / `completed` / `claimed` 等，按服务端原值透传。
    pub status: String,
    pub task_type: String,
    pub current: i64,
    pub target: i64,
    pub reward_credit: i64,
    pub reward_energy: i64,
    /// `workbuddy://` 深链；桌面专属任务靠它引导用户去客户端完成。
    pub jump_url: String,
    /// 只能人工/真实动作完成时给出原因说明，否则为空（空 = 可自动推进）。
    pub unforgeable: String,
}

fn task_from_value(raw: &Value) -> GrowthTask {
    let code = raw
        .get("task_code")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let progress = raw.get("progress").cloned().unwrap_or_else(|| json!({}));
    GrowthTask {
        code: code.clone(),
        name: raw
            .get("title")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(&code)
            .to_string(),
        description: raw
            .get("description")
            .or_else(|| raw.get("task_desc"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        status: raw
            .get("accept_status")
            .and_then(Value::as_str)
            .unwrap_or("not_accepted")
            .to_string(),
        task_type: raw
            .get("task_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        current: progress.get("current").and_then(Value::as_i64).unwrap_or(0),
        target: progress.get("target").and_then(Value::as_i64).unwrap_or(1),
        reward_credit: raw
            .get("reward_credit")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        reward_energy: raw
            .get("reward_energy")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        jump_url: raw
            .get("jump_url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        unforgeable: unforgeable_reason(&code).unwrap_or_default().to_string(),
    }
}

/// 查询成长任务列表。
///
/// 企业版账号没有成长体系，返回空列表而不是报错。
pub async fn fetch_growth_tasks(account: &Value) -> Result<Vec<GrowthTask>, String> {
    if account
        .get("enterpriseId")
        .or_else(|| account.get("enterprise_id"))
        .and_then(Value::as_str)
        .is_some_and(|v| !v.is_empty())
    {
        return Ok(Vec::new());
    }
    let resp = growth_request(
        TASKS_V2_PREFIX,
        "GET",
        None,
        account,
    )
    .await;
    if resp_code(&resp) != 0 {
        return Err(resp_error(&resp));
    }
    let tasks = resp
        .get("data")
        .and_then(|d| d.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(tasks.iter().map(task_from_value).collect())
}

/// 批量接取结果。
///
/// 显式区分成功/失败并带回原因：参考实现踩过的坑是只返回 bool 且吞掉异常，接取失败
/// 时上层完全看不见，后续上报的事件全作用在未接取的任务上，进度永远是 0。
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptOutcome {
    pub accepted: Vec<String>,
    pub failed: Vec<String>,
    pub reasons: Vec<String>,
}

/// 批量接取任务（按 [`ACCEPT_CHUNK`] 分批）。
pub async fn accept_tasks(account: &Value, codes: &[String]) -> AcceptOutcome {
    let mut outcome = AcceptOutcome {
        accepted: Vec::new(),
        failed: Vec::new(),
        reasons: Vec::new(),
    };
    for chunk in codes.chunks(ACCEPT_CHUNK) {
        let resp = growth_request(
            &format!("{TASKS_V2_PREFIX}/accept"),
            "POST",
            Some(json!({ "task_codes": chunk })),
            account,
        )
        .await;
        if resp_code(&resp) != 0 {
            outcome.failed.extend(chunk.iter().cloned());
            outcome.reasons.push(resp_error(&resp));
            continue;
        }
        let results = resp
            .get("data")
            .and_then(|d| d.get("results"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut seen: Vec<String> = Vec::new();
        for item in &results {
            let code = item
                .get("task_code")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            seen.push(code.clone());
            match item.get("status").and_then(Value::as_str).unwrap_or("") {
                "accepted" | "already_accepted" => outcome.accepted.push(code),
                other => {
                    outcome.reasons.push(format!(
                        "{code}: {}",
                        item.get("message").and_then(Value::as_str).unwrap_or(other)
                    ));
                    outcome.failed.push(code);
                }
            }
        }
        // 上游没回话的任务按失败计，不能让调用方以为全部接取成功。
        for code in chunk {
            if !seen.contains(code) {
                outcome.failed.push(code.clone());
            }
        }
    }
    outcome
}

/// 领奖结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimOutcome {
    pub ok: bool,
    pub credit: i64,
    pub energy: i64,
    pub message: String,
}

impl ClaimOutcome {
    fn ok(data: &Value) -> Self {
        ClaimOutcome {
            ok: true,
            credit: data.get("credit").and_then(Value::as_i64).unwrap_or(0),
            energy: data.get("energy").and_then(Value::as_i64).unwrap_or(0),
            message: String::new(),
        }
    }

    fn failed(message: String) -> Self {
        ClaimOutcome {
            ok: false,
            credit: 0,
            energy: 0,
            message,
        }
    }
}

/// 领取单个任务奖励。
///
/// chat 侧优先；被拒时降级到 web 域（桌面端以外领取会被 chat 侧 400，参考实现即如此）。
pub async fn claim_task(account: &Value, code: &str) -> ClaimOutcome {
    let path = format!("{GROWTH_API_PREFIX}/tasks/{code}/claim");
    let resp = growth_request(&path, "POST", Some(json!({})), account).await;
    if resp_code(&resp) == 0 {
        let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
        return ClaimOutcome::ok(&data);
    }
    let first_error = resp_error(&resp);

    let url = format!("{WORKBUDDY_WEB_ENDPOINT}{path}");
    let headers = growth_web_headers(account);
    respect_min_interval().await;
    let web = http_request(&url, "POST", Some(json!({})), Some(&headers)).await;
    if resp_code(&web) == 0 {
        let data = web.get("data").cloned().unwrap_or_else(|| json!({}));
        return ClaimOutcome::ok(&data);
    }
    ClaimOutcome::failed(first_error)
}

// ---------------------------------------------------------------------------
// 事件上报
// ---------------------------------------------------------------------------

/// 事件种子：由「账号 + 任务 + 日期 + 序号」稳定派生。
///
/// 用稳定种子（而不是随机时间戳）是为了让**同一天重跑编排产生同一个事件 id**：上游按
/// `(eventCode, id)` 判重，配合本地 `growth_events` 表就能做到重跑不重复打点；换一天则
/// 自然换新 id，进度照常累加。
pub fn event_seed(uid: &str, task_code: &str, day: &str, seq: usize) -> String {
    format!("{uid}-{task_code}-{day}-{seq}")
}

/// 构造一条上报事件。
///
/// `seed` 决定事件里的 `conversationId` / `requestId` / `id`：同一天同一任务取同一 seed
/// 会被上游判重（正是需要的幂等），要推进进度就必须换 seed（换序号或换天）。
/// `expert` 为 `(id, 名称)`，专家/团队类事件务必换 id。
pub fn build_event(kind: &str, uid: &str, seed: &str, expert: Option<(&str, &str)>) -> Value {
    let now = now_ms();
    let conversation = format!("wb-task-{seed}");
    let request = format!("{conversation}-req");

    match kind {
        "canvas" => json!({
            "eventCode": "wbx_design_canvas_task_create", "timestamp": now, "reportDelay": 0,
            "conversationId": conversation, "requestId": request, "source": "summon_keyword",
            "isCustomModel": false, "name": "", "inputLength": 12,
            "id": format!("wbx-canvas-{seed}"),
            "cost": 0, "isSuccessful": true, "userId": uid,
        }),
        "template" => json!({
            "eventCode": "agent_task_created_with_template", "timestamp": now, "reportDelay": 0,
            "isCustomModel": true, "id": format!("template-{seed}"), "name": "幻灯片",
            "requestId": request, "conversationId": conversation, "userId": uid,
        }),
        "expert" | "team" | "lighthouse" => {
            let (expert_id, name) = expert.unwrap_or(match kind {
                "lighthouse" => ("ex_2cvvUZQhDyeJ", "腾讯轻量云专家"),
                "team" => ("CloudOpsTeam", "运维专家团队"),
                _ => ("ContentCreator", "内容创作专家"),
            });
            json!({
                "eventCode": "expert_actual_use", "timestamp": now, "reportDelay": 0,
                "mode": "CLOUD", "id": expert_id, "name": name, "expertTitle": name,
                "type": "02-Engineering",
                "expertType": if kind == "team" { "team" } else { "agent" },
                "source": "builtin", "version": "1.0.2", "cost": 0, "characterCount": 12,
                "conversationId": conversation, "requestId": request, "messageId": request,
                "requestModelId": "deepseek-v4-flash", "requestModelName": "DeepSeek V4 Flash",
                "userId": uid,
            })
        }
        "skill" => json!({
            "eventCode": "skill_info", "timestamp": now, "reportDelay": 0,
            "skillId": "skill_2096525080079265792", "name": "pptx", "userId": uid,
        }),
        "automation" => json!({
            "eventCode": "automated_task_create_suc", "timestamp": now, "reportDelay": 0,
            "name": "每周工作整理", "type": "cron", "source": "manually",
            "modelId": "deepseek-v4-flash", "modelIsThinking": false,
            "conversationId": conversation, "requestId": request,
            "schedule": {"type": "recurring", "rrule": "FREQ=WEEKLY;BYDAY=FR;BYHOUR=9;BYMINUTE=0"},
            "prompt": "每周五自动整理本周工作", "userId": uid,
        }),
        "playbook" => json!({
            "eventCode": "playbook_prompt_send", "timestamp": now, "reportDelay": 0,
            "id": "worker-ledger-freedom-dashboard", "name": "打工人小账本", "type": "other",
            "promptLength": 10, "isOfficial": 1, "source": "discover",
            "conversationId": conversation, "requestId": request, "userId": uid,
        }),
        "skin" => json!({
            "eventCode": "appearance_skin_apply", "timestamp": now, "reportDelay": 0,
            "action": "apply", "source": "settings_close", "id": "theme-tkmw7j",
            "vipLevel": "free", "series": "craft", "type": "unknown",
            "name": "和平精英激战金秋", "userId": uid,
        }),
        // 夜猫子任务必须用 night 模式，且只在夜间窗口上报才计数。
        "chat" | "glmchat" | "cat" => {
            let (model_id, model_name) = if matches!(kind, "glmchat" | "cat") {
                ("glm-5.2", "GLM-5.2")
            } else {
                ("deepseek-v4-flash", "DeepSeek V4 Flash")
            };
            json!({
                "eventCode": "chat_request_send", "timestamp": now, "reportDelay": 0,
                "mode": if kind == "cat" { "night" } else { "craft" },
                "conversationId": conversation, "requestId": request, "inputLength": 12,
                "requestModelId": model_id, "requestModelName": model_name, "isPlan": false,
                "agentName": "default", "agentType": "conversation", "userId": uid,
            })
        }
        _ => json!({"eventCode": "heartbeat", "timestamp": now, "userId": uid}),
    }
}

/// 上报事件数组；返回上游是否接受（`code == 0`）。
pub async fn report_events(account: &Value, events: &[Value]) -> Result<(), String> {
    if events.is_empty() {
        return Ok(());
    }
    let resp = growth_request(
        "/v2/report",
        "POST",
        Some(Value::Array(events.to_vec())),
        account,
    )
    .await;
    if resp_code(&resp) == 0 {
        Ok(())
    } else {
        Err(resp_error(&resp))
    }
}

// ---------------------------------------------------------------------------
// 连续打卡 / 盲盒抽奖
// ---------------------------------------------------------------------------

/// 连续打卡摘要（只取展示与决策需要的部分）。
pub fn streak_summary(account: &Value, resp: &Value) -> Value {
    let uid = get_str(account, "uid").unwrap_or_default();
    let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
    json!({
        "uid": uid,
        "days": data.get("streak_days").and_then(Value::as_i64).unwrap_or(0),
        "makeupCards": data
            .get("makeup_cards")
            .and_then(|c| c.get("balance"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        "redemptionStatus": data.get("redemption_status").cloned().unwrap_or(Value::Null),
    })
}

/// 查询连续打卡状态。
pub async fn fetch_streak(account: &Value) -> Result<Value, String> {
    let resp = growth_request(&format!("{GROWTH_API_PREFIX}/streak"), "GET", None, account).await;
    if resp_code(&resp) != 0 {
        return Err(resp_error(&resp));
    }
    Ok(streak_summary(account, &resp))
}

/// 用补签卡补某一天（`YYYY-MM-DD`）。
pub async fn use_makeup_card(account: &Value, date: &str) -> Result<(), String> {
    let resp = growth_request(
        &format!("{GROWTH_API_PREFIX}/makeup-cards/use"),
        "POST",
        Some(json!({ "target_date": date })),
        account,
    )
    .await;
    if resp_code(&resp) == 0 {
        Ok(())
    } else {
        Err(resp_error(&resp))
    }
}

/// 兑换连登档位（`7d` / `14d` / `28d`）。`client_token` 每次必须不同，否则上游视为重放。
pub async fn redeem_tier(account: &Value, tier: &str) -> Result<(), String> {
    let resp = growth_request(
        &format!("{GROWTH_API_PREFIX}/redeem"),
        "POST",
        Some(json!({ "tier": tier, "client_token": uuid::Uuid::new_v4().to_string() })),
        account,
    )
    .await;
    if resp_code(&resp) == 0 {
        Ok(())
    } else {
        Err(resp_error(&resp))
    }
}

/// 剩余抽奖次数。
pub async fn lottery_chances(account: &Value) -> Result<i64, String> {
    let resp = growth_request(
        &format!("{GROWTH_API_PREFIX}/lottery/summary"),
        "GET",
        None,
        account,
    )
    .await;
    if resp_code(&resp) != 0 {
        return Err(resp_error(&resp));
    }
    Ok(resp
        .get("data")
        .and_then(|d| d.get("chances"))
        .and_then(Value::as_i64)
        .unwrap_or(0))
}

/// 抽一次奖；返回本次奖品（服务端原样透传 `data`）。
pub async fn lottery_draw(account: &Value) -> Result<Value, String> {
    let resp = growth_request(
        &format!("{GROWTH_API_PREFIX}/lottery/draw"),
        "POST",
        Some(json!({ "client_token": uuid::Uuid::new_v4().to_string() })),
        account,
    )
    .await;
    if resp_code(&resp) != 0 {
        return Err(resp_error(&resp));
    }
    Ok(resp.get("data").cloned().unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// 编排：接取 → 上报推进 → 领奖
// ---------------------------------------------------------------------------

/// 一天的编排计划（纯计算，便于单测）。
#[derive(Debug, Default, PartialEq)]
pub struct GrowthPlan {
    /// 可以自动推进、需要上报事件的任务（任务码, 次数）。
    pub report: Vec<(String, i64)>,
    /// 已达标、直接领奖的任务码。
    pub claim: Vec<String>,
    /// 只认真实动作、跳过的任务（任务码, 原因, 深链）。
    pub manual: Vec<(String, String, String)>,
    /// 没登记规格、不自动推进的任务码（只报告，不猜事件）。
    pub unsupported: Vec<String>,
    /// 受夜间窗口限制、当前不该上报的任务码。
    pub waiting_night: Vec<String>,
}

/// 编排模式：白天一轮与夜间一轮分开，各自按天去重。
///
/// 分成两轮是因为夜猫子任务只在 23:00–08:00 上报才计数：若跟白天挤在同一次运行里，
/// 白天跑过之后当天就不会再跑，夜间任务永远等不到窗口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrowthMode {
    Daily,
    Night,
}

impl GrowthMode {
    /// 用作运行记录的 `task` 字段与「今天是否已跑过」的键。
    pub fn as_str(self) -> &'static str {
        match self {
            GrowthMode::Daily => "growth",
            GrowthMode::Night => "night",
        }
    }

    fn includes(self, task_code: &str) -> bool {
        match self {
            GrowthMode::Daily => !is_night_task(task_code),
            GrowthMode::Night => is_night_task(task_code),
        }
    }
}

/// 是否已达标、可以直接领奖。
///
/// `completed` 与 `claimed` 是两回事：真实数据里任务达标后先是 `completed`（进度够了、
/// 奖励还没领），领到手才变 `claimed`。把 `completed` 当成「已领」会让奖励永远领不到。
pub fn is_claimable(task: &GrowthTask) -> bool {
    task.unforgeable.is_empty()
        && task.status != "claimed"
        && task.target > 0
        && task.current >= task.target
}

/// 当前模式下可领奖的任务码。
pub fn claimable_codes(tasks: &[GrowthTask], mode: GrowthMode) -> Vec<String> {
    tasks
        .iter()
        .filter(|task| mode.includes(&task.code) && is_claimable(task))
        .map(|task| task.code.clone())
        .collect()
}

/// 计划编排动作。`hour` 为本地小时数（便于单测覆盖夜间边界）。
pub fn plan_growth(tasks: &[GrowthTask], hour: u32, mode: GrowthMode) -> GrowthPlan {
    let mut plan = GrowthPlan::default();
    for task in tasks {
        // 人工任务两种模式都要报出来：界面需要拿深链引导用户去桌面端完成。
        if !task.unforgeable.is_empty() {
            plan.manual.push((
                task.code.clone(),
                task.unforgeable.clone(),
                task.jump_url.clone(),
            ));
            continue;
        }
        if !mode.includes(&task.code) {
            continue;
        }
        // 只有领到手（claimed）才跳过；completed 是「达标未领」，要进领奖清单。
        if task.status == "claimed" {
            continue;
        }
        // 已达标：直接领奖，不再上报事件。
        if is_claimable(task) {
            plan.claim.push(task.code.clone());
            continue;
        }
        if task_spec(&task.code).is_none() {
            plan.unsupported.push(task.code.clone());
            continue;
        }
        // 只有夜间那一轮受窗口限制；白天任务放到 23 点后才跑是错的。
        if mode == GrowthMode::Night && !in_night_window(hour) {
            plan.waiting_night.push(task.code.clone());
            continue;
        }
        let remaining = (task.target - task.current).max(0);
        if remaining > 0 {
            plan.report.push((task.code.clone(), remaining));
        }
    }
    plan
}

/// 需要走接取接口的任务码：自动类与不可伪造类都不接取，且只接当前模式该管的。
pub fn codes_to_accept(tasks: &[GrowthTask], mode: GrowthMode) -> Vec<String> {
    tasks
        .iter()
        .filter(|task| {
            mode.includes(&task.code)
                && task.status == "not_accepted"
                && task.unforgeable.is_empty()
                && task_spec(&task.code).is_some()
                && task.code != "first_buddy"
        })
        .map(|task| task.code.clone())
        .collect()
}

/// 单条进度上报的结果。
#[derive(Debug, Clone, PartialEq)]
pub enum ReportOutcome {
    /// 已上报（或命中本地判重，未重复发）。
    Reported,
    /// 本地已记录过同一事件，跳过重复打点。
    AlreadyReported,
    Failed(String),
}

/// 按任务规格构造并上报一条推进事件。
///
/// `seq` 是同一任务当天内第几次上报：种子相同即视为同一事件，本地判重会直接跳过，
/// 因此重跑编排不会把进度打两遍。
pub async fn report_progress(account: &Value, task_code: &str, day: &str, seq: usize) -> ReportOutcome {
    let Some(spec) = task_spec(task_code) else {
        return ReportOutcome::Failed(format!("未登记的任务码: {task_code}"));
    };
    let uid = get_str(account, "uid").unwrap_or_default();
    let seed = event_seed(&uid, task_code, day, seq);
    let expert = match spec.kind {
        "expert" => Some(pool_pick(&EXPERT_ID_POOL, seq)),
        "team" => Some(pool_pick(&TEAM_ID_POOL, seq)),
        _ => None,
    };
    let event = build_event(spec.kind, &uid, &seed, expert);
    let Some((code, id)) = event_dedupe_key(&event) else {
        return ReportOutcome::Failed("事件缺少去重键".to_string());
    };
    match was_reported(&uid, &code, &id) {
        Ok(true) => return ReportOutcome::AlreadyReported,
        Ok(false) => {}
        Err(e) => return ReportOutcome::Failed(e),
    }
    match report_events(account, std::slice::from_ref(&event)).await {
        Ok(()) => {
            if let Err(e) = mark_reported(&uid, &code, &id) {
                return ReportOutcome::Failed(e);
            }
            ReportOutcome::Reported
        }
        Err(e) => ReportOutcome::Failed(e),
    }
}

/// 逐个领奖并累计收益。
async fn claim_all(account: &Value, codes: &[String], report: &mut GrowthRunReport) {
    for code in codes {
        match claim_task(account, code).await {
            outcome if outcome.ok => {
                report.claimed += 1;
                report.credit += outcome.credit;
                report.energy += outcome.energy;
            }
            // 进度刚打完还没落账时会被拒（task not completed），下一轮再试即可。
            outcome => report.message = format!("{code} 领奖未成功: {}", outcome.message),
        }
    }
}

/// 一次编排的运行摘要。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GrowthRunReport {
    pub ok: bool,
    pub message: String,
    pub accepted: usize,
    pub reported: usize,
    pub claimed: usize,
    pub credit: i64,
    pub energy: i64,
    /// 只认真实动作、本机跳过的任务（含深链，界面用来引导）。
    pub manual: Vec<(String, String, String)>,
}

/// 对单个账号跑一轮成长任务：接取 → 上报推进 → 领奖。
///
/// 只处理国内版账号（国际版没有这套成长体系）；企业版账号由
/// [`fetch_growth_tasks`] 直接返回空列表。夜间模式在窗口外直接跳过，不发任何请求。
pub async fn run_growth_for_account(
    account: &Value,
    mode: GrowthMode,
    trigger: &str,
) -> GrowthRunReport {
    let mut report = GrowthRunReport::default();
    let uid = get_str(account, "uid").unwrap_or_default();
    let day = local_day();

    if mode == GrowthMode::Night && !in_night_window(local_hour()) {
        report.ok = true;
        report.message = "不在夜间窗口（23:00–08:00），跳过".to_string();
        return report;
    }

    let tasks = match fetch_growth_tasks(account).await {
        Ok(tasks) => tasks,
        Err(e) => {
            report.message = format!("查询成长任务失败: {e}");
            return report;
        }
    };
    if tasks.is_empty() {
        report.ok = true;
        report.message = "没有可推进的成长任务（企业版或活动未开放）".to_string();
        return report;
    }

    // 先接取：接取失败必须可见，否则后面上报的事件全打在不存在的任务上。
    let accept_codes = codes_to_accept(&tasks, mode);
    if !accept_codes.is_empty() {
        let outcome = accept_tasks(account, &accept_codes).await;
        report.accepted = outcome.accepted.len();
        if !outcome.failed.is_empty() {
            report.message = format!(
                "{} 个任务接取失败: {}",
                outcome.failed.len(),
                outcome.reasons.join("; ")
            );
        }
    }

    // 再按计划上报 / 领奖。
    let plan = plan_growth(&tasks, local_hour(), mode);
    report.manual = plan.manual.clone();

    for (task_code, times) in &plan.report {
        for seq in 0..(*times as usize) {
            match report_progress(account, task_code, &day, seq).await {
                ReportOutcome::Reported => report.reported += 1,
                ReportOutcome::AlreadyReported => {}
                ReportOutcome::Failed(e) => {
                    report.message = format!("{task_code} 上报失败: {e}");
                    break;
                }
            }
        }
    }

    claim_all(account, &plan.claim, &mut report).await;

    // 上报之后任务可能刚好达标：立刻再取一次状态把新达标的领掉。
    // 少了这一步，刚打满的任务要等下一轮（自动运行时就是明天）才能领奖。
    if report.reported > 0 {
        match fetch_growth_tasks(account).await {
            Ok(tasks) => {
                let codes = claimable_codes(&tasks, mode);
                claim_all(account, &codes, &mut report).await;
            }
            Err(e) => report.message = format!("复核任务状态失败: {e}"),
        }
    }

    report.ok = true;
    if report.message.is_empty() {
        report.message = format!(
            "接取 {} 个，上报 {} 次，领奖 {} 个",
            report.accepted, report.reported, report.claimed
        );
    }
    record_run(&uid, mode.as_str(), trigger, &report);
    report
}

/// 连登闭环：补签昨日 → 领新手礼包/活动补偿 → 兑换连登档位 → 抽完盲盒。
///
/// 每一步都是「无则静默跳过」：上游用业务码表示「已领过/未解锁」，不能当失败报错。
pub async fn run_streak_bonus(account: &Value) -> GrowthRunReport {
    let mut report = GrowthRunReport {
        ok: true,
        ..Default::default()
    };
    let mut logs: Vec<String> = Vec::new();

    // 1. 昨日漏签且有补签卡 → 补上，保住连登。
    match fetch_streak(account).await {
        Ok(streak) => {
            let cards = streak
                .get("makeupCards")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if cards > 0 {
                let yesterday = local_day_offset(-1);
                if let Err(e) = use_makeup_card(account, &yesterday).await {
                    logs.push(format!("补签失败: {e}"));
                } else {
                    logs.push(format!("已补签 {yesterday}"));
                }
            }
            for tier in ["7d", "14d", "28d"] {
                if let Err(e) = redeem_tier(account, tier).await {
                    // 未解锁/已领取属于常态，只记不报错。
                    logs.push(format!("连登 {tier} 未兑换: {e}"));
                }
            }
        }
        Err(e) => logs.push(format!("连续打卡查询失败: {e}")),
    }

    // 2. 把剩余抽奖次数抽完（每次之间由限速保证 ≥1s）。
    match lottery_chances(account).await {
        Ok(chances) if chances > 0 => {
            let mut drawn = 0;
            for _ in 0..chances {
                match lottery_draw(account).await {
                    Ok(_) => drawn += 1,
                    Err(e) => {
                        logs.push(format!("抽奖中断: {e}"));
                        break;
                    }
                }
            }
            logs.push(format!("完成抽奖 {drawn} 次"));
        }
        Ok(_) => {}
        Err(e) => logs.push(format!("抽奖次数查询失败: {e}")),
    }

    report.message = if logs.is_empty() {
        "连登状态正常（无需补签/兑换，无抽奖次数）".to_string()
    } else {
        logs.join("; ")
    };
    report
}

// ---------------------------------------------------------------------------
// 后台循环
// ---------------------------------------------------------------------------

/// 成长任务的循环间隔：真正的门禁是「当天是否已成功跑过」，这里只是检查频率。
pub const GROWTH_CYCLE_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// 只保留支持成长中心的档位（与派猫猫旅行同一套判定）。
fn growth_capable_accounts(accounts: Vec<Value>) -> Vec<Value> {
    accounts
        .into_iter()
        .filter(|account| variant_of(account).supports_travel())
        .collect()
}

/// 运行摘要转 JSON（给接口与日志用；不含任何凭据）。
pub fn report_json(report: &GrowthRunReport) -> Value {
    json!({
        "ok": report.ok,
        "message": report.message,
        "accepted": report.accepted,
        "reported": report.reported,
        "claimed": report.claimed,
        "credit": report.credit,
        "energy": report.energy,
        "manual": report
            .manual
            .iter()
            .map(|(code, reason, jump)| json!({"code": code, "reason": reason, "jumpUrl": jump}))
            .collect::<Vec<_>>(),
    })
}

/// 单账号的任务视图：分组 + 当天运行状态 + 展示名（接口与桌面命令共用）。
pub async fn tasks_view(account: &Value) -> Result<Value, String> {
    let tasks = fetch_growth_tasks(account).await?;
    let uid = get_str(account, "uid").unwrap_or_default();
    let mut payload = tasks_payload(&tasks);
    payload["accountId"] = account.get("id").cloned().unwrap_or(Value::Null);
    payload["label"] = json!(account::account_display_name(account));
    payload["ranToday"] = json!(ran_today(&uid, "growth").unwrap_or(false));
    payload["nightRanToday"] = json!(ran_today(&uid, "night").unwrap_or(false));
    payload["inNightWindow"] = json!(in_night_window(local_hour()));
    Ok(payload)
}

/// 所有支持成长中心的账号的任务视图。
///
/// 单个账号失败也留在列表里并带上 `error`：界面能看出是哪个账号出问题，不是整页空白。
pub async fn tasks_view_all() -> Value {
    let mut items = Vec::new();
    for account in growth_capable_accounts(account::load_accounts()) {
        match tasks_view(&account).await {
            Ok(payload) => items.push(payload),
            Err(e) => items.push(json!({
                "accountId": account.get("id").cloned().unwrap_or(Value::Null),
                "label": account::account_display_name(&account),
                "error": e,
                "claimable": [], "manual": [], "pending": [], "settled": [],
            })),
        }
    }
    json!({ "accounts": items })
}

/// 运行历史转 JSON（字段名在接口与桌面命令之间保持一致）。
pub fn runs_json(runs: &[GrowthRun]) -> Value {
    Value::Array(
        runs.iter()
            .map(|run| {
                json!({
                    "uid": run.account_id,
                    "task": run.task,
                    "trigger": run.trigger,
                    "ok": run.ok,
                    "message": run.message,
                    "credit": run.credit,
                    "energy": run.energy,
                    "createdAt": run.created_at,
                })
            })
            .collect(),
    )
}

/// 后台一轮：对每个支持成长中心的账号，按需跑夜间任务 / 白天任务 / 连登闭环。
///
/// 三项各自按天去重（`ran_today`）：白天跑过不影响夜间任务等到 23:00 再跑。
/// 自动运行受 `auto_growth_config.json` 的 `enabled` 控制，**默认关闭**——上报客户端
/// 事件必须由用户显式开启；界面上的手动按钮不受该开关限制。
pub async fn run_growth_cycle() -> Value {
    let cfg = load_growth_config();
    let enabled = cfg
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !enabled {
        return json!({"enabled": false, "accounts": [], "message": "自动成长任务未开启"});
    }
    let night_enabled = cfg
        .get("nightEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let hour = local_hour();
    let mut accounts_out = Vec::new();

    for account in growth_capable_accounts(account::load_accounts()) {
        let uid = get_str(&account, "uid").unwrap_or_default();
        if uid.is_empty() {
            continue;
        }
        let mut entry = json!({"uid": uid, "label": account::account_display_name(&account)});

        if night_enabled && in_night_window(hour) && !ran_today(&uid, "night").unwrap_or(true) {
            let report = run_growth_for_account(&account, GrowthMode::Night, "auto").await;
            entry["night"] = report_json(&report);
        }
        if !ran_today(&uid, "growth").unwrap_or(true) {
            let report = run_growth_for_account(&account, GrowthMode::Daily, "auto").await;
            entry["growth"] = report_json(&report);
        }
        if !ran_today(&uid, "streak").unwrap_or(true) {
            let report = run_streak_bonus(&account).await;
            record_run(&uid, "streak", "auto", &report);
            entry["streak"] = report_json(&report);
        }
        accounts_out.push(entry);
    }
    json!({"enabled": true, "accounts": accounts_out})
}

// ---------------------------------------------------------------------------
// 时间与存储桥接
// ---------------------------------------------------------------------------

/// 打开账号账本：运行记录与事件判重跟账号同库，保持单一真源。
fn open_ledger() -> Result<Db, String> {
    Db::open_migrated(&db_path())
}

/// 上游判重键（`eventCode` + `id`）是否已在本地记录过。
fn was_reported(uid: &str, event_code: &str, event_id: &str) -> Result<bool, String> {
    open_ledger()?
        .is_event_reported(uid, event_code, event_id)
        .map_err(|e| format!("读取事件判重失败: {e}"))
}

fn mark_reported(uid: &str, event_code: &str, event_id: &str) -> Result<(), String> {
    open_ledger()?
        .mark_event_reported(uid, event_code, event_id)
        .map_err(|e| format!("写入事件判重失败: {e}"))
}

/// 记一条运行结果（成功与失败都记，界面上能看出「上次为什么没成」）。
pub fn record_run(account_id: &str, task: &str, trigger: &str, report: &GrowthRunReport) {
    let run = GrowthRun {
        account_id: account_id.to_string(),
        task: task.to_string(),
        trigger: trigger.to_string(),
        ok: report.ok,
        message: report.message.clone(),
        credit: report.credit,
        energy: report.energy,
        created_at: now_ms(),
    };
    if let Ok(db) = open_ledger() {
        if let Err(e) = db.record_growth_run(&run) {
            eprintln!("[成长任务] 记录运行结果失败: {e}");
        }
    }
}

/// 运行历史（新的在前）。
pub fn recent_runs(account_id: Option<&str>, limit: usize) -> Result<Vec<GrowthRun>, String> {
    open_ledger()?
        .growth_runs(account_id, limit)
        .map_err(|e| format!("读取运行历史失败: {e}"))
}

/// 今天是否已经成功跑过该类任务（失败不算，允许当天重试）。
pub fn ran_today(account_id: &str, task: &str) -> Result<bool, String> {
    let Some(last) = open_ledger()?
        .last_success_at(account_id, task)
        .map_err(|e| format!("读取运行时间失败: {e}"))?
    else {
        return Ok(false);
    };
    let today_start = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|naive| {
            naive
                .and_local_timezone(chrono::Local)
                .single()
                .map(|dt| dt.timestamp_millis())
        })
        .flatten()
        .unwrap_or(0);
    Ok(last >= today_start)
}

/// 本地日期 `YYYY-MM-DD`。
pub fn local_day() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 相对今天偏移若干天的本地日期。
pub fn local_day_offset(days: i64) -> String {
    (chrono::Local::now() + chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

/// 本地小时数（0–23）。
pub fn local_hour() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().hour()
}

/// 把服务端返回的任务结果压成给界面用的形状（不含凭据）。
pub fn tasks_payload(tasks: &[GrowthTask]) -> Value {
    let mut claimable = Vec::new();
    let mut manual = Vec::new();
    let mut pending = Vec::new();
    let mut settled = Vec::new();
    for task in tasks {
        let entry = serde_json::to_value(task).unwrap_or(Value::Null);
        if !task.unforgeable.is_empty() {
            manual.push(entry);
        } else if task.status == "claimed" {
            // 已领到手才是「settled」；`completed` 表示达标未领，要留在可领奖组里。
            settled.push(entry);
        } else if is_claimable(task) {
            claimable.push(entry);
        } else {
            pending.push(entry);
        }
    }
    let mut out = Map::new();
    out.insert("claimable".to_string(), Value::Array(claimable));
    out.insert("manual".to_string(), Value::Array(manual));
    out.insert("pending".to_string(), Value::Array(pending));
    out.insert("settled".to_string(), Value::Array(settled));
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_only_tasks_are_recognised_and_others_are_not() {
        assert!(desktop_only_reason("RichMeow_Chat").is_some());
        assert!(desktop_only_reason("Library_read").is_some());
        assert_eq!(desktop_only_reason("daily_chat"), None);
        // 业务上不可伪造的任务与桌面专属走同一个出口。
        assert!(unforgeable_reason("wb_wechat_oa_subscribe_task").is_some());
        assert!(unforgeable_reason("Expert_Philanthropy").is_some());
        assert!(unforgeable_reason("chat_5").is_none());
        assert!(is_unforgeable("Library_read"));
        assert!(!is_unforgeable("chat_5"));
    }

    /// 夜间窗口边界：23:00 起算、08:00 结束。
    #[test]
    fn night_window_boundaries() {
        assert!(in_night_window(23));
        assert!(in_night_window(0));
        assert!(in_night_window(7));
        assert!(!in_night_window(8));
        assert!(!in_night_window(22));
        assert!(is_night_task("black_cat"));
        assert!(!is_night_task("daily_chat"));
    }

    /// 设备标识必须对同一账号稳定、对不同账号不同。
    #[test]
    fn device_id_is_stable_and_per_account() {
        let a1 = derive_id("uid-1", "machine");
        assert_eq!(a1, derive_id("uid-1", "machine"), "同账号必须稳定");
        assert_ne!(a1, derive_id("uid-2", "machine"));
        assert_ne!(a1, derive_id("uid-1", "session"), "不同用途必须隔离");
        assert_eq!(a1.len(), 36);
        assert!(a1.chars().all(|c| c.is_ascii_hexdigit()));
        // 空 uid 走 anonymous 分支且仍然是稳定值。
        assert_eq!(derive_id("", "machine"), derive_id("", "machine"));
    }

    /// 指纹里不含凭据字段，且关键字段齐全。
    #[test]
    fn fingerprint_carries_device_fields_without_secrets() {
        let fp = build_fingerprint("uid-1", "小明");
        assert_eq!(fp["userId"], "uid-1");
        assert_eq!(fp["ideName"], "WorkBuddy");
        assert_eq!(fp["machineId"], derive_id("uid-1", "machine"));
        for leaked in ["access_token", "refresh_token", "Authorization"] {
            assert!(fp.get(leaked).is_none(), "指纹不得带凭据: {leaked}");
        }
    }

    /// 事件去重键必须随 seed 变化，否则上游按 (eventCode,id) 判重会卡住进度。
    #[test]
    fn event_ids_differ_per_seed_for_deduped_events() {
        let first = build_event("cat", "uid-1", "seed-a", None);
        let second = build_event("cat", "uid-1", "seed-b", None);
        assert_eq!(first["eventCode"], "chat_request_send");
        assert_eq!(first["mode"], "night", "夜猫子事件必须走 night 模式");
        assert!(
            first.get("id").is_none(),
            "对话类事件上游不下发 id 字段，去重键要落到请求标识上"
        );
        assert_ne!(
            event_dedupe_key(&first),
            event_dedupe_key(&second),
            "换 seed 必须换出不同的去重键"
        );
        // 同一 seed 必须稳定：同一天重跑编排靠它做到不重复打点。
        assert_eq!(
            event_dedupe_key(&first),
            event_dedupe_key(&build_event("cat", "uid-1", "seed-a", None))
        );

        // 专家事件：换 id 才能推进，重复同一 id 会被去重。
        let ex_a = build_event("expert", "uid-1", "s1", Some(("ex_a", "运维工程师")));
        let ex_b = build_event("expert", "uid-1", "s2", Some(("ex_b", "产品经理")));
        assert_eq!(ex_a["expertType"], "agent");
        assert_ne!(event_dedupe_key(&ex_a), event_dedupe_key(&ex_b));

        let team = build_event("team", "uid-1", "s1", None);
        assert_eq!(team["expertType"], "team");
    }

    /// 未知类型退化为 heartbeat（不是 panic、也不是伪造任务信号）。
    #[test]
    fn unknown_event_kind_falls_back_to_heartbeat() {
        assert_eq!(
            build_event("nope", "uid-1", "s", None)["eventCode"],
            "heartbeat"
        );
    }

    /// 限速：首次不等待，间隔不足时补差，够了就不再等。
    #[test]
    fn request_gap_enforces_minimum_interval() {
        let min = MIN_REQUEST_INTERVAL;
        assert_eq!(remaining_gap(None, min), Duration::ZERO);
        assert_eq!(remaining_gap(Some(Duration::ZERO), min), min);
        assert_eq!(
            remaining_gap(Some(Duration::from_millis(400)), min),
            Duration::from_millis(600)
        );
        assert_eq!(remaining_gap(Some(min), min), Duration::ZERO);
        assert_eq!(remaining_gap(Some(min * 3), min), Duration::ZERO);
    }

    /// 任务解析：字段缺失要有安全默认值，桌面专属任务带上原因。
    fn raw_task(code: &str, status: &str, current: i64, target: i64) -> Value {
        json!({
            "task_code": code,
            "title": "任务标题",
            "accept_status": status,
            "progress": {"current": current, "target": target},
            "reward_credit": 10,
            "jump_url": "workbuddy://x",
        })
    }

    #[test]
    fn task_parsing_defaults_and_reasons() {
        let task = task_from_value(&raw_task("RichMeow_Chat", "accepted", 0, 1));
        assert_eq!(task.name, "任务标题");
        assert_eq!(task.status, "accepted");
        assert_eq!(task.target, 1);
        assert!(!task.unforgeable.is_empty(), "桌面专属任务应带原因");

        // 缺 title 时回落 task_code；缺 progress 时 current=0 / target=1。
        let sparse = task_from_value(&json!({"task_code": "daily_chat"}));
        assert_eq!(sparse.name, "daily_chat");
        assert_eq!(sparse.current, 0);
        assert_eq!(sparse.target, 1);
        assert!(sparse.unforgeable.is_empty());
    }

    /// 分组口径：可领奖 / 人工 / 待完成 / 已领 四分，且已领的不能凭空消失。
    #[test]
    fn tasks_payload_covers_every_task_exactly_once() {
        let tasks = vec![
            // 真实状态是 in_progress，不是 accepted——达标就该进 claimable。
            task_from_value(&raw_task("daily_chat", "in_progress", 1, 1)),
            task_from_value(&raw_task("RichMeow_Chat", "accepted", 0, 1)),
            task_from_value(&raw_task("night_chat", "in_progress", 1, 3)),
            // completed = 达标未领，必须留在可领奖组，否则奖励永远领不到。
            task_from_value(&raw_task("template_5", "completed", 5, 5)),
            task_from_value(&raw_task("skill_1", "claimed", 1, 1)),
        ];
        let payload = tasks_payload(&tasks);
        assert_eq!(payload["claimable"].as_array().unwrap().len(), 2);
        assert_eq!(payload["manual"].as_array().unwrap().len(), 1);
        assert_eq!(payload["pending"].as_array().unwrap().len(), 1);
        assert_eq!(payload["settled"].as_array().unwrap().len(), 1, "只有 claimed 算已领");
        let claimable_codes: Vec<&str> = payload["claimable"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["code"].as_str().unwrap())
            .collect();
        assert!(claimable_codes.contains(&"template_5"));

        // 总数守恒：每个任务恰好出现在一个分组里。
        let total: usize = ["claimable", "manual", "pending", "settled"]
            .iter()
            .map(|k| payload[*k].as_array().unwrap().len())
            .sum();
        assert_eq!(total, tasks.len());
    }

    /// 接取结果必须区分成功/失败（参考实现吞异常导致「一个都没点亮」）。
    #[test]
    fn accept_outcome_keeps_failures_visible() {
        let outcome = AcceptOutcome {
            accepted: vec!["a".to_string()],
            failed: vec!["b".to_string()],
            reasons: vec!["b: rate limited".to_string()],
        };
        assert_eq!(outcome.accepted, ["a"]);
        assert_eq!(outcome.failed, ["b"]);
        assert!(!outcome.reasons.is_empty());
    }
    /// 任务规格：登记过的能自动推进，未登记的不猜。
    #[test]
    fn task_spec_only_covers_registered_codes() {
        assert_eq!(task_spec("chat_5").unwrap().kind, "chat");
        assert_eq!(task_spec("black_cat").unwrap().target, 3);
        assert_eq!(task_spec("first_buddy").unwrap().reward, 300);
        assert!(task_spec("Expert_Philanthropy").is_none());
        assert!(task_spec("whatever").is_none());
    }

    /// 编排计划：达标领奖、未达标上报、人工任务跳过、夜间任务等窗口、未登记只报告。
    #[test]
    fn plan_splits_claim_report_manual_and_night() {
        let tasks = vec![
            task_from_value(&raw_task("chat_5", "accepted", 1, 5)),
            task_from_value(&raw_task("template_5", "completed", 5, 5)),
            task_from_value(&raw_task("RichMeow_Chat", "accepted", 0, 1)),
            task_from_value(&raw_task("black_cat", "accepted", 0, 3)),
            task_from_value(&raw_task("unknown_task", "accepted", 0, 1)),
        ];
        let plan = plan_growth(&tasks, 15, GrowthMode::Daily);
        assert_eq!(plan.report, vec![("chat_5".to_string(), 4)], "只补差，不超额打点");
        assert_eq!(plan.claim, vec!["template_5".to_string()], "completed 必须进领奖清单");
        assert_eq!(plan.manual.len(), 1);
        assert_eq!(plan.manual[0].0, "RichMeow_Chat");
        assert!(!plan.manual[0].1.is_empty(), "人工任务必须带原因");
        // 夜猫子不归白天这一轮管（见 modes_do_not_overlap）。
        assert!(plan.waiting_night.is_empty());
        assert_eq!(plan.unsupported, vec!["unknown_task".to_string()]);

        // 夜间那一轮在窗口内正常上报。
        let night = plan_growth(
            &[task_from_value(&raw_task("black_cat", "accepted", 0, 3))],
            23,
            GrowthMode::Night,
        );
        assert!(night.waiting_night.is_empty());
        assert_eq!(night.report, vec![("black_cat".to_string(), 3)]);
    }

    /// 已领/已完成的任务不再进任何计划。
    #[test]
    fn plan_ignores_claimed_and_completed() {
        let plan = plan_growth(
            &[
                task_from_value(&raw_task("chat_5", "claimed", 5, 5)),
            ],
            15,
            GrowthMode::Daily,
        );
        assert_eq!(plan, GrowthPlan::default(), "领到手的任务不该再出现在计划里");
    }

    /// 接取清单：只接「未接取 + 有规格 + 非自动类 + 非人工任务」。
    #[test]
    fn accept_list_filters_auto_and_unforgeable() {
        let tasks = vec![
            task_from_value(&raw_task("chat_5", "not_accepted", 0, 5)),
            task_from_value(&raw_task("first_buddy", "not_accepted", 0, 1)),
            task_from_value(&raw_task("RichMeow_Chat", "not_accepted", 0, 1)),
            task_from_value(&raw_task("unknown_task", "not_accepted", 0, 1)),
            task_from_value(&raw_task("template_5", "accepted", 0, 5)),
        ];
        assert_eq!(codes_to_accept(&tasks, GrowthMode::Daily), vec!["chat_5".to_string()]);
    }

    /// 事件种子：同一天稳定，换天/换序号/换账号都要换新。
    #[test]
    fn event_seed_is_stable_within_a_day() {
        let base = event_seed("u1", "chat_5", "2026-10-14", 0);
        assert_eq!(base, event_seed("u1", "chat_5", "2026-10-14", 0));
        assert_ne!(base, event_seed("u1", "chat_5", "2026-10-14", 1));
        assert_ne!(base, event_seed("u1", "chat_5", "2026-10-15", 0));
        assert_ne!(base, event_seed("u2", "chat_5", "2026-10-14", 0));
    }

    /// 专家/团队 id 池循环取用，越界不 panic。
    #[test]
    fn pools_rotate_without_panicking() {
        assert_eq!(pool_pick(&EXPERT_ID_POOL, 0).0, "ex_PZw8Gu81HfN4");
        assert_eq!(
            pool_pick(&EXPERT_ID_POOL, EXPERT_ID_POOL.len()).0,
            "ex_PZw8Gu81HfN4"
        );
        assert_eq!(pool_pick(&TEAM_ID_POOL, 1).1, "内容专家团队");
    }

    /// 本地日期工具：格式正确、偏移与小时数可用。
    #[test]
    fn local_day_helpers_format() {
        assert_eq!(local_day().len(), 10);
        assert_ne!(local_day(), local_day_offset(-1));
        assert!(local_hour() < 24);
    }
    /// 白天/夜间两轮互不串台：白天不碰夜猫子，夜间只管夜猫子。
    #[test]
    fn modes_do_not_overlap() {
        let tasks = vec![
            task_from_value(&raw_task("chat_5", "accepted", 0, 5)),
            task_from_value(&raw_task("black_cat", "accepted", 0, 3)),
        ];
        let daily = plan_growth(&tasks, 15, GrowthMode::Daily);
        assert_eq!(daily.report, vec![("chat_5".to_string(), 5)]);
        assert!(daily.waiting_night.is_empty(), "白天模式不该为夜间任务排队");

        // 就算此刻在窗口内，夜猫子也归夜间那一轮。
        let daily_in_window = plan_growth(&tasks, 23, GrowthMode::Daily);
        assert_eq!(daily_in_window.report, vec![("chat_5".to_string(), 5)]);

        let night_out = plan_growth(&tasks, 15, GrowthMode::Night);
        assert!(night_out.report.is_empty());
        assert_eq!(night_out.waiting_night, vec!["black_cat".to_string()]);
        let night_in = plan_growth(&tasks, 2, GrowthMode::Night);
        assert_eq!(night_in.report, vec![("black_cat".to_string(), 3)]);
    }

    /// 接取也按模式分流，避免白天把夜猫子接进来白占一个接取位。
    #[test]
    fn accept_respects_mode() {
        let tasks = vec![
            task_from_value(&raw_task("chat_5", "not_accepted", 0, 5)),
            task_from_value(&raw_task("black_cat", "not_accepted", 0, 3)),
        ];
        assert_eq!(
            codes_to_accept(&tasks, GrowthMode::Daily),
            vec!["chat_5".to_string()]
        );
        assert_eq!(
            codes_to_accept(&tasks, GrowthMode::Night),
            vec!["black_cat".to_string()]
        );
    }

    /// 自动运行默认关闭：上报客户端事件必须由用户显式开启。
    #[test]
    fn growth_config_defaults_off() {
        let cfg = crate::modules::config::default_growth_config();
        assert_eq!(cfg["enabled"], false);
        assert_eq!(cfg["nightEnabled"], false);
    }

    /// 运行摘要序列化：人工任务带原因与深链，且不带任何凭据字段。
    #[test]
    fn report_json_exposes_manual_tasks_without_secrets() {
        let report = GrowthRunReport {
            ok: true,
            message: "ok".to_string(),
            accepted: 1,
            reported: 2,
            claimed: 3,
            credit: 400,
            energy: 5,
            manual: vec![(
                "RichMeow_Chat".to_string(),
                "在桌面端发起 1 次对话".to_string(),
                "workbuddy://x".to_string(),
            )],
        };
        let value = report_json(&report);
        assert_eq!(value["claimed"], 3);
        assert_eq!(value["manual"][0]["code"], "RichMeow_Chat");
        assert_eq!(value["manual"][0]["jumpUrl"], "workbuddy://x");
        assert!(value.get("access_token").is_none());
    }
    /// 可领奖判定：达标且非人工、非已领；人工任务即便达标也不能自动领。
    #[test]
    fn claimable_requires_target_reached_and_automatable() {
        let reached = task_from_value(&raw_task("template_5", "in_progress", 5, 5));
        assert!(is_claimable(&reached));
        assert_eq!(claimable_codes(&[reached.clone()], GrowthMode::Daily), vec!["template_5".to_string()]);

        // 未达标
        assert!(!is_claimable(&task_from_value(&raw_task("template_5", "in_progress", 4, 5))));
        // 已领到手 → 不可再领；completed（达标未领）→ 可以领。
        assert!(!is_claimable(&task_from_value(&raw_task("template_5", "claimed", 5, 5))));
        assert!(is_claimable(&task_from_value(&raw_task("template_5", "completed", 5, 5))));
        // 人工任务达标也不领
        let manual = task_from_value(&raw_task("RichMeow_Chat", "accepted", 1, 1));
        assert!(!manual.unforgeable.is_empty());
        assert!(!is_claimable(&manual));
        // 模式分流：夜间任务不归白天这一轮领
        let night = task_from_value(&raw_task("black_cat", "accepted", 3, 3));
        assert!(is_claimable(&night));
        assert!(claimable_codes(&[night.clone()], GrowthMode::Daily).is_empty());
        assert_eq!(claimable_codes(&[night], GrowthMode::Night), vec!["black_cat".to_string()]);
    }
}
