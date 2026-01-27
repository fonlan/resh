// src-tauri/src/master_password.rs
// Master password management with argon2 hashing

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand::thread_rng;
use std::fs;
use std::path::{Path, PathBuf};

/// Manager for the master password
pub struct MasterPasswordManager {
    password_hash_path: PathBuf,
}

impl MasterPasswordManager {
    /// Create a new master password manager
    ///
    /// # Arguments
    /// * `app_data_dir` - The application data directory
    pub fn new(app_data_dir: impl AsRef<Path>) -> Self {
        let password_hash_path = app_data_dir.as_ref().join("master_password.hash");

        Self { password_hash_path }
    }

    /// Check if a master password has been set
    ///
    /// # Returns
    /// true if a master password hash file exists, false otherwise
    pub fn is_set(&self) -> Result<bool, String> {
        Ok(self.password_hash_path.exists())
    }

    /// Set the master password (hashes and stores it)
    ///
    /// # Arguments
    /// * `password` - The plaintext password to hash and store
    ///
    /// # Returns
    /// Result indicating success or failure
    pub fn set_password(&self, password: &str) -> Result<(), String> {
        // Generate a random salt
        let salt = SaltString::generate(thread_rng());

        // Hash the password using argon2
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| format!("Failed to hash password: {}", e))?
            .to_string();

        // Write the hash to file
        fs::write(&self.password_hash_path, password_hash)
            .map_err(|e| format!("Failed to write password hash: {}", e))?;

        tracing::info!("Master password set successfully");
        Ok(())
    }

    /// Verify a password against the stored hash
    ///
    /// # Arguments
    /// * `password` - The plaintext password to verify
    ///
    /// # Returns
    /// true if the password matches, false otherwise
    pub fn verify_password(&self, password: &str) -> Result<bool, String> {
        // Read the stored hash
        let hash_str = fs::read_to_string(&self.password_hash_path)
            .map_err(|e| format!("Failed to read password hash: {}", e))?;

        // Parse the hash
        let password_hash = PasswordHash::new(&hash_str)
            .map_err(|e| format!("Failed to parse password hash: {}", e))?;

        // Verify the password
        let argon2 = Argon2::default();
        let is_valid = argon2
            .verify_password(password.as_bytes(), &password_hash)
            .is_ok();

        Ok(is_valid)
    }
}
