//! Entry-level three-way merge for WebDAV config sync.
//!
//! Base = last successful local baseline hashes; Local / Remote = current sides.
//! `updatedAt` is never used for decisions. Whole entities are the conflict unit.

use crate::config::sync_protocol::{
    absent_summary, build_conflict, hash_additional_prompt, hash_ai_channel, hash_ai_model,
    hash_authentication, hash_proxy, hash_server, hash_sftp_command, hash_snippet,
    local_synced_hashes, make_resolution_token, rebuild_removed_ids, remote_entity_hashes,
    remote_has_tombstone, summary_additional_prompt, summary_ai_channel, summary_ai_model,
    summary_authentication, summary_proxy, summary_server, summary_sftp_command, summary_snippet,
    DeletionTombstone, EntityKey, EntitySummary, SyncConflict, SyncConflictKind, SyncEntityType,
    SyncError, SyncErrorKind, SyncResolution, SyncResolutionChoice, ADDITIONAL_PROMPT_ENTITY_ID,
    SYNC_SCHEMA_VERSION,
};
use crate::config::sync_state::AccountSyncBaseline;
use crate::config::types::{
    AiChannel, AiModel, Authentication, Config, Proxy, Server, SftpCustomCommand, Snippet,
    SyncConfig,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Pure merge product (no I/O).
#[derive(Debug, Clone)]
pub struct MergeProduct {
    /// Terminal merge error that cannot be resolved by selecting a local or remote entity.
    pub error: Option<SyncError>,
    pub conflicts: Vec<SyncConflict>,
    /// Present only when `conflicts` is empty (or all resolved via `resolutions`).
    pub merged_local: Option<Config>,
    pub merged_remote: Option<SyncConfig>,
    /// Entity hashes to store in baseline after successful apply + upload.
    pub baseline_hashes: BTreeMap<EntityKey, String>,
    pub baseline_tombstone_keys: Vec<EntityKey>,
    pub changed_entity_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidePresence {
    Absent,
    Present,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoDecision {
    KeepLocal,
    UseRemote,
    Delete, // neither side keeps entity in synced set
    Conflict(SyncConflictKind),
}

/// Three-way decision from base/local/remote content hashes.
///
/// `None` hash means absent. When no baseline exists, `base` is always `None`.
pub(crate) fn decide_three_way(
    base: Option<&str>,
    local: Option<&str>,
    remote: Option<&str>,
    has_baseline: bool,
) -> AutoDecision {
    match (base, local, remote) {
        // Identical on both sides (including both absent)
        (_, Some(l), Some(r)) if l == r => AutoDecision::KeepLocal,
        (_, None, None) => AutoDecision::Delete,

        // No base
        (None, Some(_), None) => AutoDecision::KeepLocal, // local add
        (None, None, Some(_)) => AutoDecision::UseRemote, // remote add
        (None, Some(_), Some(_)) => {
            if has_baseline {
                // Should not happen if base tracks all; treat as both modified
                AutoDecision::Conflict(SyncConflictKind::BothModified)
            } else {
                AutoDecision::Conflict(SyncConflictKind::FirstSyncMismatch)
            }
        }

        // Base present
        (Some(b), Some(l), Some(r)) => {
            let local_changed = l != b;
            let remote_changed = r != b;
            match (local_changed, remote_changed) {
                (false, false) => AutoDecision::KeepLocal,
                (true, false) => AutoDecision::KeepLocal,
                (false, true) => AutoDecision::UseRemote,
                (true, true) => AutoDecision::Conflict(SyncConflictKind::BothModified),
            }
        }
        (Some(b), None, Some(r)) => {
            if r == b {
                AutoDecision::Delete // local deleted, remote unchanged
            } else {
                AutoDecision::Conflict(SyncConflictKind::DeleteVsModify)
            }
        }
        (Some(b), Some(l), None) => {
            if l == b {
                AutoDecision::Delete // remote deleted, local unchanged
            } else {
                AutoDecision::Conflict(SyncConflictKind::DeleteVsModify)
            }
        } // (Some(_), None, None) is covered by (_, None, None) above.
    }
}

const DIRECT_MERGE_TOKEN_SECRET: &str = "direct-merge-test-only";

fn resolution_for<'a>(
    token_secret: &str,
    resolutions: &'a [SyncResolution],
    key: &EntityKey,
    local_hash: Option<&str>,
    remote_hash: Option<&str>,
    kind: SyncConflictKind,
) -> Option<&'a SyncResolution> {
    let expected = make_resolution_token(token_secret, key, local_hash, remote_hash, kind);
    resolutions.iter().find(|r| {
        r.entity_type == key.entity_type && r.id == key.id && r.resolution_token == expected
    })
}

struct EntityMaps {
    servers: HashMap<String, Server>,
    auths: HashMap<String, Authentication>,
    proxies: HashMap<String, Proxy>,
    snippets: HashMap<String, Snippet>,
    channels: HashMap<String, AiChannel>,
    models: HashMap<String, AiModel>,
    commands: HashMap<String, SftpCustomCommand>,
}

fn local_maps(config: &Config) -> EntityMaps {
    EntityMaps {
        servers: config
            .servers
            .iter()
            .cloned()
            .map(|s| (s.id.clone(), s))
            .collect(),
        auths: config
            .authentications
            .iter()
            .cloned()
            .map(|a| (a.id.clone(), a))
            .collect(),
        proxies: config
            .proxies
            .iter()
            .cloned()
            .map(|p| (p.id.clone(), p))
            .collect(),
        snippets: config
            .snippets
            .iter()
            .cloned()
            .map(|s| (s.id.clone(), s))
            .collect(),
        channels: config
            .ai_channels
            .iter()
            .cloned()
            .map(|c| (c.id.clone(), c))
            .collect(),
        models: config
            .ai_models
            .iter()
            .cloned()
            .map(|m| (m.id.clone(), m))
            .collect(),
        commands: config
            .sftp_custom_commands
            .iter()
            .cloned()
            .map(|c| (c.id.clone(), c))
            .collect(),
    }
}

fn remote_maps(remote: &SyncConfig) -> EntityMaps {
    EntityMaps {
        servers: remote
            .servers
            .iter()
            .cloned()
            .map(|s| (s.id.clone(), s))
            .collect(),
        auths: remote
            .authentications
            .iter()
            .cloned()
            .map(|a| (a.id.clone(), a))
            .collect(),
        proxies: remote
            .proxies
            .iter()
            .cloned()
            .map(|p| (p.id.clone(), p))
            .collect(),
        snippets: remote
            .snippets
            .iter()
            .cloned()
            .map(|s| (s.id.clone(), s))
            .collect(),
        channels: remote
            .ai_channels
            .iter()
            .cloned()
            .map(|c| (c.id.clone(), c))
            .collect(),
        models: remote
            .ai_models
            .iter()
            .cloned()
            .map(|m| (m.id.clone(), m))
            .collect(),
        commands: remote
            .sftp_custom_commands
            .iter()
            .cloned()
            .map(|c| (c.id.clone(), c))
            .collect(),
    }
}

const MERGED_ENTITY_TYPES: [SyncEntityType; 7] = [
    SyncEntityType::Server,
    SyncEntityType::Authentication,
    SyncEntityType::Proxy,
    SyncEntityType::Snippet,
    SyncEntityType::AiChannel,
    SyncEntityType::AiModel,
    SyncEntityType::SftpCustomCommand,
];

fn ids_are_unique(mut ids: impl Iterator<Item = String>) -> bool {
    let mut seen = BTreeSet::new();
    ids.all(|id| seen.insert(id))
}

fn validate_unique_entity_ids(local: &Config, remote: &SyncConfig) -> Result<(), SyncError> {
    let local_ok = ids_are_unique(local.servers.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(local.authentications.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(local.proxies.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(local.snippets.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(local.ai_channels.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(local.ai_models.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(
            local
                .sftp_custom_commands
                .iter()
                .map(|entity| entity.id.clone()),
        );
    let remote_ok = ids_are_unique(remote.servers.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(
            remote
                .authentications
                .iter()
                .map(|entity| entity.id.clone()),
        )
        && ids_are_unique(remote.proxies.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(remote.snippets.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(remote.ai_channels.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(remote.ai_models.iter().map(|entity| entity.id.clone()))
        && ids_are_unique(
            remote
                .sftp_custom_commands
                .iter()
                .map(|entity| entity.id.clone()),
        );
    if local_ok && remote_ok {
        Ok(())
    } else {
        Err(SyncError {
            kind: SyncErrorKind::Format,
            message: "Sync configuration contains duplicate entity IDs within a type".into(),
        })
    }
}

fn entity_exists_in_local(config: &Config, entity_type: SyncEntityType, id: &str) -> bool {
    match entity_type {
        SyncEntityType::Server => config.servers.iter().any(|entity| entity.id == id),
        SyncEntityType::Authentication => {
            config.authentications.iter().any(|entity| entity.id == id)
        }
        SyncEntityType::Proxy => config.proxies.iter().any(|entity| entity.id == id),
        SyncEntityType::Snippet => config.snippets.iter().any(|entity| entity.id == id),
        SyncEntityType::AiChannel => config.ai_channels.iter().any(|entity| entity.id == id),
        SyncEntityType::AiModel => config.ai_models.iter().any(|entity| entity.id == id),
        SyncEntityType::SftpCustomCommand => config
            .sftp_custom_commands
            .iter()
            .any(|entity| entity.id == id),
        SyncEntityType::AdditionalPrompt => false,
    }
}

fn entity_exists_in_remote(remote: &SyncConfig, entity_type: SyncEntityType, id: &str) -> bool {
    match entity_type {
        SyncEntityType::Server => remote.servers.iter().any(|entity| entity.id == id),
        SyncEntityType::Authentication => {
            remote.authentications.iter().any(|entity| entity.id == id)
        }
        SyncEntityType::Proxy => remote.proxies.iter().any(|entity| entity.id == id),
        SyncEntityType::Snippet => remote.snippets.iter().any(|entity| entity.id == id),
        SyncEntityType::AiChannel => remote.ai_channels.iter().any(|entity| entity.id == id),
        SyncEntityType::AiModel => remote.ai_models.iter().any(|entity| entity.id == id),
        SyncEntityType::SftpCustomCommand => remote
            .sftp_custom_commands
            .iter()
            .any(|entity| entity.id == id),
        SyncEntityType::AdditionalPrompt => false,
    }
}

/// Convert only unambiguous legacy `removedIds` into typed tombstones. The old format has no
/// type information, so guessing across two entity types would permit silent data loss.
fn canonicalize_remote_tombstones(
    local: &Config,
    remote: &SyncConfig,
    baseline: Option<&AccountSyncBaseline>,
) -> Result<SyncConfig, SyncError> {
    validate_unique_entity_ids(local, remote)?;

    let mut normalized = remote.clone();
    for id in &remote.removed_ids {
        let candidates: Vec<_> = MERGED_ENTITY_TYPES
            .iter()
            .copied()
            .filter(|entity_type| {
                let key = EntityKey::new(*entity_type, id.clone());
                entity_exists_in_local(local, *entity_type, id)
                    || entity_exists_in_remote(remote, *entity_type, id)
                    || baseline
                        .is_some_and(|base| base.has_entity(&key) || base.has_tombstone(&key))
            })
            .collect();

        match candidates.as_slice() {
            [] => {
                // No present or baseline entity can identify the old tombstone. It is harmless
                // to omit on rewrite rather than applying an untyped delete later.
            }
            [entity_type] => {
                if !normalized
                    .tombstones
                    .iter()
                    .any(|t| t.entity_type == *entity_type && t.id == *id)
                {
                    normalized.tombstones.push(DeletionTombstone {
                        entity_type: *entity_type,
                        id: id.clone(),
                        deleted_at: None,
                    });
                }
            }
            _ => {
                return Err(SyncError {
                    kind: SyncErrorKind::Format,
                    message: "Legacy sync deletion is ambiguous across entity types".into(),
                });
            }
        }
    }
    Ok(normalized)
}

fn local_present_hash(config: &Config, key: &EntityKey) -> Option<String> {
    match key.entity_type {
        SyncEntityType::Server => config
            .servers
            .iter()
            .find(|s| s.id == key.id && s.synced)
            .map(hash_server),
        SyncEntityType::Authentication => config
            .authentications
            .iter()
            .find(|a| a.id == key.id && a.synced)
            .map(hash_authentication),
        SyncEntityType::Proxy => config
            .proxies
            .iter()
            .find(|p| p.id == key.id && p.synced)
            .map(hash_proxy),
        SyncEntityType::Snippet => config
            .snippets
            .iter()
            .find(|s| s.id == key.id && s.synced)
            .map(hash_snippet),
        SyncEntityType::AiChannel => config
            .ai_channels
            .iter()
            .find(|c| c.id == key.id && c.synced)
            .map(hash_ai_channel),
        SyncEntityType::AiModel => config
            .ai_models
            .iter()
            .find(|m| m.id == key.id && m.synced)
            .map(hash_ai_model),
        SyncEntityType::SftpCustomCommand => config
            .sftp_custom_commands
            .iter()
            .find(|c| c.id == key.id && c.synced)
            .map(hash_sftp_command),
        SyncEntityType::AdditionalPrompt => {
            // Treat empty prompt as absent so first-sync union works; baseline still
            // stores hash(None) after a successful sync of empty content.
            match &config.additional_prompt {
                Some(_) => Some(hash_additional_prompt(&config.additional_prompt)),
                None => None,
            }
        }
    }
}

fn local_sync_is_disabled(config: &Config, key: &EntityKey) -> bool {
    match key.entity_type {
        SyncEntityType::Server => config.servers.iter().any(|s| s.id == key.id && !s.synced),
        SyncEntityType::Authentication => config
            .authentications
            .iter()
            .any(|a| a.id == key.id && !a.synced),
        SyncEntityType::Proxy => config.proxies.iter().any(|p| p.id == key.id && !p.synced),
        SyncEntityType::Snippet => config.snippets.iter().any(|s| s.id == key.id && !s.synced),
        SyncEntityType::AiChannel => config
            .ai_channels
            .iter()
            .any(|c| c.id == key.id && !c.synced),
        SyncEntityType::AiModel => config.ai_models.iter().any(|m| m.id == key.id && !m.synced),
        SyncEntityType::SftpCustomCommand => config
            .sftp_custom_commands
            .iter()
            .any(|c| c.id == key.id && !c.synced),
        SyncEntityType::AdditionalPrompt => false,
    }
}

fn remote_present_hash(remote: &SyncConfig, key: &EntityKey) -> Option<String> {
    if key.entity_type != SyncEntityType::AdditionalPrompt && remote_has_tombstone(remote, key) {
        // Tombstone wins over accidental residual body (should not exist).
        return None;
    }
    match key.entity_type {
        SyncEntityType::Server => remote
            .servers
            .iter()
            .find(|s| s.id == key.id)
            .map(hash_server),
        SyncEntityType::Authentication => remote
            .authentications
            .iter()
            .find(|a| a.id == key.id)
            .map(hash_authentication),
        SyncEntityType::Proxy => remote
            .proxies
            .iter()
            .find(|p| p.id == key.id)
            .map(hash_proxy),
        SyncEntityType::Snippet => remote
            .snippets
            .iter()
            .find(|s| s.id == key.id)
            .map(hash_snippet),
        SyncEntityType::AiChannel => remote
            .ai_channels
            .iter()
            .find(|c| c.id == key.id)
            .map(hash_ai_channel),
        SyncEntityType::AiModel => remote
            .ai_models
            .iter()
            .find(|m| m.id == key.id)
            .map(hash_ai_model),
        SyncEntityType::SftpCustomCommand => remote
            .sftp_custom_commands
            .iter()
            .find(|c| c.id == key.id)
            .map(hash_sftp_command),
        SyncEntityType::AdditionalPrompt => {
            if remote_has_tombstone(remote, key) {
                None
            } else {
                match &remote.additional_prompt {
                    Some(_) => Some(hash_additional_prompt(&remote.additional_prompt)),
                    None => None,
                }
            }
        }
    }
}

fn summaries_for(
    key: &EntityKey,
    local: &Config,
    remote: &SyncConfig,
    local_hash: Option<&str>,
    remote_hash: Option<&str>,
) -> (EntitySummary, EntitySummary, String) {
    let (local_sum, remote_sum, name) = match key.entity_type {
        SyncEntityType::Server => {
            let l = local
                .servers
                .iter()
                .find(|s| s.id == key.id && s.synced)
                .map(summary_server);
            let r = remote
                .servers
                .iter()
                .find(|s| s.id == key.id)
                .map(summary_server);
            let name = l
                .as_ref()
                .or(r.as_ref())
                .map(|s| s.display_name.clone())
                .unwrap_or_else(|| key.id.clone());
            (
                l.unwrap_or_else(|| absent_summary(&name)),
                r.unwrap_or_else(|| absent_summary(&name)),
                name,
            )
        }
        SyncEntityType::Authentication => {
            let l = local
                .authentications
                .iter()
                .find(|a| a.id == key.id && a.synced)
                .map(summary_authentication);
            let r = remote
                .authentications
                .iter()
                .find(|a| a.id == key.id)
                .map(summary_authentication);
            let name = l
                .as_ref()
                .or(r.as_ref())
                .map(|s| s.display_name.clone())
                .unwrap_or_else(|| key.id.clone());
            (
                l.unwrap_or_else(|| absent_summary(&name)),
                r.unwrap_or_else(|| absent_summary(&name)),
                name,
            )
        }
        SyncEntityType::Proxy => {
            let l = local
                .proxies
                .iter()
                .find(|p| p.id == key.id && p.synced)
                .map(summary_proxy);
            let r = remote
                .proxies
                .iter()
                .find(|p| p.id == key.id)
                .map(summary_proxy);
            let name = l
                .as_ref()
                .or(r.as_ref())
                .map(|s| s.display_name.clone())
                .unwrap_or_else(|| key.id.clone());
            (
                l.unwrap_or_else(|| absent_summary(&name)),
                r.unwrap_or_else(|| absent_summary(&name)),
                name,
            )
        }
        SyncEntityType::Snippet => {
            let l = local
                .snippets
                .iter()
                .find(|s| s.id == key.id && s.synced)
                .map(summary_snippet);
            let r = remote
                .snippets
                .iter()
                .find(|s| s.id == key.id)
                .map(summary_snippet);
            let name = l
                .as_ref()
                .or(r.as_ref())
                .map(|s| s.display_name.clone())
                .unwrap_or_else(|| key.id.clone());
            (
                l.unwrap_or_else(|| absent_summary(&name)),
                r.unwrap_or_else(|| absent_summary(&name)),
                name,
            )
        }
        SyncEntityType::AiChannel => {
            let l = local
                .ai_channels
                .iter()
                .find(|c| c.id == key.id && c.synced)
                .map(summary_ai_channel);
            let r = remote
                .ai_channels
                .iter()
                .find(|c| c.id == key.id)
                .map(summary_ai_channel);
            let name = l
                .as_ref()
                .or(r.as_ref())
                .map(|s| s.display_name.clone())
                .unwrap_or_else(|| key.id.clone());
            (
                l.unwrap_or_else(|| absent_summary(&name)),
                r.unwrap_or_else(|| absent_summary(&name)),
                name,
            )
        }
        SyncEntityType::AiModel => {
            let l = local
                .ai_models
                .iter()
                .find(|m| m.id == key.id && m.synced)
                .map(summary_ai_model);
            let r = remote
                .ai_models
                .iter()
                .find(|m| m.id == key.id)
                .map(summary_ai_model);
            let name = l
                .as_ref()
                .or(r.as_ref())
                .map(|s| s.display_name.clone())
                .unwrap_or_else(|| key.id.clone());
            (
                l.unwrap_or_else(|| absent_summary(&name)),
                r.unwrap_or_else(|| absent_summary(&name)),
                name,
            )
        }
        SyncEntityType::SftpCustomCommand => {
            let l = local
                .sftp_custom_commands
                .iter()
                .find(|c| c.id == key.id && c.synced)
                .map(summary_sftp_command);
            let r = remote
                .sftp_custom_commands
                .iter()
                .find(|c| c.id == key.id)
                .map(summary_sftp_command);
            let name = l
                .as_ref()
                .or(r.as_ref())
                .map(|s| s.display_name.clone())
                .unwrap_or_else(|| key.id.clone());
            (
                l.unwrap_or_else(|| absent_summary(&name)),
                r.unwrap_or_else(|| absent_summary(&name)),
                name,
            )
        }
        SyncEntityType::AdditionalPrompt => {
            let l = summary_additional_prompt(&local.additional_prompt);
            let r = summary_additional_prompt(&remote.additional_prompt);
            // For additional prompt, "absent" only when tombstoned; hash always exists.
            let mut l = l;
            let mut r = r;
            if local_hash.is_none() {
                l = absent_summary("Additional prompt");
            }
            if remote_hash.is_none() {
                r = absent_summary("Additional prompt");
            }
            (l, r, "Additional prompt".into())
        }
    };
    let _ = (local_hash, remote_hash);
    (local_sum, remote_sum, name)
}

/// Collect all entity keys involved in this sync.
fn collect_keys(
    local: &Config,
    remote: &SyncConfig,
    baseline: Option<&AccountSyncBaseline>,
) -> BTreeSet<EntityKey> {
    let mut keys = BTreeSet::new();
    for (k, _) in local_synced_hashes(local) {
        keys.insert(k);
    }
    for (k, _) in remote_entity_hashes(remote) {
        keys.insert(k);
    }
    for t in &remote.tombstones {
        keys.insert(t.key());
    }
    for id in &remote.removed_ids {
        // Expand legacy ids to all types that exist locally or remotely or in baseline.
        for ty in [
            SyncEntityType::Server,
            SyncEntityType::Authentication,
            SyncEntityType::Proxy,
            SyncEntityType::Snippet,
            SyncEntityType::AiChannel,
            SyncEntityType::AiModel,
            SyncEntityType::SftpCustomCommand,
        ] {
            let key = EntityKey::new(ty, id.clone());
            let local_hit = local_present_hash(local, &key).is_some()
                || match ty {
                    SyncEntityType::Server => local.servers.iter().any(|s| s.id == *id),
                    SyncEntityType::Authentication => {
                        local.authentications.iter().any(|a| a.id == *id)
                    }
                    SyncEntityType::Proxy => local.proxies.iter().any(|p| p.id == *id),
                    SyncEntityType::Snippet => local.snippets.iter().any(|s| s.id == *id),
                    SyncEntityType::AiChannel => local.ai_channels.iter().any(|c| c.id == *id),
                    SyncEntityType::AiModel => local.ai_models.iter().any(|m| m.id == *id),
                    SyncEntityType::SftpCustomCommand => {
                        local.sftp_custom_commands.iter().any(|c| c.id == *id)
                    }
                    SyncEntityType::AdditionalPrompt => false,
                };
            let remote_hit = remote_present_hash(remote, &key).is_some();
            let base_hit = baseline
                .map(|b| b.has_entity(&key) || b.has_tombstone(&key))
                .unwrap_or(false);
            if local_hit || remote_hit || base_hit {
                keys.insert(key);
            }
        }
    }
    if let Some(base) = baseline {
        for storage_key in base.entity_hashes.keys() {
            if let Some(k) = EntityKey::parse_storage_key(storage_key) {
                keys.insert(k);
            }
        }
        for t in &base.tombstones {
            keys.insert(t.key());
        }
    }
    keys.insert(EntityKey::new(
        SyncEntityType::AdditionalPrompt,
        ADDITIONAL_PROMPT_ENTITY_ID,
    ));
    keys
}

#[derive(Clone)]
enum ChosenValue {
    Local,
    Remote,
    /// A local `synced=false` row is explicitly local-only: it must not delete or overwrite its
    /// remote counterpart, and it must not be re-enabled by a sync merge.
    LocalOnly,
    Deleted,
}

/// Run three-way merge. When `conflicts` is non-empty, merged configs are `None`.
///
/// This pure convenience wrapper uses a fixed test-only token key. Production callers must use
/// `merge_configs_with_token_secret` with the per-account secret from `sync-state.json`.
pub fn merge_configs(
    local: &Config,
    remote: &SyncConfig,
    baseline: Option<&AccountSyncBaseline>,
    resolutions: &[SyncResolution],
) -> MergeProduct {
    merge_configs_with_token_secret(
        local,
        remote,
        baseline,
        resolutions,
        DIRECT_MERGE_TOKEN_SECRET,
    )
}

/// Run three-way merge with an account-private resolution-token key.
pub fn merge_configs_with_token_secret(
    local: &Config,
    remote: &SyncConfig,
    baseline: Option<&AccountSyncBaseline>,
    resolutions: &[SyncResolution],
    token_secret: &str,
) -> MergeProduct {
    let canonical_remote = match canonicalize_remote_tombstones(local, remote, baseline) {
        Ok(remote) => remote,
        Err(error) => {
            return MergeProduct {
                error: Some(error),
                conflicts: vec![],
                merged_local: None,
                merged_remote: None,
                baseline_hashes: BTreeMap::new(),
                baseline_tombstone_keys: vec![],
                changed_entity_count: 0,
            };
        }
    };
    let remote = &canonical_remote;
    let has_baseline = baseline.is_some_and(AccountSyncBaseline::has_sync_history);
    let keys = collect_keys(local, remote, baseline);
    let mut conflicts = Vec::new();
    let mut choices: BTreeMap<EntityKey, ChosenValue> = BTreeMap::new();
    let mut changed = 0usize;

    for key in &keys {
        let (local_hash, remote_hash, base_hash) =
            if key.entity_type == SyncEntityType::AdditionalPrompt {
                // Content always comparable (including None). First-sync treats empty as
                // absent so a one-sided prompt is a pure add, not a content conflict.
                let local_full = hash_additional_prompt(&local.additional_prompt);
                let remote_tombstoned = remote_has_tombstone(remote, key);
                let remote_full = if remote_tombstoned {
                    None
                } else {
                    Some(hash_additional_prompt(&remote.additional_prompt))
                };
                let base_full = baseline.and_then(|b| {
                    if b.has_tombstone(key) {
                        None
                    } else {
                        b.hash_for(key).map(|s| s.to_string())
                    }
                });
                if has_baseline {
                    (Some(local_full), remote_full, base_full)
                } else {
                    let l = local.additional_prompt.as_ref().map(|_| local_full.clone());
                    let r = remote_full.and_then(|h| remote.additional_prompt.as_ref().map(|_| h));
                    (l, r, None)
                }
            } else {
                let local_hash = local_present_hash(local, key);
                let remote_hash = remote_present_hash(remote, key);
                let base_hash = baseline.and_then(|b| {
                    if b.has_tombstone(key) {
                        None
                    } else {
                        b.hash_for(key).map(|s| s.to_string())
                    }
                });
                (local_hash, remote_hash, base_hash)
            };

        // The per-entity sync toggle is an opt-out, not a delete request. Preserve the local-only
        // row and leave the remote side untouched; explicit deletion is represented by a typed
        // tombstone or a conflict resolution, never by `synced=false`.
        if local_sync_is_disabled(local, key) {
            choices.insert(key.clone(), ChosenValue::LocalOnly);
            continue;
        }

        // First-sync safety: never reverse-wipe entire remote with empty local for same key set
        // is handled by decide_three_way (empty local + remote present + no base => UseRemote).

        let mut decision = decide_three_way(
            base_hash.as_deref(),
            local_hash.as_deref(),
            remote_hash.as_deref(),
            has_baseline,
        );

        // Empty additional prompt on both sides must not create a deletion tombstone.
        if key.entity_type == SyncEntityType::AdditionalPrompt
            && matches!(decision, AutoDecision::Delete)
            && local.additional_prompt.is_none()
            && remote.additional_prompt.is_none()
            && !remote_has_tombstone(remote, key)
        {
            decision = AutoDecision::KeepLocal;
        }

        // A tombstone is authoritative for deletion until the user explicitly chooses a restore.
        // If the local side still has the pre-delete base content, remote deletion can converge
        // automatically. Any local change or post-delete residual must instead be resolved.
        let remote_tombstoned = remote_has_tombstone(remote, key);
        let base_tombstoned = baseline.is_some_and(|b| b.has_tombstone(key));
        if local_hash.is_some()
            && remote_hash.is_none()
            && ((remote_tombstoned
                && (base_hash.is_none() || local_hash.as_deref() != base_hash.as_deref()))
                || base_tombstoned)
        {
            decision = AutoDecision::Conflict(SyncConflictKind::DeleteVsModify);
        }

        // Apply explicit resolutions for conflict outcomes
        if let AutoDecision::Conflict(kind) = decision {
            if let Some(res) = resolution_for(
                token_secret,
                resolutions,
                key,
                local_hash.as_deref(),
                remote_hash.as_deref(),
                kind,
            ) {
                decision = match res.choice {
                    SyncResolutionChoice::KeepLocal => {
                        if local_hash.is_some() {
                            AutoDecision::KeepLocal
                        } else {
                            AutoDecision::Delete
                        }
                    }
                    SyncResolutionChoice::UseRemote => {
                        if remote_hash.is_some() {
                            AutoDecision::UseRemote
                        } else {
                            AutoDecision::Delete
                        }
                    }
                };
            } else {
                let (ls, rs, name) = summaries_for(
                    key,
                    local,
                    remote,
                    local_hash.as_deref(),
                    remote_hash.as_deref(),
                );
                conflicts.push(build_conflict(
                    token_secret,
                    key.clone(),
                    kind,
                    name,
                    ls,
                    rs,
                ));
                continue;
            }
        }

        let chosen = match decision {
            AutoDecision::KeepLocal => ChosenValue::Local,
            AutoDecision::UseRemote => ChosenValue::Remote,
            AutoDecision::Delete => ChosenValue::Deleted,
            AutoDecision::Conflict(_) => unreachable!(),
        };

        // Count change relative to local synced view
        let local_side = if local_hash.is_some() {
            SidePresence::Present
        } else {
            SidePresence::Absent
        };
        let will_present = match &chosen {
            ChosenValue::Local => local_hash.is_some(),
            ChosenValue::Remote => remote_hash.is_some(),
            ChosenValue::LocalOnly => unreachable!("local-only choices skip decision counting"),
            ChosenValue::Deleted => false,
        };
        let content_change = match &chosen {
            ChosenValue::Local => false,
            ChosenValue::Remote => local_hash.as_deref() != remote_hash.as_deref(),
            ChosenValue::LocalOnly => unreachable!("local-only choices skip decision counting"),
            ChosenValue::Deleted => local_side == SidePresence::Present,
        };
        if content_change || (will_present != (local_side == SidePresence::Present)) {
            changed += 1;
        }

        choices.insert(key.clone(), chosen);
    }

    if !conflicts.is_empty() {
        return MergeProduct {
            error: None,
            conflicts,
            merged_local: None,
            merged_remote: None,
            baseline_hashes: BTreeMap::new(),
            baseline_tombstone_keys: vec![],
            changed_entity_count: 0,
        };
    }

    // Apply choices to build merged local + remote
    let lmap = local_maps(local);
    let rmap = remote_maps(remote);
    let mut out_local = local.clone();
    let mut out_remote = SyncConfig {
        version: remote.version.clone(),
        sync_schema: Some(SYNC_SCHEMA_VERSION),
        revision: remote.revision.clone(),
        servers: vec![],
        authentications: vec![],
        proxies: vec![],
        snippets: vec![],
        ai_channels: vec![],
        ai_models: vec![],
        sftp_custom_commands: vec![],
        additional_prompt: None,
        additional_prompt_updated_at: None,
        tombstones: vec![],
        removed_ids: vec![],
    };

    // Preserve non-synced local-only entities first
    out_local.servers = local
        .servers
        .iter()
        .filter(|s| !s.synced)
        .cloned()
        .collect();
    out_local.authentications = local
        .authentications
        .iter()
        .filter(|a| !a.synced)
        .cloned()
        .collect();
    out_local.proxies = local
        .proxies
        .iter()
        .filter(|p| !p.synced)
        .cloned()
        .collect();
    out_local.snippets = local
        .snippets
        .iter()
        .filter(|s| !s.synced)
        .cloned()
        .collect();
    out_local.ai_channels = local
        .ai_channels
        .iter()
        .filter(|c| !c.synced)
        .cloned()
        .collect();
    out_local.ai_models = local
        .ai_models
        .iter()
        .filter(|m| !m.synced)
        .cloned()
        .collect();
    out_local.sftp_custom_commands = local
        .sftp_custom_commands
        .iter()
        .filter(|c| !c.synced)
        .cloned()
        .collect();

    let mut baseline_hashes = BTreeMap::new();
    let mut tombstone_keys = Vec::new();

    for (key, choice) in &choices {
        match key.entity_type {
            SyncEntityType::AdditionalPrompt => {
                match choice {
                    ChosenValue::Local => {
                        out_local.additional_prompt = local.additional_prompt.clone();
                        out_local.additional_prompt_updated_at =
                            local.additional_prompt_updated_at.clone();
                        out_remote.additional_prompt = local.additional_prompt.clone();
                        out_remote.additional_prompt_updated_at =
                            local.additional_prompt_updated_at.clone();
                        baseline_hashes.insert(
                            key.clone(),
                            hash_additional_prompt(&out_local.additional_prompt),
                        );
                    }
                    ChosenValue::Remote => {
                        out_local.additional_prompt = remote.additional_prompt.clone();
                        out_local.additional_prompt_updated_at =
                            remote.additional_prompt_updated_at.clone();
                        out_remote.additional_prompt = remote.additional_prompt.clone();
                        out_remote.additional_prompt_updated_at =
                            remote.additional_prompt_updated_at.clone();
                        baseline_hashes.insert(
                            key.clone(),
                            hash_additional_prompt(&out_remote.additional_prompt),
                        );
                    }
                    ChosenValue::LocalOnly => unreachable!("additional prompt has no sync toggle"),
                    ChosenValue::Deleted => {
                        out_local.additional_prompt = None;
                        out_local.additional_prompt_updated_at = None;
                        out_remote.additional_prompt = None;
                        out_remote.additional_prompt_updated_at = None;
                        tombstone_keys.push(key.clone());
                        out_remote
                            .tombstones
                            .push(DeletionTombstone::new(key.entity_type, key.id.clone()));
                    }
                }
                continue;
            }
            _ => {}
        }

        match choice {
            ChosenValue::Deleted => {
                // Soft-delete on local if residual exists
                soft_delete_local(&mut out_local, key, &lmap);
                tombstone_keys.push(key.clone());
                out_remote
                    .tombstones
                    .push(DeletionTombstone::new(key.entity_type, key.id.clone()));
            }
            ChosenValue::Local => {
                if let Some(entity) = take_local_entity(&lmap, key) {
                    push_local_synced(&mut out_local, entity.clone());
                    push_remote_entity(&mut out_remote, entity, key);
                    if let Some(h) = local_present_hash(local, key) {
                        baseline_hashes.insert(key.clone(), h);
                    }
                }
            }
            ChosenValue::LocalOnly => {
                // The local-only row was preserved above. Keep the remote document and its
                // baseline independently, but never re-enable or overwrite the local row.
                if let Some(entity) = take_remote_entity(&rmap, key) {
                    push_remote_entity(&mut out_remote, entity, key);
                    if let Some(h) = remote_present_hash(remote, key) {
                        baseline_hashes.insert(key.clone(), h);
                    }
                }
            }
            ChosenValue::Remote => {
                if let Some(entity) = take_remote_entity(&rmap, key) {
                    push_local_synced(&mut out_local, entity.clone());
                    push_remote_entity(&mut out_remote, entity, key);
                    if let Some(h) = remote_present_hash(remote, key) {
                        baseline_hashes.insert(key.clone(), h);
                    }
                }
            }
        }
    }

    // Keep every remote tombstone that still applies, including legacy `removedIds` entries in
    // mixed-format documents. A local choice is the only explicit restoration path.
    for key in &keys {
        if !remote_has_tombstone(remote, key)
            || matches!(choices.get(key), Some(ChosenValue::Local))
            || remote_entity_in_out(&out_remote, key)
        {
            continue;
        }
        if !out_remote
            .tombstones
            .iter()
            .any(|t| t.entity_type == key.entity_type && t.id == key.id)
        {
            let tombstone = remote
                .tombstones
                .iter()
                .find(|t| t.entity_type == key.entity_type && t.id == key.id)
                .cloned()
                .unwrap_or(DeletionTombstone {
                    entity_type: key.entity_type,
                    id: key.id.clone(),
                    deleted_at: None,
                });
            out_remote.tombstones.push(tombstone);
        }
        if !tombstone_keys.contains(key) {
            tombstone_keys.push(key.clone());
        }
    }

    out_remote.removed_ids = rebuild_removed_ids(&out_remote.tombstones);

    // Reference integrity
    if let Err(err) = validate_references(&out_local) {
        let key = EntityKey::new(SyncEntityType::Server, "_reference_integrity");
        conflicts.push(build_conflict(
            token_secret,
            key,
            SyncConflictKind::ReferenceIntegrity,
            "Reference integrity".into(),
            EntitySummary {
                display_name: "Configuration".into(),
                details: err.message.clone(),
                content_hash: None,
                present: true,
            },
            EntitySummary {
                display_name: "Configuration".into(),
                details: "merge result invalid".into(),
                content_hash: None,
                present: true,
            },
        ));
        return MergeProduct {
            error: None,
            conflicts,
            merged_local: None,
            merged_remote: None,
            baseline_hashes: BTreeMap::new(),
            baseline_tombstone_keys: vec![],
            changed_entity_count: 0,
        };
    }

    MergeProduct {
        error: None,
        conflicts: vec![],
        merged_local: Some(out_local),
        merged_remote: Some(out_remote),
        baseline_hashes,
        baseline_tombstone_keys: tombstone_keys,
        changed_entity_count: changed,
    }
}

#[derive(Clone)]
enum AnyEntity {
    Server(Server),
    Auth(Authentication),
    Proxy(Proxy),
    Snippet(Snippet),
    Channel(AiChannel),
    Model(AiModel),
    Command(SftpCustomCommand),
}

fn take_local_entity(map: &EntityMaps, key: &EntityKey) -> Option<AnyEntity> {
    match key.entity_type {
        SyncEntityType::Server => map.servers.get(&key.id).cloned().map(AnyEntity::Server),
        SyncEntityType::Authentication => map.auths.get(&key.id).cloned().map(AnyEntity::Auth),
        SyncEntityType::Proxy => map.proxies.get(&key.id).cloned().map(AnyEntity::Proxy),
        SyncEntityType::Snippet => map.snippets.get(&key.id).cloned().map(AnyEntity::Snippet),
        SyncEntityType::AiChannel => map.channels.get(&key.id).cloned().map(AnyEntity::Channel),
        SyncEntityType::AiModel => map.models.get(&key.id).cloned().map(AnyEntity::Model),
        SyncEntityType::SftpCustomCommand => {
            map.commands.get(&key.id).cloned().map(AnyEntity::Command)
        }
        SyncEntityType::AdditionalPrompt => None,
    }
}

fn take_remote_entity(map: &EntityMaps, key: &EntityKey) -> Option<AnyEntity> {
    take_local_entity(map, key)
}

fn push_local_synced(local: &mut Config, entity: AnyEntity) {
    match entity {
        AnyEntity::Server(mut s) => {
            s.synced = true;
            // Replace if soft-deleted residual same id
            local.servers.retain(|x| x.id != s.id);
            local.servers.push(s);
        }
        AnyEntity::Auth(mut a) => {
            a.synced = true;
            local.authentications.retain(|x| x.id != a.id);
            local.authentications.push(a);
        }
        AnyEntity::Proxy(mut p) => {
            p.synced = true;
            local.proxies.retain(|x| x.id != p.id);
            local.proxies.push(p);
        }
        AnyEntity::Snippet(mut s) => {
            s.synced = true;
            local.snippets.retain(|x| x.id != s.id);
            local.snippets.push(s);
        }
        AnyEntity::Channel(mut c) => {
            c.synced = true;
            local.ai_channels.retain(|x| x.id != c.id);
            local.ai_channels.push(c);
        }
        AnyEntity::Model(mut m) => {
            m.synced = true;
            local.ai_models.retain(|x| x.id != m.id);
            local.ai_models.push(m);
        }
        AnyEntity::Command(mut c) => {
            c.synced = true;
            local.sftp_custom_commands.retain(|x| x.id != c.id);
            local.sftp_custom_commands.push(c);
        }
    }
}

fn push_remote_entity(remote: &mut SyncConfig, entity: AnyEntity, key: &EntityKey) {
    // Restoring removes tombstone
    remote
        .tombstones
        .retain(|t| !(t.entity_type == key.entity_type && t.id == key.id));
    match entity {
        AnyEntity::Server(mut s) => {
            s.synced = true;
            remote.servers.push(s);
        }
        AnyEntity::Auth(mut a) => {
            a.synced = true;
            remote.authentications.push(a);
        }
        AnyEntity::Proxy(mut p) => {
            p.synced = true;
            remote.proxies.push(p);
        }
        AnyEntity::Snippet(mut s) => {
            s.synced = true;
            remote.snippets.push(s);
        }
        AnyEntity::Channel(mut c) => {
            c.synced = true;
            remote.ai_channels.push(c);
        }
        AnyEntity::Model(mut m) => {
            m.synced = true;
            remote.ai_models.push(m);
        }
        AnyEntity::Command(mut c) => {
            c.synced = true;
            remote.sftp_custom_commands.push(c);
        }
    }
}

fn soft_delete_local(local: &mut Config, key: &EntityKey, lmap: &EntityMaps) {
    match key.entity_type {
        SyncEntityType::Server => {
            if let Some(mut s) = lmap.servers.get(&key.id).cloned() {
                s.synced = false;
                local.servers.retain(|x| x.id != s.id);
                local.servers.push(s);
            }
        }
        SyncEntityType::Authentication => {
            if let Some(mut a) = lmap.auths.get(&key.id).cloned() {
                a.synced = false;
                local.authentications.retain(|x| x.id != a.id);
                local.authentications.push(a);
            }
        }
        SyncEntityType::Proxy => {
            if let Some(mut p) = lmap.proxies.get(&key.id).cloned() {
                p.synced = false;
                local.proxies.retain(|x| x.id != p.id);
                local.proxies.push(p);
            }
        }
        SyncEntityType::Snippet => {
            if let Some(mut s) = lmap.snippets.get(&key.id).cloned() {
                s.synced = false;
                local.snippets.retain(|x| x.id != s.id);
                local.snippets.push(s);
            }
        }
        SyncEntityType::AiChannel => {
            if let Some(mut c) = lmap.channels.get(&key.id).cloned() {
                c.synced = false;
                local.ai_channels.retain(|x| x.id != c.id);
                local.ai_channels.push(c);
            }
        }
        SyncEntityType::AiModel => {
            if let Some(mut m) = lmap.models.get(&key.id).cloned() {
                m.synced = false;
                local.ai_models.retain(|x| x.id != m.id);
                local.ai_models.push(m);
            }
        }
        SyncEntityType::SftpCustomCommand => {
            if let Some(mut c) = lmap.commands.get(&key.id).cloned() {
                c.synced = false;
                local.sftp_custom_commands.retain(|x| x.id != c.id);
                local.sftp_custom_commands.push(c);
            }
        }
        SyncEntityType::AdditionalPrompt => {}
    }
}

fn remote_entity_in_out(remote: &SyncConfig, key: &EntityKey) -> bool {
    match key.entity_type {
        SyncEntityType::Server => remote.servers.iter().any(|s| s.id == key.id),
        SyncEntityType::Authentication => remote.authentications.iter().any(|a| a.id == key.id),
        SyncEntityType::Proxy => remote.proxies.iter().any(|p| p.id == key.id),
        SyncEntityType::Snippet => remote.snippets.iter().any(|s| s.id == key.id),
        SyncEntityType::AiChannel => remote.ai_channels.iter().any(|c| c.id == key.id),
        SyncEntityType::AiModel => remote.ai_models.iter().any(|m| m.id == key.id),
        SyncEntityType::SftpCustomCommand => {
            remote.sftp_custom_commands.iter().any(|c| c.id == key.id)
        }
        SyncEntityType::AdditionalPrompt => remote.additional_prompt.is_some(),
    }
}

/// Validate server/auth/proxy/jumphost and AI model/channel references among **synced** entities.
pub fn validate_references(config: &Config) -> Result<(), SyncError> {
    let auth_ids: BTreeSet<_> = config
        .authentications
        .iter()
        .filter(|a| a.synced)
        .map(|a| a.id.as_str())
        .collect();
    let proxy_ids: BTreeSet<_> = config
        .proxies
        .iter()
        .filter(|p| p.synced)
        .map(|p| p.id.as_str())
        .collect();
    let server_ids: BTreeSet<_> = config
        .servers
        .iter()
        .filter(|s| s.synced)
        .map(|s| s.id.as_str())
        .collect();
    let channel_ids: BTreeSet<_> = config
        .ai_channels
        .iter()
        .filter(|c| c.synced)
        .map(|c| c.id.as_str())
        .collect();

    for s in config.servers.iter().filter(|s| s.synced) {
        if let Some(ref id) = s.auth_id {
            if !id.is_empty() && !auth_ids.contains(id.as_str()) {
                return Err(SyncError {
                    kind: SyncErrorKind::ReferenceIntegrity,
                    message: format!(
                        "Server '{}' references missing authentication '{}'",
                        s.name, id
                    ),
                });
            }
        }
        if let Some(ref id) = s.proxy_id {
            if !id.is_empty() && !proxy_ids.contains(id.as_str()) {
                return Err(SyncError {
                    kind: SyncErrorKind::ReferenceIntegrity,
                    message: format!("Server '{}' references missing proxy '{}'", s.name, id),
                });
            }
        }
        if let Some(ref id) = s.jumphost_id {
            if !id.is_empty() && !server_ids.contains(id.as_str()) {
                return Err(SyncError {
                    kind: SyncErrorKind::ReferenceIntegrity,
                    message: format!("Server '{}' references missing jumphost '{}'", s.name, id),
                });
            }
        }
    }

    for c in config.ai_channels.iter().filter(|c| c.synced) {
        if let Some(ref id) = c.proxy_id {
            if !id.is_empty() && !proxy_ids.contains(id.as_str()) {
                return Err(SyncError {
                    kind: SyncErrorKind::ReferenceIntegrity,
                    message: format!("AI channel '{}' references missing proxy '{}'", c.name, id),
                });
            }
        }
    }

    for m in config.ai_models.iter().filter(|m| m.synced) {
        if !m.channel_id.is_empty() && !channel_ids.contains(m.channel_id.as_str()) {
            return Err(SyncError {
                kind: SyncErrorKind::ReferenceIntegrity,
                message: format!(
                    "AI model '{}' references missing channel '{}'",
                    m.name, m.channel_id
                ),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::sync_state::AccountSyncBaseline;

    fn sample_server(id: &str, name: &str) -> Server {
        Server {
            id: id.into(),
            name: name.into(),
            group: "g".into(),
            host: "h.example".into(),
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
        }
    }

    fn empty_remote() -> SyncConfig {
        SyncConfig {
            version: "1.0".into(),
            sync_schema: None,
            revision: None,
            servers: vec![],
            authentications: vec![],
            proxies: vec![],
            snippets: vec![],
            ai_channels: vec![],
            ai_models: vec![],
            sftp_custom_commands: vec![],
            additional_prompt: None,
            additional_prompt_updated_at: None,
            tombstones: vec![],
            removed_ids: vec![],
        }
    }

    fn full_sync_config() -> Config {
        let mut config = Config::empty();
        config.authentications.push(Authentication {
            id: "auth".into(),
            name: "Authentication base".into(),
            auth_type: "password".into(),
            key_content: None,
            passphrase: None,
            password: Some("secret".into()),
            synced: true,
            updated_at: "2020-01-01T00:00:00Z".into(),
        });
        config.proxies.push(Proxy {
            id: "proxy".into(),
            name: "Proxy base".into(),
            proxy_type: "http".into(),
            host: "proxy.example".into(),
            port: 8080,
            username: None,
            password: None,
            ignore_ssl_errors: false,
            synced: true,
            updated_at: "2020-01-01T00:00:00Z".into(),
        });
        let mut server = sample_server("server", "Server base");
        server.auth_id = Some("auth".into());
        server.proxy_id = Some("proxy".into());
        config.servers.push(server);
        config.snippets.push(Snippet {
            id: "snippet".into(),
            name: "Snippet base".into(),
            content: "echo base".into(),
            description: None,
            group: None,
            synced: true,
            updated_at: "2020-01-01T00:00:00Z".into(),
        });
        config.ai_channels.push(AiChannel {
            id: "channel".into(),
            name: "Channel base".into(),
            provider: "openai".into(),
            endpoint: Some("https://api.example".into()),
            api_key: Some("key".into()),
            proxy_id: Some("proxy".into()),
            is_active: true,
            synced: true,
            updated_at: "2020-01-01T00:00:00Z".into(),
        });
        config.ai_models.push(AiModel {
            id: "model".into(),
            name: "Model base".into(),
            channel_id: "channel".into(),
            context_window: None,
            response_reserve: None,
            enabled: true,
            synced: true,
            updated_at: "2020-01-01T00:00:00Z".into(),
        });
        config.sftp_custom_commands.push(SftpCustomCommand {
            id: "command".into(),
            name: "Command base".into(),
            pattern: "*.log".into(),
            command: "tail -f".into(),
            synced: true,
            updated_at: "2020-01-01T00:00:00Z".into(),
        });
        config.additional_prompt = Some("Prompt base".into());
        config
    }

    fn remote_from_config(config: &Config) -> SyncConfig {
        SyncConfig {
            version: config.version.clone(),
            sync_schema: Some(SYNC_SCHEMA_VERSION),
            revision: None,
            servers: config.servers.clone(),
            authentications: config.authentications.clone(),
            proxies: config.proxies.clone(),
            snippets: config.snippets.clone(),
            ai_channels: config.ai_channels.clone(),
            ai_models: config.ai_models.clone(),
            sftp_custom_commands: config.sftp_custom_commands.clone(),
            additional_prompt: config.additional_prompt.clone(),
            additional_prompt_updated_at: config.additional_prompt_updated_at.clone(),
            tombstones: vec![],
            removed_ids: vec![],
        }
    }

    fn baseline_for_config(config: &Config) -> AccountSyncBaseline {
        let mut baseline = AccountSyncBaseline::default();
        for (key, hash) in local_synced_hashes(config) {
            baseline.set_entity_hash(&key, hash);
        }
        baseline.sync_schema = SYNC_SCHEMA_VERSION;
        baseline
    }

    fn renamed_full_config(mut config: Config, suffix: &str) -> Config {
        config.servers[0].name = format!("Server {suffix}");
        config.authentications[0].name = format!("Authentication {suffix}");
        config.proxies[0].name = format!("Proxy {suffix}");
        config.snippets[0].name = format!("Snippet {suffix}");
        config.snippets[0].content = format!("echo {suffix}");
        config.ai_channels[0].name = format!("Channel {suffix}");
        config.ai_models[0].name = format!("Model {suffix}");
        config.sftp_custom_commands[0].name = format!("Command {suffix}");
        config.additional_prompt = Some(format!("Prompt {suffix}"));
        config
    }

    fn assert_all_entity_values(config: &Config, suffix: &str) {
        assert_eq!(config.servers[0].name, format!("Server {suffix}"));
        assert_eq!(
            config.authentications[0].name,
            format!("Authentication {suffix}")
        );
        assert_eq!(config.proxies[0].name, format!("Proxy {suffix}"));
        assert_eq!(config.snippets[0].content, format!("echo {suffix}"));
        assert_eq!(config.ai_channels[0].name, format!("Channel {suffix}"));
        assert_eq!(config.ai_models[0].name, format!("Model {suffix}"));
        assert_eq!(
            config.sftp_custom_commands[0].name,
            format!("Command {suffix}")
        );
        let expected_prompt = format!("Prompt {suffix}");
        assert_eq!(
            config.additional_prompt.as_deref(),
            Some(expected_prompt.as_str())
        );
    }

    fn assert_all_remote_entity_values(config: &SyncConfig, suffix: &str) {
        assert_eq!(config.servers[0].name, format!("Server {suffix}"));
        assert_eq!(
            config.authentications[0].name,
            format!("Authentication {suffix}")
        );
        assert_eq!(config.proxies[0].name, format!("Proxy {suffix}"));
        assert_eq!(config.snippets[0].content, format!("echo {suffix}"));
        assert_eq!(config.ai_channels[0].name, format!("Channel {suffix}"));
        assert_eq!(config.ai_models[0].name, format!("Model {suffix}"));
        assert_eq!(
            config.sftp_custom_commands[0].name,
            format!("Command {suffix}")
        );
        let expected_prompt = format!("Prompt {suffix}");
        assert_eq!(
            config.additional_prompt.as_deref(),
            Some(expected_prompt.as_str())
        );
    }

    #[test]
    fn every_sync_entity_type_follows_the_three_way_merge_matrix() {
        let base = full_sync_config();
        let baseline = baseline_for_config(&base);

        let unchanged = merge_configs(&base, &remote_from_config(&base), Some(&baseline), &[]);
        assert!(unchanged.conflicts.is_empty());
        assert_all_entity_values(&unchanged.merged_local.unwrap(), "base");

        let local_changed = renamed_full_config(base.clone(), "local");
        let local_only = merge_configs(
            &local_changed,
            &remote_from_config(&base),
            Some(&baseline),
            &[],
        );
        assert!(local_only.conflicts.is_empty());
        assert_all_remote_entity_values(&local_only.merged_remote.unwrap(), "local");

        let identical_both_sides = merge_configs(
            &local_changed,
            &remote_from_config(&local_changed),
            Some(&baseline),
            &[],
        );
        assert!(identical_both_sides.conflicts.is_empty());
        assert_all_entity_values(&identical_both_sides.merged_local.unwrap(), "local");

        let remote_changed = renamed_full_config(base.clone(), "remote");
        let remote_only = merge_configs(
            &base,
            &remote_from_config(&remote_changed),
            Some(&baseline),
            &[],
        );
        assert!(remote_only.conflicts.is_empty());
        assert_all_entity_values(&remote_only.merged_local.unwrap(), "remote");

        let all_entity_types = BTreeSet::from([
            SyncEntityType::Server,
            SyncEntityType::Authentication,
            SyncEntityType::Proxy,
            SyncEntityType::Snippet,
            SyncEntityType::AiChannel,
            SyncEntityType::AiModel,
            SyncEntityType::SftpCustomCommand,
            SyncEntityType::AdditionalPrompt,
        ]);
        let delete_vs_modify = merge_configs(
            &Config::empty(),
            &remote_from_config(&remote_changed),
            Some(&baseline),
            &[],
        );
        assert_eq!(
            delete_vs_modify
                .conflicts
                .iter()
                .map(|conflict| conflict.entity_type)
                .collect::<BTreeSet<_>>(),
            all_entity_types
        );

        let conflicted = merge_configs(
            &local_changed,
            &remote_from_config(&remote_changed),
            Some(&baseline),
            &[],
        );
        let entity_types: BTreeSet<_> = conflicted
            .conflicts
            .iter()
            .map(|conflict| conflict.entity_type)
            .collect();
        assert_eq!(
            entity_types,
            BTreeSet::from([
                SyncEntityType::Server,
                SyncEntityType::Authentication,
                SyncEntityType::Proxy,
                SyncEntityType::Snippet,
                SyncEntityType::AiChannel,
                SyncEntityType::AiModel,
                SyncEntityType::SftpCustomCommand,
                SyncEntityType::AdditionalPrompt,
            ])
        );
        assert!(conflicted
            .conflicts
            .iter()
            .all(|conflict| conflict.kind == SyncConflictKind::BothModified));

        let resolutions = conflicted
            .conflicts
            .iter()
            .map(|conflict| SyncResolution {
                entity_type: conflict.entity_type,
                id: conflict.id.clone(),
                choice: SyncResolutionChoice::KeepLocal,
                resolution_token: conflict.resolution_token.clone(),
            })
            .collect::<Vec<_>>();
        let resolved = merge_configs(
            &local_changed,
            &remote_from_config(&remote_changed),
            Some(&baseline),
            &resolutions,
        );
        assert!(resolved.conflicts.is_empty());
        assert_all_remote_entity_values(&resolved.merged_remote.unwrap(), "local");
    }

    #[test]
    fn three_way_table_core_cases() {
        assert_eq!(
            decide_three_way(None, Some("L"), None, false),
            AutoDecision::KeepLocal
        );
        assert_eq!(
            decide_three_way(None, None, Some("R"), false),
            AutoDecision::UseRemote
        );
        assert_eq!(
            decide_three_way(None, Some("L"), Some("L"), false),
            AutoDecision::KeepLocal
        );
        assert_eq!(
            decide_three_way(None, Some("L"), Some("R"), false),
            AutoDecision::Conflict(SyncConflictKind::FirstSyncMismatch)
        );
        assert_eq!(
            decide_three_way(Some("B"), Some("B"), Some("R"), true),
            AutoDecision::UseRemote
        );
        assert_eq!(
            decide_three_way(Some("B"), Some("L"), Some("B"), true),
            AutoDecision::KeepLocal
        );
        assert_eq!(
            decide_three_way(Some("B"), Some("L"), Some("R"), true),
            AutoDecision::Conflict(SyncConflictKind::BothModified)
        );
        assert_eq!(
            decide_three_way(Some("B"), None, Some("B"), true),
            AutoDecision::Delete
        );
        assert_eq!(
            decide_three_way(Some("B"), None, Some("R"), true),
            AutoDecision::Conflict(SyncConflictKind::DeleteVsModify)
        );
        assert_eq!(
            decide_three_way(Some("B"), Some("B"), None, true),
            AutoDecision::Delete
        );
        assert_eq!(
            decide_three_way(Some("B"), Some("L"), None, true),
            AutoDecision::Conflict(SyncConflictKind::DeleteVsModify)
        );
    }

    #[test]
    fn first_sync_union_without_wiping() {
        let mut local = Config::empty();
        local.servers.push(sample_server("local-only", "L"));
        let mut remote = empty_remote();
        remote.servers.push(sample_server("remote-only", "R"));

        let product = merge_configs(&local, &remote, None, &[]);
        assert!(product.conflicts.is_empty(), "{:?}", product.conflicts);
        let merged = product.merged_local.unwrap();
        assert_eq!(merged.servers.len(), 2);
        assert!(merged.servers.iter().any(|s| s.id == "local-only"));
        assert!(merged.servers.iter().any(|s| s.id == "remote-only"));
    }

    #[test]
    fn first_sync_same_id_different_content_conflicts() {
        let mut local = Config::empty();
        local.servers.push(sample_server("s1", "Local Name"));
        let mut remote = empty_remote();
        remote.servers.push(sample_server("s1", "Remote Name"));

        let product = merge_configs(&local, &remote, None, &[]);
        assert_eq!(product.conflicts.len(), 1);
        assert_eq!(
            product.conflicts[0].kind,
            SyncConflictKind::FirstSyncMismatch
        );
        assert!(product.merged_local.is_none());
    }

    #[test]
    fn updated_at_skew_does_not_win() {
        let mut local = Config::empty();
        let mut s = sample_server("s1", "Same");
        s.updated_at = "2099-01-01T00:00:00Z".into();
        local.servers.push(s);
        let mut remote = empty_remote();
        let mut rs = sample_server("s1", "Same");
        rs.updated_at = "2000-01-01T00:00:00Z".into();
        remote.servers.push(rs);

        let product = merge_configs(&local, &remote, None, &[]);
        assert!(product.conflicts.is_empty());
        assert!(product.merged_local.is_some());
    }

    #[test]
    fn both_modified_with_baseline_conflicts_until_resolved() {
        let mut local = Config::empty();
        local.servers.push(sample_server("s1", "Local"));
        let mut remote = empty_remote();
        remote.servers.push(sample_server("s1", "Remote"));

        let base_hash = hash_server(&sample_server("s1", "Base"));
        let mut baseline = AccountSyncBaseline::default();
        baseline.set_entity_hash(&EntityKey::new(SyncEntityType::Server, "s1"), base_hash);

        let product = merge_configs(&local, &remote, Some(&baseline), &[]);
        assert_eq!(product.conflicts.len(), 1);
        assert_eq!(product.conflicts[0].kind, SyncConflictKind::BothModified);

        let token = product.conflicts[0].resolution_token.clone();
        let resolutions = vec![SyncResolution {
            entity_type: SyncEntityType::Server,
            id: "s1".into(),
            choice: SyncResolutionChoice::UseRemote,
            resolution_token: token,
        }];
        let product = merge_configs(&local, &remote, Some(&baseline), &resolutions);
        assert!(product.conflicts.is_empty());
        let merged = product.merged_local.unwrap();
        assert_eq!(merged.servers[0].name, "Remote");
    }

    #[test]
    fn delete_vs_modify_conflicts() {
        let mut local = Config::empty();
        local.servers.push(sample_server("s1", "Changed"));
        let remote = empty_remote(); // remote deleted

        let base_hash = hash_server(&sample_server("s1", "Base"));
        let mut baseline = AccountSyncBaseline::default();
        baseline.set_entity_hash(&EntityKey::new(SyncEntityType::Server, "s1"), base_hash);

        let product = merge_configs(&local, &remote, Some(&baseline), &[]);
        assert_eq!(product.conflicts.len(), 1);
        assert_eq!(product.conflicts[0].kind, SyncConflictKind::DeleteVsModify);
    }

    #[test]
    fn additional_prompt_three_way() {
        let mut local = Config::empty();
        local.additional_prompt = Some("local-prompt".into());
        let mut remote = empty_remote();
        remote.additional_prompt = Some("remote-prompt".into());

        let mut baseline = AccountSyncBaseline::default();
        baseline.set_entity_hash(
            &EntityKey::new(
                SyncEntityType::AdditionalPrompt,
                ADDITIONAL_PROMPT_ENTITY_ID,
            ),
            hash_additional_prompt(&Some("base".into())),
        );

        let product = merge_configs(&local, &remote, Some(&baseline), &[]);
        assert_eq!(product.conflicts.len(), 1);
        assert_eq!(
            product.conflicts[0].entity_type,
            SyncEntityType::AdditionalPrompt
        );

        // Single side change
        let mut baseline2 = AccountSyncBaseline::default();
        baseline2.set_entity_hash(
            &EntityKey::new(
                SyncEntityType::AdditionalPrompt,
                ADDITIONAL_PROMPT_ENTITY_ID,
            ),
            hash_additional_prompt(&Some("remote-prompt".into())),
        );
        let product = merge_configs(&local, &remote, Some(&baseline2), &[]);
        assert!(product.conflicts.is_empty());
        assert_eq!(
            product.merged_local.unwrap().additional_prompt.as_deref(),
            Some("local-prompt")
        );
    }

    #[test]
    fn reference_integrity_fails_on_missing_auth() {
        let mut local = Config::empty();
        let mut s = sample_server("s1", "Srv");
        s.auth_id = Some("missing-auth".into());
        local.servers.push(s);
        let remote = empty_remote();
        let product = merge_configs(&local, &remote, None, &[]);
        assert!(product
            .conflicts
            .iter()
            .any(|c| c.kind == SyncConflictKind::ReferenceIntegrity));
    }

    #[test]
    fn disabling_sync_preserves_remote_without_creating_a_tombstone() {
        let mut local = Config::empty();
        let mut local_only = sample_server("s1", "Local only");
        local_only.synced = false;
        local.servers.push(local_only);
        let mut remote = empty_remote();
        remote.servers.push(sample_server("s1", "Remote copy"));

        let product = merge_configs(&local, &remote, None, &[]);
        assert!(product.conflicts.is_empty());
        let merged_local = product.merged_local.unwrap();
        let merged_remote = product.merged_remote.unwrap();
        assert!(!merged_local.servers[0].synced);
        assert_eq!(merged_local.servers[0].name, "Local only");
        assert_eq!(merged_remote.servers[0].name, "Remote copy");
        assert!(merged_remote.tombstones.is_empty());
    }

    #[test]
    fn tombstone_residual_requires_explicit_restore_resolution() {
        let mut local = Config::empty();
        local.servers.push(sample_server("s1", "Residual"));
        let mut remote = empty_remote();
        remote
            .tombstones
            .push(DeletionTombstone::new(SyncEntityType::Server, "s1"));
        let key = EntityKey::new(SyncEntityType::Server, "s1");
        let mut baseline = AccountSyncBaseline::default();
        baseline.mark_deleted(&key);

        let product = merge_configs(&local, &remote, Some(&baseline), &[]);
        assert_eq!(product.conflicts.len(), 1);
        assert_eq!(product.conflicts[0].kind, SyncConflictKind::DeleteVsModify);

        let resolution = SyncResolution {
            entity_type: SyncEntityType::Server,
            id: "s1".into(),
            choice: SyncResolutionChoice::KeepLocal,
            resolution_token: product.conflicts[0].resolution_token.clone(),
        };
        let resolved = merge_configs(&local, &remote, Some(&baseline), &[resolution]);
        assert!(resolved.conflicts.is_empty());
        let merged_remote = resolved.merged_remote.unwrap();
        assert_eq!(merged_remote.servers.len(), 1);
        assert!(merged_remote.tombstones.is_empty());
    }

    #[test]
    fn one_sided_delete_converges_to_a_typed_tombstone() {
        let mut remote = empty_remote();
        remote.servers.push(sample_server("s1", "Base"));
        let key = EntityKey::new(SyncEntityType::Server, "s1");
        let mut baseline = AccountSyncBaseline::default();
        baseline.set_entity_hash(&key, hash_server(&sample_server("s1", "Base")));

        // Local removed the entity while remote is unchanged.
        let product = merge_configs(&Config::empty(), &remote, Some(&baseline), &[]);
        assert!(product.conflicts.is_empty());

        let merged_remote = product.merged_remote.unwrap();
        assert!(merged_remote.servers.is_empty());
        assert!(merged_remote
            .tombstones
            .iter()
            .any(|t| t.entity_type == SyncEntityType::Server && t.id == "s1"));
        assert_eq!(merged_remote.removed_ids, vec!["s1"]);
    }

    #[test]
    fn choosing_remote_delete_retains_the_tombstone() {
        let mut local = Config::empty();
        local
            .servers
            .push(sample_server("s1", "Local modification"));
        let mut remote = empty_remote();
        remote
            .tombstones
            .push(DeletionTombstone::new(SyncEntityType::Server, "s1"));
        let key = EntityKey::new(SyncEntityType::Server, "s1");
        let mut baseline = AccountSyncBaseline::default();
        baseline.set_entity_hash(&key, hash_server(&sample_server("s1", "Base")));

        let conflicted = merge_configs(&local, &remote, Some(&baseline), &[]);
        assert_eq!(conflicted.conflicts.len(), 1);
        let resolution = SyncResolution {
            entity_type: SyncEntityType::Server,
            id: "s1".into(),
            choice: SyncResolutionChoice::UseRemote,
            resolution_token: conflicted.conflicts[0].resolution_token.clone(),
        };

        let resolved = merge_configs(&local, &remote, Some(&baseline), &[resolution]);
        assert!(resolved.conflicts.is_empty());
        let merged_local = resolved.merged_local.unwrap();
        assert!(merged_local
            .servers
            .iter()
            .any(|server| server.id == "s1" && !server.synced));
        let merged_remote = resolved.merged_remote.unwrap();
        assert!(merged_remote.servers.is_empty());
        assert!(merged_remote
            .tombstones
            .iter()
            .any(|t| t.entity_type == SyncEntityType::Server && t.id == "s1"));
    }

    #[test]
    fn legacy_removed_id_is_migrated_only_when_its_type_is_unique() {
        let mut local = Config::empty();
        local.servers.push(sample_server("shared", "Server"));
        let mut remote = empty_remote();
        remote.removed_ids.push("shared".into());

        let normalized = canonicalize_remote_tombstones(&local, &remote, None).unwrap();
        assert!(normalized
            .tombstones
            .iter()
            .any(|t| t.entity_type == SyncEntityType::Server && t.id == "shared"));
    }

    #[test]
    fn ambiguous_legacy_removed_id_returns_a_format_error_instead_of_cross_type_delete() {
        let mut local = Config::empty();
        local.servers.push(sample_server("shared", "Server"));
        local.authentications.push(Authentication {
            id: "shared".into(),
            name: "Auth".into(),
            auth_type: "password".into(),
            key_content: None,
            passphrase: None,
            password: Some("secret".into()),
            synced: true,
            updated_at: "2020-01-01T00:00:00Z".into(),
        });
        let mut remote = empty_remote();
        remote.removed_ids.push("shared".into());

        let product = merge_configs(&local, &remote, None, &[]);
        assert!(product.conflicts.is_empty());
        assert_eq!(
            product.error.as_ref().map(|error| &error.kind),
            Some(&SyncErrorKind::Format)
        );
    }

    #[test]
    fn duplicate_remote_entity_ids_are_rejected_before_merge() {
        let mut remote = empty_remote();
        remote.servers.push(sample_server("same", "First"));
        remote.servers.push(sample_server("same", "Second"));

        let product = merge_configs(&Config::empty(), &remote, None, &[]);
        assert_eq!(
            product.error.as_ref().map(|error| &error.kind),
            Some(&SyncErrorKind::Format)
        );
    }
}
