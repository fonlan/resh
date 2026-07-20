//! WebDAV config sync protocol: outcomes, conflicts, entity keys, content hashes.
//!
//! Content comparison uses normalized hashes (excludes display timestamps and local-only
//! `synced`). Sensitive material is included in hashes for correctness but never placed in
//! conflict summaries, logs, or error text.

use crate::config::types::{
    AiChannel, AiModel, Authentication, Config, Proxy, Server, SftpCustomCommand, Snippet,
    SyncConfig,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

/// Current remote `sync.json` schema written by this client.
pub const SYNC_SCHEMA_VERSION: u32 = 2;

/// Fixed entity id for the singleton additional-prompt field.
pub const ADDITIONAL_PROMPT_ENTITY_ID: &str = "additionalPrompt";

/// Synced entity kinds (whole-row conflict unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncEntityType {
    Server,
    Authentication,
    Proxy,
    Snippet,
    AiChannel,
    AiModel,
    SftpCustomCommand,
    AdditionalPrompt,
}

impl SyncEntityType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Authentication => "authentication",
            Self::Proxy => "proxy",
            Self::Snippet => "snippet",
            Self::AiChannel => "aiChannel",
            Self::AiModel => "aiModel",
            Self::SftpCustomCommand => "sftpCustomCommand",
            Self::AdditionalPrompt => "additionalPrompt",
        }
    }
}

impl fmt::Display for SyncEntityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Typed deletion tombstone stored on remote `sync.json` and in local baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionTombstone {
    pub entity_type: SyncEntityType,
    pub id: String,
    /// RFC3339 timestamp when this client recorded the delete (display / debug only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

impl DeletionTombstone {
    pub fn new(entity_type: SyncEntityType, id: impl Into<String>) -> Self {
        Self {
            entity_type,
            id: id.into(),
            deleted_at: Some(chrono::Utc::now().to_rfc3339()),
        }
    }

    pub fn key(&self) -> EntityKey {
        EntityKey {
            entity_type: self.entity_type,
            id: self.id.clone(),
        }
    }
}

/// Stable map key for a synced entity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityKey {
    pub entity_type: SyncEntityType,
    pub id: String,
}

impl EntityKey {
    pub fn new(entity_type: SyncEntityType, id: impl Into<String>) -> Self {
        Self {
            entity_type,
            id: id.into(),
        }
    }

    pub fn storage_key(&self) -> String {
        format!("{}:{}", self.entity_type.as_str(), self.id)
    }

    pub fn parse_storage_key(s: &str) -> Option<Self> {
        let (ty, id) = s.split_once(':')?;
        let entity_type = match ty {
            "server" => SyncEntityType::Server,
            "authentication" => SyncEntityType::Authentication,
            "proxy" => SyncEntityType::Proxy,
            "snippet" => SyncEntityType::Snippet,
            "aiChannel" => SyncEntityType::AiChannel,
            "aiModel" => SyncEntityType::AiModel,
            "sftpCustomCommand" => SyncEntityType::SftpCustomCommand,
            "additionalPrompt" => SyncEntityType::AdditionalPrompt,
            _ => return None,
        };
        Some(Self {
            entity_type,
            id: id.to_string(),
        })
    }
}

/// Why an entity needs manual resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncConflictKind {
    /// Both sides changed content to different values.
    BothModified,
    /// One side deleted while the other modified.
    DeleteVsModify,
    /// First sync / no baseline: same id, different content.
    FirstSyncMismatch,
    /// Merged graph has broken cross-references (auth/proxy/jumphost/channel).
    ReferenceIntegrity,
}

/// Safe, non-sensitive summary for UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitySummary {
    pub display_name: String,
    /// Short human-readable details without secrets.
    pub details: String,
    /// Used only by the backend to bind the opaque resolution token. It must never be sent to
    /// the frontend because some entity hashes include credential material.
    #[serde(skip)]
    pub content_hash: Option<String>,
    pub present: bool,
}

/// One conflict requiring KeepLocal / UseRemote (or batch equivalent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConflict {
    pub entity_type: SyncEntityType,
    pub id: String,
    pub display_name: String,
    pub kind: SyncConflictKind,
    pub local: EntitySummary,
    pub remote: EntitySummary,
    /// Opaque token bound to current local/remote hashes; stale resolutions are rejected.
    pub resolution_token: String,
}

