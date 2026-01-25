use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::Rng;
use sha2::{Digest, Sha256};

const NONCE_SIZE: usize = 12;

fn derive_key(api_token: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(api_token.as_bytes());
    hasher.finalize().into()
}

/// Encrypt a password using AES-256-GCM
/// Returns base64-encoded: nonce || ciphertext || tag
pub fn encrypt_password(password: &str, api_token: &str) -> Result<String, String> {
    let key = derive_key(api_token);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::thread_rng().fill(&mut nonce_bytes);
    #[allow(deprecated)]
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let mut combined = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// Decrypt a password using AES-256-GCM
/// Expects base64-encoded: nonce || ciphertext || tag
pub fn decrypt_password(encrypted: &str, api_token: &str) -> Result<String, String> {
    let key = derive_key(api_token);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;

    let combined = BASE64
        .decode(encrypted)
        .map_err(|e| format!("Invalid base64: {}", e))?;

    if combined.len() < NONCE_SIZE {
        return Err("Encrypted data too short".to_string());
    }

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_SIZE);
    #[allow(deprecated)]
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed (wrong key or corrupted data)".to_string())?;

    String::from_utf8(plaintext).map_err(|e| format!("Invalid UTF-8: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let password = "my_secret_password";
        let api_token = "test_api_token_12345";

        let encrypted = encrypt_password(password, api_token).unwrap();
        let decrypted = decrypt_password(&encrypted, api_token).unwrap();

        assert_eq!(password, decrypted);
    }

    #[test]
    fn test_wrong_key_fails() {
        let password = "my_secret_password";
        let api_token = "correct_token";
        let wrong_token = "wrong_token";

        let encrypted = encrypt_password(password, api_token).unwrap();
        let result = decrypt_password(&encrypted, wrong_token);

        assert!(result.is_err());
    }
}
