use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use url::Url;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
use std::net::{TcpListener, TcpStream};

// HIGH-001: Keychain service name for token storage
const KEYCHAIN_SERVICE: &str = "plex-desktop";
const KEYCHAIN_USERNAME: &str = "auth-token";

// MED-004: Whitelist of allowed query parameter names for deep links
// These are parameters that Plex web UI commonly uses and are safe to pass through
const ALLOWED_QUERY_PARAMS: &[&str] = &[
    "context",
    "source",
    "includeMeta",
    "includeAdvanced",
    "includeCollections",
    "includeExternalMedia",
    "type",
    "X-Plex-Container-Start",
    "X-Plex-Container-Size",
];

// MED-005: Rate limiting configuration
const RATE_LIMIT_WINDOW_SECS: u64 = 60;
const RATE_LIMIT_MAX_REQUESTS: usize = 10;

// MED-005: Rate limiter state - tracks requests per source
// Using a simple in-memory store with a static Mutex
static RATE_LIMITER: OnceLock<Mutex<HashMap<String, Vec<Instant>>>> = OnceLock::new();

fn get_rate_limiter() -> &'static Mutex<HashMap<String, Vec<Instant>>> {
    RATE_LIMITER.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub is_remote: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_identifier: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    servers: Vec<ServerConfig>,
    default_server_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_stats: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_settings: Option<serde_json::Value>,
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
        }
    }
}

fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .expect("Failed to get config directory")
        .join("plex-desktop");

    // Create config directory if it doesn't exist
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    }

    let config_path = config_dir.join("config.json");
    
    // MED-003: Set config file permissions to 0o600 if file exists
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

fn get_socket_path() -> PathBuf {
    // Use XDG_RUNTIME_DIR if available, otherwise fall back to /tmp
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    
    runtime_dir.join("plex-desktop.sock")
}

#[cfg(windows)]
const IPC_ADDR: &str = "127.0.0.1:37921";

#[cfg(windows)]
static IPC_SECRET: OnceLock<String> = OnceLock::new();

#[cfg(unix)]
fn get_lock_file_path() -> PathBuf {
    // Use XDG_RUNTIME_DIR if available, otherwise fall back to /tmp
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    
    runtime_dir.join("plex-desktop.lock")
}

#[cfg(windows)]
fn get_lock_file_path() -> PathBuf {
    // Prefer a per-user directory on Windows.
    // data_local_dir is typically: C:\Users\<User>\AppData\Local
    let base_dir = dirs::data_local_dir()
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base_dir.join("plex-desktop");
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    dir.join("plex-desktop.lock")
}

#[cfg(windows)]
fn parse_lock_file(contents: &str) -> Option<(u32, String)> {
    let mut lines = contents.lines();
    let pid = lines.next()?.trim().parse::<u32>().ok()?;
    let secret = lines.next()?.trim().to_string();
    if secret.is_empty() {
        return None;
    }
    Some((pid, secret))
}

#[cfg(unix)]
fn is_another_instance_running() -> bool {
    let lock_file = get_lock_file_path();
    
    // Check if lock file exists
    if lock_file.exists() {
        // Try to read the PID from the lock file
        if let Ok(contents) = fs::read_to_string(&lock_file) {
            if let Ok(pid) = contents.trim().parse::<u32>() {
                // Check if the process is still running
                // On Linux, sending signal 0 to a process checks if it exists
                use std::process::Command;
                let output = Command::new("kill")
                    .args(&["-0", &pid.to_string()])
                    .output();
                
                if let Ok(output) = output {
                    if output.status.success() {
                        // Process is still running
                        return true;
                    }
                }
            }
        }
        // Lock file exists but process is dead, remove it
        let _ = fs::remove_file(&lock_file);
    }
    
    false
}

#[cfg(windows)]
fn is_another_instance_running() -> bool {
    let lock_file = get_lock_file_path();

    if !lock_file.exists() {
        return false;
    }

    // Try to read PID+secret from lock file and verify the process exists.
    if let Ok(contents) = fs::read_to_string(&lock_file) {
        if let Some((pid, secret)) = parse_lock_file(&contents) {
            // Cache the secret for the running instance.
            let _ = IPC_SECRET.set(secret);

            // Use tasklist to check if PID exists.
            // Example output contains the PID if the process is running.
            let output = std::process::Command::new("cmd")
                .args([
                    "/C",
                    &format!("tasklist /FI \"PID eq {}\" /NH", pid),
                ])
                .output();

            if let Ok(output) = output {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if stdout.contains(&pid.to_string()) {
                        return true;
                    }
                }
            }
        }
    }

    // Lock file exists but process appears dead; remove it.
    let _ = fs::remove_file(&lock_file);
    false
}

#[cfg(unix)]
fn create_lock_file() -> Result<(), String> {
    let lock_file = get_lock_file_path();
    let pid = std::process::id();
    if let Some(parent) = lock_file.parent() {
        let _ = fs::create_dir_all(parent);
    }

    use std::io::Write as _;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_file)
    {
        Ok(mut f) => {
            write!(f, "{}", pid).map_err(|e| format!("Failed to write lock file: {}", e))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err("Lock file already exists".to_string());
        }
        Err(e) => return Err(format!("Failed to create lock file: {}", e)),
    }
    Ok(())
}

#[cfg(windows)]
fn create_lock_file() -> Result<(), String> {
    use rand::{rngs::OsRng, RngCore};
    let lock_file = get_lock_file_path();
    let pid = std::process::id();

    if let Some(parent) = lock_file.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut secret_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut secret_bytes);
    let secret = secret_bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    let _ = IPC_SECRET.set(secret.clone());

    use std::io::Write as _;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_file)
    {
        Ok(mut f) => {
            writeln!(f, "{}", pid).map_err(|e| format!("Failed to write lock file: {}", e))?;
            writeln!(f, "{}", secret).map_err(|e| format!("Failed to write lock file: {}", e))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err("Lock file already exists".to_string());
        }
        Err(e) => return Err(format!("Failed to create lock file: {}", e)),
    }
    Ok(())
}

#[cfg(unix)]
fn remove_lock_file() {
    let lock_file = get_lock_file_path();
    let _ = fs::remove_file(&lock_file);
    let socket_path = get_socket_path();
    let _ = fs::remove_file(&socket_path);
}

#[cfg(windows)]
fn remove_lock_file() {
    let lock_file = get_lock_file_path();
    let _ = fs::remove_file(&lock_file);
}

#[cfg(unix)]
fn send_url_to_existing_instance(url: &str) -> Result<(), String> {
    let socket_path = get_socket_path();
    
    match UnixStream::connect(&socket_path) {
        Ok(mut stream) => {
            // Send the URL
            stream.write_all(url.as_bytes())
                .map_err(|e| format!("Failed to write to socket: {}", e))?;
            stream.flush()
                .map_err(|e| format!("Failed to flush socket: {}", e))?;
            eprintln!("Sent URL to existing instance: {}", url);
            Ok(())
        }
        Err(e) => {
            Err(format!("Failed to connect to existing instance: {}. Starting new instance.", e))
        }
    }
}

#[cfg(windows)]
fn send_url_to_existing_instance(url: &str) -> Result<(), String> {
    let lock_file = get_lock_file_path();
    let secret = fs::read_to_string(&lock_file)
        .ok()
        .and_then(|c| parse_lock_file(&c))
        .map(|(_, s)| s)
        .ok_or_else(|| "Failed to read IPC auth secret from lock file".to_string())?;

    match TcpStream::connect(IPC_ADDR) {
        Ok(mut stream) => {
            // Format: "<secret>\n<url>"
            let payload = format!("{}\n{}", secret, url);
            stream
                .write_all(payload.as_bytes())
                .map_err(|e| format!("Failed to write to IPC socket: {}", e))?;
            stream
                .flush()
                .map_err(|e| format!("Failed to flush IPC socket: {}", e))?;
            eprintln!("Sent URL to existing instance: {}", url);
            Ok(())
        }
        Err(e) => Err(format!(
            "Failed to connect to existing instance: {}. Starting new instance.",
            e
        )),
    }
}

