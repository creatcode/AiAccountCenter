//! 凭据加密原语：AES-256-GCM + AAD 绑定。
//!
//! nonce 与密文分离返回（见 `db::credentials` 两列），AAD 绑定账号/平台/区域/用途，
//! 防止密文在不同记录之间被交换。主密钥 32 字节，由系统随机源生成，落盘于数据根下
//! `master.key`。
//!
//! 主密钥本身的保护按平台分：
//!
//! - **Windows**：用户范围 DPAPI（`CryptProtectData`，不加 `CRYPTPROTECT_LOCAL_MACHINE`），
//!   因此换用户或换机器都解不开——正是「不能把密钥文件复制走就算迁移」的保证。
//! - **Linux/macOS**：文件本身即密钥，unix 下以 0600 创建。
//!
//! ⚠️ release-blocker（Linux）：SECURITY_AND_DATA.md §3 要求 Linux 使用**用户显式提供**的
//! 只读 secret 文件，而不是由程序在数据根自动生成；该项未实现。

use std::path::{Path, PathBuf};

#[cfg(windows)]
use windows::Win32::Foundation::{LocalFree, HLOCAL};
#[cfg(windows)]
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, AeadCore, OsRng, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
/// GCM 认证标签长度；短于此的密文必然非法，提前拒绝而不是交给解密器。
const TAG_LEN: usize = 16;

/// 主密钥文件在数据根下的固定名。
pub fn key_path_in(store_root: &Path) -> PathBuf {
    store_root.join("master.key")
}

/// 用户范围 DPAPI 封装。不加 `CRYPTPROTECT_LOCAL_MACHINE`，也不加 `CRYPTPROTECT_UI_FORBIDDEN`
/// 之外的任何放宽：换用户/换机器一律解不开。
#[cfg(windows)]
fn seal(bytes: &[u8]) -> Result<Vec<u8>, String> {
    /// `CRYPTPROTECT_UI_FORBIDDEN`：后台运行时不允许弹框要用户确认。
    const UI_FORBIDDEN: u32 = 0x1;
    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        CryptProtectData(&mut input, None, None, None, None, UI_FORBIDDEN, &mut output)
            .map_err(|e| format!("DPAPI CryptProtectData 失败: {e}"))?;
        if output.pbData.is_null() || output.cbData == 0 {
            return Err("DPAPI 未返回密文".to_string());
        }
        let sealed = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as _));
        Ok(sealed)
    }
}

/// 解开用户范围 DPAPI 封装；密文来自其他用户/机器或已损坏时失败。
#[cfg(windows)]
fn unseal(blob: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        CryptUnprotectData(&mut input, None, None, None, None, 0, &mut output)
            .map_err(|e| format!("DPAPI CryptUnprotectData 失败（可能不是当前用户加密的）: {e}"))?;
        if output.pbData.is_null() || output.cbData == 0 {
            return Err("DPAPI 未返回明文".to_string());
        }
        let plain = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as _));
        Ok(plain)
    }
}

#[cfg(not(windows))]
fn seal(bytes: &[u8]) -> Result<Vec<u8>, String> {
    Ok(bytes.to_vec())
}

#[cfg(not(windows))]
fn unseal(blob: &[u8]) -> Result<Vec<u8>, String> {
    Ok(blob.to_vec())
}

pub struct MasterKey {
    key: [u8; KEY_LEN],
}

impl MasterKey {
    /// 从指定数据根加载或创建主密钥。
    pub fn load_or_create_in(store_root: &Path) -> Result<Self, String> {
        Self::load_or_create_at(&key_path_in(store_root))
    }

