use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMetadata {
    pub last_sync_timestamp: u64,
    pub last_sync_hash: String,
}

pub fn detect_conflict(
    local_file: &Path,
    remote_content: &[u8],
) -> Result<bool, Box<dyn std::error::Error>> {
    if !local_file.exists() {
        return Ok(false); // No conflict if file doesn't exist locally
    }

    let local_content = fs::read(local_file)?;
    let local_hash = calculate_hash(&local_content);
    let remote_hash = calculate_hash(remote_content);

    Ok(local_hash != remote_hash)
}

pub fn calculate_hash(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

impl SyncMetadata {
    pub fn new() -> Self {
        SyncMetadata {
            last_sync_timestamp: 0,
            last_sync_hash: String::new(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(SyncMetadata::new());
        }

        let json = fs::read_to_string(path)?;
        let metadata: SyncMetadata = serde_json::from_str(&json)?;
        Ok(metadata)
    }
}

impl Default for SyncMetadata {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_hash() {
        let hash1 = calculate_hash(b"test data");
        let hash2 = calculate_hash(b"test data");
        let hash3 = calculate_hash(b"different data");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
}
