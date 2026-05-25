//! Pure HTML→Typst converter.

use sha2::{Digest, Sha256};

/// Content-addressed image key: the first 16 hex chars of `sha256(url)`. The
/// converter emits `#image("/assets/{key}.png", …)` and returns `(key, url)` so
/// the image worktree can fetch and serve `/assets/{key}.png`.
pub fn image_key(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let mut s = String::with_capacity(16);
    for b in digest.iter().take(8) {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_key_is_first_16_hex_of_sha256() {
        let k = image_key("https://example.com/cat.jpg");
        assert_eq!(k.len(), 16);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        // Deterministic + first-16-of-full-digest.
        let full = format!("{:x}", Sha256::digest(b"https://example.com/cat.jpg"));
        assert_eq!(k, &full[..16]);
    }
}