#[cfg(unix)]
fn start_ipc_listener(app_handle: AppHandle) {
    let socket_path = get_socket_path();
    
    // Remove old socket if it exists
    if socket_path.exists() {
        let _ = fs::remove_file(&socket_path);
    }
    
    let socket_path_clone = socket_path.clone();
    std::thread::spawn(move || {
        match UnixListener::bind(&socket_path_clone) {
            Ok(listener) => {
                // MED-001: Set socket permissions to 0o600 (read/write for owner only)
                #[cfg(unix)]
                {
                    if let Ok(mut perms) = fs::metadata(&socket_path_clone).map(|m| m.permissions()) {
                        perms.set_mode(0o600);
                        let _ = fs::set_permissions(&socket_path_clone, perms);
                    }
                }
                eprintln!("IPC listener started at {:?}", socket_path_clone);
                
                for stream in listener.incoming() {
                    match stream {
                        Ok(mut stream) => {
                            let mut buffer = String::new();
                            if let Ok(_) = stream.read_to_string(&mut buffer) {
                                let url = buffer.trim().to_string();
                                if !url.is_empty() {
                                    eprintln!("Received URL from another instance: {}", url);
                                    
                                    // Bring window to front
                                    if let Some(window) = app_handle.get_webview_window("main") {
                                        let _ = window.set_focus();
                                        let _ = window.show();
                                    }
                                    
                                    let app_handle_clone = app_handle.clone();
                                    tauri::async_runtime::spawn(async move {
                                        // Check if it's an OAuth callback
                                        if url.contains("/auth")
                                            || url.contains("/oauth")
                                            || url.contains("?token=")
                                            || url.contains("&token=")
                                        {
                                            if let Err(e) = handle_oauth_callback(app_handle_clone.clone(), url).await {
                                                eprintln!("Error handling OAuth callback: {}", e);
                                            }
                                        } else {
                                            // Regular deep link
                                            if let Err(e) = navigate_to_deep_link(app_handle_clone, url).await {
                                                eprintln!("Error handling deep link: {}", e);
                                            }
                                        }
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Error accepting IPC connection: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to bind IPC socket: {}", e);
            }
        }
    });
}

#[cfg(windows)]
fn start_ipc_listener(app_handle: AppHandle) {
    std::thread::spawn(move || match TcpListener::bind(IPC_ADDR) {
        Ok(listener) => {
            eprintln!("IPC listener started at {}", IPC_ADDR);
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut buffer = String::new();
                        if stream.read_to_string(&mut buffer).is_ok() {
                            let expected_secret = match IPC_SECRET.get() {
                                Some(s) => s.as_str(),
                                None => {
                                    eprintln!("IPC secret not initialized; ignoring message");
                                    continue;
                                }
                            };

                            let (secret, url) = match buffer.split_once('\n') {
                                Some((s, u)) => (s.trim(), u.trim()),
                                None => {
                                    eprintln!("Malformed IPC message; ignoring");
                                    continue;
                                }
                            };

                            if secret != expected_secret {
                                eprintln!("IPC auth failed; ignoring message");
                                continue;
                            }

                            let url = url.to_string();
                            if !url.is_empty() {
                                eprintln!("Received URL from another instance: {}", url);

                                // Bring window to front
                                if let Some(window) = app_handle.get_webview_window("main") {
                                    let _ = window.set_focus();
                                    let _ = window.show();
                                }

                                let app_handle_clone = app_handle.clone();
                                tauri::async_runtime::spawn(async move {
                                    // Check if it's an OAuth callback
                                    if url.contains("/auth")
                                        || url.contains("/oauth")
                                        || url.contains("?token=")
                                        || url.contains("&token=")
                                    {
                                        if let Err(e) =
                                            handle_oauth_callback(app_handle_clone.clone(), url)
                                                .await
                                        {
                                            eprintln!("Error handling OAuth callback: {}", e);
                                        }
                                    } else {
                                        // Regular deep link
                                        if let Err(e) =
                                            navigate_to_deep_link(app_handle_clone, url).await
                                        {
                                            eprintln!("Error handling deep link: {}", e);
                                        }
                                    }
                                });
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error accepting IPC connection: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to bind IPC socket: {}", e);
        }
    });
}

fn load_config() -> AppConfig {
    let config_path = get_config_path();

    if !config_path.exists() {
        // Create an empty config file if it doesn't exist
        let default_config = AppConfig::default();
        if let Err(e) = save_config(&default_config) {
            eprintln!("Warning: Failed to create initial config file: {}", e);
        }
        return default_config;
    }

    match fs::read_to_string(&config_path) {
        Ok(content) => {
            if content.trim().is_empty() {
                AppConfig::default()
            } else {
                serde_json::from_str(&content).unwrap_or_else(|e| {
                    eprintln!("Failed to parse config file: {}, using defaults", e);
                    AppConfig::default()
                })
            }
        }
        Err(e) => {
            eprintln!("Failed to read config file: {}, using defaults", e);
            AppConfig::default()
        }
    }
}

fn save_config(config: &AppConfig) -> Result<(), String> {
    let config_path = get_config_path();
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    fs::write(&config_path, content).map_err(|e| format!("Failed to write config: {}", e))?;

    // MED-003: Set config file permissions to 0o600 (read/write for owner only)
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&config_path)
            .map_err(|e| format!("Failed to get config file metadata: {}", e))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&config_path, perms)
            .map_err(|e| format!("Failed to set config file permissions: {}", e))?;
    }

    Ok(())
}

fn validate_server_url(url: &str) -> Result<(), String> {
    Url::parse(url).map_err(|_| "Invalid URL format".to_string())?;

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("URL must start with http:// or https://".to_string());
    }

    Ok(())
}

/// HIGH-003: Normalizes a URL to its origin (scheme + host + port).
/// Returns the normalized origin string for comparison purposes.
fn normalize_url_to_origin(url: &str) -> Result<String, String> {
    let parsed = Url::parse(url).map_err(|_| "Invalid URL format".to_string())?;

    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("URL must use http:// or https:// scheme".to_string());
    }

    let host = parsed.host_str().ok_or_else(|| "Missing host in URL".to_string())?;
    let port = parsed.port();
    
    Ok(if let Some(p) = port {
        format!("{}://{}:{}", parsed.scheme(), host, p)
    } else {
        format!("{}://{}", parsed.scheme(), host)
    })
}

/// HIGH-003: Validates that a deep link baseUrl matches one of the allowed server origins.
/// This is a pure function that takes a list of allowed origins for testability.
fn validate_deep_link_base_url_against_origins(
    base_url: &str,
    allowed_origins: &[String],
) -> Result<(), String> {
    let normalized_origin = normalize_url_to_origin(base_url)?;

    if allowed_origins.iter().any(|origin| origin == &normalized_origin) {
        Ok(())
    } else {
        Err(format!(
            "Deep link baseUrl '{}' does not match any configured server",
            base_url
        ))
    }
}

/// HIGH-003: Validates that a deep link baseUrl matches a configured server.
/// Normalizes the URL to origin (scheme + host + port) and checks against configured servers.
fn validate_deep_link_base_url(base_url: &str) -> Result<(), String> {
    let config = load_config();
    let allowed_origins: Vec<String> = config
        .servers
        .iter()
        .filter_map(|server| normalize_url_to_origin(&server.base_url).ok())
        .collect();

    validate_deep_link_base_url_against_origins(base_url, &allowed_origins)
}

#[tauri::command]
fn get_servers() -> Result<Vec<ServerConfig>, String> {
    let config = load_config();
    Ok(config.servers)
}

#[tauri::command]
fn add_server(name: String, base_url: String, is_remote: bool) -> Result<(), String> {
    validate_server_url(&base_url)?;

    let mut config = load_config();

    // Check if server with this URL already exists
    if config.servers.iter().any(|s| s.base_url == base_url) {
        return Err("Server with this URL already exists".to_string());
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

    // Set as default if it's the first server
    if config.default_server_id.is_none() {
        config.default_server_id = config.servers.first().map(|s| s.id.clone());
    }

    save_config(&config)
}

#[tauri::command]
fn update_server(id: String, name: Option<String>, base_url: Option<String>) -> Result<(), String> {
    let mut config = load_config();

    let server = config
        .servers
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| "Server not found".to_string())?;

    if let Some(new_name) = name {
        server.name = new_name;
    }

    if let Some(new_url) = base_url {
        validate_server_url(&new_url)?;
        server.base_url = new_url;
    }

    save_config(&config)
}

#[tauri::command]
fn remove_server(id: String) -> Result<(), String> {
    let mut config = load_config();

    let index = config
        .servers
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| "Server not found".to_string())?;

    config.servers.remove(index);

    // If removed server was default, set new default or clear it
    if config.default_server_id.as_ref() == Some(&id) {
        config.default_server_id = config.servers.first().map(|s| s.id.clone());
    }

    save_config(&config)
}

#[tauri::command]
fn set_default_server(id: String) -> Result<(), String> {
    let mut config = load_config();

    if !config.servers.iter().any(|s| s.id == id) {
        return Err("Server not found".to_string());
    }

    config.default_server_id = Some(id);
    save_config(&config)
}

#[tauri::command]
fn get_default_server() -> Result<Option<ServerConfig>, String> {
    let config = load_config();

    if let Some(default_id) = config.default_server_id {
        config
            .servers
            .into_iter()
            .find(|s| s.id == default_id)
            .map(Some)
            .ok_or_else(|| "Default server not found".to_string())
    } else {
        Ok(None)
    }
}

// HIGH-001: Helper function to get token from keychain
fn get_token_from_keychain() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USERNAME)
        .map_err(|e| format!("Failed to create keyring entry: {}", e))?;
    
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to get token from keychain: {}", e)),
    }
}

