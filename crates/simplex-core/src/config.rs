use crate::models::ServerConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use thiserror::Error;
use url::Url;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const RATE_LIMIT_WINDOW_SECS: u64 = 60;
const RATE_LIMIT_MAX_REQUESTS: usize = 10;

static RATE_LIMITER: OnceLock<Mutex<HashMap<String, Vec<Instant>>>> = OnceLock::new();

fn get_rate_limiter() -> &'static Mutex<HashMap<String, Vec<Instant>>> {
    RATE_LIMITER.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    NotFound(String),
    #[error("Rate limit exceeded: {0}")]
    RateLimited(String),
}

// ---------------------------------------------------------------------------
// User settings types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StreamQuality {
    Original,
    Maximum(u32),
}

impl Default for StreamQuality {
    fn default() -> Self {
        Self::Original
    }
}

impl StreamQuality {
    pub const PRESETS: &'static [(u32, &'static str)] = &[
        (20_000, "20 Mbps (4K)"),
        (12_000, "12 Mbps (1080p)"),
        (8_000, "8 Mbps (1080p)"),
        (4_000, "4 Mbps (720p)"),
        (2_000, "2 Mbps (480p)"),
    ];

    pub fn label(&self) -> String {
        match self {
            Self::Original => "Original".to_string(),
            Self::Maximum(kbps) => {
                Self::PRESETS
                    .iter()
                    .find(|(k, _)| k == kbps)
                    .map(|(_, l)| l.to_string())
                    .unwrap_or_else(|| format!("{} kbps", kbps))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MismatchAction {
    Pause,
    WarnDialog,
    Ignore,
}

impl Default for MismatchAction {
    fn default() -> Self {
        Self::WarnDialog
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleAutoEnable {
    Always,
    OnMismatch,
    Never,
}

impl Default for SubtitleAutoEnable {
    fn default() -> Self {
        Self::OnMismatch
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackSettings {
    #[serde(default)]
    pub quality: StreamQuality,
    #[serde(default)]
    pub auto_adjust_quality: bool,
    #[serde(default)]
    pub preferred_codec: Option<String>,
    #[serde(default = "default_true")]
    pub remember_volume: bool,
    #[serde(default = "default_volume")]
    pub last_volume: f64,
    #[serde(default = "default_speed")]
    pub playback_speed: f64,
}

fn default_true() -> bool { true }
fn default_volume() -> f64 { 1.0 }
fn default_speed() -> f64 { 1.0 }

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            quality: StreamQuality::default(),
            auto_adjust_quality: false,
            preferred_codec: None,
            remember_volume: true,
            last_volume: 1.0,
            playback_speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    #[serde(default = "default_audio_languages")]
    pub preferred_languages: Vec<String>,
    #[serde(default)]
    pub language_mismatch_action: MismatchAction,
}

fn default_audio_languages() -> Vec<String> {
    vec!["eng".to_string(), "en".to_string()]
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            preferred_languages: default_audio_languages(),
            language_mismatch_action: MismatchAction::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleSettings {
    #[serde(default)]
    pub preferred_languages: Vec<String>,
    #[serde(default)]
    pub auto_enable: SubtitleAutoEnable,
    #[serde(default)]
    pub prefer_forced: bool,
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            preferred_languages: Vec::new(),
            auto_enable: SubtitleAutoEnable::default(),
            prefer_forced: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettings {
    #[serde(default)]
    pub playback: PlaybackSettings,
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default)]
    pub subtitles: SubtitleSettings,
}

// ---------------------------------------------------------------------------
// AppConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub servers: Vec<ServerConfig>,
    pub default_server_id: Option<String>,
    // Legacy migration field: deserialize only, never write back to disk.
    #[serde(default, skip_serializing)]
    pub auth_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_stats: Option<serde_json::Value>,
    // Legacy: kept for deserialization migration only.
    #[serde(default, skip_serializing)]
    pub device_settings: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pinned_library_keys: Vec<String>,
    #[serde(default)]
    pub user_settings: UserSettings,
}

impl AppConfig {
    pub fn pin_library(&mut self, key: &str) {
        if !self.pinned_library_keys.iter().any(|k| k == key) {
            self.pinned_library_keys.push(key.to_string());
        }
    }

    pub fn unpin_library(&mut self, key: &str) {
        self.pinned_library_keys.retain(|k| k != key);
    }

    pub fn is_library_pinned(&self, key: &str) -> bool {
        self.pinned_library_keys.iter().any(|k| k == key)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            servers: Vec::new(),
            default_server_id: None,
            auth_token: None,
            client_id: None,
            session_stats: None,
            device_settings: None,
            pinned_library_keys: Vec::new(),
            user_settings: UserSettings::default(),
        }
    }
}

pub fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .expect("Failed to get config directory")
        .join("simplex");

    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    }

    let config_path = config_dir.join("config.json");

    #[cfg(unix)]
    {
        if config_path.exists() {
            if let Ok(mut perms) = fs::metadata(&config_path).map(|m| m.permissions()) {
                perms.set_mode(0o600);
                let _ = fs::set_permissions(&config_path, perms);
            }
        }
    }

    config_path
}

pub fn load_config() -> AppConfig {
    let config_path = get_config_path();

    if !config_path.exists() {
        let default_config = AppConfig::default();
        let _ = save_config(&default_config);
        return default_config;
    }

    match fs::read_to_string(&config_path) {
        Ok(content) => {
            if content.trim().is_empty() {
                AppConfig::default()
            } else {
                serde_json::from_str(&content).unwrap_or_else(|_| AppConfig::default())
            }
        }
        Err(_) => AppConfig::default(),
    }
}

pub fn save_config(config: &AppConfig) -> Result<(), ConfigError> {
    let config_path = get_config_path();
    // Never persist auth tokens; keychain is the only supported store.
    let mut config_to_save = config.clone();
    config_to_save.auth_token = None;
    let content = serde_json::to_string_pretty(&config_to_save)?;

    fs::write(&config_path, content)?;

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&config_path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&config_path, perms)?;
    }

    Ok(())
}

