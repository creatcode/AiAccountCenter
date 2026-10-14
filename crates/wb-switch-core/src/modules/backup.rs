//! 账本备份与恢复：一致数据库快照 + 用户口令封装的可移植包。
//!
//! 备份文件 = `头部（明文，含 KDF 参数与来源版本） + AES-256-GCM 密文`，密钥由用户
//! 口令经 Argon2id 派生。密文里是一个自包含包：**主密钥 + 数据库快照**。带上主密钥
//! 是因为凭据在库里是用数据根 `master.key` 加密的，只搬数据库会得到一堆解不开的密文；
//! 主密钥再用口令包一层，就不再受本机绑定（将来换成 DPAPI 也一样可搬，见
//! SECURITY_AND_DATA.md §10）。代价是口令丢失不可恢复，因此不提供任何绕过路径。
//!
//! 恢复在**完全校验通过之后**才动当前库：口令错误、密文被改、内容不是 SQLite 都
//! 在替换前失败，保证失败时当前库一字未改。

use std::path::{Path, PathBuf};

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::modules::config::now_ms;
use crate::modules::crypto;
use crate::modules::db::{db_path_in, Db};
use crate::modules::update::APP_VERSION;

/// 备份文件格式版本；头部布局变化时递增，旧版本仍按自己的布局解析。
pub const FORMAT_VERSION: u32 = 1;

const MAGIC: &[u8; 8] = b"AACBKP01";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
/// 数据库文件头，用于确认解密结果确实是 SQLite 而不是被换成别的字节。
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
/// 密文内自包含包：`bundle_version(1) | has_key(1) | [master_key(32)] | db`。
const BUNDLE_VERSION: u8 = 1;

/// KDF 参数上限：导入外来备份时不能让它把内存/CPU 打满（恶意头部）。
const MAX_M_COST_KIB: u32 = 1 << 20; // 1 GiB
const MAX_T_COST: u32 = 16;
const MAX_P_COST: u32 = 8;
/// 口令长度上限，避免拿超长口令做 KDF 放大攻击。
const MAX_PASSPHRASE_CHARS: usize = 1024;

/// 备份元信息（头部明文部分）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    pub format_version: u32,
    pub app_version: String,
    pub created_at_ms: i64,
}

pub fn backup_dir_in(store_root: &Path) -> PathBuf {
    store_root.join("backups")
}

fn derive_key(passphrase: &str, salt: &[u8], params: &Params) -> Result<[u8; KEY_LEN], String> {
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params.clone());
    let mut key = [0u8; KEY_LEN];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("口令派生失败: {e}"))?;
    Ok(key)
}

fn check_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.trim().is_empty() {
        return Err("备份口令不能为空".to_string());
    }
    if passphrase.chars().count() > MAX_PASSPHRASE_CHARS {
        return Err(format!("备份口令不能超过 {MAX_PASSPHRASE_CHARS} 个字符"));
    }
    Ok(())
}

/// 头部布局：`magic(8) | format(u32) | created_at(i64) | m|t|p(u32×3) | salt(16) |
/// nonce(12) | app_len(u16) | app_version`。全部小端，字段顺序变更须同时递增 FORMAT_VERSION。
fn build_header(params: &Params, salt: &[u8], nonce: &[u8], created_at: i64) -> Vec<u8> {
    let app = APP_VERSION.as_bytes();
    let mut header = Vec::with_capacity(64 + app.len());
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    header.extend_from_slice(&created_at.to_le_bytes());
    header.extend_from_slice(&params.m_cost().to_le_bytes());
    header.extend_from_slice(&params.t_cost().to_le_bytes());
    header.extend_from_slice(&params.p_cost().to_le_bytes());
    header.extend_from_slice(salt);
    header.extend_from_slice(nonce);
    header.extend_from_slice(&(app.len() as u16).to_le_bytes());
    header.extend_from_slice(app);
    header
}

/// 解析结果的固定部分，避免调用方再按魔数偏移切片。
struct Header<'a> {
    info: BackupInfo,
    params: Params,
    salt: &'a [u8],
    nonce: &'a [u8],
    /// 密文起始偏移。
    body: usize,
}

