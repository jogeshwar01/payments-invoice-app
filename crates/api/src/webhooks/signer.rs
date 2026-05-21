//! HMAC-SHA256 webhook signing.
//!
//! Header: `Dodo-Signature: t=<unix_ts>,v1=<hex(HMAC(secret, "{t}.{body}"))>`.
//!
//! Receivers must:
//!   1. Reject if |now - t| > 300 s (replay window).
//!   2. Re-compute HMAC over "{t}.{body}" and compare in constant time.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn sign(secret: &[u8], timestamp: i64, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac key");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    format!("t={timestamp},v1={}", hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_deterministic() {
        let a = sign(b"secret", 1700000000, b"hello");
        let b = sign(b"secret", 1700000000, b"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn body_change_breaks_signature() {
        let a = sign(b"secret", 1700000000, b"hello");
        let b = sign(b"secret", 1700000000, b"hello!");
        assert_ne!(a, b);
    }
}
