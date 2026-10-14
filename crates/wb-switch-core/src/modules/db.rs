//! SQLite 持久层：AiAccountCenter 的版本化数据真源。
//!
//! 账号账本（`accounts` + `credentials`）已是唯一真源，不再读写 `accounts.json`。
//! 其余表在对应阶段按需迁移追加（见 SECURITY_AND_DATA.md §2）。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::modules::config::{now_ms, store_dir};

/// 当前 schema 版本（`PRAGMA user_version`）。
const SCHEMA_VERSION: i32 = 3;

/// 数据库文件在数据根下的固定名。
pub fn db_path_in(store_root: &Path) -> PathBuf {
    store_root.join("app.db")
}

pub fn db_path() -> PathBuf {
    db_path_in(&store_dir())
}

/// 账号行：身份与显示列供查询/唯一约束，其余字段整体存 JSON（`data`）。
///
/// `uid` 可为空：历史记录里存在缺 UID 的账号，空 UID 不能互相合并，所以存 NULL 而
/// 不是空串——SQLite 的 UNIQUE 认为 NULL 彼此不同（SECURITY_AND_DATA.md §2）。
pub struct AccountRow {
    pub id: String,
    pub platform: String,
    pub region: String,
    pub uid: Option<String>,
    pub display: String,
    pub data: String,
}

/// 一次成长任务编排的运行记录。
pub struct GrowthRun {
    pub account_id: String,
    /// `growth` / `night` / `streak` 等任务类别。
    pub task: String,
    /// `auto` / `manual`。
    pub trigger: String,
    pub ok: bool,
    pub message: String,
    pub credit: i64,
    pub energy: i64,
    pub created_at: i64,
}

/// 凭据行：`nonce` 与 `ciphertext` 分离，密文由 [`crate::modules::crypto`] 生成。
pub struct CredentialRow {
    pub account_id: String,
    pub purpose: String,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

pub struct Db {
    /// 仅限本 crate：跨模块测试需要在账本上构造损坏/移位等故障场景。
    pub(crate) conn: Connection,
}

impl Db {
    /// 打开指定路径的数据库（启用 WAL、外键与忙等超时）。
    ///
    /// `busy_timeout` 让后台任务与 Web 服务各自的连接在写锁竞争时等待而不是立即
    /// 返回 `SQLITE_BUSY`（同一数据目录只有一个业务实例，见 §6 单实例租约）。
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(Db { conn })
    }