// HIGH-001: Helper function to set token in keychain
fn set_token_in_keychain(token: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USERNAME)
        .map_err(|e| format!("Failed to create keyring entry: {}", e))?;
    
    entry.set_password(token)
        .map_err(|e| format!("Failed to set token in keychain: {}", e))?;
    
    Ok(())
}

// HIGH-001: Migrate token from config file to keychain (one-time migration)
fn migrate_token_from_config() -> Result<(), String> {
    let config = load_config();
    
    // If token exists in config and not in keychain, migrate it
    if let Some(token) = config.auth_token {
        // Check if token already exists in keychain
        let keychain_has_token = match get_token_from_keychain() {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                eprintln!("Warning: Failed to check keychain during migration: {}", e);
                // If we can't check keychain, try to migrate anyway (will overwrite if exists)
                false
            }
        };
        
        if !keychain_has_token {
            // Token in config but not in keychain, migrate it
            eprintln!("Migrating token from config file to keychain...");
            if let Err(e) = set_token_in_keychain(&token) {
                eprintln!("Warning: Failed to migrate token to keychain: {}", e);
                return Err(e);
            }
            
            // Remove token from config file
            let mut config = load_config();
            config.auth_token = None;
            if let Err(e) = save_config(&config) {
                eprintln!("Warning: Failed to remove token from config file: {}", e);
                // Don't fail migration if we can't remove from config
            } else {
                eprintln!("Token migrated successfully");
            }
        } else {
            // Token already in keychain, remove from config file
            let mut config = load_config();
            if config.auth_token.is_some() {
                config.auth_token = None;
                if let Err(e) = save_config(&config) {
                    eprintln!("Warning: Failed to remove token from config file: {}", e);
                }
            }
        }
    }
    
    Ok(())
}

#[tauri::command]
fn get_auth_token() -> Result<Option<String>, String> {
    // HIGH-001: Try keychain first
    match get_token_from_keychain() {
        Ok(Some(token)) => Ok(Some(token)),
        Ok(None) => {
            // Token not in keychain, try migration from config (non-blocking)
            let _ = migrate_token_from_config();
            // Try keychain again after migration attempt
            get_token_from_keychain()
        }
        Err(e) => {
            // If keychain fails, try migration as fallback
            let _ = migrate_token_from_config();
            get_token_from_keychain()
        }
    }
}

#[tauri::command]
fn set_auth_token(token: String) -> Result<(), String> {
    eprintln!("Setting auth token...");
    
    // HIGH-001: Check if we already have a token
    let had_token = get_token_from_keychain()
        .map(|opt| opt.is_some())
        .unwrap_or(false);
    
    // Store token in keychain
    set_token_in_keychain(&token)?;
    eprintln!("Auth token stored successfully in keychain");

    // If this is a new token and we don't have servers, trigger discovery
    if !had_token {
        eprintln!("New token detected, will trigger server discovery");
        // The frontend will handle the discovery via the periodic check
    }

    Ok(())
}

#[tauri::command]
fn get_client_id() -> Result<Option<String>, String> {
    let config = load_config();
    Ok(config.client_id)
}

#[tauri::command]
fn set_client_id(client_id: String) -> Result<(), String> {
    eprintln!("Setting client ID...");
    let mut config = load_config();
    config.client_id = Some(client_id);
    save_config(&config)?;
    eprintln!("Client ID stored successfully");
    Ok(())
}

#[tauri::command]
fn set_session_stats(stats: serde_json::Value) -> Result<(), String> {
    eprintln!("Setting session stats...");
    let mut config = load_config();
    config.session_stats = Some(stats);
    save_config(&config)?;
    eprintln!("Session stats stored successfully");
    Ok(())
}

#[tauri::command]
fn set_device_settings(settings: serde_json::Value) -> Result<(), String> {
    eprintln!("Setting device settings...");
    let mut config = load_config();
    config.device_settings = Some(settings);
    save_config(&config)?;
    eprintln!("Device settings stored successfully");
    Ok(())
}

fn is_oauth_url(url: &str) -> bool {
    url.contains("plex.tv/auth")
        || url.contains("plex.tv/login")
        || url.contains("plex.tv/sign-in")
        || url.contains("/oauth2/")
        || url.contains("authorize")
}

fn extract_token_from_url(url: &str) -> Option<String> {
    eprintln!("Extracting token from URL (redacted)");

    // First, try simple string parsing for plex-desktop:// URLs
    if url.starts_with("plex-desktop://") {
        if let Some(token_start) = url.find("?token=") {
            let token_part = &url[token_start + 7..];
            if let Some(token_end) = token_part.find('&') {
                return Some(token_part[..token_end].to_string());
            } else {
                return Some(token_part.to_string());
            }
        }
        if let Some(token_start) = url.find("&token=") {
            let token_part = &url[token_start + 7..];
            if let Some(token_end) = token_part.find('&') {
                return Some(token_part[..token_end].to_string());
            } else {
                return Some(token_part.to_string());
            }
        }
    }

    // Fallback to URL parsing for other formats
    let parsed = match Url::parse(url) {
        Ok(p) => p,
        Err(_) => return None,
    };

    // Check query parameters for token
    for (key, value) in parsed.query_pairs() {
        if key == "token" || key == "access_token" || key == "authToken" {
            eprintln!("Found token in query (redacted)");
            return Some(value.to_string());
        }
    }

    // Check hash fragment for token
    if let Some(fragment) = parsed.fragment() {
        let fragment_params: HashMap<String, String> = fragment
            .split('&')
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    Some((key.to_string(), value.to_string()))
                } else {
                    None
                }
            })
            .collect();

        if let Some(token) = fragment_params
            .get("token")
            .or_else(|| fragment_params.get("access_token"))
            .or_else(|| fragment_params.get("authToken"))
        {
            eprintln!("Found token in fragment (redacted)");
            return Some(token.clone());
        }
    }

    eprintln!("No token found in URL");
    None
}

#[tauri::command]
async fn handle_oauth_callback(app: AppHandle, url: String) -> Result<(), String> {
    eprintln!("Handling OAuth callback (URL redacted)");

    // Extract token from callback URL
    if let Some(token) = extract_token_from_url(&url) {
        eprintln!("Token extracted successfully, storing...");

        // Store the token
        set_auth_token(token.clone())?;
        eprintln!("Token stored successfully");

        // Notify frontend that auth is complete
        app.emit("oauth-complete", token.clone())
            .map_err(|e| format!("Failed to emit event: {}", e))?;
        eprintln!("OAuth complete event emitted");

        // Navigate back to app.plex.tv
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.navigate(
                tauri::Url::parse("https://app.plex.tv")
                    .map_err(|e| format!("Invalid URL: {}", e))?,
            );
            eprintln!("Navigated to app.plex.tv");
        } else {
            eprintln!("Warning: Main window not found, cannot navigate");
        }

        Ok(())
    } else {
        eprintln!("ERROR: No token found in OAuth callback (URL redacted)");
        Err("No token found in OAuth callback".to_string())
    }
}

#[tauri::command]
async fn open_in_browser(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| format!("Failed to open browser: {}", e))?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct DiscoveredServer {
    name: String,
    base_url: String,
    machine_identifier: Option<String>,
}