/// User choice for a single conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncResolutionChoice {
    KeepLocal,
    UseRemote,
}

/// Resolution payload from UI / batch apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResolution {
    pub entity_type: SyncEntityType,
    pub id: String,
    pub choice: SyncResolutionChoice,
    pub resolution_token: String,
}

/// Structured sync errors (network / format / integrity). Not used for normal conflicts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncErrorKind {
    Network,
    Format,
    ReferenceIntegrity,
    ConcurrentLocalChange,
    ConcurrentRemoteChange,
    SafeSyncUnavailable,
    IncompatibleSchema,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncError {
    pub kind: SyncErrorKind,
    /// Safe message for UI/logs (must not embed secrets).
    pub message: String,
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for SyncError {}

/// Backend-authoritative result of a sync attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SyncOutcome {
    /// Local and remote applied; baseline updated.
    Applied {
        #[serde(default)]
        changed_entity_count: usize,
    },
    /// Manual resolution required; local config and remote file left unchanged.
    Conflicts {
        conflicts: Vec<SyncConflict>,
        /// Opaque token bound to the current remote ETag and conflict set. A resolution command
        /// must present it; any local/remote change requires a fresh sync attempt.
        attempt_token: String,
    },
    /// Remote changed under us (e.g. ETag 412); caller should re-download and recompute.
    ConcurrentRemoteChange { message: String },
    /// Non-conflict failure.
    Failed { error: SyncError },
}

impl SyncOutcome {
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::Applied { .. })
    }

    pub fn into_result(self) -> Result<(), String> {
        match self {
            Self::Applied { .. } => Ok(()),
            Self::Conflicts { conflicts, .. } => Err(format!(
                "Sync conflicts: {} item(s) need resolution",
                conflicts.len()
            )),
            Self::ConcurrentRemoteChange { message } => {
                Err(format!("Concurrent remote change: {}", message))
            }
            Self::Failed { error } => Err(error.message),
        }
    }
}

// --- Content hashing & summaries -----------------------------------------------------------

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Account isolation key for local `sync-state.json` (url + username; never password).
pub fn sync_account_key(url: &str, username: &str) -> String {
    let normalized_url = url.trim().trim_end_matches('/');
    let material = format!("{}\n{}", normalized_url, username.trim());
    hex_sha256(material.as_bytes())
}

fn hash_json(value: &serde_json::Value) -> String {
    // serde_json Map iterates in insertion order; we build BTree-backed objects only.
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    hex_sha256(&bytes)
}

fn sorted_string_array(items: &[String]) -> Vec<String> {
    let mut v = items.to_vec();
    v.sort();
    v
}

fn port_forwards_normalized(server: &Server) -> Vec<serde_json::Value> {
    let mut items: Vec<_> = server
        .port_forwards
        .iter()
        .map(|pf| {
            serde_json::json!({
                "local": pf.local,
                "remote": pf.remote,
            })
        })
        .collect();
    items.sort_by(|a, b| {
        let al = a.get("local").and_then(|v| v.as_u64()).unwrap_or(0);
        let bl = b.get("local").and_then(|v| v.as_u64()).unwrap_or(0);
        al.cmp(&bl).then_with(|| {
            let ar = a.get("remote").and_then(|v| v.as_u64()).unwrap_or(0);
            let br = b.get("remote").and_then(|v| v.as_u64()).unwrap_or(0);
            ar.cmp(&br)
        })
    });
    items
}

/// Normalized content hash for a server (excludes `synced` / timestamps).
pub fn hash_server(s: &Server) -> String {
    // Embedded lists keep relative order as business-defined; port forwards sorted.
    let value = serde_json::json!({
        "id": s.id,
        "name": s.name,
        "group": s.group,
        "host": s.host,
        "port": s.port,
        "username": s.username,
        "authId": s.auth_id,
        "proxyId": s.proxy_id,
        "jumphostId": s.jumphost_id,
        "portForwards": port_forwards_normalized(s),
        "keepAlive": s.keep_alive,
        "autoExecCommands": s.auto_exec_commands,
        "snippets": s.snippets.iter().map(|x| hash_snippet(x)).collect::<Vec<_>>(),
        "aiModels": s.ai_models.iter().map(|x| hash_ai_model(x)).collect::<Vec<_>>(),
        "sftpCustomCommands": s.sftp_custom_commands.iter().map(|x| hash_sftp_command(x)).collect::<Vec<_>>(),
        "sftpFavoritePaths": sorted_string_array(&s.sftp_favorite_paths),
        "additionalPrompt": s.additional_prompt,
    });
    hash_json(&value)
}

