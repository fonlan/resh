use crate::config::types::{Config, Server, Authentication, Proxy, SyncConfig};
use crate::webdav::client::WebDAVClient;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub struct SyncManager {
    client: WebDAVClient,
}

impl SyncManager {
    pub fn new(url: String, username: String, password: String) -> Self {
        Self {
            client: WebDAVClient::new(url, username, password),
        }
    }

    pub async fn sync(&self, local_config: &mut Config) -> Result<(), String> {
        // 1. Download sync.json from WebDAV
        let remote_sync_config = match self.client.download("sync.json").await {
            Ok(content) => {
                serde_json::from_slice::<SyncConfig>(&content)
                    .map_err(|e| format!("Failed to parse remote sync.json: {}", e))?
            }
            Err(_) => {
                SyncConfig {
                    version: local_config.version.clone(),
                    servers: vec![],
                    authentications: vec![],
                    proxies: vec![],
                    removed_ids: vec![],
                }
            }
        };

        // 2. Merge logic: Remote -> Local
        self.merge_remote_to_local(local_config, &remote_sync_config);

        // 3. Merge logic: Local -> Remote (only synced items)
        let mut new_remote_sync_config = remote_sync_config.clone();
        self.merge_local_to_remote(local_config, &mut new_remote_sync_config);

        // 4. Upload new sync.json to WebDAV
        let sync_json = serde_json::to_vec_pretty(&new_remote_sync_config)
            .map_err(|e| format!("Failed to serialize sync config: {}", e))?;
        
        self.client.upload("sync.json", &sync_json).await
            .map_err(|e| format!("Failed to upload sync.json: {}", e))?;

        Ok(())
    }

    fn merge_remote_to_local(&self, local: &mut Config, remote: &SyncConfig) {
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

        // Merge Servers
        let mut local_servers: HashMap<String, Server> = local.servers.drain(..).map(|s| (s.id.clone(), s)).collect();
        for remote_server in &remote.servers {
            if !remote_server.synced { continue; }
            if let Some(local_server) = local_servers.get_mut(&remote_server.id) {
                if is_newer(&remote_server.updated_at, &local_server.updated_at) {
                    *local_server = remote_server.clone();
                }
            } else {
                local_servers.insert(remote_server.id.clone(), remote_server.clone());
            }
        }
        local.servers = local_servers.into_values().collect();

        // Merge Authentications
        let mut local_auths: HashMap<String, Authentication> = local.authentications.drain(..).map(|a| (a.id.clone(), a)).collect();
        for remote_auth in &remote.authentications {
            if !remote_auth.synced { continue; }
            if let Some(local_auth) = local_auths.get_mut(&remote_auth.id) {
                if is_newer(&remote_auth.updated_at, &local_auth.updated_at) {
                    *local_auth = remote_auth.clone();
                }
            } else {
                local_auths.insert(remote_auth.id.clone(), remote_auth.clone());
            }
        }
        local.authentications = local_auths.into_values().collect();

        // Merge Proxies
        let mut local_proxies: HashMap<String, Proxy> = local.proxies.drain(..).map(|p| (p.id.clone(), p)).collect();
        for remote_proxy in &remote.proxies {
            if !remote_proxy.synced { continue; }
            if let Some(local_proxy) = local_proxies.get_mut(&remote_proxy.id) {
                if is_newer(&remote_proxy.updated_at, &local_proxy.updated_at) {
                    *local_proxy = remote_proxy.clone();
                }
            } else {
                local_proxies.insert(remote_proxy.id.clone(), remote_proxy.clone());
            }
        }
        local.proxies = local_proxies.into_values().collect();
    }

    fn merge_local_to_remote(&self, local: &Config, remote: &mut SyncConfig) {
        // 1. Identify what should be removed from remote
        // If an item exists in remote but is missing in local or has synced=false in local
        let local_servers: HashMap<String, &Server> = local.servers.iter().map(|s| (s.id.clone(), s)).collect();
        let local_auths: HashMap<String, &Authentication> = local.authentications.iter().map(|a| (a.id.clone(), a)).collect();
        let local_proxies: HashMap<String, &Proxy> = local.proxies.iter().map(|p| (p.id.clone(), p)).collect();

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

        // 2. Add/Update from local
        for local_server in &local.servers {
            if !local_server.synced { continue; }
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
            if let Some(remote_proxy) = remote_proxies.get_mut(&local_proxy.id) {
                if is_newer(&local_proxy.updated_at, &remote_proxy.updated_at) {
                    *remote_proxy = local_proxy.clone();
                }
            } else {
                remote_proxies.insert(local_proxy.id.clone(), local_proxy.clone());
            }
        }
        remote.proxies = remote_proxies.into_values().collect();
    }
}

fn is_newer(a: &str, b: &str) -> bool {
    let dt_a = DateTime::parse_from_rfc3339(a).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now());
    let dt_b = DateTime::parse_from_rfc3339(b).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now() - chrono::Duration::days(365 * 10)); // Very old date
    dt_a > dt_b
}