    /// 从指定路径加载主密钥；不存在则随机生成并落盘。
    ///
    /// Windows 上文件内容是 DPAPI 密文；若读到恰好 32 字节，说明是 DPAPI 之前的旧明文
    /// 密钥（DPAPI 密文远长于此），按原值加载并就地改写成密文，不更换密钥。
    pub fn load_or_create_at(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建数据目录失败: {e}"))?;
        }
        if let Some(existing) = Self::read_existing_at(path)? {
            return Ok(existing);
        }
        let key = generate_key();
        create_key_file(path, &key)?;
        Ok(MasterKey { key })
    }

    /// 读取已存在的主密钥（解开平台保护），**不创建**；文件不存在返回 `None`。
    pub fn read_existing_at(path: &Path) -> Result<Option<Self>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read(path).map_err(|e| format!("读取主密钥失败: {e}"))?;
        let legacy_plaintext = cfg!(windows) && raw.len() == KEY_LEN;
        let bytes = if legacy_plaintext {
            raw
        } else {
            unseal(&raw).map_err(|e| format!("主密钥不可用: {e}"))?
        };
        if bytes.len() != KEY_LEN {
            return Err("主密钥长度不符，可能已损坏".to_string());
        }
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(&bytes);
        if legacy_plaintext {
            // 旧明文一律升级为平台保护，密钥值不变（旧密文照旧可解）。
            overwrite_key_file(path, &key)?;
            eprintln!("[主密钥] 已就地改为 DPAPI 保护（密钥本身未变）。");
        }
        Ok(Some(MasterKey { key }))
    }

    /// 读取指定数据根下的已存在主密钥。
    pub fn read_existing_in(store_root: &Path) -> Result<Option<Self>, String> {
        Self::read_existing_at(&key_path_in(store_root))
    }

    /// 原始 32 字节密钥。
    ///
    /// 仅供需要把密钥**再封装**进别处的调用方（备份包）使用；调用方不得把它落盘成明文。
    pub fn raw(&self) -> &[u8; KEY_LEN] {
        &self.key
    }

    /// 用随机 nonce 加密，返回 `(nonce, ciphertext||tag)` 两段，分别入库。
    pub fn encrypt_parts(&self, plaintext: &[u8], aad: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
        let cipher = Aes256Gcm::new(GenericArray::from_slice(&self.key));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = cipher
            .encrypt(&nonce, Payload { msg: plaintext, aad })
            .map_err(|e| format!("加密失败: {e}"))?;
        Ok((nonce.to_vec(), ct))
    }

    /// 解密 `encrypt_parts` 的输出；密钥或 AAD 不符、密文被篡改均返回错误。
    pub fn decrypt_parts(
        &self,
        nonce: &[u8],
        ciphertext: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, String> {
        if nonce.len() != NONCE_LEN || ciphertext.len() < TAG_LEN {
            return Err("密文或 nonce 长度非法".to_string());
        }
        let cipher = Aes256Gcm::new(GenericArray::from_slice(&self.key));
        cipher
            .decrypt(Nonce::from_slice(nonce), Payload { msg: ciphertext, aad })
            .map_err(|_| "解密失败（密钥或 AAD 不符）".to_string())
    }
}

