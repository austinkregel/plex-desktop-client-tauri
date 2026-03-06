use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UsersError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeUser {
    pub id: u64,
    pub title: String,
    pub username: Option<String>,
    pub thumb: Option<String>,
    pub admin: Option<bool>,
    pub guest: Option<bool>,
    pub restricted: Option<bool>,
    #[serde(rename = "protected")]
    pub is_protected: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsersResponse {
    users: Vec<HomeUser>,
}

/// Get home users (requires admin token).
pub async fn get_home_users(token: &str) -> Result<Vec<HomeUser>, UsersError> {
    let client = super::plex_client(token)?;
    let resp: UsersResponse = client
        .get("https://plex.tv/api/v2/home/users")
        .send()
        .await?
        .json()
        .await?;
    Ok(resp.users)
}

/// Switch to a different home user. Returns a new token for that user.
#[derive(Debug, Deserialize)]
pub struct SwitchUserResponse {
    #[serde(rename = "authToken")]
    pub auth_token: Option<String>,
}

pub async fn switch_user(
    token: &str,
    user_id: u64,
    pin: Option<&str>,
) -> Result<SwitchUserResponse, UsersError> {
    let client = super::plex_client(token)?;
    let url = format!("https://plex.tv/api/v2/home/users/{}/switch", user_id);
    let mut req = client.post(&url);
    if let Some(p) = pin {
        req = req.query(&[("pin", p)]);
    }
    let resp: SwitchUserResponse = req.send().await?.json().await?;
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_home_user_deserialize() {
        let json = r#"{"id": 1, "title": "Admin User", "username": "admin", "admin": true, "guest": false, "restricted": false, "protected": true}"#;
        let user: HomeUser = serde_json::from_str(json).unwrap();
        assert_eq!(user.id, 1);
        assert_eq!(user.title, "Admin User");
        assert_eq!(user.admin, Some(true));
        assert_eq!(user.is_protected, Some(true));
    }

    #[test]
    fn test_home_user_minimal() {
        let json = r#"{"id": 2, "title": "Kid"}"#;
        let user: HomeUser = serde_json::from_str(json).unwrap();
        assert_eq!(user.id, 2);
        assert_eq!(user.title, "Kid");
        assert!(user.username.is_none());
        assert!(user.admin.is_none());
    }

    #[test]
    fn test_users_response_deserialize() {
        let json = r#"{"users": [{"id": 1, "title": "Admin"}, {"id": 2, "title": "Guest"}]}"#;
        let resp: UsersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.users.len(), 2);
    }

    #[test]
    fn test_switch_user_response_with_token() {
        let json = r#"{"authToken": "newtoken123"}"#;
        let resp: SwitchUserResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.auth_token, Some("newtoken123".to_string()));
    }

    #[test]
    fn test_switch_user_response_no_token() {
        let json = r#"{"authToken": null}"#;
        let resp: SwitchUserResponse = serde_json::from_str(json).unwrap();
        assert!(resp.auth_token.is_none());
    }
}