    /// 打开已存在或新建的库并迁移到当前版本（必要时创建父目录）。
    pub fn open_migrated(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建数据目录失败: {e}"))?;
        }
        let mut db = Self::open(path).map_err(|e| format!("打开数据库失败: {e}"))?;
        db.migrate().map_err(|e| format!("数据库迁移失败: {e}"))?;
        Ok(db)
    }

    /// 打开默认数据真源。
    pub fn open_default() -> Result<Self, String> {
        Self::open_migrated(&db_path())
    }

    /// 版本化迁移：每个版本在独立事务内执行；失败回滚、`user_version` 不提升，
    /// 下次启动重试。DDL 一律幂等（`IF NOT EXISTS`）。
    pub fn migrate(&mut self) -> rusqlite::Result<()> {
        let current: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let mut v = current;
        while v < SCHEMA_VERSION {
            {
                let tx = self.conn.transaction()?;
                match v {
                    0 => tx.execute_batch(SCHEMA_V1)?,
                    1 => tx.execute_batch(SCHEMA_V2)?,
                    2 => tx.execute_batch(SCHEMA_V3)?,
                    _ => unreachable!("未知 schema 版本 {v}"),
                }
                tx.commit()?;
            }
            // 提交成功后再提升版本：迁移失败时版本保持旧值，可重试。
            self.conn.pragma_update(None, "user_version", v + 1)?;
            v += 1;
        }
        Ok(())
    }

    // ---- 管理登录 ----

    pub fn set_admin_password(&self, hash: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO admin(id, password_hash, created_at) VALUES(1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET password_hash = excluded.password_hash",
            params![hash, now_ms()],
        )?;
        Ok(())
    }

    pub fn admin_password_hash(&self) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT password_hash FROM admin WHERE id = 1", [], |r| r.get(0))
            .optional()
    }

    // ---- 管理会话 ----

    pub fn create_session(&self, token_hash: &str, expires_at: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO admin_sessions(token_hash, created_at, expires_at) VALUES(?1, ?2, ?3)",
            params![token_hash, now_ms(), expires_at],
        )?;
        Ok(())
    }

    pub fn session_valid(&self, token_hash: &str) -> rusqlite::Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM admin_sessions
             WHERE token_hash = ?1 AND revoked = 0 AND expires_at > ?2",
            params![token_hash, now_ms()],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn revoke_session(&self, token_hash: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE admin_sessions SET revoked = 1 WHERE token_hash = ?1",
            params![token_hash],
        )?;
        Ok(())
    }

    /// 撤销全部管理会话：清空过期记录并作废所有现存令牌（改密码后使用）。
    pub fn revoke_all_sessions(&self) -> rusqlite::Result<()> {
        self.conn.execute("UPDATE admin_sessions SET revoked = 1", [])?;
        Ok(())
    }

    /// 清理过期且已撤销的会话记录，避免表无限增长。
    pub fn purge_expired_sessions(&self) -> rusqlite::Result<usize> {
        self.conn.execute(
            "DELETE FROM admin_sessions WHERE expires_at < ?1",
            params![now_ms()],
        )
    }

    // ---- 单实例租约 ----

    /// 尝试获取实例租约：无租约或租约已过期或属于本 owner 时成功并续期；
    /// 被其他未过期实例持有时返回 false。用 `BEGIN IMMEDIATE` 串行化读写。
    pub fn acquire_instance_lease(&mut self, owner: &str, ttl_ms: i64) -> rusqlite::Result<bool> {
        let now = now_ms();
        let expires = now + ttl_ms;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, i64)> = tx
            .query_row(
                "SELECT owner, expires_at FROM instance_lease WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let acquired = match existing {
            Some((o, exp)) if o != owner && exp > now => false,
            _ => {
                tx.execute(
                    "INSERT INTO instance_lease(id, owner, expires_at, heartbeat_at)
                     VALUES(1, ?1, ?2, ?2)
                     ON CONFLICT(id) DO UPDATE SET
                       owner = excluded.owner,
                       expires_at = excluded.expires_at,
                       heartbeat_at = excluded.heartbeat_at",
                    params![owner, expires],
                )?;
                true
            }
        };
        tx.commit()?;
        Ok(acquired)
    }

    /// 续期本 owner 的租约；返回是否仍持有（租约可能已被接管）。
    pub fn renew_instance_lease(&self, owner: &str, ttl_ms: i64) -> rusqlite::Result<bool> {
        let expires = now_ms() + ttl_ms;
        let n = self.conn.execute(
            "UPDATE instance_lease SET expires_at = ?1, heartbeat_at = ?1
             WHERE id = 1 AND owner = ?2",
            params![expires, owner],
        )?;
        Ok(n > 0)
    }

    /// 当前持有未过期租约的 owner（没有则 None）。
    ///
    /// 供离线操作（如恢复账本）判断「是否还有实例在用这个数据目录」。
    pub fn active_instance_owner(&self) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT owner FROM instance_lease WHERE id = 1 AND expires_at > ?1",
                params![now_ms()],
                |r| r.get(0),
            )
            .optional()
    }

    /// 释放本 owner 的租约（优雅退出）。
    pub fn release_instance_lease(&self, owner: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM instance_lease WHERE id = 1 AND owner = ?1",
            params![owner],
        )?;
        Ok(())
    }

    // ---- 账号账本 ----

    pub fn account_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))
    }

    pub fn read_accounts(&self) -> rusqlite::Result<Vec<AccountRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, platform, region, uid, display, data FROM accounts ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AccountRow {
                id: r.get(0)?,
                platform: r.get(1)?,
                region: r.get(2)?,
                uid: r.get(3)?,
                display: r.get(4)?,
                data: r.get(5)?,
            })
        })?;
        rows.collect()
    }

    pub fn read_credentials(&self, account_id: &str) -> rusqlite::Result<Vec<CredentialRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, purpose, nonce, ciphertext FROM credentials
             WHERE account_id = ?1 AND status = 'active'",
        )?;
        let rows = stmt.query_map(params![account_id], |r| {
            Ok(CredentialRow {
                account_id: r.get(0)?,
                purpose: r.get(1)?,
                nonce: r.get(2)?,
                ciphertext: r.get(3)?,
            })
        })?;
        rows.collect()
    }

    /// 整体替换账号账本（账号 + 凭据）于单个事务内。
    ///
    /// 先清空再写入：`accounts` 上的 `ON DELETE CASCADE` 一并清掉旧凭据，避免
    /// 出现账本里已删除账号的孤儿密文。调用方保证 `accounts` 内 id 唯一。
    pub fn write_accounts(
        &mut self,
        accounts: &[AccountRow],
        credentials: &[CredentialRow],
    ) -> rusqlite::Result<()> {
        let now = now_ms();
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM accounts", [])?;
        for a in accounts {
            tx.execute(
                "INSERT INTO accounts(id, platform, region, uid, display, enabled, created_at, updated_at, data)
                 VALUES(?1, ?2, ?3, ?4, ?5, 1, ?6, ?6, ?7)",
                params![a.id, a.platform, a.region, a.uid.as_deref(), a.display, now, a.data],
            )?;
        }
        for c in credentials {
            tx.execute(
                "INSERT INTO credentials(account_id, purpose, nonce, ciphertext, version, expires_at, status, created_at, updated_at)
                 VALUES(?1, ?2, ?3, ?4, 1, NULL, 'active', ?5, ?5)",
                params![c.account_id, c.purpose, c.nonce, c.ciphertext, now],
            )?;
        }
        tx.commit()
    }

    // ---- 成长任务运行记录与事件去重 ----

    /// 记一条成长任务运行结果。
    pub fn record_growth_run(&self, run: &GrowthRun) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO growth_runs(account_id, task, trigger, ok, message, credit, energy, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run.account_id,
                run.task,
                run.trigger,
                i64::from(run.ok),
                run.message,
                run.credit,
                run.energy,
                run.created_at
            ],
        )?;
        Ok(())
    }

    /// 最近的运行记录（`account_id` 为 None 时返回所有账号），新的在前。
    pub fn growth_runs(
        &self,
        account_id: Option<&str>,
        limit: usize,
    ) -> rusqlite::Result<Vec<GrowthRun>> {
        let limit = limit as i64;
        let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<GrowthRun> {
            Ok(GrowthRun {
                account_id: r.get(0)?,
                task: r.get(1)?,
                trigger: r.get(2)?,
                ok: r.get::<_, i64>(3)? != 0,
                message: r.get(4)?,
                credit: r.get(5)?,
                energy: r.get(6)?,
                created_at: r.get(7)?,
            })
        };
        match account_id {
            Some(id) => {
                let mut stmt = self.conn.prepare(
                    "SELECT account_id, task, trigger, ok, message, credit, energy, created_at
                     FROM growth_runs WHERE account_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![id, limit], map)?.collect();
                rows
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT account_id, task, trigger, ok, message, credit, energy, created_at
                     FROM growth_runs ORDER BY created_at DESC LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![limit], map)?.collect();
                rows
            }
        }
    }

    /// 某账号某类任务最近一次成功的时间（用于「今天是否已跑过」的判断）。
    pub fn last_success_at(&self, account_id: &str, task: &str) -> rusqlite::Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT MAX(created_at) FROM growth_runs
                 WHERE account_id = ?1 AND task = ?2 AND ok = 1",
                params![account_id, task],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()
            .map(Option::flatten)
    }

    /// 该事件是否已经上报过（本地判重，键与上游一致）。
    pub fn is_event_reported(
        &self,
        account_id: &str,
        event_code: &str,
        event_id: &str,
    ) -> rusqlite::Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM growth_events
             WHERE account_id = ?1 AND event_code = ?2 AND event_id = ?3",
            params![account_id, event_code, event_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// 标记事件已上报；重复标记不报错（幂等）。
    pub fn mark_event_reported(
        &self,
        account_id: &str,
        event_code: &str,
        event_id: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO growth_events(account_id, event_code, event_id, created_at)
             VALUES(?1, ?2, ?3, ?4)",
            params![account_id, event_code, event_id, now_ms()],
        )?;
        Ok(())
    }

    /// 清理过期的事件判重记录（早于 `before`），返回删除条数。
    pub fn prune_growth_events(&self, before: i64) -> rusqlite::Result<usize> {
        self.conn.execute(
            "DELETE FROM growth_events WHERE created_at < ?1",
            params![before],
        )
    }

    /// 一致快照：把当前库完整复制到 `dest`（含 WAL 中已提交内容）。
    ///
    /// 用于备份；直接复制主库文件会漏掉 WAL/SHM（SECURITY_AND_DATA.md §7）。
    pub fn backup_to(&self, dest: &Path) -> Result<(), String> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建备份目录失败: {e}"))?;
        }
        self.conn
            .execute("VACUUM INTO ?1", params![dest.to_string_lossy()])
            .map_err(|e| format!("生成数据库快照失败: {e}"))?;
        Ok(())
    }
}

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id          TEXT PRIMARY KEY,
    platform    TEXT NOT NULL,
    region      TEXT NOT NULL,
    uid         TEXT NOT NULL,
    display     TEXT NOT NULL DEFAULT '',
    enabled     INTEGER NOT NULL DEFAULT 1,
    remark      TEXT NOT NULL DEFAULT '',
    group_name  TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    UNIQUE(platform, region, uid)
);

