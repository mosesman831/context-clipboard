//! AES-256-GCM helpers for encrypting clip fields at rest.
//!
//! Security notes:
//! - Plaintext is never logged. Callers must not log decrypted values either.
//! - A fresh random 96-bit nonce is generated per encryption. Never reuse a
//!   nonce with the same key.
//! - [`load_or_create_dev_key`] is a development/test convenience that stores a
//!   raw key file with `0600` permissions. In production the daemon swaps this
//!   for an OS keychain / Secret Service entry
//!   (`latticeag.context-clipboard.db-key`); this core crate deliberately does
//!   not depend on any keychain backend.

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;

use crate::error::{Error, Result};

/// Length of an AES-256 key in bytes.
pub const KEY_LEN: usize = 32;
/// Length of the AES-GCM nonce in bytes (96 bits, the recommended size).
pub const NONCE_LEN: usize = 12;

/// A 256-bit symmetric key. Zeroized on drop.
#[derive(Clone)]
pub struct Key([u8; KEY_LEN]);

impl Key {
    /// Construct a key from raw bytes.
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Key(bytes)
    }

    /// Borrow the raw key bytes.
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // Best-effort scrub; volatile write to avoid the compiler eliding it.
        for b in self.0.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

// Never leak key material via Debug output.
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Key").finish_non_exhaustive()
    }
}

/// Generate a fresh random 256-bit key from the OS CSPRNG.
pub fn generate_key() -> Key {
    let mut bytes = [0u8; KEY_LEN];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    Key(bytes)
}

/// Encrypt `plaintext`, returning `(nonce, ciphertext)`.
///
/// The ciphertext includes the AEAD authentication tag appended by the cipher.
pub fn encrypt(key: &Key, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
        .map_err(|e| Error::Crypto(format!("invalid key: {e}")))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| Error::Crypto("encryption failed".to_string()))?;

    Ok((nonce_bytes.to_vec(), ciphertext))
}

/// Decrypt `ciphertext` given the `nonce` used at encryption time.
pub fn decrypt(key: &Key, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if nonce.len() != NONCE_LEN {
        return Err(Error::Crypto(format!(
            "nonce must be {NONCE_LEN} bytes, got {}",
            nonce.len()
        )));
    }
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
        .map_err(|e| Error::Crypto(format!("invalid key: {e}")))?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| Error::Crypto("decryption failed (bad key, nonce, or tag)".to_string()))
}

/// Load a development key from `path`, creating one (with `0600` permissions)
/// if it does not exist.
///
/// This is intended for tests and local development only; production key
/// storage is the daemon's responsibility via the OS keychain.
pub fn load_or_create_dev_key(path: &Path) -> Result<Key> {
    if path.exists() {
        let bytes = std::fs::read(path)?;
        if bytes.len() != KEY_LEN {
            return Err(Error::Crypto(format!(
                "dev key file has invalid length: {} (expected {KEY_LEN})",
                bytes.len()
            )));
        }
        let mut k = [0u8; KEY_LEN];
        k.copy_from_slice(&bytes);
        return Ok(Key(k));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let key = generate_key();
    write_key_file(path, key.as_bytes())?;
    Ok(key)
}

#[cfg(unix)]
fn write_key_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_key_file(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = generate_key();
        let msg = b"top secret clipboard contents";
        let (nonce, ct) = encrypt(&key, msg).expect("encrypt");
        assert_eq!(nonce.len(), NONCE_LEN);
        assert_ne!(&ct[..], &msg[..], "ciphertext must differ from plaintext");
        let pt = decrypt(&key, &nonce, &ct).expect("decrypt");
        assert_eq!(pt, msg);
    }

    #[test]
    fn distinct_nonces_per_encryption() {
        let key = generate_key();
        let (n1, _) = encrypt(&key, b"a").expect("encrypt");
        let (n2, _) = encrypt(&key, b"a").expect("encrypt");
        assert_ne!(n1, n2, "nonces must not repeat");
    }

    #[test]
    fn wrong_key_fails() {
        let k1 = generate_key();
        let k2 = generate_key();
        let (nonce, ct) = encrypt(&k1, b"data").expect("encrypt");
        assert!(decrypt(&k2, &nonce, &ct).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = generate_key();
        let (nonce, mut ct) = encrypt(&key, b"data").expect("encrypt");
        ct[0] ^= 0xff;
        assert!(decrypt(&key, &nonce, &ct).is_err());
    }

    #[test]
    fn dev_key_persists_and_is_0600() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("db-key");
        let k1 = load_or_create_dev_key(&path).expect("create");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let k2 = load_or_create_dev_key(&path).expect("load");
        assert_eq!(k1.as_bytes(), k2.as_bytes());
    }
}