fn parse_header(blob: &[u8]) -> Result<Header<'_>, String> {
    /// `magic(8) | format(4) | created_at(8) | m|t|p(4×3) | salt | nonce | app_len(2)`。
    const FIXED_LEN: usize = 8 + 4 + 8 + 4 + 4 + 4 + SALT_LEN + NONCE_LEN + 2;
    if blob.len() < FIXED_LEN || &blob[..8] != MAGIC {
        return Err("不是 AiAccountCenter 备份文件".to_string());
    }
    let u32_at = |offset: usize| u32::from_le_bytes(blob[offset..offset + 4].try_into().unwrap());
    let format_version = u32_at(8);
    let created_at_ms = i64::from_le_bytes(blob[12..20].try_into().unwrap());
    let (m_cost, t_cost, p_cost) = (u32_at(20), u32_at(24), u32_at(28));
    let salt = &blob[32..32 + SALT_LEN];
    let nonce = &blob[32 + SALT_LEN..32 + SALT_LEN + NONCE_LEN];
    let app_len = u16::from_le_bytes(
        blob[32 + SALT_LEN + NONCE_LEN..FIXED_LEN]
            .try_into()
            .unwrap(),
    ) as usize;

    if format_version != FORMAT_VERSION {
        return Err(format!(
            "备份格式版本 {format_version} 不受支持（当前支持 {FORMAT_VERSION}）"
        ));
    }
    if m_cost > MAX_M_COST_KIB || t_cost > MAX_T_COST || p_cost > MAX_P_COST {
        return Err("备份头部 KDF 参数超出允许范围".to_string());
    }
    if blob.len() < FIXED_LEN + app_len {
        return Err("备份文件头部被截断".to_string());
    }
    let body = FIXED_LEN + app_len;
    let app_version = String::from_utf8_lossy(&blob[FIXED_LEN..body]).to_string();
    let params = Params::new(m_cost, t_cost, p_cost, Some(KEY_LEN))
        .map_err(|e| format!("KDF 参数非法: {e}"))?;
    Ok(Header {
        info: BackupInfo {
            format_version,
            app_version,
            created_at_ms,
        },
        params,
        salt,
        nonce,
        body,
    })
}

/// 生成备份：一致快照 → Argon2id 派生口令密钥 → AES-256-GCM 封装。
///
/// 返回写入的备份文件路径与来源库信息。
pub fn create_backup(
    store_root: &Path,
    passphrase: &str,
    dest: Option<&Path>,
) -> Result<(PathBuf, BackupInfo), String> {
    check_passphrase(passphrase)?;
    let db_path = db_path_in(store_root);
    if !db_path.exists() {
        return Err("账号账本不存在，无需备份".to_string());
    }
    let snapshot_dir = backup_dir_in(store_root);
    std::fs::create_dir_all(&snapshot_dir).map_err(|e| format!("创建备份目录失败: {e}"))?;
    let stamp = now_ms();
    let plain = snapshot_dir.join(format!("ledger-{stamp}.db"));
    {
        // 一致快照：直接复制主库会漏掉 WAL 中已提交的内容。
        let db = Db::open(&db_path).map_err(|e| format!("打开账号账本失败: {e}"))?;
        db.backup_to(&plain)?;
    }
    let db_bytes = std::fs::read(&plain).map_err(|e| format!("读取快照失败: {e}"))?;
    let _ = std::fs::remove_file(&plain);

    // 主密钥随包带走：库里凭据是用它加密的，缺了它备份只能恢复出一堆解不开的密文。
    // 磁盘上它可能是平台保护过的（Windows DPAPI），这里取解开后的 32 字节原值，
    // 再由口令包一层，从而换机可恢复。
    let master_key = crypto::MasterKey::read_existing_in(store_root)?.map(|key| key.raw().to_vec());
    let mut bundle = Vec::with_capacity(2 + KEY_LEN + db_bytes.len());
    bundle.push(BUNDLE_VERSION);
    bundle.push(u8::from(master_key.is_some()));
    if let Some(key) = &master_key {
        bundle.extend_from_slice(key);
    }
    bundle.extend_from_slice(&db_bytes);

    let params = Params::default();
    let salt = random_bytes::<SALT_LEN>();
    let nonce = random_bytes::<NONCE_LEN>();
    let header = build_header(&params, &salt, &nonce, stamp);
    let key = derive_key(passphrase, &salt, &params)?;
    let cipher = Aes256Gcm::new(GenericArray::from_slice(&key));
    // 头部本身作为 AAD：格式版本/KDF 参数/来源版本被改动即解密失败。
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &bundle,
                aad: &header,
            },
        )
        .map_err(|e| format!("封装备份失败: {e}"))?;

    let dest = dest
        .map(Path::to_path_buf)
        .unwrap_or_else(|| snapshot_dir.join(format!("ai-account-center-{stamp}.aacbak")));
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建备份目标目录失败: {e}"))?;
    }
    let mut out = header.clone();
    out.extend_from_slice(&ciphertext);
    std::fs::write(&dest, &out).map_err(|e| format!("写入备份文件失败: {e}"))?;

    Ok((
        dest,
        BackupInfo {
            format_version: FORMAT_VERSION,
            app_version: APP_VERSION.to_string(),
            created_at_ms: stamp,
        },
    ))
}

