use crate::config::types::{Config, Server, Authentication, Proxy, Snippet, SyncConfig, AiChannel, AiModel, SftpCustomCommand};
use crate::webdav::client::WebDAVClient;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub struct SyncManager {
    client: WebDAVClient,
}

impl SyncManager {
    pub fn new(url: String, username: String, password: String, proxy: Option<Proxy>) -> Self {
        Self {
            client: WebDAVClient::new(url, username, password, proxy),
        }
    }

    pub async fn sync(&self, local_config: &mut Config, recently_removed_ids: Vec<String>) -> Result<(), String> {
        // 1. Download sync.json from WebDAV
        let remote_sync_config = match self.client.download("sync.json").await {
            Ok(Some(content)) => {
                let config = serde_json::from_slice::<SyncConfig>(&content)
                    .map_err(|e| format!("Failed to parse remote sync.json: {}", e))?;
                tracing::info!("Downloaded remote sync.json: {} servers, {} snippets", config.servers.len(), config.snippets.len());
                config
            }
            Ok(None) => {
                tracing::info!("Remote sync.json not found (first sync)");
                SyncConfig {
                    version: local_config.version.clone(),
                    servers: vec![],
                    authentications: vec![],
                    proxies: vec![],
                    snippets: vec![],
                    ai_channels: vec![],
                    ai_models: vec![],
                    sftp_custom_commands: vec![],
                    additional_prompt: local_config.additional_prompt.clone(),
                    additional_prompt_updated_at: local_config.additional_prompt_updated_at.clone(),
                    removed_ids: vec![],
                }
            }
            Err(e) => {
                tracing::error!("Failed to download sync.json: {:?}", e);
                return Err(format!("Sync failed: {:?}", e));
            }
        };

        // 2. Merge logic: Remote -> Local
        self.merge_remote_to_local(local_config, &remote_sync_config, &recently_removed_ids);

        // Safety Check: If remote had items but local is empty after merge, something is wrong.
        // This prevents a "fresh" client from wiping remote if merge fails silently.
        if !remote_sync_config.snippets.is_empty() && local_config.snippets.is_empty() && recently_removed_ids.is_empty() {
            tracing::error!("CRITICAL: Remote has {} snippets but Local is empty after merge! Aborting sync to prevent data loss.", remote_sync_config.snippets.len());
            return Err("Merge integrity check failed: Snippets missing after merge".to_string());
        }
        if !remote_sync_config.servers.is_empty() && local_config.servers.is_empty() && recently_removed_ids.is_empty() {
            tracing::error!("CRITICAL: Remote has {} servers but Local is empty after merge! Aborting sync to prevent data loss.", remote_sync_config.servers.len());
            return Err("Merge integrity check failed: Servers missing after merge".to_string());
        }

        // 3. Merge logic: Local -> Remote (only synced items)
        let mut new_remote_sync_config = remote_sync_config.clone();
        self.merge_local_to_remote(local_config, &mut new_remote_sync_config);
        tracing::debug!("Merge complete. Local -> Remote: {} servers, {} snippets, {} channels", 
            new_remote_sync_config.servers.len(), 
            new_remote_sync_config.snippets.len(),
            new_remote_sync_config.ai_channels.len()
        );

        // 4. Upload new sync.json to WebDAV
        let sync_json = serde_json::to_vec_pretty(&new_remote_sync_config)
            .map_err(|e| format!("Failed to serialize sync config: {}", e))?;
        
        self.client.upload("sync.json", &sync_json).await
            .map_err(|e| format!("Failed to upload sync.json: {}", e))?;
        
        tracing::info!("Uploaded updated sync.json: {} servers, {} snippets", new_remote_sync_config.servers.len(), new_remote_sync_config.snippets.len());

        Ok(())
    }