#[tauri::command]
async fn discover_servers() -> Result<Vec<DiscoveredServer>, String> {
    eprintln!("Starting server discovery...");
    let mut discovered = Vec::new();

    let common_ports = vec![32400];
    
    // Get local network interfaces and common IP ranges
    let mut ip_addresses = Vec::new();
    
    // Always check localhost
    ip_addresses.push("127.0.0.1".to_string());
    
    // Try to get local network IPs
    // Common private network ranges:
    // - 192.168.0.0/16 (192.168.0.1 - 192.168.255.254)
    // - 10.0.0.0/8 (10.0.0.1 - 10.255.255.254)
    // - 172.16.0.0/12 (172.16.0.1 - 172.31.255.254)
    
    // For now, we'll check common gateway IPs and a few ranges
    // In a production app, you'd want to:
    // 1. Get actual network interfaces using a crate like `if-addrs`
    // 2. Use SSDP for proper discovery
    // 3. Or use the Plex resources API (which we do if we have a token)
    
    // Check common router/gateway IPs
    let common_gateways = vec![
        "192.168.1.1", "192.168.0.1", "192.168.2.1",
        "10.0.0.1", "172.16.0.1",
    ];
    
    for gateway in &common_gateways {
        ip_addresses.push(gateway.to_string());
    }
    
    // Check a small range around common gateway IPs (limited to avoid too many requests)
    // We'll check the gateway IP and a few nearby IPs
    for gateway in &common_gateways {
        if let Some(parts) = gateway.split('.').collect::<Vec<&str>>().get(0..3) {
            let base = parts.join(".");
            // Check gateway and a few nearby IPs (e.g., 192.168.1.1-10)
            for i in 1..=10 {
                ip_addresses.push(format!("{}.{}", base, i));
            }
        }
    }
    
    // Remove duplicates
    ip_addresses.sort();
    ip_addresses.dedup();
    
    eprintln!("Checking {} IP addresses on port 32400...", ip_addresses.len());
    
    // Check each IP address with timeout to avoid hanging
    // We'll check them sequentially to avoid overwhelming the network
    let start = std::time::Instant::now();
    let timeout_duration = Duration::from_secs(5);
    let mut checked = 0;
    
    for ip in ip_addresses {
        if start.elapsed() > timeout_duration {
            eprintln!("Discovery timeout reached, stopping checks");
            break;
        }
        
        for port in &common_ports {
            if start.elapsed() > timeout_duration {
                break;
            }
            
            let base_url = format!("http://{}:{}", ip, port);
            checked += 1;
            
            // Use a short timeout for each individual check
            match tokio::time::timeout(Duration::from_millis(500), fetch_plex_server_info(&base_url)).await {
                Ok(Ok(server_info)) => {
                    eprintln!("Found server: {} at {}", server_info.name, server_info.base_url);
                    discovered.push(server_info);
                }
                Ok(Err(_)) => {
                    // Server not reachable or not a Plex server - this is normal
                }
                Err(_) => {
                    // Timeout on individual check - continue
                }
            }
        }
    }
    
    eprintln!("Discovery complete, found {} servers (checked {} addresses)", discovered.len(), checked);
    Ok(discovered)
}

async fn fetch_plex_server_info(base_url: &str) -> Result<DiscoveredServer, String> {
    let info_url = format!("{}/", base_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    match client.get(&info_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                let text = response
                    .text()
                    .await
                    .map_err(|e| format!("Failed to read response: {}", e))?;

                // Parse XML to get machineIdentifier and friendlyName
                let machine_id = extract_machine_identifier(&text);
                let name =
                    extract_server_name(&text).unwrap_or_else(|| "Plex Media Server".to_string());

                Ok(DiscoveredServer {
                    name,
                    base_url: base_url.to_string(),
                    machine_identifier: machine_id,
                })
            } else {
                Err(format!("Server returned status: {}", response.status()))
            }
        }
        Err(_) => Err("Server not reachable".to_string()),
    }
}

fn extract_machine_identifier(xml: &str) -> Option<String> {
    // Simple XML parsing - look for machineIdentifier attribute
    if let Some(start) = xml.find("machineIdentifier=\"") {
        let start_idx = start + "machineIdentifier=\"".len();
        if let Some(end) = xml[start_idx..].find('"') {
            return Some(xml[start_idx..start_idx + end].to_string());
        }
    }
    None
}

fn extract_server_name(xml: &str) -> Option<String> {
    // Simple XML parsing - look for friendlyName attribute
    if let Some(start) = xml.find("friendlyName=\"") {
        let start_idx = start + "friendlyName=\"".len();
        if let Some(end) = xml[start_idx..].find('"') {
            return Some(xml[start_idx..start_idx + end].to_string());
        }
    }
    None
}

#[tauri::command]
async fn auto_add_discovered_server(
    name: String,
    base_url: String,
    machine_identifier: Option<String>,
) -> Result<(), String> {
    let is_remote = base_url.starts_with("https://");
    add_server(name.clone(), base_url.clone(), is_remote)?;

    // Update machine identifier if provided
    if let Some(machine_id) = machine_identifier {
        let mut config = load_config();
        if let Some(server) = config.servers.iter_mut().find(|s| s.base_url == base_url) {
            server.machine_identifier = Some(machine_id);
            save_config(&config)?;
        }
    }

    Ok(())
}

// Plex Resources API structures
#[derive(Debug, Serialize, Deserialize)]
struct PlexResourcesResponse {
    #[serde(rename = "MediaContainer")]
    media_container: PlexResourcesContainer,
}

#[derive(Debug, Serialize, Deserialize)]
struct PlexResourcesContainer {
    device: Vec<PlexDevice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PlexDevice {
    name: String,
    #[serde(rename = "provides")]
    provides: Option<String>,
    #[serde(rename = "server")]
    server: Option<String>,
    connections: Option<Vec<PlexConnection>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PlexConnection {
    protocol: String,
    address: String,
    port: u16,
    uri: Option<String>,
    local: Option<u8>,
    relay: Option<u8>,
}

#[tauri::command]
async fn fetch_plex_resources(token: String) -> Result<Vec<DiscoveredServer>, String> {
    eprintln!("Fetching Plex resources with token...");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url =
        "https://clients.plex.tv/api/v2/resources?includeHttps=1&includeRelay=1&includeIPv6=1";

    let response = client
        .get(url)
        .header("X-Plex-Token", &token)
        .header("X-Plex-Product", "Plex Desktop")
        .header("X-Plex-Version", "1.0.0")
        .header("X-Plex-Client-Identifier", "plex-desktop-app")
        .header("X-Plex-Platform", "Linux")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch resources: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Resources API returned status: {}",
            response.status()
        ));
    }

    let text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    eprintln!("Resources API response: {}", text);

    // Parse JSON response
    let resources: PlexResourcesResponse = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse resources JSON: {}", e))?;

    let mut discovered = Vec::new();

    for device in resources.media_container.device {
        // Only process servers (devices that provide "server")
        if let Some(provides) = &device.provides {
            if provides.contains("server") {
                // Find the best connection (prefer local, non-relay)
                let mut best_connection: Option<&PlexConnection> = None;

                if let Some(connections) = &device.connections {
                    // Prefer local, non-relay connections
                    for conn in connections {
                        if conn.local == Some(1) && conn.relay != Some(1) {
                            best_connection = Some(conn);
                            break;
                        }
                    }

                    // Fallback to first connection
                    if best_connection.is_none() {
                        best_connection = connections.first();
                    }
                }

                if let Some(conn) = best_connection {
                    let base_url = format!("{}://{}:{}", conn.protocol, conn.address, conn.port);

                    // Try to get machine identifier from the server
                    let machine_id = fetch_server_machine_id(&base_url).await.ok();

                    discovered.push(DiscoveredServer {
                        name: device.name,
                        base_url,
                        machine_identifier: machine_id,
                    });
                }
            }
        }
    }

    eprintln!("Discovered {} servers from resources API", discovered.len());
    Ok(discovered)
}

async fn fetch_server_machine_id(base_url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let info_url = format!("{}/", base_url);
    match client.get(&info_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                let text = response
                    .text()
                    .await
                    .map_err(|e| format!("Failed to read response: {}", e))?;
                extract_machine_identifier(&text)
                    .ok_or_else(|| "Machine identifier not found".to_string())
            } else {
                Err(format!("Server returned status: {}", response.status()))
            }
        }
        Err(e) => Err(format!("Failed to connect: {}", e)),
    }
}