/// 只读解析备份头部（不解密）。
pub fn read_backup_info(src: &Path) -> Result<BackupInfo, String> {
    let blob = std::fs::read(src).map_err(|e| format!("读取备份文件失败: {e}"))?;
    parse_header(&blob).map(|header| header.info)
}

/// 备份目录里的一条记录（列表用，不解密）。
pub struct BackupEntry {
    pub file_name: String,
    pub size: u64,
    /// 头部可解析时的来源信息；文件损坏时为 None 并带上 `error`。
    pub info: Option<BackupInfo>,
    pub error: Option<String>,
}

/// 列出数据根 `backups/` 下的备份文件（按文件名倒序，新的在前）。
///
/// 单个文件损坏只影响它自己：列表里照常出现并带上原因，不整批失败。
pub fn list_backups(store_root: &Path) -> Result<Vec<BackupEntry>, String> {
    let dir = backup_dir_in(store_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("读取备份目录失败: {e}"))? {
        let entry = entry.map_err(|e| format!("读取备份目录失败: {e}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.ends_with(BACKUP_EXTENSION) {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let (info, error) = match read_backup_info(&path) {
            Ok(info) => (Some(info), None),
            Err(e) => (None, Some(e)),
        };
        entries.push(BackupEntry {
            file_name,
            size,
            info,
            error,
        });
    }
    entries.sort_by(|a, b| b.file_name.cmp(&a.file_name));
    Ok(entries)
}

/// 备份文件扩展名；列表与「只接受备份目录内的文件」校验共用。
pub const BACKUP_EXTENSION: &str = ".aacbak";

/// 只接受 `backups/` 目录内的备份文件名，避免恢复接口被拿去读任意服务端路径。
pub fn backup_path_in(store_root: &Path, file_name: &str) -> Result<PathBuf, String> {
    if file_name.is_empty()
        || !file_name.ends_with(BACKUP_EXTENSION)
        || file_name.contains(['/', '\\'])
        || file_name.contains("..")
    {
        return Err("备份文件名非法".to_string());
    }
    let dir = backup_dir_in(store_root);
    let path = dir.join(file_name);
    // 再核对规范化结果确实落在备份目录内（防重解析点/符号链接逃逸）。
    let dir = dir.canonicalize().map_err(|e| format!("备份目录不可用: {e}"))?;
    let path = path
        .canonicalize()
        .map_err(|_| "备份文件不存在".to_string())?;
    if !path.starts_with(&dir) {
        return Err("备份文件名非法".to_string());
    }
    Ok(path)
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    for chunk in out.chunks_mut(16) {
        let uuid = uuid::Uuid::new_v4();
        let bytes = uuid.as_bytes();
        let len = chunk.len().min(16);
        chunk.copy_from_slice(&bytes[..len]);
    }
    out
}

/// 从备份恢复账号账本。
///
/// 校验（头部 → 解密 → 内容确为 SQLite 且可打开）全部通过后才替换当前库；替换前
/// 先把当前库另存为 `backups/pre-restore-<ts>.db`，并把旧 `-wal`/`-shm` 一并挪走，
/// 避免新库被旧 WAL 覆盖。
pub fn restore_backup(store_root: &Path, passphrase: &str, src: &Path) -> Result<BackupInfo, String> {
    check_passphrase(passphrase)?;
    let blob = std::fs::read(src).map_err(|e| format!("读取备份文件失败: {e}"))?;
    let header = parse_header(&blob)?;
    let key = derive_key(passphrase, header.salt, &header.params)?;
    let cipher = Aes256Gcm::new(GenericArray::from_slice(&key));
    // 头部整体作为 AAD：格式版本/KDF 参数/来源版本被改动即解密失败。
    let plain = cipher
        .decrypt(
            Nonce::from_slice(header.nonce),
            Payload {
                msg: &blob[header.body..],
                aad: &blob[..header.body],
            },
        )
        .map_err(|_| "口令错误或备份文件已损坏".to_string())?;

    let (master_key, db_bytes) = parse_bundle(&plain)?;
    if db_bytes.len() < SQLITE_MAGIC.len() || &db_bytes[..SQLITE_MAGIC.len()] != SQLITE_MAGIC {
        return Err("备份内容不是有效的账号账本".to_string());
    }

    let db_path = db_path_in(store_root);
    let key_path = crypto::key_path_in(store_root);
    std::fs::create_dir_all(store_root).map_err(|e| format!("创建数据目录失败: {e}"))?;
    let staged_db = store_root.join("app.db.restore-staged");
    let staged_key = store_root.join("master.key.restore-staged");
    std::fs::write(&staged_db, db_bytes).map_err(|e| format!("写入待恢复账本失败: {e}"))?;
    if let Some(key) = &master_key {
        // 立刻按平台保护落盘，不做明文中转。
        crypto::write_key_at(&staged_key, key)?;
    }

    // 先在本机打开校验一次：坏库不能覆盖好库。校验用的连接会生成 WAL 边车文件，
    // 一并清掉以免污染数据根。
    let opened = Db::open(&staged_db).and_then(|db| db.account_count());
    for side in [
        staged_db.with_extension("db-wal"),
        staged_db.with_extension("db-shm"),
    ] {
        let _ = std::fs::remove_file(side);
    }
    if let Err(e) = opened {
        let _ = std::fs::remove_file(&staged_db);
        let _ = std::fs::remove_file(&staged_key);
        return Err(format!("备份内的账本无法打开: {e}"));
    }

    // 替换前把当前账本与主密钥各留一份：两者是配套的，只留一个没有意义。
    let safety_dir = backup_dir_in(store_root);
    std::fs::create_dir_all(&safety_dir).map_err(|e| format!("创建备份目录失败: {e}"))?;
    let stamp = now_ms();
    for (path, suffix) in [(&db_path, "db"), (&key_path, "key")] {
        if path.exists() {
            let safety = safety_dir.join(format!("pre-restore-{stamp}.{suffix}"));
            std::fs::copy(path, &safety).map_err(|e| format!("保存恢复前数据失败: {e}"))?;
        }
    }
    for suffix in ["-wal", "-shm"] {
        let side = db_path.with_extension(format!("db{suffix}"));
        if side.exists() {
            let _ = std::fs::remove_file(&side);
        }
    }
    std::fs::rename(&staged_db, &db_path).map_err(|e| format!("替换账号账本失败: {e}"))?;
    if master_key.is_some() {
        std::fs::rename(&staged_key, &key_path).map_err(|e| format!("替换主密钥失败: {e}"))?;
    }
    Ok(header.info)
}

/// 解析密文内的自包含包；返回 `(master_key, db_bytes)`。
fn parse_bundle(plain: &[u8]) -> Result<(Option<Vec<u8>>, &[u8]), String> {
    if plain.len() < 2 || plain[0] != BUNDLE_VERSION {
        return Err("备份内容版本不受支持".to_string());
    }
    let mut offset = 2;
    let master_key = if plain[1] == 1 {
        if plain.len() < offset + KEY_LEN {
            return Err("备份内容被截断".to_string());
        }
        let key = plain[offset..offset + KEY_LEN].to_vec();
        offset += KEY_LEN;
        Some(key)
    } else {
        None
    };
    Ok((master_key, &plain[offset..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_store(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wb-switch-backup-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seed(store: &Path, accounts: &[serde_json::Value]) {
        crate::modules::account::save_accounts_in(store, accounts).unwrap();
    }

    /// 备份 → 换一个空数据根恢复 → 账号与凭据都完整可用。
    #[test]
    fn backup_and_restore_into_fresh_store_roundtrips_ledger() {
        let src_store = temp_store("src");
        seed(
            &src_store,
            &[json!({"id": "a1", "uid": "u1", "nickname": "小明", "variant": "ai", "access_token": "SECRET"})],
        );
        let dest = src_store.join("out.aacbak");
        let (path, info) = create_backup(&src_store, "correct horse", Some(&dest)).unwrap();
        assert_eq!(path, dest);
        assert_eq!(info.format_version, FORMAT_VERSION);
        assert_eq!(info.app_version, APP_VERSION);

        let restored_store = temp_store("restored");
        let restored = restore_backup(&restored_store, "correct horse", &dest).unwrap();
        assert_eq!(restored, info);

        let accounts = crate::modules::account::load_accounts_in(&restored_store);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0]["nickname"], "小明");
        assert_eq!(accounts[0]["variant"], "ai");
        assert_eq!(accounts[0]["access_token"], "SECRET");

        std::fs::remove_dir_all(&src_store).unwrap();
        std::fs::remove_dir_all(&restored_store).unwrap();
    }

    /// 备份文件里不得出现明文凭据（口令封装的意义所在）。
    #[test]
    fn backup_file_does_not_leak_plaintext_credentials() {
        let store = temp_store("noleak");
        seed(
            &store,
            &[json!({"id": "a1", "uid": "u1", "access_token": "SECRET_ACCESS_VALUE"})],
        );
        let dest = store.join("out.aacbak");
        create_backup(&store, "pw", Some(&dest)).unwrap();
        let raw = std::fs::read(&dest).unwrap();
        let haystack = String::from_utf8_lossy(&raw);
        assert!(!haystack.contains("SECRET_ACCESS_VALUE"));
        assert!(!haystack.contains("SQLite format 3"));
        std::fs::remove_dir_all(&store).unwrap();
    }

    /// 口令错误：拒绝且当前库一字未改。
    #[test]
    fn wrong_passphrase_rejects_without_touching_current_ledger() {
        let store = temp_store("wrong-pw");
        seed(&store, &[json!({"id": "a1", "uid": "u1", "access_token": "SECRET"})]);
        let dest = store.join("out.aacbak");
        create_backup(&store, "right", Some(&dest)).unwrap();
        let before = std::fs::read(db_path_in(&store)).unwrap();

        let err = restore_backup(&store, "wrong", &dest).unwrap_err();
        assert!(err.contains("口令错误") || err.contains("损坏"), "实际: {err}");
        assert_eq!(std::fs::read(db_path_in(&store)).unwrap(), before, "当前库不得被改动");
        assert!(
            !store.join("app.db.restore-staged").exists(),
            "失败后不得留下中间文件"
        );
        std::fs::remove_dir_all(&store).unwrap();
    }

    /// 密文被篡改（含头部 KDF 参数）必须拒绝。
    #[test]
    fn tampered_backup_is_rejected() {
        let store = temp_store("tamper");
        seed(&store, &[json!({"id": "a1", "uid": "u1"})]);
        let dest = store.join("out.aacbak");
        create_backup(&store, "pw", Some(&dest)).unwrap();

        let mut raw = std::fs::read(&dest).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        std::fs::write(&dest, &raw).unwrap();
        assert!(restore_backup(&store, "pw", &dest).is_err(), "篡改密文必须拒绝");

        // 只改头部（AAD）同样拒绝。
        let mut raw = std::fs::read(&dest).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        raw[24] = raw[24].wrapping_add(1); // m_cost 高位
        std::fs::write(&dest, &raw).unwrap();
        let err = restore_backup(&store, "pw", &dest).unwrap_err();
        assert!(err.contains("KDF") || err.contains("口令错误") || err.contains("损坏"), "实际: {err}");
        std::fs::remove_dir_all(&store).unwrap();
    }

    /// 非备份文件（随便一个文件）必须被识别并拒绝。
    #[test]
    fn non_backup_file_is_rejected() {
        let store = temp_store("notbak");
        seed(&store, &[json!({"id": "a1", "uid": "u1"})]);
        let bogus = store.join("bogus.bin");
        std::fs::write(&bogus, b"hello world").unwrap();
        let err = restore_backup(&store, "pw", &bogus).unwrap_err();
        assert!(err.contains("不是 AiAccountCenter 备份文件"), "实际: {err}");

        // 空口令直接拒绝。
        assert!(create_backup(&store, "   ", None).is_err());
        assert!(restore_backup(&store, "", &bogus).is_err());
        std::fs::remove_dir_all(&store).unwrap();
    }

    /// 恢复接口只接受备份目录内的合法文件名，不能拿来读任意服务端路径。
    #[test]
    fn backup_path_rejects_escaping_names() {
        let store = temp_store("path");
        seed(&store, &[json!({"id": "a1", "uid": "u1"})]);
        let (path, _) = create_backup(&store, "pw", None).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(backup_path_in(&store, &name).is_ok(), "自己生成的备份名应通过");

        for bad in ["../app.db", "..\\app.db", "sub/out.aacbak", "", "app.db"] {
            assert!(backup_path_in(&store, bad).is_err(), "应拒绝: {bad}");
        }
        std::fs::remove_dir_all(&store).unwrap();
    }

    /// 列表只列 `.aacbak`、按文件名倒序，损坏文件单独带错误而不影响整批。
    #[test]
    fn list_backups_reports_entries_and_broken_files() {
        assert!(
            list_backups(&temp_store("list-empty")).unwrap().is_empty(),
            "目录不存在应返回空列表"
        );

        let store = temp_store("list");
        seed(&store, &[json!({"id": "a1", "uid": "u1"})]);
        let (created, _) = create_backup(&store, "pw", None).unwrap();
        std::fs::write(backup_dir_in(&store).join("zz-broken.aacbak"), b"junk").unwrap();
        std::fs::write(backup_dir_in(&store).join("ignored.txt"), b"junk").unwrap();

        let entries = list_backups(&store).unwrap();
        assert_eq!(entries.len(), 2, "非 .aacbak 文件不应入列");
        assert_eq!(entries[0].file_name, "zz-broken.aacbak", "按文件名倒序");
        assert!(entries[0].info.is_none() && entries[0].error.is_some());
        assert_eq!(
            entries[1].file_name,
            created.file_name().unwrap().to_string_lossy()
        );
        assert!(entries[1].info.is_some() && entries[1].error.is_none());
        std::fs::remove_dir_all(&store).unwrap();
    }

    /// 原库被破坏后用备份覆盖恢复（真实恢复路径，非仅新建数据根）。
    #[test]
    fn restore_overwrites_broken_ledger_and_keeps_pre_restore_copy() {
        let store = temp_store("overwrite");
        let accounts = [json!({"id": "a1", "uid": "u1", "access_token": "SECRET"})];
        seed(&store, &accounts);
        let dest = store.join("out.aacbak");
        create_backup(&store, "pw", Some(&dest)).unwrap();

        std::fs::write(db_path_in(&store), b"SQLite format 3\0 not really a db").unwrap();
        restore_backup(&store, "pw", &dest).unwrap();

        let loaded = crate::modules::account::load_accounts_in(&store);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0]["access_token"], "SECRET");
        let copies: Vec<String> = std::fs::read_dir(backup_dir_in(&store))
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with("pre-restore-"))
            .collect();
        assert!(
            copies.iter().any(|name| name.ends_with(".db")),
            "恢复前应留一份当前账本副本，实际 {copies:?}"
        );
        assert!(
            copies.iter().any(|name| name.ends_with(".key")),
            "主密钥与账本配套，也应留副本，实际 {copies:?}"
        );
        std::fs::remove_dir_all(&store).unwrap();
    }

    /// 覆盖恢复会替换目标库的主密钥；必须能从备份里恢复出可用凭据。
    #[test]
    fn restore_replaces_target_master_key_so_credentials_stay_readable() {
        let src = temp_store("key-src");
        seed(&src, &[json!({"id": "a1", "uid": "u1", "access_token": "SECRET"})]);
        let bak = src.join("out.aacbak");
        create_backup(&src, "pw", Some(&bak)).unwrap();

        // 目标数据根已有一套自己的账号与主密钥（与备份不同源）。
        let dst = temp_store("key-dst");
        seed(&dst, &[json!({"id": "old", "uid": "old-uid", "access_token": "OLD"})]);
        assert_ne!(
            std::fs::read(crypto::key_path_in(&src)).unwrap(),
            std::fs::read(crypto::key_path_in(&dst)).unwrap(),
            "两套数据根的主密钥应不同，测试才有意义"
        );

        restore_backup(&dst, "pw", &bak).unwrap();
        let loaded = crate::modules::account::load_accounts_in(&dst);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0]["uid"], "u1");
        assert_eq!(loaded[0]["access_token"], "SECRET", "恢复后凭据必须可解");
        #[cfg(windows)]
        assert_ne!(
            std::fs::read(crypto::key_path_in(&dst)).unwrap().len(),
            32,
            "恢复出的主密钥应立刻受 DPAPI 保护，不做明文中转"
        );

        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&dst).unwrap();
    }
}