    fn merge_remote_to_local(&self, local: &mut Config, remote: &SyncConfig, recently_removed_ids: &[String]) {
        // Handle removals first: if an ID is in removed_ids, disable sync locally
        for server in &mut local.servers {
            if remote.removed_ids.contains(&server.id) {
                server.synced = false;
            }
        }
        for auth in &mut local.authentications {
            if remote.removed_ids.contains(&auth.id) {
                auth.synced = false;
            }
        }
        for proxy in &mut local.proxies {
            if remote.removed_ids.contains(&proxy.id) {
                proxy.synced = false;
            }
        }
        for snippet in &mut local.snippets {
            if remote.removed_ids.contains(&snippet.id) {
                snippet.synced = false;
            }
        }
        for channel in &mut local.ai_channels {
            if remote.removed_ids.contains(&channel.id) {
                channel.synced = false;
            }
        }
        for model in &mut local.ai_models {
            if remote.removed_ids.contains(&model.id) {
                model.synced = false;
            }
        }
        for cmd in &mut local.sftp_custom_commands {
            if remote.removed_ids.contains(&cmd.id) {
                cmd.synced = false;
            }
        }

        // Merge Servers
        let mut local_servers: HashMap<String, Server> = local.servers.drain(..).map(|s| (s.id.clone(), s)).collect();
        for remote_server in &remote.servers {
            if recently_removed_ids.contains(&remote_server.id) {
                continue;
            }
            // If it's in remote sync.json, it SHOULD be synced to local, regardless of its own synced flag.
            // The flag is primarily for local-to-remote control.
            if let Some(local_server) = local_servers.get_mut(&remote_server.id) {
                if is_newer(&remote_server.updated_at, &local_server.updated_at) {
                    *local_server = remote_server.clone();
                    local_server.synced = true; // Ensure it stays synced
                }
            } else {
                let mut s = remote_server.clone();
                s.synced = true;
                local_servers.insert(s.id.clone(), s);
            }
        }
        local.servers = local_servers.into_values().collect();

        // Merge Authentications
        let mut local_auths: HashMap<String, Authentication> = local.authentications.drain(..).map(|a| (a.id.clone(), a)).collect();
        for remote_auth in &remote.authentications {
            if recently_removed_ids.contains(&remote_auth.id) {
                continue;
            }
            if let Some(local_auth) = local_auths.get_mut(&remote_auth.id) {
                if is_newer(&remote_auth.updated_at, &local_auth.updated_at) {
                    *local_auth = remote_auth.clone();
                    local_auth.synced = true;
                }
            } else {
                let mut a = remote_auth.clone();
                a.synced = true;
                local_auths.insert(a.id.clone(), a);
            }
        }
        local.authentications = local_auths.into_values().collect();

        // Merge Proxies
        let mut local_proxies: HashMap<String, Proxy> = local.proxies.drain(..).map(|p| (p.id.clone(), p)).collect();
        for remote_proxy in &remote.proxies {
            if recently_removed_ids.contains(&remote_proxy.id) {
                continue;
            }
            if let Some(local_proxy) = local_proxies.get_mut(&remote_proxy.id) {
                if is_newer(&remote_proxy.updated_at, &local_proxy.updated_at) {
                    *local_proxy = remote_proxy.clone();
                    local_proxy.synced = true;
                }
            } else {
                let mut p = remote_proxy.clone();
                p.synced = true;
                local_proxies.insert(p.id.clone(), p);
            }
        }
        local.proxies = local_proxies.into_values().collect();

        // Merge Snippets
        let mut local_snippets: HashMap<String, Snippet> = local.snippets.drain(..).map(|s| (s.id.clone(), s)).collect();
        for remote_snippet in &remote.snippets {
            if recently_removed_ids.contains(&remote_snippet.id) {
                continue;
            }
            if let Some(local_snippet) = local_snippets.get_mut(&remote_snippet.id) {
                if is_newer(&remote_snippet.updated_at, &local_snippet.updated_at) {
                    *local_snippet = remote_snippet.clone();
                    local_snippet.synced = true;
                }
            } else {
                let mut s = remote_snippet.clone();
                s.synced = true;
                local_snippets.insert(s.id.clone(), s);
            }
        }
        local.snippets = local_snippets.into_values().collect();

        // Merge AI Channels
        let mut local_channels: HashMap<String, AiChannel> = local.ai_channels.drain(..).map(|c| (c.id.clone(), c)).collect();
        for remote_channel in &remote.ai_channels {
            if recently_removed_ids.contains(&remote_channel.id) {
                continue;
            }
            if let Some(local_channel) = local_channels.get_mut(&remote_channel.id) {
                if is_newer(&remote_channel.updated_at, &local_channel.updated_at) {
                    *local_channel = remote_channel.clone();
                    local_channel.synced = true;
                }
            } else {
                let mut c = remote_channel.clone();
                c.synced = true;
                local_channels.insert(c.id.clone(), c);
            }
        }
        local.ai_channels = local_channels.into_values().collect();

        // Merge AI Models
        let mut local_models: HashMap<String, AiModel> = local.ai_models.drain(..).map(|m| (m.id.clone(), m)).collect();
        for remote_model in &remote.ai_models {
            if recently_removed_ids.contains(&remote_model.id) {
                continue;
            }
            if let Some(local_model) = local_models.get_mut(&remote_model.id) {
                if is_newer(&remote_model.updated_at, &local_model.updated_at) {
                    *local_model = remote_model.clone();
                    local_model.synced = true;
                }
            } else {
                let mut m = remote_model.clone();
                m.synced = true;
                local_models.insert(m.id.clone(), m);
            }
        }
        local.ai_models = local_models.into_values().collect();

        // Merge SFTP Custom Commands
        let mut local_commands: HashMap<String, SftpCustomCommand> = local.sftp_custom_commands.drain(..).map(|c| (c.id.clone(), c)).collect();
        for remote_command in &remote.sftp_custom_commands {
            if recently_removed_ids.contains(&remote_command.id) {
                continue;
            }
            if let Some(local_command) = local_commands.get_mut(&remote_command.id) {
                if is_newer(&remote_command.updated_at, &local_command.updated_at) {
                    *local_command = remote_command.clone();
                    local_command.synced = true;
                }
            } else {
                let mut c = remote_command.clone();
                c.synced = true;
                local_commands.insert(c.id.clone(), c);
            }
        }
        local.sftp_custom_commands = local_commands.into_values().collect();

        let remote_ts = remote.additional_prompt_updated_at.as_deref();
        let local_ts = local.additional_prompt_updated_at.as_deref();

        let should_update = match (remote_ts, local_ts) {
            (Some(rt), Some(lt)) => is_newer(rt, lt),
            (Some(_), None) => {
                // Only update if remote has a non-null value
                remote.additional_prompt.is_some()
            },
            (None, Some(_)) => false,
            (None, None) => {
                // Both have no timestamps, only update if local is None and remote has a value
                local.additional_prompt.is_none() && remote.additional_prompt.is_some()
            },
        };

        if should_update {
            local.additional_prompt = remote.additional_prompt.clone();
            local.additional_prompt_updated_at = remote.additional_prompt_updated_at.clone();
        }
    }