pub fn hash_authentication(a: &Authentication) -> String {
    let value = serde_json::json!({
        "id": a.id,
        "name": a.name,
        "type": a.auth_type,
        "keyContent": a.key_content,
        "passphrase": a.passphrase,
        "password": a.password,
    });
    hash_json(&value)
}

pub fn hash_proxy(p: &Proxy) -> String {
    let value = serde_json::json!({
        "id": p.id,
        "name": p.name,
        "type": p.proxy_type,
        "host": p.host,
        "port": p.port,
        "username": p.username,
        "password": p.password,
        "ignoreSslErrors": p.ignore_ssl_errors,
    });
    hash_json(&value)
}

pub fn hash_snippet(s: &Snippet) -> String {
    let value = serde_json::json!({
        "id": s.id,
        "name": s.name,
        "content": s.content,
        "description": s.description,
        "group": s.group,
    });
    hash_json(&value)
}

pub fn hash_ai_channel(c: &AiChannel) -> String {
    let value = serde_json::json!({
        "id": c.id,
        "name": c.name,
        "type": c.provider,
        "endpoint": c.endpoint,
        "apiKey": c.api_key,
        "proxyId": c.proxy_id,
        "isActive": c.is_active,
    });
    hash_json(&value)
}

pub fn hash_ai_model(m: &AiModel) -> String {
    let value = serde_json::json!({
        "id": m.id,
        "name": m.name,
        "channelId": m.channel_id,
        "contextWindow": m.context_window,
        "responseReserve": m.response_reserve,
        "enabled": m.enabled,
    });
    hash_json(&value)
}

pub fn hash_sftp_command(c: &SftpCustomCommand) -> String {
    let value = serde_json::json!({
        "id": c.id,
        "name": c.name,
        "pattern": c.pattern,
        "command": c.command,
    });
    hash_json(&value)
}

pub fn hash_additional_prompt(prompt: &Option<String>) -> String {
    hash_json(&serde_json::json!({ "additionalPrompt": prompt }))
}

pub fn summary_server(s: &Server) -> EntitySummary {
    EntitySummary {
        display_name: s.name.clone(),
        details: format!("{}@{}:{} · group={}", s.username, s.host, s.port, s.group),
        content_hash: Some(hash_server(s)),
        present: true,
    }
}

pub fn summary_authentication(a: &Authentication) -> EntitySummary {
    let secret = match a.auth_type.as_str() {
        "key" => {
            if a.key_content
                .as_ref()
                .map(|k| !k.is_empty())
                .unwrap_or(false)
            {
                "key=set"
            } else {
                "key=empty"
            }
        }
        _ => {
            if a.password.as_ref().map(|p| !p.is_empty()).unwrap_or(false) {
                "password=set"
            } else {
                "password=empty"
            }
        }
    };
    EntitySummary {
        display_name: a.name.clone(),
        details: format!("type={} · {}", a.auth_type, secret),
        content_hash: Some(hash_authentication(a)),
        present: true,
    }
}

pub fn summary_proxy(p: &Proxy) -> EntitySummary {
    let cred = if p.password.as_ref().map(|x| !x.is_empty()).unwrap_or(false) {
        "password=set"
    } else {
        "password=empty"
    };
    EntitySummary {
        display_name: p.name.clone(),
        details: format!("{}://{}:{} · {}", p.proxy_type, p.host, p.port, cred),
        content_hash: Some(hash_proxy(p)),
        present: true,
    }
}

pub fn summary_snippet(s: &Snippet) -> EntitySummary {
    EntitySummary {
        display_name: s.name.clone(),
        details: format!(
            "len={} · group={}",
            s.content.len(),
            s.group.as_deref().unwrap_or("-")
        ),
        content_hash: Some(hash_snippet(s)),
        present: true,
    }
}

