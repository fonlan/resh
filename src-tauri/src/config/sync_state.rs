//! Local-only WebDAV sync baseline (`sync-state.json`).
//!
//! Isolated by account key (endpoint URL + username). Stores remote ETag/revision and
//! normalized entity content hashes — never full passwords, private keys, or API keys.

use crate::config::sync_protocol::{
    EntityKey, SyncEntityType, ADDITIONAL_PROMPT_ENTITY_ID, SYNC_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const SYNC_STATE_FILE: &str = "sync-state.json";

/// Per-account baseline after last successful sync.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSyncBaseline {
    /// Last successful remote ETag (opaque). Empty if unknown / first run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_etag: Option<String>,
    /// Optional remote revision string from `sync.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_revision: Option<String>,
    /// Schema version of last written remote document.
    #[serde(default)]
    pub sync_schema: u32,
    /// `entityType:id` → normalized content hash at last successful sync.
    #[serde(default)]
    pub entity_hashes: BTreeMap<String, String>,
    /// Typed deletion tombstones known at last success (for offline delete tracking).
    #[serde(default)]
    pub tombstones: Vec<BaselineTombstone>,
    /// Random per-account key for opaque conflict-resolution tokens. This is not synced and
    /// never contains user credentials or entity content.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resolution_secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineTombstone {
    pub entity_type: SyncEntityType,
    pub id: String,
}

impl BaselineTombstone {
    pub fn from_key(key: &EntityKey) -> Self {
        Self {
            entity_type: key.entity_type,
            id: key.id.clone(),
        }
    }

    pub fn key(&self) -> EntityKey {
        EntityKey::new(self.entity_type, self.id.clone())
    }
}

/// Root document for `sync-state.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncStateFile {
    #[serde(default)]
    pub version: u32,
    /// account_key → baseline
    #[serde(default)]
    pub accounts: BTreeMap<String, AccountSyncBaseline>,
}

impl SyncStateFile {
    pub fn baseline_for(&self, account_key: &str) -> Option<&AccountSyncBaseline> {
        self.accounts.get(account_key)
    }

    pub fn baseline_for_mut(&mut self, account_key: &str) -> &mut AccountSyncBaseline {
        self.accounts.entry(account_key.to_string()).or_default()
    }
}

impl AccountSyncBaseline {
    /// A baseline only participates in three-way decisions after a successful sync. A freshly
    /// allocated record may exist solely to persist the opaque conflict-token key.
    pub fn has_sync_history(&self) -> bool {
        self.sync_schema != 0
            || self.remote_etag.is_some()
            || self.remote_revision.is_some()
            || !self.entity_hashes.is_empty()
            || !self.tombstones.is_empty()
    }

    pub fn ensure_resolution_secret(&mut self) -> &str {
        if self.resolution_secret.is_empty() {
            self.resolution_secret = uuid::Uuid::new_v4().to_string();
        }
        &self.resolution_secret
    }

    pub fn hash_for(&self, key: &EntityKey) -> Option<&str> {
        self.entity_hashes
            .get(&key.storage_key())
            .map(|s| s.as_str())
    }

    pub fn has_entity(&self, key: &EntityKey) -> bool {
        self.entity_hashes.contains_key(&key.storage_key())
    }

    pub fn has_tombstone(&self, key: &EntityKey) -> bool {
        self.tombstones
            .iter()
            .any(|t| t.entity_type == key.entity_type && t.id == key.id)
    }

    pub fn set_entity_hash(&mut self, key: &EntityKey, hash: String) {
        self.entity_hashes.insert(key.storage_key(), hash);
        self.tombstones
            .retain(|t| !(t.entity_type == key.entity_type && t.id == key.id));
    }

    pub fn mark_deleted(&mut self, key: &EntityKey) {
        self.entity_hashes.remove(&key.storage_key());
        if !self.has_tombstone(key) {
            self.tombstones.push(BaselineTombstone::from_key(key));
        }
    }

