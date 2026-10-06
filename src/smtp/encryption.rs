use crate::utils::crypto::fill_random;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use sha2::{Digest, Sha256};

const NONCE_SIZE: usize = 12;

fn derive_key(encryption_key: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(encryption_key.as_bytes());
    hasher.finalize().into()
}

/// Encrypt a password using AES-256-GCM
/// Returns base64-encoded: nonce || ciphertext || tag
pub fn encrypt_password(password: &str, encryption_key: &str) -> Result<String, String> {
    let key = derive_key(encryption_key);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Failed to create cipher: {}", e))?;

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    fill_random(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);

    let ciphertext = cipher
        .encrypt(&nonce, password.as_bytes())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let mut combined = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// Decrypt a password using AES-256-GCM
/// Expects base64-encoded: nonce || ciphertext || tag
pub fn decrypt_password(encrypted: &str, encryption_key: &str) -> Result<String, String> {
    let key = derive_key(encryption_key);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Failed to create cipher: {}", e))?;

    let combined = BASE64
        .decode(encrypted)
        .map_err(|e| format!("Invalid base64: {}", e))?;

    if combined.len() < NONCE_SIZE {
        return Err("Encrypted data too short".to_string());
    }

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_SIZE);
    let nonce = Nonce::try_from(nonce_bytes).map_err(|_| "Invalid nonce length".to_string())?;

    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| "Decryption failed (wrong key or corrupted data)".to_string())?;

    String::from_utf8(plaintext).map_err(|e| format!("Invalid UTF-8: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Produced by the previous aes-gcm 0.10 build (and independently decrypted with Python's
    // `cryptography`). Settings stored in existing databases must stay readable.
    #[test]
    fn ciphertext_from_the_previous_aes_gcm_version_still_decrypts() {
        let legacy = "xOmFHWGNeN/mB1spZvKnLjueI3k4czPmG2yY9rQoUHzH11txPZCkqOgNdTQ=";
        assert_eq!(
            decrypt_password(legacy, "legacy-key-for-test").unwrap(),
            "smtp-secret-p@ss"
        );
        assert!(decrypt_password(legacy, "another-key").is_err());
    }

    #[test]
    fn tampered_ciphertext_and_short_input_are_rejected() {
        let encrypted = encrypt_password("secret", "k").unwrap();
        let mut raw = BASE64.decode(&encrypted).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        assert!(decrypt_password(&BASE64.encode(&raw), "k").is_err());
        assert!(decrypt_password(&BASE64.encode([0u8; 5]), "k").is_err());
    }

    #[test]
    fn each_encryption_uses_a_fresh_nonce() {
        let a = encrypt_password("same", "k").unwrap();
        let b = encrypt_password("same", "k").unwrap();
        assert_ne!(a, b);
        assert_eq!(decrypt_password(&a, "k").unwrap(), "same");
        assert_eq!(decrypt_password(&b, "k").unwrap(), "same");
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let password = "my_secret_password";
        let encryption_key = "test_encryption_key_12345";

        let encrypted = encrypt_password(password, encryption_key).unwrap();
        let decrypted = decrypt_password(&encrypted, encryption_key).unwrap();

        assert_eq!(password, decrypted);
    }

    #[test]
    fn test_wrong_key_fails() {
        let password = "my_secret_password";
        let encryption_key = "correct_token";
        let wrong_token = "wrong_token";

        let encrypted = encrypt_password(password, encryption_key).unwrap();
        let result = decrypt_password(&encrypted, wrong_token);

        assert!(result.is_err());
    }
}
