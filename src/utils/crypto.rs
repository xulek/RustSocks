//! Small helpers shared by the code that handles secrets: operating-system randomness and
//! lowercase hex encoding.

use rand::TryRng;

/// Fill `buf` with bytes from the operating system's cryptographically secure RNG.
///
/// # Panics
/// If the operating system cannot provide randomness. There is no safe fallback for secrets and
/// tokens, so this is treated as fatal (the previous `OsRng` behaved the same way).
pub fn fill_random(buf: &mut [u8]) {
    rand::rngs::SysRng
        .try_fill_bytes(buf)
        .expect("operating system random number generator failed");
}

/// Return `len` random bytes from the operating system's CSPRNG.
pub fn random_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    fill_random(&mut bytes);
    bytes
}

/// Lowercase hexadecimal encoding, e.g. for SHA-256 digests and HMAC signatures.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::{Digest, Sha256};

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        assert_eq!(to_hex(&[]), "");
        assert_eq!(to_hex(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }

    // Reference values computed with Python's hashlib and hmac, independent of these crates.
    #[test]
    fn sha256_hex_matches_reference() {
        assert_eq!(
            to_hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hmac_sha256_hex_matches_reference() {
        let mut mac = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        mac.update(b"message");
        assert_eq!(
            to_hex(&mac.finalize().into_bytes()),
            "8b5f48702995c1598c573db1e21866a9b825d4a794d169d7060a03605796360b"
        );
    }

    #[test]
    fn random_bytes_have_requested_length_and_differ() {
        let a = random_bytes(32);
        let b = random_bytes(32);
        assert_eq!(a.len(), 32);
        assert_ne!(a, b, "two 256-bit draws must not collide");
        let mut buf = [0u8; 16];
        fill_random(&mut buf);
        assert_ne!(buf, [0u8; 16]);
    }
}