#[tauri::command]
async fn extract_token_from_webview(app: AppHandle) -> Result<Option<String>, String> {
    eprintln!("Attempting to extract token from webview...");

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;

    // Inject JavaScript to intercept fetch/XHR requests and extract X-Plex-Token from URL query params
    let js_code = r#"
        (function() {
            let interceptedToken = null;
            
            // Helper function to extract token from URL
            function extractTokenFromUrl(url) {
                if (!url || typeof url !== 'string') return null;
                
                try {
                    const urlObj = new URL(url);
                    const token = urlObj.searchParams.get('X-Plex-Token') || 
                                 urlObj.searchParams.get('x-plex-token');
                    return token;
                } catch (e) {
                    // If URL parsing fails, try manual parsing
                    const match = url.match(/[?&]X-Plex-Token=([^&]+)/i);
                    if (match) {
                        return decodeURIComponent(match[1]);
                    }
                }
                return null;
            }
            
            // Intercept fetch requests
            const originalFetch = window.fetch;
            window.fetch = function(...args) {
                const [url, options = {}] = args;
                
                // Check if request is to plex.tv domain
                if (typeof url === 'string' && url.includes('plex.tv')) {
                    const token = extractTokenFromUrl(url);
                if (token) {
                    interceptedToken = token;
                    window.__plex_desktop_token = token;
                    window.__plex_token_updated = true;
                    console.log('Intercepted X-Plex-Token from fetch URL (redacted)');
                }
                }
                
                return originalFetch.apply(this, args);
            };
            
            // Intercept XMLHttpRequest
            const originalOpen = XMLHttpRequest.prototype.open;
            
            XMLHttpRequest.prototype.open = function(method, url, ...rest) {
                this._url = url;
                
                // Extract token from URL if it's a plex.tv request
                if (url && typeof url === 'string' && url.includes('plex.tv')) {
                    const token = extractTokenFromUrl(url);
                    if (token) {
                        interceptedToken = token;
                        window.__plex_desktop_token = token;
                        window.__plex_token_updated = true;
                        console.log('Intercepted X-Plex-Token from XHR URL (redacted)');
                    }
                }
                
                return originalOpen.apply(this, [method, url, ...rest]);
            };
            
            // Also monitor for URL changes that might contain tokens
            let lastUrl = window.location.href;
            const urlCheckInterval = setInterval(() => {
                const currentUrl = window.location.href;
                if (currentUrl !== lastUrl && currentUrl.includes('plex.tv')) {
                    const token = extractTokenFromUrl(currentUrl);
                    if (token) {
                        interceptedToken = token;
                        window.__plex_desktop_token = token;
                        window.__plex_token_updated = true;
                        console.log('Intercepted X-Plex-Token from URL change (redacted)');
                    }
                    lastUrl = currentUrl;
                }
            }, 1000);
            
            // Also check for token in current URL on load
            const currentUrl = window.location.href;
            if (currentUrl.includes('plex.tv')) {
                const token = extractTokenFromUrl(currentUrl);
                if (token) {
                    interceptedToken = token;
                    window.__plex_desktop_token = token;
                    window.__plex_token_updated = true;
                    console.log('Intercepted X-Plex-Token from initial URL (redacted)');
                }
            }
            
            // Initialize the update flag
            window.__plex_token_updated = false;
            
            // Function to check and notify about token
            function checkAndNotifyToken() {
                try {
                    // Check localStorage - Plex uses myPlexAccessToken as the primary key
                    let storedToken = localStorage.getItem('myPlexAccessToken') ||
                                     localStorage.getItem('token') || 
                                     localStorage.getItem('authToken') ||
                                     localStorage.getItem('plex-token') ||
                                     localStorage.getItem('X-Plex-Token');
                    
                    if (storedToken && storedToken !== window.__plex_desktop_token) {
                        window.__plex_desktop_token = storedToken;
                        window.__plex_token_updated = true;
                        console.log('Found token in storage (redacted)');
                        
                        // Try to notify via postMessage
                        try {
                            if (window.parent && window.parent !== window) {
                                window.parent.postMessage({
                                    type: 'plex-token-found',
                                    token: storedToken
                                }, '*');
                                console.log('Sent token to parent via postMessage');
                            }
                            if (window.top && window.top !== window && window.top !== window.parent) {
                                window.top.postMessage({
                                    type: 'plex-token-found',
                                    token: storedToken
                                }, '*');
                            }
                        } catch (e) {
                            console.error('Failed to send token via postMessage:', e);
                        }
                        
                        // Also try Tauri internals if available
                        if (window.__TAURI_INTERNALS__) {
                            try {
                                window.__TAURI_INTERNALS__.invoke('set_auth_token', { token: storedToken });
                                console.log('Notified Tauri of token via internals');
                            } catch (e) {
                                console.error('Failed to notify Tauri of token:', e);
                            }
                        }
                    }
                    
                    // Check for clientID
                    const clientID = localStorage.getItem('clientID');
                    if (clientID && clientID !== window.__plex_client_id) {
                        window.__plex_client_id = clientID;
                        console.log('Found clientID in storage:', clientID);
                        
                        try {
                            if (window.parent && window.parent !== window) {
                                window.parent.postMessage({
                                    type: 'plex-client-id-found',
                                    clientId: clientID
                                }, '*');
                            }
                        } catch (e) {
                            console.error('Failed to send clientID via postMessage:', e);
                        }
                    }
                } catch (e) {
                    console.error('Error checking storage:', e);
                }
            }
            
            // Check immediately
            checkAndNotifyToken();
            
            // Also periodically check for tokens (in case user logs in after page load)
            setInterval(checkAndNotifyToken, 2000);
            
            // Also try to get from existing storage (localStorage, cookies, etc.)
            try {
                // Check localStorage - Plex uses myPlexAccessToken as the primary key
                let storedToken = localStorage.getItem('myPlexAccessToken') ||
                                 localStorage.getItem('token') || 
                                 localStorage.getItem('authToken') ||
                                 localStorage.getItem('plex-token') ||
                                 localStorage.getItem('X-Plex-Token');
                
                // Also store clientID if available (Plex uses this for API requests)
                const clientID = localStorage.getItem('clientID');
                if (clientID) {
                    window.__plex_client_id = clientID;
                    console.log('Found clientID in storage:', clientID);
                    
                    // Try to notify via postMessage
                    try {
                        if (window.parent && window.parent !== window) {
                            window.parent.postMessage({
                                type: 'plex-client-id-found',
                                clientId: clientID
                            }, '*');
                        }
                        if (window.top && window.top !== window && window.top !== window.parent) {
                            window.top.postMessage({
                                type: 'plex-client-id-found',
                                clientId: clientID
                            }, '*');
                        }
                    } catch (e) {
                        console.error('Failed to send clientID via postMessage:', e);
                    }
                    
                    // Also try Tauri internals if available
                    if (window.__TAURI_INTERNALS__) {
                        try {
                            window.__TAURI_INTERNALS__.invoke('set_client_id', { clientId: clientID });
                        } catch (e) {
                            console.error('Failed to notify Tauri of clientID:', e);
                        }
                    }
                }
                
                // Check cookies
                if (!storedToken) {
                    const cookies = document.cookie.split(';');
                    for (let cookie of cookies) {
                        const parts = cookie.trim().split('=');
                        if (parts.length >= 2) {
                            const name = parts[0].trim();
                            const value = parts.slice(1).join('=');
                            if (name === 'token' || name === 'authToken' || name === 'plex-token' || name === 'X-Plex-Token' || name === 'myPlexAccessToken') {
                                storedToken = decodeURIComponent(value);
                                break;
                            }
                        }
                    }
                }
                
                // Check window.Plex object (if Plex web app exposes it)
                if (!storedToken && window.Plex && window.Plex.token) {
                    storedToken = window.Plex.token;
                }
                
                // Also check window.Plex for myPlexAccessToken
                if (!storedToken && window.Plex && window.Plex.myPlexAccessToken) {
                    storedToken = window.Plex.myPlexAccessToken;
                }
                
                if (storedToken) {
                    window.__plex_desktop_token = storedToken;
                    window.__plex_token_updated = true; // Mark as updated so frontend picks it up
                    console.log('Found token in storage (redacted)');
                    
                    // Try to notify Tauri immediately
                    if (window.__TAURI_INTERNALS__) {
                        try {
                            window.__TAURI_INTERNALS__.invoke('set_auth_token', { token: storedToken });
                            console.log('Notified Tauri of token');
                        } catch (e) {
                            console.error('Failed to notify Tauri of token:', e);
                        }
                    }
                }
                
                // Store sessionstats and deviceSettings if available (for future use)
                try {
                    const sessionStats = localStorage.getItem('sessionstats');
                    const deviceSettings = localStorage.getItem('deviceSettings');
                    if (sessionStats) {
                        window.__plex_session_stats = sessionStats;
                        try {
                            const statsObj = JSON.parse(sessionStats);
                            if (window.__TAURI_INTERNALS__) {
                                window.__TAURI_INTERNALS__.invoke('set_session_stats', { stats: statsObj });
                            }
                        } catch (e) {
                            console.error('Failed to store session stats:', e);
                        }
                    }
                    if (deviceSettings) {
                        window.__plex_device_settings = deviceSettings;
                        try {
                            const settingsObj = JSON.parse(deviceSettings);
                            if (window.__TAURI_INTERNALS__) {
                                window.__TAURI_INTERNALS__.invoke('set_device_settings', { settings: settingsObj });
                            }
                        } catch (e) {
                            console.error('Failed to store device settings:', e);
                        }
                    }
                } catch (e) {
                    // Ignore errors parsing JSON objects
                }
            } catch (e) {
                console.error('Error reading storage:', e);
            }
            
            return window.__plex_desktop_token || null;
        })()
    "#;

    // Execute the script
    let _ = window.eval(js_code);

    // Wait a bit for requests to be intercepted
    std::thread::sleep(Duration::from_millis(500));

    eprintln!("Token interception script installed - monitoring network requests to plex.tv");
    Ok(None) // Token will be captured when requests are made
}

