use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Invalid token")]
    InvalidToken,
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlexUser {
    pub id: u64,
    pub username: String,
    pub email: Option<String>,
    pub thumb: Option<String>,
    pub title: Option<String>,
}

/// Validate a token against plex.tv and get user info.
pub async fn validate_token(token: &str) -> Result<PlexUser, AuthError> {
    let client = super::plex_client(token)?;
    let resp = client.get("https://plex.tv/api/v2/user").send().await?;

    if !resp.status().is_success() {
        return Err(AuthError::InvalidToken);
    }

    let user: PlexUser = resp.json().await?;
    Ok(user)
}

/// Create a Plex PIN for OAuth flow (user scans QR or clicks link).
#[derive(Debug, Deserialize)]
pub struct PlexPin {
    pub id: u64,
    pub code: String,
    #[serde(rename = "authToken")]
    pub auth_token: Option<String>,
}

pub async fn create_pin(client_id: &str) -> Result<PlexPin, AuthError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let resp = client
        .post("https://plex.tv/api/v2/pins")
        .header("X-Plex-Product", "Simplex")
        .header("X-Plex-Client-Identifier", client_id)
        .header("Accept", "application/json")
        .query(&[("strong", "true")])
        .send()
        .await?;

    let pin: PlexPin = resp.json().await?;
    Ok(pin)
}

/// Check if a PIN has been claimed (user completed auth).
pub async fn check_pin(pin_id: u64, client_id: &str) -> Result<PlexPin, AuthError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let url = format!("https://plex.tv/api/v2/pins/{}", pin_id);
    let resp = client
        .get(&url)
        .header("X-Plex-Client-Identifier", client_id)
        .header("Accept", "application/json")
        .send()
        .await?;

    let pin: PlexPin = resp.json().await?;
    Ok(pin)
}

/// Get the OAuth authorization URL that the user should visit.
pub fn get_auth_url(pin_code: &str, client_id: &str) -> String {
    format!(
        "https://app.plex.tv/auth#?clientID={}&code={}&context%5Bdevice%5D%5Bproduct%5D=Simplex",
        client_id, pin_code
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_auth_url_contains_required_params() {
        let url = get_auth_url("ABCDEF", "my-client-id");
        assert!(url.starts_with("https://app.plex.tv/auth#?"));
        assert!(url.contains("clientID=my-client-id"));
        assert!(url.contains("code=ABCDEF"));
        assert!(url.contains("Simplex"));
    }

    #[test]
    fn test_get_auth_url_special_characters() {
        let url = get_auth_url("CODE123", "client-with-dashes");
        assert!(url.contains("clientID=client-with-dashes"));
        assert!(url.contains("code=CODE123"));
    }

    #[test]
    fn test_plex_user_deserialize() {
        let json = r#"{"id": 12345, "username": "testuser", "email": "test@example.com", "thumb": "https://plex.tv/users/abc/avatar", "title": "Test User"}"#;
        let user: PlexUser = serde_json::from_str(json).unwrap();
        assert_eq!(user.id, 12345);
        assert_eq!(user.username, "testuser");
        assert_eq!(user.email, Some("test@example.com".to_string()));
        assert_eq!(user.title, Some("Test User".to_string()));
    }

    #[test]
    fn test_plex_user_deserialize_minimal() {
        let json = r#"{"id": 1, "username": "u"}"#;
        let user: PlexUser = serde_json::from_str(json).unwrap();
        assert_eq!(user.id, 1);
        assert_eq!(user.username, "u");
        assert!(user.email.is_none());
        assert!(user.thumb.is_none());
        assert!(user.title.is_none());
    }

    #[test]
    fn test_plex_pin_deserialize_unclaimed() {
        let json = r#"{"id": 999, "code": "ABCD", "authToken": null}"#;
        let pin: PlexPin = serde_json::from_str(json).unwrap();
        assert_eq!(pin.id, 999);
        assert_eq!(pin.code, "ABCD");
        assert!(pin.auth_token.is_none());
    }

    #[test]
    fn test_plex_pin_deserialize_claimed() {
        let json = r#"{"id": 999, "code": "ABCD", "authToken": "supersecrettoken"}"#;
        let pin: PlexPin = serde_json::from_str(json).unwrap();
        assert_eq!(pin.id, 999);
        assert_eq!(pin.auth_token, Some("supersecrettoken".to_string()));
    }
}