pub fn summary_ai_channel(c: &AiChannel) -> EntitySummary {
    let key = if c.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false) {
        "apiKey=set"
    } else {
        "apiKey=empty"
    };
    EntitySummary {
        display_name: c.name.clone(),
        details: format!("type={} · {}", c.provider, key),
        content_hash: Some(hash_ai_channel(c)),
        present: true,
    }
}

pub fn summary_ai_model(m: &AiModel) -> EntitySummary {
    EntitySummary {
        display_name: m.name.clone(),
        details: format!("channel={} · enabled={}", m.channel_id, m.enabled),
        content_hash: Some(hash_ai_model(m)),
        present: true,
    }
}

pub fn summary_sftp_command(c: &SftpCustomCommand) -> EntitySummary {
    EntitySummary {
        display_name: c.name.clone(),
        details: format!("patternChars={}", c.pattern.chars().count()),
        content_hash: Some(hash_sftp_command(c)),
        present: true,
    }
}

pub fn summary_additional_prompt(prompt: &Option<String>) -> EntitySummary {
    let present = prompt.is_some();
    let details = match prompt {
        Some(p) => format!("chars={}", p.chars().count()),
        None => "empty".to_string(),
    };
    EntitySummary {
        display_name: "Additional prompt".to_string(),
        details,
        content_hash: Some(hash_additional_prompt(prompt)),
        present,
    }
}

pub fn absent_summary(display_name: &str) -> EntitySummary {
    EntitySummary {
        display_name: display_name.to_string(),
        details: "deleted".to_string(),
        content_hash: None,
        present: false,
    }
}

pub fn make_resolution_token(
    token_secret: &str,
    key: &EntityKey,
    local_hash: Option<&str>,
    remote_hash: Option<&str>,
    kind: SyncConflictKind,
) -> String {
    let material = format!(
        "{}|{}|{}|{}|{:?}",
        token_secret,
        key.storage_key(),
        local_hash.unwrap_or("-"),
        remote_hash.unwrap_or("-"),
        kind
    );
    hex_sha256(material.as_bytes())
}

/// Build an opaque token for one complete resolution attempt. It is intentionally bound to the
/// downloaded ETag as well as each conflict's token, so an otherwise identical remote document
/// with a new ETag must be refreshed before a user choice can be committed.
pub fn make_conflict_attempt_token(
    token_secret: &str,
    remote_etag: Option<&str>,
    conflicts: &[SyncConflict],
) -> String {
    let mut conflict_tokens: Vec<&str> = conflicts
        .iter()
        .map(|conflict| conflict.resolution_token.as_str())
        .collect();
    conflict_tokens.sort_unstable();
    let material = format!(
        "{}|{}|{}",
        token_secret,
        remote_etag.unwrap_or("<missing-etag>"),
        conflict_tokens.join("|")
    );
    hex_sha256(material.as_bytes())
}

/// Build conflict object with safe summaries.
pub fn build_conflict(
    token_secret: &str,
    key: EntityKey,
    kind: SyncConflictKind,
    display_name: String,
    local: EntitySummary,
    remote: EntitySummary,
) -> SyncConflict {
    let token = make_resolution_token(
        token_secret,
        &key,
        local.content_hash.as_deref(),
        remote.content_hash.as_deref(),
        kind,
    );
    SyncConflict {
        entity_type: key.entity_type,
        id: key.id,
        display_name,
        kind,
        local,
        remote,
        resolution_token: token,
    }
}

