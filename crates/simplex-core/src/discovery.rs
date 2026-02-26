//! Server discovery for Plex media servers.
//!
//! Scans localhost and common IPs for Plex servers, and fetches server lists
//! from the Plex.tv resources API when an auth token is available.

use crate::config::{add_server, load_config, save_config};
use crate::keychain;
use crate::models::{DiscoveredServer, PlexConnection, PlexResource, PlexResourcesResponse};
use std::time::Duration;
use thiserror::Error;

const PRODUCT_NAME: &str = "Simplex";
const CLIENT_IDENTIFIER: &str = "simplex-app";

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Config error: {0}")]
    Config(#[from] crate::config::ConfigError),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("{0}")]
    Other(String),
}

impl From<crate::keychain::KeychainError> for DiscoveryError {
    fn from(e: crate::keychain::KeychainError) -> Self {
        DiscoveryError::Other(e.to_string())
    }
}

/// Scans localhost and common IPs for Plex servers.
pub async fn discover_servers() -> Result<Vec<DiscoveredServer>, DiscoveryError> {
    tracing::info!("Starting server discovery...");
    let mut discovered = Vec::new();

    let common_ports = vec![32400];

    let mut ip_addresses = Vec::new();
    ip_addresses.push("127.0.0.1".to_string());

    let common_gateways = vec![
        "192.168.1.1",
        "192.168.0.1",
        "192.168.2.1",
        "10.0.0.1",
        "172.16.0.1",
    ];

    for gateway in &common_gateways {
        ip_addresses.push(gateway.to_string());
    }

    for gateway in &common_gateways {
        if let Some(parts) = gateway.split('.').collect::<Vec<&str>>().get(0..3) {
            let base = parts.join(".");
            for i in 1..=10 {
                ip_addresses.push(format!("{}.{}", base, i));
            }
        }
    }

    ip_addresses.sort();
    ip_addresses.dedup();

    tracing::info!("Checking {} IP addresses on port 32400...", ip_addresses.len());

    let start = std::time::Instant::now();
    let timeout_duration = Duration::from_secs(5);
    let mut checked = 0;

    for ip in ip_addresses {
        if start.elapsed() > timeout_duration {
            tracing::info!("Discovery timeout reached, stopping checks");
            break;
        }

        for port in &common_ports {
            if start.elapsed() > timeout_duration {
                break;
            }

            let base_url = format!("http://{}:{}", ip, port);
            checked += 1;

            match tokio::time::timeout(
                Duration::from_millis(500),
                fetch_plex_server_info(&base_url),
            )
            .await
            {
                Ok(Ok(server_info)) => {
                    tracing::info!("Found server: {} at {}", server_info.name, server_info.base_url);
                    discovered.push(server_info);
                }
                Ok(Err(_)) => {}
                Err(_) => {}
            }
        }
    }

    tracing::info!(
        "Discovery complete, found {} servers (checked {} addresses)",
        discovered.len(),
        checked
    );
    Ok(discovered)
}

/// Probes a single URL to check if it's a Plex server and extract info.
pub async fn fetch_plex_server_info(base_url: &str) -> Result<DiscoveredServer, DiscoveryError> {
    let info_url = format!("{}/", base_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| DiscoveryError::Http(format!("Failed to create HTTP client: {}", e)))?;

    match client.get(&info_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                let text = response
                    .text()
                    .await
                    .map_err(|e| DiscoveryError::Http(format!("Failed to read response: {}", e)))?;

                let machine_id = extract_machine_identifier(&text);
                let name =
                    extract_server_name(&text).unwrap_or_else(|| "Plex Media Server".to_string());

                Ok(DiscoveredServer {
                    name,
                    base_url: base_url.to_string(),
                    machine_identifier: machine_id,
                })
            } else {
                Err(DiscoveryError::Http(format!(
                    "Server returned status: {}",
                    response.status()
                )))
            }
        }
        Err(_) => Err(DiscoveryError::Http("Server not reachable".to_string())),
    }
}

