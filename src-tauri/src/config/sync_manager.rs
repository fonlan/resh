use crate::config::sync_merge::merge_configs_with_token_secret;
use crate::config::sync_protocol::{
    sync_account_key, SyncError, SyncErrorKind, SyncOutcome, SyncResolution, SYNC_SCHEMA_VERSION,
};
use crate::config::sync_state::{AccountSyncBaseline, SyncStateStore};
use crate::config::types::{Config, SyncConfig};
use crate::webdav::client::{UploadCondition, WebDAVClient, WebDavError};
use std::path::PathBuf;

pub struct SyncManager {
    client: WebDAVClient,
    account_key: String,
    state_store: Option<SyncStateStore>,
}

impl SyncManager {
    pub fn new(
        url: String,
        username: String,
        password: String,
        proxy: Option<crate::config::types::Proxy>,
    ) -> Self {
        let account_key = sync_account_key(&url, &username);
        Self {
            client: WebDAVClient::new(url, username, password, proxy),
            account_key,
            state_store: None,
        }
    }

    /// Attach local baseline store (app data dir). Required for three-way sync correctness.
    pub fn with_state_store(mut self, app_data_dir: PathBuf) -> Self {
        self.state_store = Some(SyncStateStore::new(app_data_dir));
        self
    }

    pub fn account_key(&self) -> &str {
        &self.account_key
    }

    /// Download remote, three-way merge with local baseline, upload when clean.
    ///
    /// `resolutions` applies user choices for outstanding conflicts (token-bound).
    /// `recently_removed_ids` is accepted for call-site compatibility but no longer drives
    /// merge; deletions are detected via baseline vs local synced set.
    pub async fn sync(
        &self,
        local_config: &mut Config,
        _recently_removed_ids: Vec<String>,
    ) -> Result<SyncOutcome, String> {
        self.sync_with_resolutions(local_config, &[]).await
    }