/// 把原始密钥按平台保护后写入指定路径（覆盖已有内容）。
///
/// 供备份恢复落位使用：恢复出来的密钥必须立刻以平台保护形式落盘，不做明文中转。
pub fn write_key_at(path: &Path, key: &[u8]) -> Result<(), String> {
    if key.len() != KEY_LEN {
        return Err("主密钥长度不符，拒绝写入".to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建数据目录失败: {e}"))?;
    }
    overwrite_key_file(path, key)
}

/// 新建主密钥文件：unix 下以 0600 独占创建，避免生成过程中出现宽松权限的窗口。
fn create_key_file(path: &Path, key: &[u8]) -> Result<(), String> {
    let sealed = seal(key)?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("创建主密钥文件失败: {e}"))?;
        file.write_all(&sealed)
            .map_err(|e| format!("写入主密钥失败: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, sealed).map_err(|e| format!("写入主密钥失败: {e}"))?;
    }
    Ok(())
}

/// 覆盖已有主密钥文件（仅用于把旧明文密钥改为 DPAPI 密文，密钥值不变）。
fn overwrite_key_file(path: &Path, key: &[u8]) -> Result<(), String> {
    let sealed = seal(key)?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("改写主密钥文件失败: {e}"))?;
        file.write_all(&sealed)
            .map_err(|e| format!("写入主密钥失败: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, sealed).map_err(|e| format!("写入主密钥失败: {e}"))?;
    }
    Ok(())
}

/// 用系统安全随机源生成 32 字节主密钥（uuid v4 底层走 getrandom）。
fn generate_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    key[..16].copy_from_slice(a.as_bytes());
    key[16..].copy_from_slice(b.as_bytes());
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_key(name: &str) -> (MasterKey, PathBuf) {
        let dir = std::env::temp_dir().join(format!("aac-crypto-test-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("store");
        std::fs::create_dir_all(&root).unwrap();
        (MasterKey::load_or_create_in(&root).unwrap(), dir)
    }

    #[test]
    fn roundtrip() {
        let (k, _) = temp_key("rt");
        let aad = b"workbuddy|cn|uid|access";
        let (nonce, ct) = k.encrypt_parts(b"secret-token", aad).unwrap();
        assert_eq!(nonce.len(), NONCE_LEN);
        assert!(!ct.starts_with(b"secret-token"));
        let out = k.decrypt_parts(&nonce, &ct, aad).unwrap();
        assert_eq!(out, b"secret-token");
    }

    #[test]
    fn wrong_aad_fails() {
        let (k, _) = temp_key("aad");
        let (nonce, ct) = k.encrypt_parts(b"secret", b"aad-a").unwrap();
        assert!(k.decrypt_parts(&nonce, &ct, b"aad-b").is_err());
    }

    #[test]
    fn tamper_fails() {
        let (k, _) = temp_key("tamper");
        let (nonce, mut ct) = k.encrypt_parts(b"secret", b"aad").unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        assert!(k.decrypt_parts(&nonce, &ct, b"aad").is_err());
    }

    /// 密文移位：换一条记录的 AAD（账号/用途不同）必须解密失败。
    #[test]
    fn moved_ciphertext_fails() {
        let (k, _) = temp_key("move");
        let (nonce, ct) = k.encrypt_parts(b"secret", b"a1|workbuddy|cn|access_token").unwrap();
        assert!(k
            .decrypt_parts(&nonce, &ct, b"a2|workbuddy|cn|access_token")
            .is_err());
        assert!(k
            .decrypt_parts(&nonce, &ct, b"a1|workbuddy|cn|refresh_token")
            .is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let (k1, _) = temp_key("k1");
        let (k2, _) = temp_key("k2");
        let (nonce, ct) = k1.encrypt_parts(b"secret", b"aad").unwrap();
        assert!(k2.decrypt_parts(&nonce, &ct, b"aad").is_err());
    }

    #[test]
    fn key_persists_across_reload() {
        let (k, dir) = temp_key("persist");
        let (nonce, ct) = k.encrypt_parts(b"data", b"aad").unwrap();
        let reloaded = MasterKey::load_or_create_in(&dir.join("store")).unwrap();
        assert_eq!(reloaded.decrypt_parts(&nonce, &ct, b"aad").unwrap(), b"data");
    }

    /// DPAPI 之前的旧版明文密钥：必须按原值继续可用（旧密文不能失效），
    /// 且在 Windows 上就地改写为 DPAPI 密文。
    #[test]
    fn legacy_plaintext_key_keeps_value_and_gets_protected() {
        let (_, dir) = temp_key("legacy");
        let store = dir.join("store");
        let legacy = [7u8; KEY_LEN];
        std::fs::write(key_path_in(&store), legacy).unwrap();

        let expected = MasterKey { key: legacy };
        let (nonce, ct) = expected.encrypt_parts(b"data", b"aad").unwrap();

        let loaded = MasterKey::load_or_create_in(&store).unwrap();
        assert_eq!(
            loaded.decrypt_parts(&nonce, &ct, b"aad").unwrap(),
            b"data",
            "旧明文密钥的密钥值不得改变"
        );

        #[cfg(windows)]
        assert_ne!(
            std::fs::read(key_path_in(&store)).unwrap().len(),
            KEY_LEN,
            "Windows 上应改写为 DPAPI 密文"
        );
    }

    /// Windows 上换用户/换机器解不开：模拟「把密钥文件复制到另一上下文」。
    #[cfg(windows)]
    #[test]
    fn sealed_key_is_bound_to_this_user() {
        let sealed = seal(&[1u8; KEY_LEN]).unwrap();
        assert_ne!(sealed.len(), KEY_LEN, "DPAPI 密文不应等于原始 32 字节");
        assert_eq!(unseal(&sealed).unwrap(), vec![1u8; KEY_LEN]);
        // 随机字节不是合法 DPAPI 密文，必须明确失败而不是当明文用。
        assert!(unseal(&[2u8; KEY_LEN]).is_err());
    }

    #[test]
    fn corrupt_key_length_rejected() {
        let (_, dir) = temp_key("corrupt");
        std::fs::write(key_path_in(&dir.join("store")), b"too-short").unwrap();
        assert!(MasterKey::load_or_create_in(&dir.join("store")).is_err());
    }
}