/// Calls the plex.tv API for server list using an auth token.
pub async fn fetch_plex_resources(token: &str) -> Result<Vec<DiscoveredServer>, DiscoveryError> {
    tracing::info!("Fetching Plex resources with token...");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| DiscoveryError::Http(format!("Failed to create HTTP client: {}", e)))?;

    let url =
        "https://clients.plex.tv/api/v2/resources?includeHttps=1&includeRelay=1&includeIPv6=1";

    let response = client
        .get(url)
        .header("X-Plex-Token", token)
        .header("X-Plex-Product", PRODUCT_NAME)
        .header("X-Plex-Version", "1.0.0")
        .header("X-Plex-Client-Identifier", CLIENT_IDENTIFIER)
        .header("X-Plex-Platform", "Linux")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| DiscoveryError::Http(format!("Failed to fetch resources: {}", e)))?;

    if !response.status().is_success() {
        return Err(DiscoveryError::Http(format!(
            "Resources API returned status: {}",
            response.status()
        )));
    }

    let text = response
        .text()
        .await
        .map_err(|e| DiscoveryError::Http(format!("Failed to read response: {}", e)))?;

    let resources: PlexResourcesResponse = serde_json::from_str(&text)
        .map_err(|e| {
            tracing::error!("Failed to parse resources JSON. Body starts with: {}", &text[..text.len().min(200)]);
            DiscoveryError::Parse(format!("Failed to parse resources JSON: {}", e))
        })?;

    tracing::info!("Parsed {} resources from API", resources.len());

    let discovered = servers_from_resources(&resources);

    tracing::info!("Discovered {} servers from resources API", discovered.len());
    Ok(discovered)
}

/// Gets the machine identifier from a Plex server's root endpoint.
pub async fn fetch_server_machine_id(base_url: &str) -> Result<String, DiscoveryError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| DiscoveryError::Http(format!("Failed to create HTTP client: {}", e)))?;

    let info_url = format!("{}/", base_url);
    match client.get(&info_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                let text = response
                    .text()
                    .await
                    .map_err(|e| DiscoveryError::Http(format!("Failed to read response: {}", e)))?;
                extract_machine_identifier(&text)
                    .ok_or_else(|| DiscoveryError::Other("Machine identifier not found".to_string()))
            } else {
                Err(DiscoveryError::Http(format!(
                    "Server returned status: {}",
                    response.status()
                )))
            }
        }
        Err(e) => Err(DiscoveryError::Http(format!("Failed to connect: {}", e))),
    }
}

/// Adds a discovered server to config and optionally updates its machine identifier.
pub fn auto_add_discovered_server(
    name: String,
    base_url: String,
    machine_identifier: Option<String>,
) -> Result<(), DiscoveryError> {
    let is_remote = base_url.starts_with("https://");
    add_server(name.clone(), base_url.clone(), is_remote)?;

    if let Some(machine_id) = machine_identifier {
        let mut config = load_config();
        if let Some(server) = config.servers.iter_mut().find(|s| s.base_url == base_url) {
            server.machine_identifier = Some(machine_id);
            save_config(&config)?;
        }
    }

    Ok(())
}

/// Stores the token, fetches resources from Plex API, and auto-adds discovered servers.
pub async fn discover_servers_from_token(token: String) -> Result<Vec<DiscoveredServer>, DiscoveryError> {
    tracing::info!("Discovering servers using token...");

    keychain::set_auth_token(&token)?;
    tracing::info!("Token stored successfully");

    let servers = fetch_plex_resources(&token).await?;
    tracing::info!("Fetched {} servers from resources API", servers.len());

    for server in &servers {
        tracing::info!("Auto-adding server: {} at {}", server.name, server.base_url);
        if let Err(e) = auto_add_discovered_server(
            server.name.clone(),
            server.base_url.clone(),
            server.machine_identifier.clone(),
        ) {
            tracing::warn!("Failed to auto-add server {}: {}", server.name, e);
        } else {
            tracing::info!("Successfully added server: {}", server.name);
        }
    }

    let final_config = load_config();
    tracing::info!("Final config has {} servers", final_config.servers.len());

    Ok(servers)
}

/// Extracts server entries from a parsed Plex v2 resources response.
/// Filters to devices that provide "server" and selects the best connection.
pub fn servers_from_resources(resources: &[PlexResource]) -> Vec<DiscoveredServer> {
    let mut discovered = Vec::new();

    for device in resources {
        let provides = device.provides.as_deref().unwrap_or("");
        if !provides.contains("server") {
            continue;
        }

        if let Some(conn) = best_server_connection(&device.connections) {
            let base_url = conn.uri.clone().unwrap_or_else(|| {
                format!("{}://{}:{}", conn.protocol, conn.address, conn.port)
            });

            discovered.push(DiscoveredServer {
                name: device.name.clone(),
                base_url,
                machine_identifier: device.client_identifier.clone(),
            });
        }
    }

    discovered
}