pub fn validate_server_url(url: &str) -> Result<(), ConfigError> {
    Url::parse(url).map_err(|_| ConfigError::Validation("Invalid URL format".to_string()))?;

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ConfigError::Validation(
            "URL must start with http:// or https://".to_string(),
        ));
    }

    Ok(())
}

/// Normalizes a URL to its origin (scheme + host + port).
pub fn normalize_url_to_origin(url: &str) -> Result<String, ConfigError> {
    let parsed =
        Url::parse(url).map_err(|_| ConfigError::Validation("Invalid URL format".to_string()))?;

    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(ConfigError::Validation(
            "URL must use http:// or https:// scheme".to_string(),
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| ConfigError::Validation("Missing host in URL".to_string()))?;
    let port = parsed.port();

    Ok(if let Some(p) = port {
        format!("{}://{}:{}", parsed.scheme(), host, p)
    } else {
        format!("{}://{}", parsed.scheme(), host)
    })
}

/// Validates that a deep link baseUrl matches one of the allowed server origins.
pub fn validate_deep_link_base_url_against_origins(
    base_url: &str,
    allowed_origins: &[String],
) -> Result<(), ConfigError> {
    let normalized_origin = normalize_url_to_origin(base_url)?;

    if allowed_origins.iter().any(|origin| origin == &normalized_origin) {
        Ok(())
    } else {
        Err(ConfigError::Validation(format!(
            "Deep link baseUrl '{}' does not match any configured server",
            base_url
        )))
    }
}

/// Validates that a deep link baseUrl matches a configured server.
pub fn validate_deep_link_base_url(base_url: &str) -> Result<(), ConfigError> {
    let config = load_config();
    let allowed_origins: Vec<String> = config
        .servers
        .iter()
        .filter_map(|server| normalize_url_to_origin(&server.base_url).ok())
        .collect();

    validate_deep_link_base_url_against_origins(base_url, &allowed_origins)
}

pub fn get_servers() -> Result<Vec<ServerConfig>, ConfigError> {
    let config = load_config();
    Ok(config.servers)
}

pub fn add_server(name: String, base_url: String, is_remote: bool) -> Result<(), ConfigError> {
    validate_server_url(&base_url)?;

    let mut config = load_config();

    if config.servers.iter().any(|s| s.base_url == base_url) {
        return Err(ConfigError::Validation(
            "Server with this URL already exists".to_string(),
        ));
    }

    let id = format!("server-{}", config.servers.len() + 1);
    let server = ServerConfig {
        id,
        name,
        base_url,
        is_remote,
        machine_identifier: None,
    };

    config.servers.push(server);

    if config.default_server_id.is_none() {
        config.default_server_id = config.servers.first().map(|s| s.id.clone());
    }

    save_config(&config)
}

pub fn update_server(
    id: String,
    name: Option<String>,
    base_url: Option<String>,
) -> Result<(), ConfigError> {
    let mut config = load_config();

    let server = config
        .servers
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| ConfigError::NotFound("Server not found".to_string()))?;

    if let Some(new_name) = name {
        server.name = new_name;
    }

    if let Some(new_url) = base_url {
        validate_server_url(&new_url)?;
        server.base_url = new_url;
    }

    save_config(&config)
}