    fn merge_local_to_remote(&self, local: &Config, remote: &mut SyncConfig) {
        // 1. Identify what should be removed from remote
        // If an item exists in remote but is missing in local or has synced=false in local
        let local_servers: HashMap<String, &Server> = local.servers.iter().map(|s| (s.id.clone(), s)).collect();
        let local_auths: HashMap<String, &Authentication> = local.authentications.iter().map(|a| (a.id.clone(), a)).collect();
        let local_proxies: HashMap<String, &Proxy> = local.proxies.iter().map(|p| (p.id.clone(), p)).collect();
        let local_snippets: HashMap<String, &Snippet> = local.snippets.iter().map(|s| (s.id.clone(), s)).collect();
        let local_channels: HashMap<String, &AiChannel> = local.ai_channels.iter().map(|c| (c.id.clone(), c)).collect();
        let local_models: HashMap<String, &AiModel> = local.ai_models.iter().map(|m| (m.id.clone(), m)).collect();
        let local_commands: HashMap<String, &SftpCustomCommand> = local.sftp_custom_commands.iter().map(|c| (c.id.clone(), c)).collect();

        let mut remote_servers: HashMap<String, Server> = remote.servers.drain(..).map(|s| (s.id.clone(), s)).collect();
        let mut to_remove_servers = Vec::new();
        for id in remote_servers.keys() {
            match local_servers.get(id) {
                None => to_remove_servers.push(id.clone()), // Deleted locally
                Some(s) if !s.synced => to_remove_servers.push(id.clone()), // Disabled sync locally
                _ => {}
            }
        }
        for id in to_remove_servers {
            remote_servers.remove(&id);
            if !remote.removed_ids.contains(&id) {
                remote.removed_ids.push(id);
            }
        }

        let mut remote_auths: HashMap<String, Authentication> = remote.authentications.drain(..).map(|a| (a.id.clone(), a)).collect();
        let mut to_remove_auths = Vec::new();
        for id in remote_auths.keys() {
            match local_auths.get(id) {
                None => to_remove_auths.push(id.clone()),
                Some(a) if !a.synced => to_remove_auths.push(id.clone()),
                _ => {}
            }
        }
        for id in to_remove_auths {
            remote_auths.remove(&id);
            if !remote.removed_ids.contains(&id) {
                remote.removed_ids.push(id);
            }
        }

        let mut remote_proxies: HashMap<String, Proxy> = remote.proxies.drain(..).map(|p| (p.id.clone(), p)).collect();
        let mut to_remove_proxies = Vec::new();
        for id in remote_proxies.keys() {
            match local_proxies.get(id) {
                None => to_remove_proxies.push(id.clone()),
                Some(p) if !p.synced => to_remove_proxies.push(id.clone()),
                _ => {}
            }
        }
        for id in to_remove_proxies {
            remote_proxies.remove(&id);
            if !remote.removed_ids.contains(&id) {
                remote.removed_ids.push(id);
            }
        }

        let mut remote_snippets: HashMap<String, Snippet> = remote.snippets.drain(..).map(|s| (s.id.clone(), s)).collect();
        let mut to_remove_snippets = Vec::new();
        for id in remote_snippets.keys() {
            match local_snippets.get(id) {
                None => to_remove_snippets.push(id.clone()),
                Some(s) if !s.synced => to_remove_snippets.push(id.clone()),
                _ => {}
            }
        }
        for id in to_remove_snippets {
            remote_snippets.remove(&id);
            if !remote.removed_ids.contains(&id) {
                remote.removed_ids.push(id);
            }
        }

        let mut remote_channels: HashMap<String, AiChannel> = remote.ai_channels.drain(..).map(|c| (c.id.clone(), c)).collect();
        let mut to_remove_channels = Vec::new();
        for id in remote_channels.keys() {
            match local_channels.get(id) {
                None => to_remove_channels.push(id.clone()),
                Some(c) if !c.synced => to_remove_channels.push(id.clone()),
                _ => {}
            }
        }
        for id in to_remove_channels {
            remote_channels.remove(&id);
            if !remote.removed_ids.contains(&id) {
                remote.removed_ids.push(id);
            }
        }

        let mut remote_models: HashMap<String, AiModel> = remote.ai_models.drain(..).map(|m| (m.id.clone(), m)).collect();
        let mut to_remove_models = Vec::new();
        for id in remote_models.keys() {
            match local_models.get(id) {
                None => to_remove_models.push(id.clone()),
                Some(m) if !m.synced => to_remove_models.push(id.clone()),
                _ => {}
            }
        }
        for id in to_remove_models {
            remote_models.remove(&id);
            if !remote.removed_ids.contains(&id) {
                remote.removed_ids.push(id);
            }
        }

        let mut remote_commands: HashMap<String, SftpCustomCommand> = remote.sftp_custom_commands.drain(..).map(|c| (c.id.clone(), c)).collect();
        let mut to_remove_commands = Vec::new();
        for id in remote_commands.keys() {
            match local_commands.get(id) {
                None => to_remove_commands.push(id.clone()),
                Some(c) if !c.synced => to_remove_commands.push(id.clone()),
                _ => {}
            }
        }
        for id in to_remove_commands {
            remote_commands.remove(&id);
            if !remote.removed_ids.contains(&id) {
                remote.removed_ids.push(id);
            }
        }

        // 2. Add/Update from local
        for local_server in &local.servers {
            if !local_server.synced { continue; }
            // If we are syncing it, ensure it's not in removed_ids
            remote.removed_ids.retain(|id| id != &local_server.id);
            
            if let Some(remote_server) = remote_servers.get_mut(&local_server.id) {
                if is_newer(&local_server.updated_at, &remote_server.updated_at) {
                    *remote_server = local_server.clone();
                }
            } else {
                remote_servers.insert(local_server.id.clone(), local_server.clone());
            }
        }
        remote.servers = remote_servers.into_values().collect();

        for local_auth in &local.authentications {
            if !local_auth.synced { continue; }
            remote.removed_ids.retain(|id| id != &local_auth.id);

            if let Some(remote_auth) = remote_auths.get_mut(&local_auth.id) {
                if is_newer(&local_auth.updated_at, &remote_auth.updated_at) {
                    *remote_auth = local_auth.clone();
                }
            } else {
                remote_auths.insert(local_auth.id.clone(), local_auth.clone());
            }
        }
        remote.authentications = remote_auths.into_values().collect();

        for local_proxy in &local.proxies {
            if !local_proxy.synced { continue; }
            remote.removed_ids.retain(|id| id != &local_proxy.id);

            if let Some(remote_proxy) = remote_proxies.get_mut(&local_proxy.id) {
                if is_newer(&local_proxy.updated_at, &remote_proxy.updated_at) {
                    *remote_proxy = local_proxy.clone();
                }
            } else {
                remote_proxies.insert(local_proxy.id.clone(), local_proxy.clone());
            }
        }
        remote.proxies = remote_proxies.into_values().collect();

        for local_snippet in &local.snippets {
            if !local_snippet.synced { continue; }
            remote.removed_ids.retain(|id| id != &local_snippet.id);

            if let Some(remote_snippet) = remote_snippets.get_mut(&local_snippet.id) {
                if is_newer(&local_snippet.updated_at, &remote_snippet.updated_at) {
                    *remote_snippet = local_snippet.clone();
                }
            } else {
                remote_snippets.insert(local_snippet.id.clone(), local_snippet.clone());
            }
        }
        remote.snippets = remote_snippets.into_values().collect();

        for local_channel in &local.ai_channels {
            if !local_channel.synced { continue; }
            remote.removed_ids.retain(|id| id != &local_channel.id);

            if let Some(remote_channel) = remote_channels.get_mut(&local_channel.id) {
                if is_newer(&local_channel.updated_at, &remote_channel.updated_at) {
                    *remote_channel = local_channel.clone();
                }
            } else {
                remote_channels.insert(local_channel.id.clone(), local_channel.clone());
            }
        }
        remote.ai_channels = remote_channels.into_values().collect();

        for local_model in &local.ai_models {
            if !local_model.synced { continue; }
            remote.removed_ids.retain(|id| id != &local_model.id);

            if let Some(remote_model) = remote_models.get_mut(&local_model.id) {
                if is_newer(&local_model.updated_at, &remote_model.updated_at) {
                    *remote_model = local_model.clone();
                }
            } else {
                remote_models.insert(local_model.id.clone(), local_model.clone());
            }
        }
        remote.ai_models = remote_models.into_values().collect();

        for local_command in &local.sftp_custom_commands {
            if !local_command.synced { continue; }
            remote.removed_ids.retain(|id| id != &local_command.id);

            if let Some(remote_command) = remote_commands.get_mut(&local_command.id) {
                if is_newer(&local_command.updated_at, &remote_command.updated_at) {
                    *remote_command = local_command.clone();
                }
            } else {
                remote_commands.insert(local_command.id.clone(), local_command.clone());
            }
        }
        remote.sftp_custom_commands = remote_commands.into_values().collect();

        remote.additional_prompt = local.additional_prompt.clone();
        remote.additional_prompt_updated_at = local.additional_prompt_updated_at.clone();
    }
}

fn is_newer(a: &str, b: &str) -> bool {
    if a == b { return false; }
    let dt_a = DateTime::parse_from_rfc3339(a).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now());
    let dt_b = DateTime::parse_from_rfc3339(b).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now() - chrono::Duration::days(365 * 10)); // Very old date
    dt_a > dt_b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::*;

    #[test]
    fn test_deserialize_sync_config_robustness() {
        // Test missing updatedAt (should use default)
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
        
        let config_no_date = serde_json::from_str::<SyncConfig>(json_no_date).expect("Failed to parse no date JSON");
        assert_eq!(config_no_date.snippets.len(), 1);
        // We can't easily check the value of date since it's "now", but we know it parsed.

        // Test body alias
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
        let config_body: SyncConfig = serde_json::from_str(json_body).expect("Failed to parse body JSON");
        assert_eq!(config_body.snippets[0].content, "echo body");
    }
}
