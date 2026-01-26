use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use pbkdf2::pbkdf2_hmac;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

#[allow(dead_code)]
const SALT_LEN: usize = 16;
#[allow(dead_code)]
const NONCE_LEN: usize = 12;
#[allow(dead_code)]
const PBKDF2_ITERATIONS: u32 = 480_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct EncryptedData {
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[allow(dead_code)]
pub fn encrypt(plaintext: &[u8], password: &str) -> Result<EncryptedData, String> {
    let mut rng = rand::thread_rng();
    
    // Generate random salt and nonce
    let salt: Vec<u8> = (0..SALT_LEN).map(|_| rng.gen()).collect();
    let nonce_bytes: Vec<u8> = (0..NONCE_LEN).map(|_| rng.gen()).collect();
    
    // Derive key from password
    let mut key = [0u8; 32]; // 256 bits for AES-256
    pbkdf2_hmac::<Sha256>(
        password.as_bytes(),
        &salt,
        PBKDF2_ITERATIONS,
        &mut key,
    );
    
    // Encrypt
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Failed to initialize cipher: {}", e))?;
    
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, Payload::from(plaintext))
        .map_err(|e| format!("Encryption failed: {}", e))?;
    
    Ok(EncryptedData {
        salt: hex::encode(&salt),
        nonce: hex::encode(&nonce_bytes),
        ciphertext: hex::encode(&ciphertext),
    })
}

#[allow(dead_code)]
pub fn decrypt(encrypted: &EncryptedData, password: &str) -> Result<Vec<u8>, String> {
    // Decode hex strings
    let salt = hex::decode(&encrypted.salt)
        .map_err(|e| format!("Invalid salt encoding: {}", e))?;
    let nonce_bytes = hex::decode(&encrypted.nonce)
        .map_err(|e| format!("Invalid nonce encoding: {}", e))?;
    let ciphertext = hex::decode(&encrypted.ciphertext)
        .map_err(|e| format!("Invalid ciphertext encoding: {}", e))?;
    
    // Derive key from password
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(
        password.as_bytes(),
        &salt,
        PBKDF2_ITERATIONS,
        &mut key,
    );
    
    // Decrypt
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Failed to initialize cipher: {}", e))?;
    
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, Payload::from(ciphertext.as_ref()))
        .map_err(|e| format!("Decryption failed: {}", e))?;
    
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let plaintext = b"Hello, World!";
        let password = "my_secure_password";

        let encrypted = encrypt(plaintext, password).expect("Encryption failed");
        let decrypted = decrypt(&encrypted, password).expect("Decryption failed");

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_decrypt_wrong_password_fails() {
        let plaintext = b"Hello, World!";
        let password = "my_secure_password";

        let encrypted = encrypt(plaintext, password).expect("Encryption failed");
        let result = decrypt(&encrypted, "wrong_password");

        assert!(result.is_err());
    }
}
