//! The encryption seam: AEAD `seal`/`open` over a per-user [`Key`]. Everything
//! the framework embeds in a PDF (the manifest, and later per-component state)
//! goes through here, so the device — and any third party we share a PDF with —
//! sees only ciphertext. The framework holds the key (from the secrets store)
//! and is the only reader.

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use zeroize::Zeroize;

use crate::error::{Error, Result};

/// XChaCha20-Poly1305 uses a 24-byte nonce; we prepend it to each ciphertext.
const NONCE_LEN: usize = 24;
/// Poly1305 authentication tag length, always appended by the AEAD.
const TAG_LEN: usize = 16;

/// A 32-byte symmetric key. Construct from raw bytes (tests / advanced callers)
/// or obtain the per-user key from [`crate::secrets::SecretStore`].
///
/// `Debug` is intentionally not derived so key material can't be logged.
#[derive(Clone)]
pub struct Key([u8; 32]);

impl Drop for Key {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl Key {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Key(bytes)
    }

    /// The raw key bytes. Crate-internal (used by the secrets store and tests);
    /// deliberately not part of the public API so release builds expose no
    /// key-exfiltration method.
    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Seal `plaintext` under `key`. Output is `nonce (24B) ‖ ciphertext ‖ tag`.
pub fn seal(key: &Key, plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|e| Error::Crypto(e.to_string()))?;
    let xnonce = XNonce::from_slice(&nonce);
    let ct = cipher
        .encrypt(xnonce, plaintext)
        .map_err(|e| Error::Crypto(e.to_string()))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Open a blob produced by [`seal`]. Verifies the auth tag; a wrong key or any
/// tampering yields `Error::Crypto`.
pub fn open(key: &Key, sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return Err(Error::Crypto(
            "sealed blob too short to contain nonce + tag".into(),
        ));
    }
    let (nonce, ct) = sealed.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    cipher
        .decrypt(XNonce::from_slice(nonce), ct)
        .map_err(|e| Error::Crypto(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_a() -> Key {
        Key::from_bytes([7u8; 32])
    }

    #[test]
    fn round_trips() {
        let pt = b"the queue lives in Readwise";
        let sealed = seal(&key_a(), pt).unwrap();
        assert_eq!(open(&key_a(), &sealed).unwrap(), pt);
    }

    #[test]
    fn wrong_key_fails() {
        let sealed = seal(&key_a(), b"secret").unwrap();
        let other = Key::from_bytes([8u8; 32]);
        assert!(matches!(open(&other, &sealed), Err(Error::Crypto(_))));
    }

    #[test]
    fn tampering_fails() {
        let mut sealed = seal(&key_a(), b"secret").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;
        assert!(matches!(open(&key_a(), &sealed), Err(Error::Crypto(_))));
    }

    #[test]
    fn nonce_randomizes_output() {
        let a = seal(&key_a(), b"same").unwrap();
        let b = seal(&key_a(), b"same").unwrap();
        assert_ne!(a, b, "each seal must use a fresh random nonce");
    }
}