/// Selects the best connection for a server.
/// Priority: local non-relay > any non-relay > first available.
pub fn best_server_connection(connections: &[PlexConnection]) -> Option<&PlexConnection> {
    connections.iter()
        .find(|c| c.local == Some(true) && c.relay != Some(true))
        .or_else(|| connections.iter().find(|c| c.relay != Some(true)))
        .or_else(|| connections.first())
}

/// Simple XML attribute extraction for machineIdentifier.
pub fn extract_machine_identifier(xml: &str) -> Option<String> {
    if let Some(start) = xml.find("machineIdentifier=\"") {
        let start_idx = start + "machineIdentifier=\"".len();
        if let Some(end) = xml[start_idx..].find('"') {
            return Some(xml[start_idx..start_idx + end].to_string());
        }
    }
    None
}

/// Simple XML attribute extraction for friendlyName.
pub fn extract_server_name(xml: &str) -> Option<String> {
    if let Some(start) = xml.find("friendlyName=\"") {
        let start_idx = start + "friendlyName=\"".len();
        if let Some(end) = xml[start_idx..].find('"') {
            return Some(xml[start_idx..start_idx + end].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{PlexConnection, PlexResource};

    fn make_connection(
        protocol: &str,
        address: &str,
        port: u16,
        uri: Option<&str>,
        local: Option<bool>,
        relay: Option<bool>,
    ) -> PlexConnection {
        PlexConnection {
            protocol: protocol.into(),
            address: address.into(),
            port,
            uri: uri.map(Into::into),
            local,
            relay,
        }
    }

    fn make_resource(name: &str, provides: Option<&str>, connections: Vec<PlexConnection>) -> PlexResource {
        PlexResource {
            name: name.into(),
            provides: provides.map(Into::into),
            client_identifier: Some(format!("{}-id", name)),
            connections,
        }
    }

    // -- XML extraction tests --

    #[test]
    fn test_extract_machine_identifier() {
        let xml = r#"<MediaContainer machineIdentifier="abc123def456" friendlyName="My Plex Server"/>"#;
        assert_eq!(
            extract_machine_identifier(xml),
            Some("abc123def456".to_string())
        );

        let xml_no_id = r#"<MediaContainer friendlyName="My Plex Server"/>"#;
        assert_eq!(extract_machine_identifier(xml_no_id), None);

        let xml_empty = "";
        assert_eq!(extract_machine_identifier(xml_empty), None);
    }

    #[test]
    fn test_extract_server_name() {
        let xml = r#"<MediaContainer machineIdentifier="abc123" friendlyName="My Plex Server"/>"#;
        assert_eq!(
            extract_server_name(xml),
            Some("My Plex Server".to_string())
        );

        let xml_no_name = r#"<MediaContainer machineIdentifier="abc123"/>"#;
        assert_eq!(extract_server_name(xml_no_name), None);

        let xml_empty = "";
        assert_eq!(extract_server_name(xml_empty), None);
    }

    // -- best_server_connection tests --

    #[test]
    fn test_best_connection_prefers_local_non_relay() {
        let connections = vec![
            make_connection("https", "relay.plex.tv", 443, None, Some(false), Some(true)),
            make_connection("http", "192.168.1.50", 32400, None, Some(true), Some(false)),
            make_connection("https", "ext.example.com", 443, None, Some(false), Some(false)),
        ];
        let best = best_server_connection(&connections).unwrap();
        assert_eq!(best.address, "192.168.1.50");
    }

    #[test]
    fn test_best_connection_falls_back_to_first() {
        let connections = vec![
            make_connection("https", "ext.example.com", 443, None, Some(false), Some(false)),
            make_connection("https", "relay.plex.tv", 443, None, Some(false), Some(true)),
        ];
        let best = best_server_connection(&connections).unwrap();
        assert_eq!(best.address, "ext.example.com");
    }

    #[test]
    fn test_best_connection_empty_returns_none() {
        assert!(best_server_connection(&[]).is_none());
    }

    #[test]
    fn test_best_connection_skips_local_relay() {
        let connections = vec![
            make_connection("http", "192.168.1.50", 32400, None, Some(true), Some(true)),
            make_connection("https", "ext.example.com", 443, None, Some(false), Some(false)),
        ];
        let best = best_server_connection(&connections).unwrap();
        assert_eq!(best.address, "ext.example.com");
    }

    #[test]
    fn test_best_connection_local_none_relay_none_not_preferred() {
        let connections = vec![
            make_connection("http", "10.0.0.1", 32400, None, None, None),
            make_connection("http", "192.168.1.10", 32400, None, Some(true), Some(false)),
        ];
        let best = best_server_connection(&connections).unwrap();
        assert_eq!(best.address, "192.168.1.10");
    }

    // -- servers_from_resources tests --

    #[test]
    fn test_servers_from_resources_filters_to_servers_only() {
        let resources = vec![
            make_resource("My Server", Some("server"), vec![
                make_connection("http", "192.168.1.50", 32400, None, Some(true), Some(false)),
            ]),
            make_resource("Plex Web", Some("player"), vec![
                make_connection("https", "app.plex.tv", 443, None, None, None),
            ]),
            make_resource("Plexamp", Some("player,pubsub-player"), vec![]),
        ];
        let servers = servers_from_resources(&resources);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "My Server");
    }

    #[test]
    fn test_servers_from_resources_uses_uri_when_available() {
        let resources = vec![
            make_resource("Server", Some("server"), vec![
                make_connection(
                    "https", "1.2.3.4", 32400,
                    Some("https://1-2-3-4.abcdef.plex.direct:32400"),
                    Some(true), Some(false),
                ),
            ]),
        ];
        let servers = servers_from_resources(&resources);
        assert_eq!(servers[0].base_url, "https://1-2-3-4.abcdef.plex.direct:32400");
    }

    #[test]
    fn test_servers_from_resources_constructs_url_without_uri() {
        let resources = vec![
            make_resource("Server", Some("server"), vec![
                make_connection("http", "192.168.1.50", 32400, None, Some(true), Some(false)),
            ]),
        ];
        let servers = servers_from_resources(&resources);
        assert_eq!(servers[0].base_url, "http://192.168.1.50:32400");
    }

    #[test]
    fn test_servers_from_resources_uses_client_identifier_as_machine_id() {
        let resources = vec![
            make_resource("Server", Some("server"), vec![
                make_connection("http", "10.0.0.1", 32400, None, Some(true), Some(false)),
            ]),
        ];
        let servers = servers_from_resources(&resources);
        assert_eq!(servers[0].machine_identifier, Some("Server-id".into()));
    }

    #[test]
    fn test_servers_from_resources_skips_server_without_connections() {
        let resources = vec![
            make_resource("Server", Some("server"), vec![]),
        ];
        let servers = servers_from_resources(&resources);
        assert!(servers.is_empty());
    }

    #[test]
    fn test_servers_from_resources_empty_input() {
        let servers = servers_from_resources(&[]);
        assert!(servers.is_empty());
    }

    #[test]
    fn test_servers_from_resources_device_with_no_provides() {
        let resources = vec![
            make_resource("Mystery", None, vec![
                make_connection("http", "10.0.0.1", 32400, None, None, None),
            ]),
        ];
        let servers = servers_from_resources(&resources);
        assert!(servers.is_empty());
    }

    #[test]
    fn test_servers_from_resources_multiple_servers() {
        let resources = vec![
            make_resource("Server A", Some("server"), vec![
                make_connection("http", "192.168.1.10", 32400, None, Some(true), Some(false)),
            ]),
            make_resource("Server B", Some("server"), vec![
                make_connection("https", "remote.example.com", 443, Some("https://remote.example.com:443"), Some(false), Some(false)),
            ]),
            make_resource("Phone", Some("player"), vec![]),
        ];
        let servers = servers_from_resources(&resources);
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "Server A");
        assert_eq!(servers[0].base_url, "http://192.168.1.10:32400");
        assert_eq!(servers[1].name, "Server B");
        assert_eq!(servers[1].base_url, "https://remote.example.com:443");
    }

    #[test]
    fn test_servers_from_resources_prefers_local_over_relay() {
        let resources = vec![
            make_resource("Server", Some("server"), vec![
                make_connection("https", "relay.plex.tv", 443, Some("https://relay.plex.tv"), Some(false), Some(true)),
                make_connection("http", "192.168.1.50", 32400, Some("http://192.168.1.50:32400"), Some(true), Some(false)),
            ]),
        ];
        let servers = servers_from_resources(&resources);
        assert_eq!(servers[0].base_url, "http://192.168.1.50:32400");
    }
}
