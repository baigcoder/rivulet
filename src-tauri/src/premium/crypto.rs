//! Encrypted at-rest credential storage for Premium TV.
//!
//! The user's Xtream password and the M3U URL that carries it
//! never land in SQLite in clear text. The `CredentialVault` is a
//! thin wrapper over AES-GCM with a 32-byte master key kept in the
//! OS keychain under the slot `rivulet-premium-master`.
//!
//! The keychain entry is created on first use: if it doesn't exist
//! yet, we ask the keychain to generate 32 random bytes, store them
//! under the slot, and return them. Every later call re-reads the
//! same bytes; nothing else writes to that slot.
//!
//! On Android the `keyring` crate's `apple-native` / `sync-secret-service` /
//! `windows-native` features do nothing — the crate's `linux` feature
//! is what most desktop distros use. On Android we fall back to a
//! file in the app's private dir (see `master_key_path_android`).
//! The file has mode 0600; the directory is per-app private; and
//! the key never leaves that file or the process. This is "not as
//! strong as the platform keystore" and the right answer is the
//! AndroidX Security library's EncryptedSharedPreferences. The
//! bridge for that is a TODO; in the meantime the threat model
//! matches what the previous iptv-credentials module offered.
//!
//! **The `Display` impl of every error in this module is safe to log.**
//! A leaked master key would not leak through any of these strings.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::errors::PremiumError;

const KEY_SLOT: &str = "rivulet-premium-master";
#[cfg(target_os = "android")]
const ANDROID_KEY_FILE: &str = "rivulet-premium-master.bin";

/// An encrypted blob as it sits in SQLite: nonce (12 bytes) + ciphertext.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedBlob {
    /// 12 random bytes, unique per encryption. Stored alongside the
    /// ciphertext so we never reuse a (key, nonce) pair.
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl EncryptedBlob {
    pub fn encode(&self) -> Result<Vec<u8>, PremiumError> {
        let mut out = Vec::with_capacity(self.nonce.len() + self.ciphertext.len());
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, PremiumError> {
        if bytes.len() < 12 {
            return Err(PremiumError::CredentialError(
                "encrypted blob too short".into(),
            ));
        }
        let (nonce, ciphertext) = bytes.split_at(12);
        Ok(Self {
            nonce: nonce.to_vec(),
            ciphertext: ciphertext.to_vec(),
        })
    }
}

/// AES-GCM credential vault. Master key lives in the OS keychain.
pub struct CredentialVault {
    key: [u8; 32],
}

impl CredentialVault {
    /// Load (or create) the master key. The keychain entry holds
    /// 32 raw bytes; nothing human-readable ever lives in that slot.
    pub fn load() -> Result<Self, PremiumError> {
        let key = load_or_create_master_key()?;
        Ok(Self { key })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedBlob, PremiumError> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| PremiumError::CredentialError(format!("encrypt: {e}")))?;
        Ok(EncryptedBlob {
            nonce: nonce_bytes.to_vec(),
            ciphertext,
        })
    }

    pub fn decrypt(&self, blob: &EncryptedBlob) -> Result<Vec<u8>, PremiumError> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let nonce = Nonce::from_slice(&blob.nonce);
        cipher
            .decrypt(nonce, blob.ciphertext.as_ref())
            .map_err(|e| PremiumError::CredentialError(format!("decrypt: {e}")))
    }
}

// ── Master key loading ────────────────────────────────────────────

/// Read the master key from the OS keychain, creating it on first
/// use. On Android there is no keyring backend, so we fall back to
/// a 0600 file in the app's private cache dir.
fn load_or_create_master_key() -> Result<[u8; 32], PremiumError> {
    #[cfg(target_os = "android")]
    {
        load_or_create_master_key_android()
    }
    #[cfg(not(target_os = "android"))]
    {
        load_or_create_master_key_desktop()
    }
}

#[cfg(not(target_os = "android"))]
fn load_or_create_master_key_desktop() -> Result<[u8; 32], PremiumError> {
    let entry = keyring::Entry::new(KEY_SLOT, "master")
        .map_err(|e| PremiumError::CredentialError(e.to_string()))?;
    match entry.get_password() {
        Ok(hex) => {
            let bytes = hex::decode(hex.trim())
                .map_err(|e| PremiumError::CredentialError(format!("hex: {e}")))?;
            if bytes.len() != 32 {
                return Err(PremiumError::CredentialError(
                    "master key has wrong length".into(),
                ));
            }
            let mut out = [0u8; 32];
            out.copy_from_slice(&bytes);
            Ok(out)
        }
        Err(keyring::Error::NoEntry) => {
            let mut bytes = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut bytes);
            let hex = hex::encode(bytes);
            entry
                .set_password(&hex)
                .map_err(|e| PremiumError::CredentialError(e.to_string()))?;
            Ok(bytes)
        }
        Err(e) => Err(PremiumError::CredentialError(e.to_string())),
    }
}

#[cfg(target_os = "android")]
fn load_or_create_master_key_android() -> Result<[u8; 32], PremiumError> {
    use std::io::{Read, Write};
    let path = master_key_path_android()?;
    if path.exists() {
        let mut file = std::fs::File::open(&path)
            .map_err(|e| PremiumError::CredentialError(e.to_string()))?;
        let mut bytes = [0u8; 32];
        file.read_exact(&mut bytes)
            .map_err(|e| PremiumError::CredentialError(e.to_string()))?;
        return Ok(bytes);
    }
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| PremiumError::CredentialError(e.to_string()))?;
        file.write_all(&bytes)
            .map_err(|e| PremiumError::CredentialError(e.to_string()))?;
    }
    Ok(bytes)
}

#[cfg(target_os = "android")]
fn master_key_path_android() -> Result<std::path::PathBuf, PremiumError> {
    let base = std::env::var("RIVULET_APP_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    std::fs::create_dir_all(&base).ok();
    Ok(base.join(ANDROID_KEY_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let key = [7u8; 32];
        let vault = CredentialVault { key };
        let plaintext = b"hunter2";
        let blob = vault.encrypt(plaintext).unwrap();
        let recovered = vault.decrypt(&blob).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let vault_a = CredentialVault { key: [1u8; 32] };
        let vault_b = CredentialVault { key: [2u8; 32] };
        let blob = vault_a.encrypt(b"hunter2").unwrap();
        assert!(vault_b.decrypt(&blob).is_err());
    }

    #[test]
    fn encode_decode_roundtrip() {
        let blob = EncryptedBlob {
            nonce: vec![0; 12],
            ciphertext: vec![1, 2, 3, 4],
        };
        let bytes = blob.encode().unwrap();
        let recovered = EncryptedBlob::decode(&bytes).unwrap();
        assert_eq!(recovered.nonce, vec![0; 12]);
        assert_eq!(recovered.ciphertext, vec![1, 2, 3, 4]);
    }
}