pub fn remove_server(id: String) -> Result<(), ConfigError> {
    let mut config = load_config();

    let index = config
        .servers
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| ConfigError::NotFound("Server not found".to_string()))?;

    config.servers.remove(index);

    if config.default_server_id.as_ref() == Some(&id) {
        config.default_server_id = config.servers.first().map(|s| s.id.clone());
    }

    save_config(&config)
}

pub fn set_default_server(id: String) -> Result<(), ConfigError> {
    let mut config = load_config();

    if !config.servers.iter().any(|s| s.id == id) {
        return Err(ConfigError::NotFound("Server not found".to_string()));
    }

    config.default_server_id = Some(id);
    save_config(&config)
}

pub fn get_default_server() -> Result<Option<ServerConfig>, ConfigError> {
    let config = load_config();

    if let Some(default_id) = config.default_server_id {
        config
            .servers
            .into_iter()
            .find(|s| s.id == default_id)
            .map(Some)
            .ok_or_else(|| ConfigError::NotFound("Default server not found".to_string()))
    } else {
        Ok(None)
    }
}

pub fn get_client_id() -> Result<Option<String>, ConfigError> {
    let config = load_config();
    Ok(config.client_id)
}

pub fn set_client_id(client_id: String) -> Result<(), ConfigError> {
    let mut config = load_config();
    config.client_id = Some(client_id);
    save_config(&config)
}

pub fn set_session_stats(stats: serde_json::Value) -> Result<(), ConfigError> {
    let mut config = load_config();
    config.session_stats = Some(stats);
    save_config(&config)
}

pub fn set_device_settings(settings: serde_json::Value) -> Result<(), ConfigError> {
    let mut config = load_config();
    config.device_settings = Some(settings);
    save_config(&config)
}

pub fn load_user_settings() -> UserSettings {
    load_config().user_settings
}

pub fn save_user_settings(settings: &UserSettings) -> Result<(), ConfigError> {
    let mut config = load_config();
    config.user_settings = settings.clone();
    save_config(&config)
}

pub fn update_user_settings<F: FnOnce(&mut UserSettings)>(f: F) -> Result<(), ConfigError> {
    let mut config = load_config();
    f(&mut config.user_settings);
    save_config(&config)
}

pub fn resolve_server_url(
    base_url: Option<&str>,
    server_id: Option<&str>,
) -> Result<String, ConfigError> {
    if let Some(url) = base_url {
        validate_server_url(url)?;
        return Ok(url.to_string());
    }

    if let Some(id) = server_id {
        let config = load_config();
        if let Some(server) = config.servers.iter().find(|s| {
            s.machine_identifier
                .as_ref()
                .map(|m| m == id)
                .unwrap_or(false)
        }) {
            return Ok(server.base_url.clone());
        }
    }

    let config = load_config();
    if let Some(default_id) = config.default_server_id {
        if let Some(server) = config.servers.iter().find(|s| s.id == default_id) {
            return Ok(server.base_url.clone());
        }
    }

    if let Some(server) = config.servers.first() {
        return Ok(server.base_url.clone());
    }

    Err(ConfigError::NotFound(
        "No server configured. Please add a server in settings.".to_string(),
    ))
}