/// Collect normalized hashes for all currently synced local entities.
pub fn local_synced_hashes(config: &Config) -> BTreeMap<EntityKey, String> {
    let mut map = BTreeMap::new();
    for s in config.servers.iter().filter(|s| s.synced) {
        map.insert(
            EntityKey::new(SyncEntityType::Server, s.id.clone()),
            hash_server(s),
        );
    }
    for a in config.authentications.iter().filter(|a| a.synced) {
        map.insert(
            EntityKey::new(SyncEntityType::Authentication, a.id.clone()),
            hash_authentication(a),
        );
    }
    for p in config.proxies.iter().filter(|p| p.synced) {
        map.insert(
            EntityKey::new(SyncEntityType::Proxy, p.id.clone()),
            hash_proxy(p),
        );
    }
    for s in config.snippets.iter().filter(|s| s.synced) {
        map.insert(
            EntityKey::new(SyncEntityType::Snippet, s.id.clone()),
            hash_snippet(s),
        );
    }
    for c in config.ai_channels.iter().filter(|c| c.synced) {
        map.insert(
            EntityKey::new(SyncEntityType::AiChannel, c.id.clone()),
            hash_ai_channel(c),
        );
    }
    for m in config.ai_models.iter().filter(|m| m.synced) {
        map.insert(
            EntityKey::new(SyncEntityType::AiModel, m.id.clone()),
            hash_ai_model(m),
        );
    }
    for c in config.sftp_custom_commands.iter().filter(|c| c.synced) {
        map.insert(
            EntityKey::new(SyncEntityType::SftpCustomCommand, c.id.clone()),
            hash_sftp_command(c),
        );
    }
    // Singleton: always tracked when either side has content or baseline; hash even for None
    // so three-way can see local clear vs remote set.
    map.insert(
        EntityKey::new(
            SyncEntityType::AdditionalPrompt,
            ADDITIONAL_PROMPT_ENTITY_ID,
        ),
        hash_additional_prompt(&config.additional_prompt),
    );
    map
}

/// Collect hashes for entities present on remote `SyncConfig` (not tombstoned as delete-only).
pub fn remote_entity_hashes(remote: &SyncConfig) -> BTreeMap<EntityKey, String> {
    let mut map = BTreeMap::new();
    for s in &remote.servers {
        map.insert(
            EntityKey::new(SyncEntityType::Server, s.id.clone()),
            hash_server(s),
        );
    }
    for a in &remote.authentications {
        map.insert(
            EntityKey::new(SyncEntityType::Authentication, a.id.clone()),
            hash_authentication(a),
        );
    }
    for p in &remote.proxies {
        map.insert(
            EntityKey::new(SyncEntityType::Proxy, p.id.clone()),
            hash_proxy(p),
        );
    }
    for s in &remote.snippets {
        map.insert(
            EntityKey::new(SyncEntityType::Snippet, s.id.clone()),
            hash_snippet(s),
        );
    }
    for c in &remote.ai_channels {
        map.insert(
            EntityKey::new(SyncEntityType::AiChannel, c.id.clone()),
            hash_ai_channel(c),
        );
    }
    for m in &remote.ai_models {
        map.insert(
            EntityKey::new(SyncEntityType::AiModel, m.id.clone()),
            hash_ai_model(m),
        );
    }
    for c in &remote.sftp_custom_commands {
        map.insert(
            EntityKey::new(SyncEntityType::SftpCustomCommand, c.id.clone()),
            hash_sftp_command(c),
        );
    }
    map.insert(
        EntityKey::new(
            SyncEntityType::AdditionalPrompt,
            ADDITIONAL_PROMPT_ENTITY_ID,
        ),
        hash_additional_prompt(&remote.additional_prompt),
    );
    map
}

/// Check only type-safe tombstones. Legacy `removedIds` must be resolved to a unique entity type
/// by the merge layer before it is allowed to affect a deletion decision.
pub fn remote_has_tombstone(remote: &SyncConfig, key: &EntityKey) -> bool {
    remote
        .tombstones
        .iter()
        .any(|t| t.entity_type == key.entity_type && t.id == key.id)
}