#[tauri::command]
async fn get_intercepted_token(app: AppHandle) -> Result<Option<String>, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;

    // Try to read the intercepted token from window
    let read_js = r#"
        (function() {
            return window.__plex_desktop_token || null;
        })()
    "#;

    match window.eval(read_js) {
        Ok(_) => {
            // In Tauri 2, eval doesn't return the value directly
            // We'll need to use a different approach
            eprintln!("Checking for intercepted token...");
            Ok(None)
        }
        Err(e) => Err(format!("Error reading token: {}", e)),
    }
}

#[tauri::command]
async fn discover_servers_from_token(token: String) -> Result<Vec<DiscoveredServer>, String> {
    eprintln!("Discovering servers using token...");

    // Store the token first
    set_auth_token(token.clone())?;
    eprintln!("Token stored successfully");

    // Fetch resources from Plex API
    let servers = fetch_plex_resources(token).await?;
    eprintln!("Fetched {} servers from resources API", servers.len());

    // Auto-add discovered servers
    for server in &servers {
        eprintln!("Auto-adding server: {} at {}", server.name, server.base_url);
        if let Err(e) = auto_add_discovered_server(
            server.name.clone(),
            server.base_url.clone(),
            server.machine_identifier.clone(),
        )
        .await
        {
            eprintln!("Failed to auto-add server {}: {}", server.name, e);
        } else {
            eprintln!("Successfully added server: {}", server.name);
        }
    }
    
    // Verify config was saved
    let final_config = load_config();
    eprintln!("Final config has {} servers", final_config.servers.len());
    eprintln!("Config file path: {:?}", get_config_path());
    eprintln!("Config file exists: {}", get_config_path().exists());

    Ok(servers)
}

fn resolve_server_url(base_url: Option<&str>, server_id: Option<&str>) -> Result<String, String> {
    // Priority 1: baseUrl from deep link (override)
    if let Some(url) = base_url {
        validate_server_url(url)?;
        return Ok(url.to_string());
    }

    // Priority 2: Lookup by serverId
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

    // Priority 3: Default server
    let config = load_config();
    if let Some(default_id) = config.default_server_id {
        if let Some(server) = config.servers.iter().find(|s| s.id == default_id) {
            return Ok(server.base_url.clone());
        }
    }

    // Priority 4: First server if any exists
    if let Some(server) = config.servers.first() {
        return Ok(server.base_url.clone());
    }

    Err("No server configured. Please add a server in settings.".to_string())
}

fn parse_deep_link(
    url: &str,
) -> Result<
    (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ),
    String,
> {
    let parsed = Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;

    if parsed.scheme() != "plex-desktop" {
        return Err("Invalid protocol scheme".to_string());
    }

    // Handle case where 'server' is the hostname (plex-desktop://server/...)
    // Also handle case where 'auth' is the hostname (plex-desktop://auth?token=...)
    let path = if parsed.host_str() == Some("server") {
        // Reconstruct path with /server prefix
        let original_path = parsed.path();
        if original_path.starts_with('/') {
            format!("/server{}", original_path)
        } else {
            format!("/server/{}", original_path)
        }
    } else if parsed.host_str() == Some("auth") || parsed.host_str() == Some("oauth") {
        // Reconstruct path for OAuth callbacks
        format!("/{}", parsed.host_str().unwrap_or(""))
    } else {
        parsed.path().to_string()
    };

    eprintln!("Parsed path: {}", path);

    let query_params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    // MED-004: Build query string with validated parameters only
    // Only allow whitelisted query parameters to prevent XSS via malicious params
    let mut extra_params = Vec::new();
    for (key, value) in &query_params {
        // Skip our internal parameters
        if key == "key" || key == "baseUrl" || key == "serverId" {
            continue;
        }
        
        // MED-004: Only allow whitelisted parameter names
        if ALLOWED_QUERY_PARAMS.contains(&key.as_str()) {
            extra_params.push(format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            ));
        } else {
            eprintln!("MED-004: Rejected unknown query parameter: {}", key);
        }
    }
    let query_string = if extra_params.is_empty() {
        None
    } else {
        Some(extra_params.join("&"))
    };

    // OAuth callback format: plex-desktop://auth?token={token} or plex-desktop://auth?url={callback_url}
    if path == "/auth" || path == "/oauth" {
        if let Some(token) = query_params.get("token") {
            return Ok((
                Some(format!("oauth://token?token={}", token)),
                None,
                None,
                None,
            ));
        }
        if let Some(callback_url) = query_params.get("url") {
            if let Ok(decoded_url) = urlencoding::decode(callback_url) {
                return Ok((
                    Some(format!(
                        "oauth://callback?url={}",
                        urlencoding::encode(&decoded_url)
                    )),
                    None,
                    None,
                    None,
                ));
            }
        }
    }

    // Format 1: plex-desktop://open?url={encoded_url}
    if let Some(encoded_url) = query_params.get("url") {
        if let Ok(decoded_url) = urlencoding::decode(encoded_url) {
            if let Ok(parsed_url) = Url::parse(&decoded_url) {
                // HIGH-003: Preserve port when constructing base_url
                let host = parsed_url.host_str().unwrap_or("");
                let port = parsed_url.port();
                let base_url = if let Some(p) = port {
                    format!("{}://{}:{}", parsed_url.scheme(), host, p)
                } else {
                    format!("{}://{}", parsed_url.scheme(), host)
                };
                let key = parsed_url.path().to_string();
                return Ok((Some(base_url), None, Some(key), query_string));
            }
        }
    }

    // Format 2: plex-desktop://server/{serverId}/details?key={key}&context={context}
    if path.starts_with("/server/") {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 3 {
            let server_id = parts[2].to_string();
            let key = query_params.get("key").cloned();
            return Ok((None, Some(server_id), key, query_string));
        }
    }

    // Format 3: plex-desktop://open?baseUrl={baseUrl}&serverId={serverId}&key={key}
    // Format 4: plex-desktop://open?baseUrl={baseUrl}&key={key}
    if let Some(base_url) = query_params.get("baseUrl") {
        let decoded_base_url =
            urlencoding::decode(base_url).map_err(|_| "Failed to decode baseUrl".to_string())?;
        let server_id = query_params.get("serverId").cloned();
        let key = query_params.get("key").cloned();
        return Ok((
            Some(decoded_base_url.to_string()),
            server_id,
            key,
            query_string,
        ));
    }

    // Format 5: Simple key-only (uses default server)
    if let Some(key) = query_params.get("key") {
        return Ok((None, None, Some(key.clone()), query_string));
    }

    Err("Unable to parse deep link format".to_string())
}

