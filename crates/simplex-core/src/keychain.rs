use thiserror::Error;

const KEYCHAIN_SERVICE: &str = "simplex";
const KEYCHAIN_USERNAME: &str = "auth-token";

#[derive(Debug, Error)]
pub enum KeychainError {
    #[error("Keyring error: {0}")]
    Keyring(String),
    #[error("Config error: {0}")]
    Config(#[from] crate::config::ConfigError),
}

pub fn get_token() -> Result<Option<String>, KeychainError> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USERNAME)
        .map_err(|e| KeychainError::Keyring(e.to_string()))?;
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(KeychainError::Keyring(e.to_string())),
    }
}

pub fn set_token(token: &str) -> Result<(), KeychainError> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USERNAME)
        .map_err(|e| KeychainError::Keyring(e.to_string()))?;
    entry
        .set_password(token)
        .map_err(|e| KeychainError::Keyring(e.to_string()))?;
    Ok(())
}

pub fn migrate_token_from_config() -> Result<(), KeychainError> {
    let config = crate::config::load_config();
    if let Some(token) = config.auth_token {
        let keychain_has_token = get_token()?.is_some();
        if !keychain_has_token {
            tracing::info!("Migrating token from config file to keychain...");
            set_token(&token)?;
            let mut config = crate::config::load_config();
            config.auth_token = None;
            let _ = crate::config::save_config(&config);
            tracing::info!("Token migrated successfully");
        } else {
            let mut config = crate::config::load_config();
            if config.auth_token.is_some() {
                config.auth_token = None;
                let _ = crate::config::save_config(&config);
            }
        }
    }
    Ok(())
}

/// Get the auth token, trying keychain first, then migrating from config if needed.
pub fn get_auth_token() -> Result<Option<String>, KeychainError> {
    if let Some(token) = get_token()? {
        return Ok(Some(token));
    }
    let _ = migrate_token_from_config();
    get_token()
}

/// Set the auth token in the keychain.
pub fn set_auth_token(token: &str) -> Result<(), KeychainError> {
    set_token(token)?;
    tracing::info!("Auth token stored successfully in keychain");
    Ok(())
}