CREATE TABLE IF NOT EXISTS credentials (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    purpose     TEXT NOT NULL,
    nonce       BLOB NOT NULL,
    ciphertext  BLOB NOT NULL,
    version     INTEGER NOT NULL DEFAULT 1,
    expires_at  INTEGER,
    status      TEXT NOT NULL DEFAULT 'active',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    UNIQUE(account_id, purpose)
);

CREATE TABLE IF NOT EXISTS admin (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    password_hash TEXT NOT NULL,
    created_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS admin_sessions (
    token_hash TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    revoked    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS instance_lease (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    owner        TEXT NOT NULL,
    expires_at   INTEGER NOT NULL,
    heartbeat_at INTEGER NOT NULL
);
"#;

/// v3：成长任务运行记录与事件去重。
///
/// `growth_events` 以 `(account_id, event_code, event_id)` 为主键：上游按同样的键判重，
/// 先在本地挡住重复上报，避免同一天重跑编排时把进度打点算两遍（那正是风控关注的形态）。
/// `growth_runs` 既是运行日志，也用于「今天是否已经成功跑过」的判定与界面展示。
const SCHEMA_V3: &str = r#"
CREATE TABLE IF NOT EXISTS growth_runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  TEXT NOT NULL,
    task        TEXT NOT NULL,
    trigger     TEXT NOT NULL,
    ok          INTEGER NOT NULL,
    message     TEXT NOT NULL DEFAULT '',
    credit      INTEGER NOT NULL DEFAULT 0,
    energy      INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_growth_runs_account
    ON growth_runs(account_id, task, created_at);

CREATE TABLE IF NOT EXISTS growth_events (
    account_id  TEXT NOT NULL,
    event_code  TEXT NOT NULL,
    event_id    TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    PRIMARY KEY (account_id, event_code, event_id)
);
"#;

/// v2：账号行承载完整账号 JSON（凭据字段拆到 `credentials`），并放开 `uid` 的非空约束。
///
/// SQLite 不能就地改列约束，只能重建表。`DROP TABLE accounts` 会按外键级联清空
/// `credentials`——v1 阶段没有任何代码写过这两张表，因此实际数据为零，这里不做额外
/// 搬运；此后所有凭据写入都发生在 v2 之后。
const SCHEMA_V2: &str = r#"
CREATE TABLE accounts_v2 (
    id          TEXT PRIMARY KEY,
    platform    TEXT NOT NULL,
    region      TEXT NOT NULL,
    uid         TEXT,
    display     TEXT NOT NULL DEFAULT '',
    enabled     INTEGER NOT NULL DEFAULT 1,
    remark      TEXT NOT NULL DEFAULT '',
    group_name  TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    data        TEXT NOT NULL DEFAULT '{}',
    UNIQUE(platform, region, uid)
);

INSERT INTO accounts_v2(id, platform, region, uid, display, enabled, remark, group_name, created_at, updated_at, data)
    SELECT id, platform, region, uid, display, enabled, remark, group_name, created_at, updated_at, '{}'
    FROM accounts;

DROP TABLE accounts;
ALTER TABLE accounts_v2 RENAME TO accounts;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("aac-db-test-{name}-{}", uuid::Uuid::new_v4()));
        let mut db = Db::open_migrated(&dir.join("app.db")).unwrap();
        db.migrate().unwrap();
        db
    }

    fn account_row(id: &str, platform: &str, region: &str, uid: &str) -> AccountRow {
        AccountRow {
            id: id.to_string(),
            platform: platform.to_string(),
            region: region.to_string(),
            uid: Some(uid.to_string()),
            display: id.to_string(),
            data: format!(r#"{{"id":"{id}"}}"#),
        }
    }

    fn uidless_row(id: &str) -> AccountRow {
        AccountRow {
            id: id.to_string(),
            platform: "workbuddy".to_string(),
            region: "cn".to_string(),
            uid: None,
            display: id.to_string(),
            data: format!(r#"{{"id":"{id}"}}"#),
        }
    }

    #[test]
    fn migrate_is_idempotent_and_sets_version() {
        let mut db = temp_db("migrate");
        let v: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        // 重复迁移不报错、版本不变。
        db.migrate().unwrap();
        let v2: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v2, SCHEMA_VERSION);
    }

    /// 把库预先置成 v1（含一行账号），返回 (Db, 路径)。
    fn v1_db(name: &str, uid: &str) -> (Db, PathBuf) {
        let dir = std::env::temp_dir().join(format!("aac-db-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.db");
        let mut db = Db::open(&path).unwrap();
        let tx = db.conn.transaction().unwrap();
        tx.execute_batch(SCHEMA_V1).unwrap();
        tx.commit().unwrap();
        db.conn.pragma_update(None, "user_version", 1).unwrap();
        db.conn
            .execute(
                "INSERT INTO accounts(id, platform, region, uid, display, created_at, updated_at)
                 VALUES('a1','workbuddy','cn',?1,'A',0,0)",
                params![uid],
            )
            .unwrap();
        (db, path)
    }

    /// v1 旧库必须能升到当前版本，且原有账号行不丢、拿到空的 `data`。
    #[test]
    fn migrate_v1_to_current_rebuilds_table_keeping_rows() {
        let (mut db, _path) = v1_db("v1", "u1");
        db.migrate().unwrap();
        let v: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let rows = db.read_accounts().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "a1");
        assert_eq!(rows[0].uid.as_deref(), Some("u1"));
        assert_eq!(rows[0].data, "{}");
    }

    /// 缺 UID 的账号可以并存：NULL 不参与唯一约束，历史记录不会被误合并。
    #[test]
    fn uid_less_accounts_coexist() {
        let mut db = temp_db("no-uid");
        db.write_accounts(&[uidless_row("a1"), uidless_row("a2")], &[])
            .unwrap();
        assert_eq!(db.account_count().unwrap(), 2);
        assert!(db.read_accounts().unwrap().iter().all(|r| r.uid.is_none()));
    }

    /// 迁移中途失败：`user_version` 不提升、原库数据不受损，可修好后重试。
    ///
    /// 构造方式是让 v2 的目标表名已被占用（上次迁移留下的残留表），重建必然报错，
    /// 正是「迁移脚本与库状态不一致」这一真实故障形态。
    #[test]
    fn failed_migration_keeps_version_and_original_data() {
        let (mut db, _path) = v1_db("fail", "u1");
        db.conn
            .execute("CREATE TABLE accounts_v2 (id TEXT PRIMARY KEY)", [])
            .unwrap();

        assert!(db.migrate().is_err(), "目标表已存在应让迁移失败而不是静默跳过");

        let v: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1, "失败时不得提升 user_version");
        let kept: String = db
            .conn
            .query_row("SELECT id FROM accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, "a1", "原库数据应原样保留");

        // 清掉残留后重试即可成功（失败可恢复）。
        db.conn.execute("DROP TABLE accounts_v2", []).unwrap();
        db.migrate().unwrap();
        let v: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn account_uid_unique_prevents_cross_platform_merge() {
        let mut db = temp_db("uid");
        db.write_accounts(
            &[
                account_row("a1", "workbuddy", "cn", "uid-1"),
                account_row("a2", "workbuddy", "intl", "uid-1"),
            ],
            &[],
        )
        .unwrap();
        let dup = db.conn.execute(
            "INSERT INTO accounts(id, platform, region, uid, display, created_at, updated_at)
             VALUES('x','workbuddy','cn','uid-1','D',0,0)",
            [],
        );
        assert!(dup.is_err());
    }

    #[test]
    fn write_accounts_replaces_ledger_and_cascades_credentials() {
        let mut db = temp_db("replace");
        let creds = vec![CredentialRow {
            account_id: "a1".to_string(),
            purpose: "access_token".to_string(),
            nonce: b"0123456789ab".to_vec(),
            ciphertext: b"cipher".to_vec(),
        }];
        db.write_accounts(&[account_row("a1", "workbuddy", "cn", "u1")], &creds)
            .unwrap();
        assert_eq!(db.account_count().unwrap(), 1);
        assert_eq!(db.read_credentials("a1").unwrap().len(), 1);

        // 整体替换后旧账号与旧密文都不应残留。
        db.write_accounts(&[account_row("a2", "workbuddy", "cn", "u2")], &[])
            .unwrap();
        assert_eq!(db.account_count().unwrap(), 1);
        assert!(db.read_credentials("a1").unwrap().is_empty());
        let orphans: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(orphans, 0);
    }

    #[test]
    fn admin_password_roundtrip() {
        let db = temp_db("admin");
        assert!(db.admin_password_hash().unwrap().is_none());
        db.set_admin_password("$argon2id$...").unwrap();
        assert_eq!(db.admin_password_hash().unwrap().as_deref(), Some("$argon2id$..."));
    }

    #[test]
    fn revoke_all_sessions_invalidates_every_token() {
        let db = temp_db("revoke-all");
        let future = now_ms() + 60_000;
        db.create_session("h1", future).unwrap();
        db.create_session("h2", future).unwrap();
        assert!(db.session_valid("h1").unwrap());
        db.revoke_all_sessions().unwrap();
        assert!(!db.session_valid("h1").unwrap());
        assert!(!db.session_valid("h2").unwrap());
    }

    #[test]
    fn backup_to_produces_readable_snapshot() {
        let mut db = temp_db("backup");
        db.write_accounts(&[account_row("a1", "workbuddy", "cn", "u1")], &[])
            .unwrap();
        let dir = std::env::temp_dir().join(format!("aac-db-backup-{}", uuid::Uuid::new_v4()));
        let dest = dir.join("snapshot.db");
        db.backup_to(&dest).unwrap();

        let copy = Db::open(&dest).unwrap();
        assert_eq!(copy.account_count().unwrap(), 1);
        assert_eq!(copy.read_accounts().unwrap()[0].id, "a1");
    }

    #[test]
    fn instance_lease_blocks_second_and_allows_takeover() {
        let dir = std::env::temp_dir().join(format!("aac-lease-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.db");
        let mut a = Db::open_migrated(&path).unwrap();
        let mut b = Db::open(&path).unwrap();
        let mut c = Db::open(&path).unwrap();

        assert!(a.acquire_instance_lease("a", 60_000).unwrap());
        // 第二个实例被拒绝
        assert!(!b.acquire_instance_lease("b", 60_000).unwrap());
        // 本 owner 续期成功
        assert!(a.renew_instance_lease("a", 60_000).unwrap());
        // 非持有者续期失败
        assert!(!b.renew_instance_lease("b", 60_000).unwrap());
        // 释放后可接管
        a.release_instance_lease("a").unwrap();
        assert!(b.acquire_instance_lease("b", 60_000).unwrap());
        // 过期后可接管（直接改写过期时间模拟）
        b.conn
            .execute("UPDATE instance_lease SET expires_at = ?1", params![now_ms() - 1000])
            .unwrap();
        assert!(c.acquire_instance_lease("c", 60_000).unwrap());
    }

    /// v2 → v3：老库补上成长任务表，原数据不受影响。
    #[test]
    fn migrate_v2_to_v3_adds_growth_tables() {
        let (mut db, _path) = v1_db("v2v3", "u1");
        db.migrate().unwrap();
        // 造一个「停在 v2」的状态，再走 2 → 3。
        db.conn.pragma_update(None, "user_version", 2).unwrap();
        db.conn
            .execute_batch("DROP TABLE IF EXISTS growth_events; DROP TABLE IF EXISTS growth_runs;")
            .unwrap();

        db.migrate().unwrap();
        let v: i32 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 3);
        assert_eq!(db.account_count().unwrap(), 1, "迁移不得动到已有账号");
        assert!(db.growth_runs(None, 10).unwrap().is_empty());
    }

    #[test]
    fn growth_runs_record_and_query() {
        let db = temp_db("growth-runs");
        let run = |task: &str, ok: bool, at: i64| GrowthRun {
            account_id: "a1".to_string(),
            task: task.to_string(),
            trigger: "auto".to_string(),
            ok,
            message: "done".to_string(),
            credit: 30,
            energy: 5,
            created_at: at,
        };
        db.record_growth_run(&run("growth", true, 100)).unwrap();
        db.record_growth_run(&run("growth", false, 200)).unwrap();
        db.record_growth_run(&run("streak", true, 300)).unwrap();

        // 只有成功的才算「今天已跑过」，失败可以重试。
        assert_eq!(db.last_success_at("a1", "growth").unwrap(), Some(100));
        assert_eq!(db.last_success_at("a1", "streak").unwrap(), Some(300));
        assert_eq!(db.last_success_at("a2", "growth").unwrap(), None);
        assert_eq!(db.last_success_at("a1", "night").unwrap(), None);

        let all = db.growth_runs(None, 10).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].created_at, 300, "新的在前");
        assert_eq!(db.growth_runs(Some("a1"), 10).unwrap().len(), 3);
        assert!(db.growth_runs(Some("nobody"), 10).unwrap().is_empty());
    }

    #[test]
    fn growth_event_dedupe_is_idempotent_and_prunable() {
        let db = temp_db("growth-events");
        assert!(!db.is_event_reported("a1", "chat_request_send", "ev-1").unwrap());
        db.mark_event_reported("a1", "chat_request_send", "ev-1").unwrap();
        assert!(db.is_event_reported("a1", "chat_request_send", "ev-1").unwrap());
        // 幂等：重复标记不报错、也不产生第二行。
        db.mark_event_reported("a1", "chat_request_send", "ev-1").unwrap();
        let rows: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM growth_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        // 换账号或换 id 都是新事件。
        assert!(!db.is_event_reported("a2", "chat_request_send", "ev-1").unwrap());
        assert!(!db.is_event_reported("a1", "chat_request_send", "ev-2").unwrap());

        // 清理只删过期记录。
        assert_eq!(db.prune_growth_events(0).unwrap(), 0, "未过期的不能删");
        assert_eq!(db.prune_growth_events(now_ms() + 1).unwrap(), 1);
        assert!(!db.is_event_reported("a1", "chat_request_send", "ev-1").unwrap());
    }
}