fn construct_plex_url(
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

    // Build query string
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

// MED-005: Rate limiting helper function
fn check_rate_limit(source: &str) -> Result<(), String> {
    let limiter = get_rate_limiter();
    let mut store = limiter.lock().map_err(|e| format!("Rate limiter lock error: {}", e))?;
    
    let now = Instant::now();
    let window_start = now - Duration::from_secs(RATE_LIMIT_WINDOW_SECS);
    
    // Clean up old entries
    let requests = store.entry(source.to_string()).or_insert_with(Vec::new);
    requests.retain(|&time| time > window_start);
    
    // Check if limit exceeded
    if requests.len() >= RATE_LIMIT_MAX_REQUESTS {
        return Err(format!(
            "Rate limit exceeded: maximum {} requests per {} seconds",
            RATE_LIMIT_MAX_REQUESTS, RATE_LIMIT_WINDOW_SECS
        ));
    }
    
    // Record this request
    requests.push(now);
    Ok(())
}

#[tauri::command]
async fn navigate_to_deep_link(app: AppHandle, url: String) -> Result<(), String> {
    // MED-005: Apply rate limiting
    // Use a simple identifier - in production, you might want to use IP address or session ID
    let rate_limit_source = "deep-link"; // Could be enhanced to use actual source identifier
    check_rate_limit(rate_limit_source)?;
    
    eprintln!("Navigating to deep link: {}", url);
    let (base_url_override, server_id, key, extra_query) = parse_deep_link(&url)?;

    eprintln!(
        "Parsed - base_url_override: {:?}, server_id: {:?}, key: {:?}, extra_query: {:?}",
        base_url_override, server_id, key, extra_query
    );

    // Resolve the actual server URL
    let resolved_base_url = if let Some(override_url) = base_url_override {
        // HIGH-003: Validate that the override URL matches a configured server
        validate_deep_link_base_url(&override_url)?;
        override_url
    } else {
        let resolved = resolve_server_url(None, server_id.as_deref())?;
        eprintln!("Resolved server URL: {}", resolved);
        resolved
    };

    // Construct the Plex web URL
    let plex_url = construct_plex_url(
        &resolved_base_url,
        server_id.as_deref(),
        key.as_deref(),
        extra_query.as_deref(),
    );
    eprintln!("Constructed Plex URL: {}", plex_url);

    // Get the main window and navigate
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;

    // Navigate using Tauri's window API
    let _ =
        window.navigate(tauri::Url::parse(&plex_url).map_err(|e| format!("Invalid URL: {}", e))?);

    eprintln!("Navigation complete");
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Check for command line arguments (deep links)
    let args: Vec<String> = std::env::args().collect();
    let mut deep_link_url: Option<String> = None;
    
    for arg in args.iter().skip(1) {
        if arg.starts_with("plex-desktop://") {
            deep_link_url = Some(arg.clone());
            break;
        }
    }
    
    // Check if another instance is already running
    if is_another_instance_running() {
        if let Some(url) = deep_link_url {
            eprintln!("Another instance is running, sending URL to it: {}", url);
            if send_url_to_existing_instance(&url).is_ok() {
                // Successfully sent to existing instance, exit
                std::process::exit(0);
            }
            // If sending failed, continue and start a new instance
        } else {
            eprintln!("Another instance is already running, exiting");
            std::process::exit(0);
        }
    }
    
    // Create lock file to indicate we're running
    if let Err(e) = create_lock_file() {
        eprintln!("Warning: Failed to create lock file: {}", e);
    }
    
    // Clean up lock file on exit
    let _ = ctrlc::set_handler(move || {
        remove_lock_file();
        std::process::exit(0);
    });
    
    // HIGH-001: Migrate token from config file to keychain on startup
    if let Err(e) = migrate_token_from_config() {
        eprintln!("Warning: Token migration failed: {}", e);
    }
    
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_servers,
            add_server,
            update_server,
            remove_server,
            set_default_server,
            get_default_server,
            navigate_to_deep_link,
            get_auth_token,
            set_auth_token,
            get_client_id,
            set_client_id,
            set_session_stats,
            set_device_settings,
            handle_oauth_callback,
            open_in_browser,
            discover_servers,
            auto_add_discovered_server,
            extract_token_from_webview,
            get_intercepted_token,
            fetch_plex_resources,
            discover_servers_from_token,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Exit the application when the main window is closed
                if window.label() == "main" {
                    eprintln!("Main window closing, exiting application");
                    remove_lock_file();
                    std::process::exit(0);
                }
            }
        })
        .setup(|app| {
            // Start IPC listener to receive URLs from other instances
            let app_handle = app.handle().clone();
            start_ipc_listener(app_handle);
            
            // Ensure config file exists
            let _ = load_config(); // This will create the file if it doesn't exist
            eprintln!("Config file path: {:?}", get_config_path());

            // Auto-discover servers on startup if config is empty
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // Wait a bit for the app to fully initialize
                std::thread::sleep(Duration::from_secs(2));

                // Run async code in a new runtime
                tauri::async_runtime::block_on(async {
                    let config = load_config();
                    eprintln!("Current config has {} servers", config.servers.len());
                    eprintln!("Config file exists: {}", get_config_path().exists());

                    if config.servers.is_empty() {
                        eprintln!("No servers configured, attempting auto-discovery...");

                        // First try localhost discovery
                        if let Ok(discovered) = discover_servers().await {
                            eprintln!("Localhost discovery found {} servers", discovered.len());
                            for server in discovered {
                                eprintln!(
                                    "Auto-adding discovered server: {} at {}",
                                    server.name, server.base_url
                                );
                                if let Err(e) = auto_add_discovered_server(
                                    server.name,
                                    server.base_url,
                                    server.machine_identifier,
                                )
                                .await
                                {
                                    eprintln!("Failed to auto-add server: {}", e);
                                }
                            }
                        } else {
                            eprintln!("Localhost discovery failed or found no servers");
                        }

                        // Also try token-based discovery if we have a token
                        let config_after_local = load_config();
                        if let Some(token) = &config_after_local.auth_token {
                            eprintln!("Found stored token, attempting token-based discovery...");
                            if let Ok(discovered) = discover_servers_from_token(token.clone()).await {
                                eprintln!("Token-based discovery found {} servers", discovered.len());
                            } else {
                                eprintln!("Token-based discovery failed");
                            }
                        } else {
                            eprintln!("No token found, skipping token-based discovery");
                        }
                    } else {
                        eprintln!("Servers already configured, skipping auto-discovery");
                    }
                });
            });

            // Handle protocol URLs passed as command line arguments
            for arg in std::env::args().skip(1) {
                eprintln!("Received argument: {}", arg);
                if arg.starts_with("plex-desktop://") {
                    let app_handle = app.handle().clone();
                    let url = arg.clone();
                    eprintln!("Processing protocol URL: {}", url);

                    // Check if it's an OAuth callback
                    if url.contains("/auth")
                        || url.contains("/oauth")
                        || url.contains("?token=")
                        || url.contains("&token=")
                    {
                        eprintln!("Detected OAuth callback, handling...");
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = handle_oauth_callback(app_handle, url).await {
                                eprintln!("Error handling OAuth callback: {}", e);
                            }
                        });
                    } else {
                        // Regular deep link
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = navigate_to_deep_link(app_handle, url).await {
                                eprintln!("Error handling deep link: {}", e);
                            }
                        });
                    }
                }
            }

            // OAuth interception will be handled via:
            // 1. Frontend JavaScript detecting OAuth URLs and calling open_in_browser
            // 2. Custom protocol callback (plex-desktop://auth?token=...)
            // 3. Protocol handler in setup() above

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;
    use std::env;

    // Helper to create a temporary config directory for testing
    fn setup_test_config() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        (temp_dir, config_path)
    }

    #[test]
    fn test_validate_server_url() {
        // Valid URLs
        assert!(validate_server_url("http://localhost:32400").is_ok());
        assert!(validate_server_url("https://plex.example.com").is_ok());
        
        // Invalid URLs
        assert!(validate_server_url("not-a-url").is_err());
        assert!(validate_server_url("ftp://example.com").is_err());
        assert!(validate_server_url("localhost:32400").is_err()); // Missing scheme
    }

    #[test]
    fn test_extract_token_from_url() {
        // Test plex-desktop:// URLs
        assert_eq!(
            extract_token_from_url("plex-desktop://auth?token=abc123"),
            Some("abc123".to_string())
        );
        assert_eq!(
            extract_token_from_url("plex-desktop://auth?token=abc123&other=value"),
            Some("abc123".to_string())
        );
        assert_eq!(
            extract_token_from_url("plex-desktop://auth?other=value&token=xyz789"),
            Some("xyz789".to_string())
        );

        // Test regular URLs with query params
        assert_eq!(
            extract_token_from_url("https://example.com/callback?token=test123"),
            Some("test123".to_string())
        );
        assert_eq!(
            extract_token_from_url("https://example.com/callback?access_token=test456"),
            Some("test456".to_string())
        );

        // Test URLs with hash fragments
        assert_eq!(
            extract_token_from_url("https://example.com/callback#token=fragment123"),
            Some("fragment123".to_string())
        );

        // Test URLs without tokens
        assert_eq!(extract_token_from_url("https://example.com"), None);
        assert_eq!(extract_token_from_url("plex-desktop://server/123"), None);
    }

    #[test]
    fn test_is_oauth_url() {
        assert!(is_oauth_url("https://plex.tv/auth"));
        assert!(is_oauth_url("https://plex.tv/login"));
        assert!(is_oauth_url("https://plex.tv/sign-in"));
        assert!(is_oauth_url("https://example.com/oauth2/authorize"));
        assert!(is_oauth_url("https://example.com/authorize"));
        
        assert!(!is_oauth_url("https://app.plex.tv"));
        assert!(!is_oauth_url("https://plex.tv/web"));
    }

    #[test]
    fn test_parse_deep_link() {
        // Format 1: plex-desktop://server/{serverId}/details?key={key}
        let (base_url, server_id, key, extra) = parse_deep_link(
            "plex-desktop://server/5e30efc7d6347d6365e8a4f11c03fd3fc334fd60/details?key=/library/metadata/102033&context=source:hub.tv.inprogress~0~0"
        ).unwrap();
        assert_eq!(base_url, None);
        assert_eq!(server_id, Some("5e30efc7d6347d6365e8a4f11c03fd3fc334fd60".to_string()));
        assert_eq!(key, Some("/library/metadata/102033".to_string()));
        assert!(extra.is_some());
        assert!(extra.unwrap().contains("context"));

        // Format 2: OAuth callback
        let result = parse_deep_link("plex-desktop://auth?token=abc123");
        assert!(result.is_ok());
        let (base_url, server_id, key, extra) = result.unwrap();
        assert!(base_url.is_some());
        let base_url_str = base_url.unwrap();
        assert!(base_url_str.contains("oauth://token") || base_url_str.contains("token=abc123"));

        // Format 3: baseUrl override
        let (base_url, server_id, key, extra) = parse_deep_link(
            "plex-desktop://open?baseUrl=http%3A%2F%2Flocalhost%3A32400&key=%2Flibrary%2Fmetadata%2F123"
        ).unwrap();
        assert_eq!(base_url, Some("http://localhost:32400".to_string()));
        assert_eq!(key, Some("/library/metadata/123".to_string()));

        // Format 4: Simple key-only
        let (base_url, server_id, key, extra) = parse_deep_link(
            "plex-desktop://open?key=/library/metadata/456"
        ).unwrap();
        assert_eq!(base_url, None);
        assert_eq!(server_id, None);
        assert_eq!(key, Some("/library/metadata/456".to_string()));

        // Invalid scheme
        assert!(parse_deep_link("https://example.com").is_err());
    }

    #[test]
    fn test_parse_deep_link_format1_port_preservation() {
        // HIGH-003: Test that Format 1 preserves ports when parsing baseUrl
        // Format 1: plex-desktop://open?url={encoded_url}
        
        // Test with port
        let encoded_with_port = urlencoding::encode("http://192.168.1.100:32400/web/index.html#!/server/abc123/details?key=/library/metadata/123");
        let (base_url, server_id, key, _) = parse_deep_link(
            &format!("plex-desktop://open?url={}", encoded_with_port)
        ).unwrap();
        assert_eq!(base_url, Some("http://192.168.1.100:32400".to_string()));
        assert_eq!(key, Some("/web/index.html#!/server/abc123/details".to_string()));

        // Test without port (default port)
        let encoded_no_port = urlencoding::encode("https://plex.example.com/web/index.html#!/details?key=/library/metadata/456");
        let (base_url, server_id, key, _) = parse_deep_link(
            &format!("plex-desktop://open?url={}", encoded_no_port)
        ).unwrap();
        assert_eq!(base_url, Some("https://plex.example.com".to_string()));
        assert_eq!(key, Some("/web/index.html#!/details".to_string()));

        // Test with non-standard port
        let encoded_custom_port = urlencoding::encode("http://localhost:8080/web/index.html#!/details?key=/library/metadata/789");
        let (base_url, server_id, key, _) = parse_deep_link(
            &format!("plex-desktop://open?url={}", encoded_custom_port)
        ).unwrap();
        assert_eq!(base_url, Some("http://localhost:8080".to_string()));
    }

    #[test]
    fn test_validate_deep_link_base_url_allowlist() {
        // HIGH-003: Test the baseUrl allowlist validation helper
        
        // Setup: Create a list of allowed origins
        let allowed_origins = vec![
            "http://localhost:32400".to_string(),
            "https://plex.example.com".to_string(),
            "http://192.168.1.100:32400".to_string(),
        ];

        // Test: Valid URLs that match allowed origins
        assert!(validate_deep_link_base_url_against_origins(
            "http://localhost:32400",
            &allowed_origins
        ).is_ok());
        
        assert!(validate_deep_link_base_url_against_origins(
            "https://plex.example.com",
            &allowed_origins
        ).is_ok());
        
        assert!(validate_deep_link_base_url_against_origins(
            "http://192.168.1.100:32400",
            &allowed_origins
        ).is_ok());

        // Test: URLs with paths/query should still match (normalized to origin)
        assert!(validate_deep_link_base_url_against_origins(
            "http://localhost:32400/web/index.html",
            &allowed_origins
        ).is_ok());
        
        assert!(validate_deep_link_base_url_against_origins(
            "https://plex.example.com:443/web/index.html?key=value",
            &allowed_origins
        ).is_ok());

        // Test: Invalid URLs that don't match
        assert!(validate_deep_link_base_url_against_origins(
            "http://unconfigured-server:32400",
            &allowed_origins
        ).is_err());
        
        assert!(validate_deep_link_base_url_against_origins(
            "https://malicious.example.com",
            &allowed_origins
        ).is_err());

        // Test: Port mismatch
        assert!(validate_deep_link_base_url_against_origins(
            "http://localhost:8080",
            &allowed_origins
        ).is_err());

        // Test: Scheme mismatch
        assert!(validate_deep_link_base_url_against_origins(
            "https://localhost:32400",
            &allowed_origins
        ).is_err());

        // Test: Invalid URL format
        assert!(validate_deep_link_base_url_against_origins(
            "not-a-url",
            &allowed_origins
        ).is_err());

        // Test: Invalid scheme
        assert!(validate_deep_link_base_url_against_origins(
            "ftp://localhost:32400",
            &allowed_origins
        ).is_err());
    }

    #[test]
    fn test_normalize_url_to_origin() {
        // Test normalization helper function
        
        // With port
        assert_eq!(
            normalize_url_to_origin("http://localhost:32400").unwrap(),
            "http://localhost:32400"
        );
        
        // Without port (default)
        assert_eq!(
            normalize_url_to_origin("https://plex.example.com").unwrap(),
            "https://plex.example.com"
        );
        
        // With path and query (should normalize to origin only)
        assert_eq!(
            normalize_url_to_origin("http://192.168.1.100:32400/web/index.html?key=value").unwrap(),
            "http://192.168.1.100:32400"
        );
        
        // Invalid URL
        assert!(normalize_url_to_origin("not-a-url").is_err());
        
        // Invalid scheme
        assert!(normalize_url_to_origin("ftp://example.com").is_err());
    }

    #[test]
    fn test_construct_plex_url() {
        // With server ID and key
        let url = construct_plex_url(
            "http://localhost:32400",
            Some("abc123"),
            Some("/library/metadata/456"),
            None
        );
        assert!(url.contains("http://localhost:32400/web/index.html"));
        assert!(url.contains("#!/server/abc123/details"));
        assert!(url.contains("key=%2Flibrary%2Fmetadata%2F456"));

        // Without server ID
        let url = construct_plex_url(
            "http://localhost:32400",
            None,
            Some("/library/metadata/789"),
            None
        );
        assert!(url.contains("#!/details"));
        assert!(url.contains("key=%2Flibrary%2Fmetadata%2F789"));

        // With extra query params
        let url = construct_plex_url(
            "http://localhost:32400",
            Some("abc123"),
            Some("/library/metadata/456"),
            Some("context=test")
        );
        assert!(url.contains("key="));
        assert!(url.contains("context=test"));
    }

    #[test]
    fn test_extract_machine_identifier() {
        let xml = r#"<MediaContainer machineIdentifier="abc123def456" friendlyName="My Plex Server"/>"#;
        assert_eq!(extract_machine_identifier(xml), Some("abc123def456".to_string()));

        let xml_no_id = r#"<MediaContainer friendlyName="My Plex Server"/>"#;
        assert_eq!(extract_machine_identifier(xml_no_id), None);

        let xml_empty = "";
        assert_eq!(extract_machine_identifier(xml_empty), None);
    }

    #[test]
    fn test_extract_server_name() {
        let xml = r#"<MediaContainer machineIdentifier="abc123" friendlyName="My Plex Server"/>"#;
        assert_eq!(extract_server_name(xml), Some("My Plex Server".to_string()));

        let xml_no_name = r#"<MediaContainer machineIdentifier="abc123"/>"#;
        assert_eq!(extract_server_name(xml_no_name), None);

        let xml_empty = "";
        assert_eq!(extract_server_name(xml_empty), None);
    }

    // Note: Integration tests for config loading/saving would require mocking
    // the config directory, which is more complex. These would be better as
    // integration tests in a separate tests/ directory.
}