/// After merge, rebuild compat `removed_ids` from typed tombstones.
pub fn rebuild_removed_ids(tombstones: &[DeletionTombstone]) -> Vec<String> {
    let mut ids: Vec<String> = tombstones.iter().map(|t| t.id.clone()).collect();
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updated_at_does_not_affect_server_hash() {
        let mut a = Server {
            id: "1".into(),
            name: "n".into(),
            group: "g".into(),
            host: "h".into(),
            port: 22,
            username: "u".into(),
            auth_id: None,
            proxy_id: None,
            jumphost_id: None,
            port_forwards: vec![],
            keep_alive: 0,
            auto_exec_commands: vec![],
            snippets: vec![],
            ai_models: vec![],
            sftp_custom_commands: vec![],
            sftp_favorite_paths: vec![],
            additional_prompt: None,
            synced: true,
            created_at: None,
            updated_at: "2020-01-01T00:00:00Z".into(),
        };
        let mut b = a.clone();
        b.updated_at = "2099-01-01T00:00:00Z".into();
        b.synced = false;
        assert_eq!(hash_server(&a), hash_server(&b));
        a.name = "other".into();
        assert_ne!(hash_server(&a), hash_server(&b));
    }

    #[test]
    fn auth_summary_redacts_secrets() {
        let a = Authentication {
            id: "a1".into(),
            name: "prod".into(),
            auth_type: "password".into(),
            key_content: None,
            passphrase: None,
            password: Some("super-secret-password".into()),
            synced: true,
            updated_at: "2020-01-01T00:00:00Z".into(),
        };
        let s = summary_authentication(&a);
        assert!(!s.details.contains("super-secret"));
        assert!(s.details.contains("password=set"));
    }

    #[test]
    fn account_key_ignores_trailing_slash_and_password() {
        let k1 = sync_account_key("https://dav.example.com/resh/", "alice");
        let k2 = sync_account_key("https://dav.example.com/resh", "alice");
        assert_eq!(k1, k2);
        let k3 = sync_account_key("https://dav.example.com/resh", "bob");
        assert_ne!(k1, k3);
    }

    #[test]
    fn untyped_legacy_tombstones_need_merge_time_type_resolution() {
        let mut remote = SyncConfig::empty("1.0");
        remote
            .tombstones
            .push(DeletionTombstone::new(SyncEntityType::Server, "typed"));
        remote.removed_ids.push("legacy".into());
        assert!(remote_has_tombstone(
            &remote,
            &EntityKey::new(SyncEntityType::Server, "typed")
        ));
        assert!(!remote_has_tombstone(
            &remote,
            &EntityKey::new(SyncEntityType::Snippet, "legacy")
        ));
    }

    #[test]
    fn conflict_payload_omits_credential_hashes_and_uses_a_keyed_token() {
        let auth = Authentication {
            id: "a1".into(),
            name: "prod".into(),
            auth_type: "password".into(),
            key_content: None,
            passphrase: None,
            password: Some("super-secret-password".into()),
            synced: true,
            updated_at: "2020-01-01T00:00:00Z".into(),
        };
        let key = EntityKey::new(SyncEntityType::Authentication, "a1");
        let summary = summary_authentication(&auth);
        let conflict = build_conflict(
            "account-private-token-key",
            key.clone(),
            SyncConflictKind::BothModified,
            "prod".into(),
            summary.clone(),
            summary,
        );
        let payload = serde_json::to_string(&conflict).unwrap();
        assert!(!payload.contains("contentHash"));
        assert!(!payload.contains("super-secret-password"));
        assert_ne!(
            conflict.resolution_token,
            make_resolution_token(
                "a-different-account-key",
                &key,
                Some(&hash_authentication(&auth)),
                Some(&hash_authentication(&auth)),
                SyncConflictKind::BothModified,
            )
        );
    }

    #[test]
    fn conflict_attempt_token_is_bound_to_remote_etag_and_conflict_set() {
        let conflict = SyncConflict {
            entity_type: SyncEntityType::Snippet,
            id: "snippet-1".into(),
            display_name: "Deploy".into(),
            kind: SyncConflictKind::BothModified,
            local: EntitySummary {
                display_name: "Deploy".into(),
                details: "len=12".into(),
                content_hash: None,
                present: true,
            },
            remote: EntitySummary {
                display_name: "Deploy".into(),
                details: "len=15".into(),
                content_hash: None,
                present: true,
            },
            resolution_token: "item-token".into(),
        };

        let v1 = make_conflict_attempt_token("account-secret", Some("\"v1\""), &[conflict.clone()]);
        let v2 = make_conflict_attempt_token("account-secret", Some("\"v2\""), &[conflict.clone()]);
        let changed_set = make_conflict_attempt_token("account-secret", Some("\"v1\""), &[]);

        assert_ne!(v1, v2);
        assert_ne!(v1, changed_set);
    }
}