    pub async fn sync_with_resolutions(
        &self,
        local_config: &mut Config,
        resolutions: &[SyncResolution],
    ) -> Result<SyncOutcome, String> {
        let (remote_sync_config, remote_existed, downloaded_etag) =
            match self.client.download("sync.json").await {
                Ok(Some(document)) => {
                    let config = match serde_json::from_slice::<SyncConfig>(&document.content) {
                        Ok(config) => config,
                        Err(error) => {
                            // Do not expose remote document content: it can carry credentials.
                            return Ok(SyncOutcome::Failed {
                                error: SyncError {
                                    kind: SyncErrorKind::Format,
                                    message: format!(
                                    "Remote sync.json has an invalid format at line {}, column {}",
                                    error.line(),
                                    error.column()
                                ),
                                },
                            });
                        }
                    };
                    tracing::info!(
                        "Downloaded remote sync.json: {} servers, {} snippets, schema={:?}",
                        config.servers.len(),
                        config.snippets.len(),
                        config.sync_schema
                    );
                    (config, true, document.etag)
                }
                Ok(None) => {
                    tracing::info!("Remote sync.json not found (first sync)");
                    (SyncConfig::empty(local_config.version.clone()), false, None)
                }
                Err(error) => {
                    tracing::error!("Failed to download sync.json");
                    return Ok(SyncOutcome::Failed {
                        error: SyncError {
                            kind: SyncErrorKind::Network,
                            message: format!("Sync download failed: {}", error),
                        },
                    });
                }
            };

        let mut baseline = match self.load_baseline() {
            Ok(baseline) => baseline,
            Err(error) => {
                return Ok(SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::Format,
                        message: format!("Local sync-state.json has an invalid format: {}", error),
                    },
                });
            }
        };
        // Conflict tokens are keyed with an account-local random secret, persisted before the
        // first conflict is returned so a subsequent resolution command can validate it without
        // exposing raw credential-derived content hashes to the frontend.
        let token_secret = {
            let baseline = baseline.get_or_insert_with(AccountSyncBaseline::default);
            baseline.ensure_resolution_secret().to_owned()
        };
        if let Some(store) = &self.state_store {
            if let Err(error) = store.save_account(
                &self.account_key,
                baseline.clone().expect("baseline was initialized above"),
            ) {
                return Ok(SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::Internal,
                        message: format!("Could not persist local sync baseline: {}", error),
                    },
                });
            }
        }
        let product = merge_configs_with_token_secret(
            local_config,
            &remote_sync_config,
            baseline.as_ref(),
            resolutions,
            &token_secret,
        );

        if let Some(error) = product.error.clone() {
            return Ok(SyncOutcome::Failed { error });
        }

        if !product.conflicts.is_empty() {
            tracing::info!(
                "Sync paused: {} conflict(s) require resolution",
                product.conflicts.len()
            );
            return Ok(SyncOutcome::Conflicts {
                conflicts: product.conflicts,
            });
        }

        let Some(merged_local) = product.merged_local else {
            return Ok(SyncOutcome::Failed {
                error: SyncError {
                    kind: SyncErrorKind::Internal,
                    message: "Merge produced no local config without conflicts".into(),
                },
            });
        };
        let Some(mut merged_remote) = product.merged_remote else {
            return Ok(SyncOutcome::Failed {
                error: SyncError {
                    kind: SyncErrorKind::Internal,
                    message: "Merge produced no remote config without conflicts".into(),
                },
            });
        };

        // Integrity: refuse to wipe remote content when merge emptied synced lists unexpectedly.
        if !remote_sync_config.snippets.is_empty() && merged_remote.snippets.is_empty() {
            let local_had = local_config.snippets.iter().any(|s| s.synced);
            if local_had {
                // possible intentional delete of all — only abort if local still had none deleted path
            }
            if merged_local.snippets.iter().filter(|s| s.synced).count() == 0
                && remote_sync_config.snippets.len() > 1
                && product.changed_entity_count == 0
            {
                return Ok(SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::Internal,
                        message: "Merge integrity check failed: snippets missing after merge"
                            .into(),
                    },
                });
            }
        }

        merged_remote.sync_schema = Some(SYNC_SCHEMA_VERSION);
        let sync_json = match serde_json::to_vec_pretty(&merged_remote) {
            Ok(json) => json,
            Err(error) => {
                return Ok(SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::Internal,
                        message: format!(
                            "Could not serialize merged sync configuration: {}",
                            error
                        ),
                    },
                });
            }
        };

        let condition = if remote_existed {
            let Some(etag) = downloaded_etag.clone() else {
                return Ok(SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::SafeSyncUnavailable,
                        message: "WebDAV server did not provide an ETag for sync.json; refusing an unsafe overwrite"
                            .into(),
                    },
                });
            };
            UploadCondition::IfMatch(etag)
        } else {
            UploadCondition::CreateOnly
        };
        let uploaded_etag = match self
            .client
            .upload_conditionally("sync.json", &sync_json, condition)
            .await
        {
            Ok(etag) => etag,
            Err(WebDavError::PreconditionFailed) => {
                return Ok(SyncOutcome::ConcurrentRemoteChange {
                    message: "sync.json changed on the server while this sync was in progress"
                        .into(),
                });
            }
            Err(error) => {
                tracing::error!("Failed to upload sync.json");
                return Ok(SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::Network,
                        message: format!("Failed to upload sync.json: {}", error),
                    },
                });
            }
        };

        tracing::info!(
            "Uploaded updated sync.json: {} servers, {} snippets",
            merged_remote.servers.len(),
            merged_remote.snippets.len()
        );

        // Persist the post-write ETag only when the server supplies one. Never retain the ETag
        // from the pre-write GET because it is stale after a successful PUT.
        if let Some(store) = &self.state_store {
            let mut account = baseline.unwrap_or_default();
            account.replace_from_hashes(
                &product.baseline_hashes,
                &product.baseline_tombstone_keys,
                uploaded_etag,
                merged_remote.revision.clone(),
            );
            if let Err(error) = store.save_account(&self.account_key, account) {
                return Ok(SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::Internal,
                        message: format!("Could not persist local sync baseline: {}", error),
                    },
                });
            }
        } else {
            tracing::warn!(
                "SyncStateStore not configured; three-way baseline will not persist across runs"
            );
        }

        *local_config = merged_local;
        Ok(SyncOutcome::Applied {
            changed_entity_count: product.changed_entity_count,
        })
    }

    fn load_baseline(&self) -> Result<Option<AccountSyncBaseline>, String> {
        let Some(store) = &self.state_store else {
            return Ok(None);
        };
        store.load_account(&self.account_key)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::types::*;

    #[test]
    fn test_deserialize_sync_config_robustness() {
        let json_no_date = r#"{
            "version": "1.0",
            "snippets": [
                {
                    "id": "s4",
                    "name": "Test No Date",
                    "content": "echo no date"
                }
            ]
        }"#;

        let config_no_date =
            serde_json::from_str::<SyncConfig>(json_no_date).expect("Failed to parse no date JSON");
        assert_eq!(config_no_date.snippets.len(), 1);
        assert!(config_no_date.tombstones.is_empty());
        assert!(config_no_date.sync_schema.is_none());

        let json_body = r#"{
            "version": "1.0",
            "snippets": [
                {
                    "id": "s5",
                    "name": "Test Body",
                    "body": "echo body"
                }
            ]
        }"#;
        let config_body: SyncConfig =
            serde_json::from_str(json_body).expect("Failed to parse body JSON");
        assert_eq!(config_body.snippets[0].content, "echo body");
    }

    #[test]
    fn deserializes_typed_tombstones() {
        let json = r#"{
            "version": "1.0",
            "tombstones": [
                { "entityType": "server", "id": "s1" }
            ],
            "removedIds": ["s1"]
        }"#;
        let c: SyncConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c.tombstones.len(), 1);
        assert_eq!(c.removed_ids, vec!["s1".to_string()]);
    }
}