    pub fn clear_tombstone(&mut self, key: &EntityKey) {
        self.tombstones
            .retain(|t| !(t.entity_type == key.entity_type && t.id == key.id));
    }

    /// Replace entity hashes from a successful merge map.
    pub fn replace_from_hashes(
        &mut self,
        hashes: &BTreeMap<EntityKey, String>,
        tombstones: &[EntityKey],
        remote_etag: Option<String>,
        remote_revision: Option<String>,
    ) {
        self.entity_hashes.clear();
        for (k, h) in hashes {
            // Skip pure-empty additional prompt from baseline presence if desired?
            // Always store additional prompt hash so clear vs set is tracked.
            let _ = ADDITIONAL_PROMPT_ENTITY_ID;
            self.entity_hashes.insert(k.storage_key(), h.clone());
        }
        self.tombstones = tombstones.iter().map(BaselineTombstone::from_key).collect();
        self.remote_etag = remote_etag;
        self.remote_revision = remote_revision;
        self.sync_schema = SYNC_SCHEMA_VERSION;
    }
}

/// Load/save `sync-state.json` under the app data directory.
#[derive(Clone)]
pub struct SyncStateStore {
    path: PathBuf,
}

impl SyncStateStore {
    pub fn new(app_data_dir: impl AsRef<Path>) -> Self {
        Self {
            path: app_data_dir.as_ref().join(SYNC_STATE_FILE),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<SyncStateFile, String> {
        if !self.path.exists() {
            return Ok(SyncStateFile {
                version: 1,
                accounts: BTreeMap::new(),
            });
        }
        let content = fs::read_to_string(&self.path)
            .map_err(|e| format!("Failed to read sync-state.json: {}", e))?;
        let state: SyncStateFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse sync-state.json: {}", e))?;
        Ok(state)
    }

    pub fn save(&self, state: &SyncStateFile) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create app data dir: {}", e))?;
        }
        let json = serde_json::to_string_pretty(state)
            .map_err(|e| format!("Failed to serialize sync-state.json: {}", e))?;
        // Refuse to persist obvious secret-looking field names at top level of entity payloads
        // (we only store hashes; assert structure).
        debug_assert!(!json.contains("\"password\":"));
        debug_assert!(!json.contains("\"keyContent\":"));
        debug_assert!(!json.contains("\"apiKey\":"));
        fs::write(&self.path, json).map_err(|e| format!("Failed to write sync-state.json: {}", e))
    }

    pub fn load_account(&self, account_key: &str) -> Result<Option<AccountSyncBaseline>, String> {
        Ok(self.load()?.accounts.get(account_key).cloned())
    }

    pub fn save_account(
        &self,
        account_key: &str,
        baseline: AccountSyncBaseline,
    ) -> Result<(), String> {
        let mut file = self.load()?;
        file.version = 1;
        file.accounts.insert(account_key.to_string(), baseline);
        self.save(&file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::sync_protocol::sync_account_key;

    #[test]
    fn round_trip_account_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let store = SyncStateStore::new(dir.path());
        let key_a = sync_account_key("https://a.example/dav", "u1");
        let key_b = sync_account_key("https://b.example/dav", "u1");

        let mut base = AccountSyncBaseline::default();
        base.set_entity_hash(&EntityKey::new(SyncEntityType::Server, "s1"), "abc".into());
        base.remote_etag = Some("\"etag-1\"".into());
        store.save_account(&key_a, base.clone()).unwrap();

        assert!(store.load_account(&key_b).unwrap().is_none());
        let loaded = store.load_account(&key_a).unwrap().unwrap();
        assert_eq!(loaded.remote_etag.as_deref(), Some("\"etag-1\""));
        assert_eq!(
            loaded.hash_for(&EntityKey::new(SyncEntityType::Server, "s1")),
            Some("abc")
        );

        // Ensure no secret material in file
        let raw = fs::read_to_string(store.path()).unwrap();
        assert!(!raw.contains("password"));
        assert!(!raw.contains("BEGIN OPENSSH"));
    }
}