pub fn check_rate_limit(source: &str) -> Result<(), ConfigError> {
    let limiter = get_rate_limiter();
    let mut store = limiter
        .lock()
        .map_err(|e| ConfigError::RateLimited(format!("Rate limiter lock error: {}", e)))?;

    let now = Instant::now();
    let window_start = now - Duration::from_secs(RATE_LIMIT_WINDOW_SECS);

    let requests = store.entry(source.to_string()).or_insert_with(Vec::new);
    requests.retain(|&time| time > window_start);

    if requests.len() >= RATE_LIMIT_MAX_REQUESTS {
        return Err(ConfigError::RateLimited(format!(
            "Rate limit exceeded: maximum {} requests per {} seconds",
            RATE_LIMIT_MAX_REQUESTS, RATE_LIMIT_WINDOW_SECS
        )));
    }

    requests.push(now);
    Ok(())
}

/// Construct a fully-qualified Plex web URL from base URL, optional server ID, key, and extra query.
pub fn construct_plex_url(
    base_url: &str,
    server_id: Option<&str>,
    key: Option<&str>,
    extra_query: Option<&str>,
) -> String {
    let mut url = base_url.trim_end_matches('/').to_string();
    url.push_str("/web/index.html");

    if let Some(id) = server_id {
        url.push_str(&format!("#!/server/{}/details", id));
    } else {
        url.push_str("#!/details");
    }

    let mut query_parts = Vec::new();

    if let Some(k) = key {
        query_parts.push(format!("key={}", urlencoding::encode(k)));
    }

    if let Some(extra) = extra_query {
        query_parts.push(extra.to_string());
    }

    if !query_parts.is_empty() {
        url.push('?');
        url.push_str(&query_parts.join("&"));
    }

    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_server_url() {
        assert!(validate_server_url("http://localhost:32400").is_ok());
        assert!(validate_server_url("https://plex.example.com").is_ok());

        assert!(validate_server_url("not-a-url").is_err());
        assert!(validate_server_url("ftp://example.com").is_err());
        assert!(validate_server_url("localhost:32400").is_err());
    }

    #[test]
    fn test_normalize_url_to_origin() {
        assert_eq!(
            normalize_url_to_origin("http://localhost:32400").unwrap(),
            "http://localhost:32400"
        );

        assert_eq!(
            normalize_url_to_origin("https://plex.example.com").unwrap(),
            "https://plex.example.com"
        );

        assert_eq!(
            normalize_url_to_origin("http://192.168.1.100:32400/web/index.html?key=value").unwrap(),
            "http://192.168.1.100:32400"
        );

        assert!(normalize_url_to_origin("not-a-url").is_err());
        assert!(normalize_url_to_origin("ftp://example.com").is_err());
    }

    #[test]
    fn test_validate_deep_link_base_url_allowlist() {
        let allowed_origins = vec![
            "http://localhost:32400".to_string(),
            "https://plex.example.com".to_string(),
            "http://192.168.1.100:32400".to_string(),
        ];

        assert!(validate_deep_link_base_url_against_origins(
            "http://localhost:32400",
            &allowed_origins
        )
        .is_ok());

        assert!(validate_deep_link_base_url_against_origins(
            "https://plex.example.com",
            &allowed_origins
        )
        .is_ok());

        assert!(validate_deep_link_base_url_against_origins(
            "http://192.168.1.100:32400",
            &allowed_origins
        )
        .is_ok());

        assert!(validate_deep_link_base_url_against_origins(
            "http://localhost:32400/web/index.html",
            &allowed_origins
        )
        .is_ok());

        assert!(validate_deep_link_base_url_against_origins(
            "https://plex.example.com:443/web/index.html?key=value",
            &allowed_origins
        )
        .is_ok());

        assert!(validate_deep_link_base_url_against_origins(
            "http://unconfigured-server:32400",
            &allowed_origins
        )
        .is_err());

        assert!(validate_deep_link_base_url_against_origins(
            "https://malicious.example.com",
            &allowed_origins
        )
        .is_err());

        assert!(validate_deep_link_base_url_against_origins(
            "http://localhost:8080",
            &allowed_origins
        )
        .is_err());

        assert!(validate_deep_link_base_url_against_origins(
            "https://localhost:32400",
            &allowed_origins
        )
        .is_err());

        assert!(validate_deep_link_base_url_against_origins(
            "not-a-url",
            &allowed_origins
        )
        .is_err());

        assert!(validate_deep_link_base_url_against_origins(
            "ftp://localhost:32400",
            &allowed_origins
        )
        .is_err());
    }

    #[test]
    fn test_construct_plex_url() {
        let url = construct_plex_url(
            "http://localhost:32400",
            Some("abc123"),
            Some("/library/metadata/456"),
            None,
        );
        assert!(url.contains("http://localhost:32400/web/index.html"));
        assert!(url.contains("#!/server/abc123/details"));
        assert!(url.contains("key=%2Flibrary%2Fmetadata%2F456"));

        let url = construct_plex_url(
            "http://localhost:32400",
            None,
            Some("/library/metadata/789"),
            None,
        );
        assert!(url.contains("#!/details"));
        assert!(url.contains("key=%2Flibrary%2Fmetadata%2F789"));

        let url = construct_plex_url(
            "http://localhost:32400",
            Some("abc123"),
            Some("/library/metadata/456"),
            Some("context=test"),
        );
        assert!(url.contains("key="));
        assert!(url.contains("context=test"));
    }

    #[test]
    fn test_app_config_serialize_roundtrip() {
        let config = AppConfig {
            servers: vec![ServerConfig {
                id: "server-1".to_string(),
                name: "My Server".to_string(),
                base_url: "http://localhost:32400".to_string(),
                is_remote: false,
                machine_identifier: Some("abc123".to_string()),
            }],
            default_server_id: Some("server-1".to_string()),
            auth_token: None,
            client_id: Some("test-client".to_string()),
            session_stats: None,
            device_settings: None,
            pinned_library_keys: vec!["2".to_string(), "4".to_string()],
            user_settings: UserSettings::default(),
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.servers.len(), 1);
        assert_eq!(deserialized.servers[0].name, "My Server");
        assert_eq!(deserialized.default_server_id, Some("server-1".to_string()));
        assert!(deserialized.auth_token.is_none());
        assert_eq!(deserialized.pinned_library_keys, vec!["2".to_string(), "4".to_string()]);
        assert_eq!(deserialized.user_settings.playback.quality, StreamQuality::Original);
    }

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert!(config.servers.is_empty());
        assert!(config.default_server_id.is_none());
        assert!(config.auth_token.is_none());
        assert!(config.client_id.is_none());
        assert!(config.pinned_library_keys.is_empty());
    }

    #[test]
    fn test_auth_token_never_serialized() {
        let mut config = AppConfig::default();
        config.auth_token = Some("secret-token".to_string());
        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(!json.contains("auth_token"));
        assert!(!json.contains("secret-token"));
    }

    #[test]
    fn test_pin_unpin_library_helpers() {
        let mut config = AppConfig::default();
        assert!(!config.is_library_pinned("3"));

        config.pin_library("3");
        assert!(config.is_library_pinned("3"));
        assert_eq!(config.pinned_library_keys, vec!["3".to_string()]);

        // Duplicate pin should be ignored.
        config.pin_library("3");
        assert_eq!(config.pinned_library_keys, vec!["3".to_string()]);

        config.pin_library("7");
        assert_eq!(
            config.pinned_library_keys,
            vec!["3".to_string(), "7".to_string()]
        );

        config.unpin_library("3");
        assert!(!config.is_library_pinned("3"));
        assert_eq!(config.pinned_library_keys, vec!["7".to_string()]);
    }

    #[test]
    fn test_check_rate_limit() {
        let key = format!("test-source-{:?}", std::thread::current().id());
        for i in 0..10 {
            assert!(check_rate_limit(&key).is_ok(), "Request {} should succeed", i);
        }
        let result = check_rate_limit(&key);
        assert!(result.is_err());
    }

    // -- UserSettings tests --

    #[test]
    fn test_user_settings_defaults() {
        let s = UserSettings::default();
        assert_eq!(s.playback.quality, StreamQuality::Original);
        assert!(!s.playback.auto_adjust_quality);
        assert!(s.playback.preferred_codec.is_none());
        assert!(s.playback.remember_volume);
        assert!((s.playback.last_volume - 1.0).abs() < f64::EPSILON);
        assert!((s.playback.playback_speed - 1.0).abs() < f64::EPSILON);

        assert_eq!(s.audio.preferred_languages, vec!["eng", "en"]);
        assert_eq!(s.audio.language_mismatch_action, MismatchAction::WarnDialog);

        assert!(s.subtitles.preferred_languages.is_empty());
        assert_eq!(s.subtitles.auto_enable, SubtitleAutoEnable::OnMismatch);
        assert!(!s.subtitles.prefer_forced);
    }

    #[test]
    fn test_user_settings_serialize_roundtrip() {
        let mut s = UserSettings::default();
        s.playback.quality = StreamQuality::Maximum(8_000);
        s.playback.auto_adjust_quality = true;
        s.playback.preferred_codec = Some("hevc".to_string());
        s.playback.playback_speed = 1.5;
        s.audio.preferred_languages = vec!["jpn".to_string(), "eng".to_string()];
        s.audio.language_mismatch_action = MismatchAction::Pause;
        s.subtitles.preferred_languages = vec!["eng".to_string()];
        s.subtitles.auto_enable = SubtitleAutoEnable::Always;
        s.subtitles.prefer_forced = true;

        let json = serde_json::to_string_pretty(&s).unwrap();
        let deserialized: UserSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.playback.quality, StreamQuality::Maximum(8_000));
        assert!(deserialized.playback.auto_adjust_quality);
        assert_eq!(deserialized.playback.preferred_codec.as_deref(), Some("hevc"));
        assert!((deserialized.playback.playback_speed - 1.5).abs() < f64::EPSILON);
        assert_eq!(deserialized.audio.preferred_languages, vec!["jpn", "eng"]);
        assert_eq!(deserialized.audio.language_mismatch_action, MismatchAction::Pause);
        assert_eq!(deserialized.subtitles.preferred_languages, vec!["eng"]);
        assert_eq!(deserialized.subtitles.auto_enable, SubtitleAutoEnable::Always);
        assert!(deserialized.subtitles.prefer_forced);
    }

    #[test]
    fn test_user_settings_backward_compat_empty_json() {
        let json = "{}";
        let s: UserSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.playback.quality, StreamQuality::Original);
        assert_eq!(s.audio.preferred_languages, vec!["eng", "en"]);
        assert_eq!(s.subtitles.auto_enable, SubtitleAutoEnable::OnMismatch);
    }

    #[test]
    fn test_app_config_backward_compat_no_user_settings_field() {
        let json = r#"{
            "servers": [],
            "default_server_id": null,
            "client_id": "test"
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.user_settings.playback.quality, StreamQuality::Original);
        assert_eq!(config.user_settings.audio.preferred_languages, vec!["eng", "en"]);
    }

    #[test]
    fn test_app_config_with_legacy_device_settings_still_deserializes() {
        let json = r#"{
            "servers": [],
            "default_server_id": null,
            "device_settings": {"quality": "original", "autoAdjustQuality": false}
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(config.device_settings.is_some());
        assert_eq!(config.user_settings.playback.quality, StreamQuality::Original);
    }

    #[test]
    fn test_device_settings_not_serialized() {
        let mut config = AppConfig::default();
        config.device_settings = Some(serde_json::json!({"quality": "original"}));
        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(!json.contains("device_settings"));
    }

    #[test]
    fn test_user_settings_is_serialized_in_config() {
        let mut config = AppConfig::default();
        config.user_settings.playback.quality = StreamQuality::Maximum(4_000);
        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(json.contains("user_settings"));
        assert!(json.contains("4000"));
    }

    // -- StreamQuality tests --

    #[test]
    fn test_stream_quality_label_original() {
        assert_eq!(StreamQuality::Original.label(), "Original");
    }

    #[test]
    fn test_stream_quality_label_known_presets() {
        assert_eq!(StreamQuality::Maximum(20_000).label(), "20 Mbps (4K)");
        assert_eq!(StreamQuality::Maximum(12_000).label(), "12 Mbps (1080p)");
        assert_eq!(StreamQuality::Maximum(8_000).label(), "8 Mbps (1080p)");
        assert_eq!(StreamQuality::Maximum(4_000).label(), "4 Mbps (720p)");
        assert_eq!(StreamQuality::Maximum(2_000).label(), "2 Mbps (480p)");
    }

    #[test]
    fn test_stream_quality_label_custom_kbps() {
        assert_eq!(StreamQuality::Maximum(6_000).label(), "6000 kbps");
    }

    #[test]
    fn test_stream_quality_serde_roundtrip() {
        let original = StreamQuality::Original;
        let json = serde_json::to_string(&original).unwrap();
        let back: StreamQuality = serde_json::from_str(&json).unwrap();
        assert_eq!(back, StreamQuality::Original);

        let max = StreamQuality::Maximum(8_000);
        let json = serde_json::to_string(&max).unwrap();
        let back: StreamQuality = serde_json::from_str(&json).unwrap();
        assert_eq!(back, StreamQuality::Maximum(8_000));
    }

    // -- MismatchAction / SubtitleAutoEnable enum tests --

    #[test]
    fn test_mismatch_action_default() {
        assert_eq!(MismatchAction::default(), MismatchAction::WarnDialog);
    }

    #[test]
    fn test_mismatch_action_serde_roundtrip() {
        for action in [MismatchAction::Pause, MismatchAction::WarnDialog, MismatchAction::Ignore] {
            let json = serde_json::to_string(&action).unwrap();
            let back: MismatchAction = serde_json::from_str(&json).unwrap();
            assert_eq!(back, action);
        }
    }

    #[test]
    fn test_subtitle_auto_enable_default() {
        assert_eq!(SubtitleAutoEnable::default(), SubtitleAutoEnable::OnMismatch);
    }

    #[test]
    fn test_subtitle_auto_enable_serde_roundtrip() {
        for variant in [SubtitleAutoEnable::Always, SubtitleAutoEnable::OnMismatch, SubtitleAutoEnable::Never] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: SubtitleAutoEnable = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    // -- PlaybackSettings default field tests --

    #[test]
    fn test_playback_settings_partial_json_uses_defaults() {
        let json = r#"{"quality": "original"}"#;
        let s: PlaybackSettings = serde_json::from_str(json).unwrap();
        assert!(!s.auto_adjust_quality);
        assert!(s.remember_volume);
        assert!((s.last_volume - 1.0).abs() < f64::EPSILON);
        assert!((s.playback_speed - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_audio_settings_partial_json_uses_defaults() {
        let json = "{}";
        let s: AudioSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.preferred_languages, vec!["eng", "en"]);
        assert_eq!(s.language_mismatch_action, MismatchAction::WarnDialog);
    }

    #[test]
    fn test_subtitle_settings_partial_json_uses_defaults() {
        let json = "{}";
        let s: SubtitleSettings = serde_json::from_str(json).unwrap();
        assert!(s.preferred_languages.is_empty());
        assert_eq!(s.auto_enable, SubtitleAutoEnable::OnMismatch);
        assert!(!s.prefer_forced);
    }
}
