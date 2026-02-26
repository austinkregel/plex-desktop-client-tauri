use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub is_remote: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_identifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredServer {
    pub name: String,
    pub base_url: String,
    pub machine_identifier: Option<String>,
}

/// Plex v2 Resources API returns a flat JSON array of PlexResource objects.
pub type PlexResourcesResponse = Vec<PlexResource>;

#[derive(Debug, Serialize, Deserialize)]
pub struct PlexResource {
    pub name: String,
    pub provides: Option<String>,
    #[serde(rename = "clientIdentifier")]
    pub client_identifier: Option<String>,
    #[serde(default)]
    pub connections: Vec<PlexConnection>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlexConnection {
    pub protocol: String,
    pub address: String,
    pub port: u16,
    pub uri: Option<String>,
    pub local: Option<bool>,
    pub relay: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plex_resources_response_deserialize_flat_array() {
        let json = r#"[
            {
                "name": "Living Room",
                "provides": "server",
                "clientIdentifier": "abc123",
                "connections": [
                    {"protocol": "http", "address": "192.168.1.100", "port": 32400, "uri": "http://192.168.1.100:32400", "local": true, "relay": false}
                ]
            },
            {
                "name": "Plex Web",
                "provides": "player",
                "clientIdentifier": "def456",
                "connections": []
            }
        ]"#;
        let resources: PlexResourcesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].name, "Living Room");
        assert_eq!(resources[0].provides, Some("server".into()));
        assert_eq!(resources[0].client_identifier, Some("abc123".into()));
        assert_eq!(resources[0].connections.len(), 1);
        assert_eq!(resources[0].connections[0].protocol, "http");
        assert_eq!(resources[0].connections[0].port, 32400);
        assert_eq!(resources[0].connections[0].local, Some(true));
        assert_eq!(resources[0].connections[0].relay, Some(false));
        assert_eq!(resources[1].name, "Plex Web");
        assert_eq!(resources[1].provides, Some("player".into()));
    }

    #[test]
    fn test_plex_resources_response_empty_array() {
        let json = "[]";
        let resources: PlexResourcesResponse = serde_json::from_str(json).unwrap();
        assert!(resources.is_empty());
    }

    #[test]
    fn test_plex_resource_missing_optional_fields() {
        let json = r#"[{"name": "Server", "connections": []}]"#;
        let resources: PlexResourcesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resources[0].name, "Server");
        assert!(resources[0].provides.is_none());
        assert!(resources[0].client_identifier.is_none());
        assert!(resources[0].connections.is_empty());
    }

    #[test]
    fn test_plex_resource_connections_default_to_empty() {
        let json = r#"[{"name": "Server"}]"#;
        let resources: PlexResourcesResponse = serde_json::from_str(json).unwrap();
        assert!(resources[0].connections.is_empty());
    }

    #[test]
    fn test_plex_connection_with_uri() {
        let json = r#"[{
            "name": "Server",
            "connections": [
                {"protocol": "https", "address": "1.2.3.4", "port": 32400, "uri": "https://1-2-3-4.plex.direct:32400", "local": false, "relay": false}
            ]
        }]"#;
        let resources: PlexResourcesResponse = serde_json::from_str(json).unwrap();
        let conn = &resources[0].connections[0];
        assert_eq!(conn.uri, Some("https://1-2-3-4.plex.direct:32400".into()));
        assert_eq!(conn.local, Some(false));
    }

    #[test]
    fn test_plex_connection_without_optional_booleans() {
        let json = r#"[{
            "name": "Server",
            "connections": [
                {"protocol": "http", "address": "10.0.0.5", "port": 32400}
            ]
        }]"#;
        let resources: PlexResourcesResponse = serde_json::from_str(json).unwrap();
        let conn = &resources[0].connections[0];
        assert!(conn.uri.is_none());
        assert!(conn.local.is_none());
        assert!(conn.relay.is_none());
    }

    #[test]
    fn test_plex_resource_multiple_connections() {
        let json = r#"[{
            "name": "Server",
            "provides": "server",
            "clientIdentifier": "svr1",
            "connections": [
                {"protocol": "http", "address": "192.168.1.50", "port": 32400, "local": true, "relay": false},
                {"protocol": "https", "address": "ext.example.com", "port": 443, "uri": "https://ext.example.com:443", "local": false, "relay": false},
                {"protocol": "https", "address": "relay.plex.tv", "port": 443, "local": false, "relay": true}
            ]
        }]"#;
        let resources: PlexResourcesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resources[0].connections.len(), 3);
        assert_eq!(resources[0].connections[2].relay, Some(true));
    }

    #[test]
    fn test_server_config_serialize_roundtrip() {
        let config = ServerConfig {
            id: "test-id".into(),
            name: "My Server".into(),
            base_url: "http://localhost:32400".into(),
            is_remote: false,
            machine_identifier: Some("machine123".into()),
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-id");
        assert_eq!(parsed.machine_identifier, Some("machine123".into()));
    }

    #[test]
    fn test_server_config_omits_none_machine_identifier() {
        let config = ServerConfig {
            id: "id".into(),
            name: "Name".into(),
            base_url: "http://x".into(),
            is_remote: false,
            machine_identifier: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("machine_identifier"));
    }
}
