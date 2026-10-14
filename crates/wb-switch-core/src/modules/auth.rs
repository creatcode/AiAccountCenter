//! 管理登录原语：密码哈希（Argon2id）与会话令牌（随机 32 字节 + SHA-256 摘要存库）。
//!
//! 明文令牌只在 Set-Cookie 时经 HTTP 返回一次；服务端只存令牌的 SHA-256 摘要，
//! 数据库泄露不能直接重放会话。密码用 Argon2id 默认参数（内存/时间成本见 argon2 0.5）。

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use sha2::{Digest, Sha256};

/// 对明文密码做 Argon2id 哈希，返回 PHC 字符串。
pub fn hash_password(password: &str) -> Result<String, String> {
    let mut salt_bytes = [0u8; 16];
    salt_bytes.copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| format!("生成盐失败: {e}"))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("密码哈希失败: {e}"))
}

/// 校验密码；哈希非法或密码不符都返回 false（不泄露具体原因）。
pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// 生成高熵会话令牌（两个 uuid v4，共 256 bit 熵，十六进制）。
pub fn generate_session_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// 计算令牌的 SHA-256 摘要（base64），用于服务端持久化比对。
pub fn hash_token(token: &str) -> String {
    B64.encode(Sha256::digest(token.as_bytes()))
}

/// 首次初始化引导码字符集：去掉 0/O/1/I，避免人工抄写歧义。
const INIT_CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const INIT_CODE_LEN: usize = 12;

/// 生成一次性初始化码（12 位，约 59 bit 熵）。
///
/// 字符集长度整除 256，逐字节取模不引入偏置；两个 uuid v4 提供随机字节。
pub fn generate_init_code() -> String {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    (0..INIT_CODE_LEN)
        .map(|i| INIT_CODE_ALPHABET[bytes[i] as usize % INIT_CODE_ALPHABET.len()] as char)
        .collect()
}

/// 定长比较，避免逐字符提前返回泄露已匹配前缀（用于初始化码比对）。
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn invalid_hash_rejected() {
        assert!(!verify_password("x", "not-a-hash"));
        assert!(!verify_password("x", ""));
    }

    /// 初始化码：长度与字符集固定，多次生成不重复。
    #[test]
    fn init_code_shape_is_stable() {
        let code = generate_init_code();
        assert_eq!(code.chars().count(), INIT_CODE_LEN);
        assert!(code
            .chars()
            .all(|c| INIT_CODE_ALPHABET.contains(&(c as u8))));
        assert!(
            !code.contains(['0', 'O', '1', 'I']),
            "不应出现易混字符: {code}"
        );
        assert_ne!(generate_init_code(), code);
    }

    /// 定长比较：同长不同内容、长度不同都判不等。
    #[test]
    fn constant_time_eq_matches_equality() {
        assert!(constant_time_eq(b"abcdef", b"abcdef"));
        assert!(!constant_time_eq(b"abcdef", b"abcdeg"));
        assert!(!constant_time_eq(b"abcdef", b"abcde"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn token_is_unique_and_hashable() {
        let a = generate_session_token();
        let b = generate_session_token();
        assert_ne!(a, b);
        assert_ne!(hash_token(&a), hash_token(&b));
        assert_eq!(hash_token(&a), hash_token(&a));
        // 摘要不含明文令牌
        assert!(!hash_token(&a).contains(&a[..8]));
    }
}
