use crate::config::sync_merge::merge_configs_with_token_secret;
use crate::config::sync_protocol::{
    make_conflict_attempt_token, sync_account_key, SyncError, SyncErrorKind, SyncOutcome,
    SyncResolution, SYNC_SCHEMA_VERSION,
};
use crate::config::sync_state::{AccountSyncBaseline, SyncStateStore};
use crate::config::types::{Config, SyncConfig};
use crate::webdav::client::{UploadCondition, WebDAVClient, WebDavError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One retry lets us recompute against a changed remote without allowing a permanently busy
/// endpoint to keep a config save or manual sync in flight indefinitely.
const MAX_REMOTE_CONCURRENCY_RETRIES: usize = 1;

/// Persists the fact that a WebDAV account has adopted conflict-safe sync metadata. Legacy
/// clients only write `sync.json`, so they cannot erase this separate resource.
const SYNC_SCHEMA_SENTINEL_FILE: &str = ".resh-sync-schema.json";

#[derive(Debug, Deserialize, Serialize)]
struct SyncSchemaSentinel {
    #[serde(rename = "syncSchema")]
    sync_schema: u32,
}

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

    /// Re-run the complete GET → merge → conditional PUT sequence once if the server reports a
    /// failed write precondition. Each retry re-downloads the document and re-evaluates conflicts
    /// against the same local snapshot and baseline; it never falls back to an unconditional PUT.
    pub async fn sync_with_resolutions(
        &self,
        local_config: &mut Config,
        resolutions: &[SyncResolution],
    ) -> Result<SyncOutcome, String> {
        for attempt in 0..=MAX_REMOTE_CONCURRENCY_RETRIES {
            let outcome = self
                .sync_once_with_resolutions(local_config, resolutions, None)
                .await?;
            match outcome {
                SyncOutcome::ConcurrentRemoteChange { .. }
                    if attempt < MAX_REMOTE_CONCURRENCY_RETRIES =>
                {
                    tracing::warn!(
                        attempt = attempt + 1,
                        max_retries = MAX_REMOTE_CONCURRENCY_RETRIES,
                        "WebDAV sync.json changed during conditional upload; recomputing sync"
                    );
                }
                SyncOutcome::ConcurrentRemoteChange { .. } => {
                    return Ok(SyncOutcome::ConcurrentRemoteChange {
                        message: format!(
                            "sync.json kept changing during conditional upload after {} retry; retry synchronization",
                            MAX_REMOTE_CONCURRENCY_RETRIES
                        ),
                    });
                }
                outcome => return Ok(outcome),
            }
        }

        unreachable!("retry loop always returns");
    }

    /// Commit user-provided conflict resolutions only when they still correspond to the exact
    /// conflict attempt that was displayed. Unlike ordinary sync this never retries a failed
    /// conditional PUT: a new ETag requires the user to review a fresh attempt.
    pub async fn resolve_conflicts(
        &self,
        local_config: &mut Config,
        resolutions: &[SyncResolution],
        attempt_token: &str,
    ) -> Result<SyncOutcome, String> {
        self.sync_once_with_resolutions(local_config, resolutions, Some(attempt_token))
            .await
    }

    async fn sync_once_with_resolutions(
        &self,
        local_config: &mut Config,
        resolutions: &[SyncResolution],
        expected_attempt_token: Option<&str>,
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
        let remote_is_legacy = remote_existed
            && remote_sync_config.sync_schema.unwrap_or_default() < SYNC_SCHEMA_VERSION;
        let baseline_has_current_schema = baseline.as_ref().is_some_and(|baseline| {
            baseline.has_sync_history() && baseline.sync_schema >= SYNC_SCHEMA_VERSION
        });
        let sentinel_schema = if remote_is_legacy && !baseline_has_current_schema {
            match self.client.download(SYNC_SCHEMA_SENTINEL_FILE).await {
                Ok(Some(document)) => {
                    match serde_json::from_slice::<SyncSchemaSentinel>(&document.content) {
                        Ok(sentinel) => Some(sentinel.sync_schema),
                        Err(error) => {
                            return Ok(SyncOutcome::Failed {
                            error: SyncError {
                                kind: SyncErrorKind::Format,
                                message: format!(
                                    "Remote sync schema sentinel has an invalid format at line {}, column {}",
                                    error.line(),
                                    error.column()
                                ),
                            },
                        });
                        }
                    }
                }
                Ok(None) => None,
                Err(error) => {
                    return Ok(SyncOutcome::Failed {
                        error: SyncError {
                            kind: SyncErrorKind::Network,
                            message: format!("Could not read remote sync schema sentinel: {error}"),
                        },
                    });
                }
            }
        } else {
            None
        };
        if let Some(error) = validate_remote_schema(
            &remote_sync_config,
            remote_existed,
            baseline.as_ref(),
            sentinel_schema,
        ) {
            return Ok(SyncOutcome::Failed { error });
        }
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
        if let Some(expected_attempt_token) = expected_attempt_token {
            let unresolved = merge_configs_with_token_secret(
                local_config,
                &remote_sync_config,
                baseline.as_ref(),
                &[],
                &token_secret,
            );
            if let Some(error) = unresolved.error {
                return Ok(SyncOutcome::Failed { error });
            }
            let expected = make_conflict_attempt_token(
                &token_secret,
                downloaded_etag.as_deref(),
                &unresolved.conflicts,
            );
            if unresolved.conflicts.is_empty() || expected != expected_attempt_token {
                return Ok(SyncOutcome::ConcurrentRemoteChange {
                    message: "Sync conflicts changed before they were resolved; refresh synchronization before applying choices".into(),
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
            let attempt_token = make_conflict_attempt_token(
                &token_secret,
                downloaded_etag.as_deref(),
                &product.conflicts,
            );
            return Ok(SyncOutcome::Conflicts {
                conflicts: product.conflicts,
                attempt_token,
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

        // A legacy client only knows sync.json and can silently discard typed tombstones. Once a
        // legacy document is migrated, retain an independent marker so a fresh device can detect
        // a later downgrade even without a local sync-state baseline.
        if remote_is_legacy {
            let sentinel = serde_json::to_vec(&SyncSchemaSentinel {
                sync_schema: SYNC_SCHEMA_VERSION,
            })
            .expect("schema sentinel serialization is infallible");
            match self
                .client
                .upload_conditionally(
                    SYNC_SCHEMA_SENTINEL_FILE,
                    &sentinel,
                    UploadCondition::CreateOnly,
                )
                .await
            {
                Ok(_) | Err(WebDavError::PreconditionFailed) => {}
                Err(error) => {
                    return Ok(SyncOutcome::Failed {
                        error: SyncError {
                            kind: SyncErrorKind::Network,
                            message: format!(
                                "Could not persist remote sync schema sentinel: {error}"
                            ),
                        },
                    });
                }
            }
        }

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

fn validate_remote_schema(
    remote: &SyncConfig,
    remote_existed: bool,
    baseline: Option<&AccountSyncBaseline>,
    sentinel_schema: Option<u32>,
) -> Option<SyncError> {
    if !remote_existed {
        return None;
    }

    let schema = remote.sync_schema.unwrap_or_default();
    if schema > SYNC_SCHEMA_VERSION {
        return Some(SyncError {
            kind: SyncErrorKind::IncompatibleSchema,
            message: format!(
                "Remote sync.json uses schema {schema}, which is newer than this Resh version supports ({SYNC_SCHEMA_VERSION}); upgrade Resh before syncing"
            ),
        });
    }

    if let Some(sentinel_schema) = sentinel_schema {
        if sentinel_schema > SYNC_SCHEMA_VERSION {
            return Some(SyncError {
                kind: SyncErrorKind::IncompatibleSchema,
                message: format!(
                    "Remote sync schema sentinel uses schema {sentinel_schema}, which is newer than this Resh version supports ({SYNC_SCHEMA_VERSION}); upgrade Resh before syncing"
                ),
            });
        }
        if sentinel_schema >= SYNC_SCHEMA_VERSION && schema < SYNC_SCHEMA_VERSION {
            return Some(SyncError {
                kind: SyncErrorKind::IncompatibleSchema,
                message: format!(
                    "Remote sync.json is legacy but this WebDAV account has a conflict-safe schema sentinel. An older Resh client may have overwritten synchronization metadata; syncing is blocked to prevent tombstone loss. Upgrade every syncing device before retrying"
                ),
            });
        }
    }

    let previously_used_current_schema = baseline.is_some_and(|baseline| {
        baseline.has_sync_history() && baseline.sync_schema >= SYNC_SCHEMA_VERSION
    });
    if schema < SYNC_SCHEMA_VERSION && previously_used_current_schema {
        return Some(SyncError {
            kind: SyncErrorKind::IncompatibleSchema,
            message: format!(
                "Remote sync.json reverted from conflict-safe schema {SYNC_SCHEMA_VERSION} to legacy schema {schema}. An older Resh client may have overwritten synchronization metadata; syncing is blocked to prevent tombstone loss. Upgrade every syncing device before retrying"
            ),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        sync_protocol::{hash_server, EntityKey, SyncConflictKind, SyncEntityType},
        sync_state::{AccountSyncBaseline, SyncStateStore},
        types::*,
    };
    use tempfile::tempdir;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        time::{timeout, Duration},
    };

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

    #[tokio::test]
    async fn retries_once_after_a_failed_if_match_and_uses_the_fresh_etag() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            serve_script(
                listener,
                vec![
                    (
                        "200 OK",
                        vec!["ETag: \"v1\""],
                        b"{\"version\":\"1.0\",\"syncSchema\":2}",
                    ),
                    ("412 Precondition Failed", vec![], b""),
                    (
                        "200 OK",
                        vec!["ETag: \"v2\""],
                        b"{\"version\":\"1.0\",\"syncSchema\":2}",
                    ),
                    ("204 No Content", vec!["ETag: \"v3\""], b""),
                ],
            )
            .await
        });

        let state_dir = tempdir().unwrap();
        let manager = SyncManager::new(
            format!("http://127.0.0.1:{port}"),
            "user".to_string(),
            "password".to_string(),
            None,
        )
        .with_state_store(state_dir.path().to_path_buf());
        let mut local = Config::empty();

        let outcome = manager.sync(&mut local, vec![]).await.unwrap();
        let requests = server.await.unwrap();

        assert!(matches!(outcome, SyncOutcome::Applied { .. }));
        assert_eq!(requests.len(), 4);
        assert!(requests[0].starts_with("GET /sync.json HTTP/1.1"));
        assert!(requests[1]
            .to_ascii_lowercase()
            .contains("if-match: \"v1\""));
        assert!(requests[3]
            .to_ascii_lowercase()
            .contains("if-match: \"v2\""));
    }

    #[tokio::test]
    async fn stops_after_the_bounded_remote_concurrency_retry() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            serve_script(
                listener,
                vec![
                    (
                        "200 OK",
                        vec!["ETag: \"v1\""],
                        b"{\"version\":\"1.0\",\"syncSchema\":2}",
                    ),
                    ("412 Precondition Failed", vec![], b""),
                    (
                        "200 OK",
                        vec!["ETag: \"v2\""],
                        b"{\"version\":\"1.0\",\"syncSchema\":2}",
                    ),
                    ("409 Conflict", vec![], b""),
                ],
            )
            .await
        });

        let state_dir = tempdir().unwrap();
        let manager = SyncManager::new(
            format!("http://127.0.0.1:{port}"),
            "user".to_string(),
            "password".to_string(),
            None,
        )
        .with_state_store(state_dir.path().to_path_buf());
        let mut local = Config::empty();

        let outcome = manager.sync(&mut local, vec![]).await.unwrap();
        let requests = server.await.unwrap();

        assert!(matches!(
            outcome,
            SyncOutcome::ConcurrentRemoteChange { ref message }
                if message.contains("after 1 retry")
        ));
        assert_eq!(requests.len(), 4, "must not retry indefinitely");
    }

    #[tokio::test]
    async fn legacy_remote_is_migrated_on_first_sync() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            serve_script(
                listener,
                vec![
                    ("200 OK", vec!["ETag: \"legacy\""], b"{\"version\":\"1.0\"}"),
                    ("404 Not Found", vec![], b""),
                    ("204 No Content", vec!["ETag: \"current\""], b""),
                    ("201 Created", vec!["ETag: \"sentinel\""], b""),
                ],
            )
            .await
        });

        let state_dir = tempdir().unwrap();
        let manager = SyncManager::new(
            format!("http://127.0.0.1:{port}"),
            "user".to_string(),
            "password".to_string(),
            None,
        )
        .with_state_store(state_dir.path().to_path_buf());
        let mut local = Config::empty();

        let outcome = manager.sync(&mut local, vec![]).await.unwrap();
        let requests = server.await.unwrap();
        let baseline = SyncStateStore::new(state_dir.path())
            .load_account(manager.account_key())
            .unwrap()
            .unwrap();

        assert!(matches!(outcome, SyncOutcome::Applied { .. }));
        assert_eq!(requests.len(), 4);
        assert!(requests[1].starts_with("GET /.resh-sync-schema.json HTTP/1.1"));
        let uploaded: SyncConfig = serde_json::from_str(
            requests[2]
                .split_once("\r\n\r\n")
                .expect("PUT request must contain a JSON body")
                .1,
        )
        .unwrap();
        assert_eq!(uploaded.sync_schema, Some(SYNC_SCHEMA_VERSION));
        assert_eq!(baseline.sync_schema, SYNC_SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn legacy_schema_after_a_current_sync_is_blocked_before_put() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            serve_script(
                listener,
                vec![("200 OK", vec!["ETag: \"legacy\""], b"{\"version\":\"1.0\"}")],
            )
            .await
        });

        let state_dir = tempdir().unwrap();
        let manager = SyncManager::new(
            format!("http://127.0.0.1:{port}"),
            "user".to_string(),
            "password".to_string(),
            None,
        )
        .with_state_store(state_dir.path().to_path_buf());
        let mut baseline = AccountSyncBaseline::default();
        baseline.sync_schema = SYNC_SCHEMA_VERSION;
        baseline.remote_etag = Some("\"current\"".to_string());
        SyncStateStore::new(state_dir.path())
            .save_account(manager.account_key(), baseline)
            .unwrap();

        let outcome = manager.sync(&mut Config::empty(), vec![]).await.unwrap();
        let requests = server.await.unwrap();

        assert!(matches!(
            outcome,
            SyncOutcome::Failed {
                error: SyncError {
                    kind: SyncErrorKind::IncompatibleSchema,
                    ..
                }
            }
        ));
        assert_eq!(
            requests.len(),
            1,
            "unsafe legacy document must not be overwritten"
        );
    }

    #[tokio::test]
    async fn legacy_schema_with_conflict_safe_sentinel_is_blocked_on_a_fresh_device() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            serve_script(
                listener,
                vec![
                    ("200 OK", vec!["ETag: \"legacy\""], b"{\"version\":\"1.0\"}"),
                    ("200 OK", vec!["ETag: \"sentinel\""], b"{\"syncSchema\":2}"),
                ],
            )
            .await
        });

        let state_dir = tempdir().unwrap();
        let manager = SyncManager::new(
            format!("http://127.0.0.1:{port}"),
            "new-device".to_string(),
            "password".to_string(),
            None,
        )
        .with_state_store(state_dir.path().to_path_buf());

        let outcome = manager.sync(&mut Config::empty(), vec![]).await.unwrap();
        let requests = server.await.unwrap();

        assert!(matches!(
            outcome,
            SyncOutcome::Failed {
                error: SyncError {
                    kind: SyncErrorKind::IncompatibleSchema,
                    ..
                }
            }
        ));
        assert_eq!(
            requests.len(),
            2,
            "fresh devices must not overwrite a downgrade"
        );
        assert!(requests.iter().all(|request| request.starts_with("GET ")));
    }

    #[tokio::test]
    async fn offline_two_device_edits_require_manual_resolution_without_a_second_put() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_remote = remote_with_server("Base");
        let expected_device_a_remote = remote_with_server("Device A");
        let device_a_remote = expected_device_a_remote.clone();
        let server = tokio::spawn(async move {
            serve_owned_script(
                listener,
                vec![
                    ("200 OK".into(), vec!["ETag: \"v1\"".into()], base_remote),
                    ("204 No Content".into(), vec!["ETag: \"v2\"".into()], vec![]),
                    (
                        "200 OK".into(),
                        vec!["ETag: \"v2\"".into()],
                        device_a_remote,
                    ),
                ],
            )
            .await
        });

        let device_a_state = tempdir().unwrap();
        let device_b_state = tempdir().unwrap();
        let base_url = format!("http://127.0.0.1:{port}");
        let manager_a = SyncManager::new(base_url.clone(), "user".into(), "password".into(), None)
            .with_state_store(device_a_state.path().to_path_buf());
        let manager_b = SyncManager::new(base_url, "user".into(), "password".into(), None)
            .with_state_store(device_b_state.path().to_path_buf());
        let baseline = server_baseline();
        SyncStateStore::new(device_a_state.path())
            .save_account(manager_a.account_key(), baseline.clone())
            .unwrap();
        SyncStateStore::new(device_b_state.path())
            .save_account(manager_b.account_key(), baseline)
            .unwrap();

        let mut device_a = config_with_server("Device A");
        let mut device_b = config_with_server("Device B");
        let a_outcome = manager_a.sync(&mut device_a, vec![]).await.unwrap();
        let b_outcome = manager_b.sync(&mut device_b, vec![]).await.unwrap();
        let requests = server.await.unwrap();

        assert!(matches!(a_outcome, SyncOutcome::Applied { .. }));
        assert!(matches!(
            b_outcome,
            SyncOutcome::Conflicts { ref conflicts, .. }
                if conflicts.len() == 1 && conflicts[0].kind == SyncConflictKind::BothModified
        ));
        assert_eq!(
            requests.len(),
            3,
            "device B must not overwrite device A's edit"
        );
        assert!(requests[1]
            .to_ascii_lowercase()
            .contains("if-match: \"v1\""));
        let uploaded: SyncConfig = serde_json::from_str(
            requests[1]
                .split_once("\r\n\r\n")
                .expect("device A PUT must contain a JSON body")
                .1,
        )
        .unwrap();
        assert_eq!(uploaded.servers[0].name, "Device A");
        assert_eq!(
            serde_json::to_vec(&uploaded).unwrap(),
            expected_device_a_remote,
            "the simulated remote state must come from device A's actual PUT"
        );
    }

    fn sample_server(id: &str, name: &str) -> Server {
        Server {
            id: id.into(),
            name: name.into(),
            group: "group".into(),
            host: "example.test".into(),
            port: 22,
            username: "user".into(),
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
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn config_with_server(name: &str) -> Config {
        let mut config = Config::empty();
        config.servers.push(sample_server("server", name));
        config
    }

    fn remote_with_server(name: &str) -> Vec<u8> {
        let mut remote = SyncConfig::empty("1.0");
        remote.servers.push(sample_server("server", name));
        serde_json::to_vec(&remote).unwrap()
    }

    fn server_baseline() -> AccountSyncBaseline {
        let mut baseline = AccountSyncBaseline::default();
        baseline.set_entity_hash(
            &EntityKey::new(SyncEntityType::Server, "server"),
            hash_server(&sample_server("server", "Base")),
        );
        baseline.sync_schema = SYNC_SCHEMA_VERSION;
        baseline
    }

    async fn serve_script(
        listener: TcpListener,
        script: Vec<(&'static str, Vec<&'static str>, &'static [u8])>,
    ) -> Vec<String> {
        let mut requests = Vec::with_capacity(script.len());
        for (status, headers, body) in script {
            let (stream, _) = timeout(Duration::from_secs(2), listener.accept())
                .await
                .expect("timed out waiting for scripted WebDAV request")
                .unwrap();
            let (mut stream, request) = read_request(stream).await;
            write_response(&mut stream, status, &headers, body).await;
            requests.push(request);
        }
        requests
    }

    async fn serve_owned_script(
        listener: TcpListener,
        script: Vec<(String, Vec<String>, Vec<u8>)>,
    ) -> Vec<String> {
        let mut requests = Vec::with_capacity(script.len());
        for (status, headers, body) in script {
            let (stream, _) = timeout(Duration::from_secs(2), listener.accept())
                .await
                .expect("timed out waiting for scripted WebDAV request")
                .unwrap();
            let (mut stream, request) = read_request(stream).await;
            write_owned_response(&mut stream, &status, &headers, &body).await;
            requests.push(request);
        }
        requests
    }

    async fn read_request(mut stream: TcpStream) -> (TcpStream, String) {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        let headers_end = loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "test HTTP client closed before sending headers");
            request.extend_from_slice(&buffer[..read]);
            if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..headers_end]);
        let content_length = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while request.len() < headers_end + content_length {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(
                read > 0,
                "test HTTP client closed before sending request body"
            );
            request.extend_from_slice(&buffer[..read]);
        }
        (stream, String::from_utf8_lossy(&request).into_owned())
    }

    async fn write_response(stream: &mut TcpStream, status: &str, headers: &[&str], body: &[u8]) {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        for header in headers {
            response.push_str(header);
            response.push_str("\r\n");
        }
        response.push_str("\r\n");
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.write_all(body).await.unwrap();
        stream.shutdown().await.unwrap();
    }

    async fn write_owned_response(
        stream: &mut TcpStream,
        status: &str,
        headers: &[String],
        body: &[u8],
    ) {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        for header in headers {
            response.push_str(header);
            response.push_str("\r\n");
        }
        response.push_str("\r\n");
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.write_all(body).await.unwrap();
        stream.shutdown().await.unwrap();
    }
}
