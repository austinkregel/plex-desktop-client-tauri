use std::collections::HashMap;
use thiserror::Error;
use url::Url;

const SCHEME: &str = "simplex";

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

#[derive(Debug, Error)]
pub enum DeepLinkError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Invalid protocol scheme")]
    InvalidScheme,
    #[error("Unable to parse deep link format")]
    UnableToParse,
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub struct DeepLinkResult {
    pub base_url: Option<String>,
    pub server_id: Option<String>,
    pub key: Option<String>,
    pub extra_query: Option<String>,
}

/// Returns true if the URL appears to be an OAuth/auth callback URL.
pub fn is_oauth_url(url: &str) -> bool {
    url.contains("plex.tv/auth")
        || url.contains("plex.tv/login")
        || url.contains("plex.tv/sign-in")
        || url.contains("/oauth2/")
        || url.contains("authorize")
}

pub fn parse_deep_link(url: &str) -> Result<DeepLinkResult, DeepLinkError> {
    let parsed = Url::parse(url).map_err(|e| DeepLinkError::InvalidUrl(e.to_string()))?;

    if parsed.scheme() != SCHEME {
        return Err(DeepLinkError::InvalidScheme);
    }

    // Handle case where 'server' is the hostname (simplex://server/...)
    // Also handle case where 'auth' is the hostname (simplex://auth?token=...)
    let path = if parsed.host_str() == Some("server") {
        let original_path = parsed.path();
        if original_path.starts_with('/') {
            format!("/server{}", original_path)
        } else {
            format!("/server/{}", original_path)
        }
    } else if parsed.host_str() == Some("auth") || parsed.host_str() == Some("oauth") {
        format!("/{}", parsed.host_str().unwrap_or(""))
    } else {
        parsed.path().to_string()
    };

    let query_params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    // Build query string with validated parameters only
    let mut extra_params = Vec::new();
    for (key, value) in &query_params {
        if key == "key" || key == "baseUrl" || key == "serverId" {
            continue;
        }
        if ALLOWED_QUERY_PARAMS.contains(&key.as_str()) {
            extra_params.push(format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            ));
        }
    }
    let query_string = if extra_params.is_empty() {
        None
    } else {
        Some(extra_params.join("&"))
    };

    // OAuth callback format: simplex://auth?token={token} or simplex://auth?url={callback_url}
    if path == "/auth" || path == "/oauth" {
        if let Some(token) = query_params.get("token") {
            return Ok(DeepLinkResult {
                base_url: Some(format!("oauth://token?token={}", token)),
                server_id: None,
                key: None,
                extra_query: None,
            });
        }
        if let Some(callback_url) = query_params.get("url") {
            if let Ok(decoded_url) = urlencoding::decode(callback_url) {
                return Ok(DeepLinkResult {
                    base_url: Some(format!(
                        "oauth://callback?url={}",
                        urlencoding::encode(&decoded_url)
                    )),
                    server_id: None,
                    key: None,
                    extra_query: None,
                });
            }
        }
    }

    // Format 1: simplex://open?url={encoded_url}
    if let Some(encoded_url) = query_params.get("url") {
        if let Ok(decoded_url) = urlencoding::decode(encoded_url) {
            if let Ok(parsed_url) = Url::parse(&decoded_url) {
                let host = parsed_url.host_str().unwrap_or("");
                let port = parsed_url.port();
                let base_url = if let Some(p) = port {
                    format!("{}://{}:{}", parsed_url.scheme(), host, p)
                } else {
                    format!("{}://{}", parsed_url.scheme(), host)
                };
                let key = parsed_url.path().to_string();
                return Ok(DeepLinkResult {
                    base_url: Some(base_url),
                    server_id: None,
                    key: Some(key),
                    extra_query: query_string,
                });
            }
        }
    }

    // Format 2: simplex://server/{serverId}/details?key={key}&context={context}
    if path.starts_with("/server/") {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 3 {
            let server_id = parts[2].to_string();
            let key = query_params.get("key").cloned();
            return Ok(DeepLinkResult {
                base_url: None,
                server_id: Some(server_id),
                key,
                extra_query: query_string,
            });
        }
    }

    // Format 3: simplex://open?baseUrl={baseUrl}&serverId={serverId}&key={key}
    // Format 4: simplex://open?baseUrl={baseUrl}&key={key}
    if let Some(base_url) = query_params.get("baseUrl") {
        let decoded_base_url = urlencoding::decode(base_url)
            .map_err(|_| DeepLinkError::Other("Failed to decode baseUrl".to_string()))?;
        let server_id = query_params.get("serverId").cloned();
        let key = query_params.get("key").cloned();
        return Ok(DeepLinkResult {
            base_url: Some(decoded_base_url.to_string()),
            server_id,
            key,
            extra_query: query_string,
        });
    }

    // Format 5: Simple key-only (uses default server)
    if let Some(key) = query_params.get("key") {
        return Ok(DeepLinkResult {
            base_url: None,
            server_id: None,
            key: Some(key.clone()),
            extra_query: query_string,
        });
    }

    Err(DeepLinkError::UnableToParse)
}

/// Extract a fully-qualified Plex web URL from a `simplex://` deep link.
///
/// Supports the format: `simplex://open?url={urlencoded_http_or_https_url}`
///
/// Also accepts `plex-desktop://` for backwards compatibility.
fn is_deep_link_scheme(url: &str) -> bool {
    url.starts_with("simplex://") || url.starts_with("plex-desktop://")
}

pub fn extract_direct_web_url_from_deep_link(deep_link: &str) -> Option<String> {
    let parsed = Url::parse(deep_link).ok()?;
    if parsed.scheme() != "simplex" && parsed.scheme() != "plex-desktop" {
        return None;
    }

    let is_open =
        parsed.host_str() == Some("open") || parsed.path().eq_ignore_ascii_case("/open");
    if !is_open {
        return None;
    }

    let query_params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
    let encoded = query_params.get("url")?;
    let decoded = urlencoding::decode(encoded).ok()?;
    Some(decoded.to_string())
}

/// Extract auth token from URL. Supports both `simplex://` and `plex-desktop://` for backwards compatibility.
pub fn extract_token_from_url(url: &str) -> Option<String> {
    // First, try simple string parsing for simplex:// and plex-desktop:// URLs
    if is_deep_link_scheme(url) {
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
            return Some(token.clone());
        }
    }

    None
}

/// Construct the target URL for the media server web UI.
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
    fn test_parse_deep_link() {
        // Format 2: simplex://server/{serverId}/details?key={key}
        let result = parse_deep_link(
            "simplex://server/5e30efc7d6347d6365e8a4f11c03fd3fc334fd60/details?key=/library/metadata/102033&context=source:hub.tv.inprogress~0~0",
        )
        .unwrap();
        assert_eq!(result.base_url, None);
        assert_eq!(
            result.server_id,
            Some("5e30efc7d6347d6365e8a4f11c03fd3fc334fd60".to_string())
        );
        assert_eq!(
            result.key,
            Some("/library/metadata/102033".to_string())
        );
        assert!(result.extra_query.is_some());
        assert!(result.extra_query.unwrap().contains("context"));

        // OAuth callback
        let result = parse_deep_link("simplex://auth?token=abc123").unwrap();
        assert!(result.base_url.is_some());
        let base_url_str = result.base_url.unwrap();
        assert!(
            base_url_str.contains("oauth://token") || base_url_str.contains("token=abc123")
        );

        // Format 3: baseUrl override
        let result = parse_deep_link(
            "simplex://open?baseUrl=http%3A%2F%2Flocalhost%3A32400&key=%2Flibrary%2Fmetadata%2F123",
        )
        .unwrap();
        assert_eq!(result.base_url, Some("http://localhost:32400".to_string()));
        assert_eq!(result.key, Some("/library/metadata/123".to_string()));

        // Format 4: Simple key-only
        let result = parse_deep_link("simplex://open?key=/library/metadata/456").unwrap();
        assert_eq!(result.base_url, None);
        assert_eq!(result.server_id, None);
        assert_eq!(result.key, Some("/library/metadata/456".to_string()));

        // Invalid scheme
        assert!(parse_deep_link("https://example.com").is_err());
    }

    #[test]
    fn test_parse_deep_link_format1_port_preservation() {
        // Format 1: simplex://open?url={encoded_url}
        let encoded_with_port = urlencoding::encode(
            "http://192.168.1.100:32400/web/index.html#!/server/abc123/details?key=/library/metadata/123",
        );
        let result = parse_deep_link(&format!("simplex://open?url={}", encoded_with_port)).unwrap();
        assert_eq!(
            result.base_url,
            Some("http://192.168.1.100:32400".to_string())
        );
        assert_eq!(result.key, Some("/web/index.html".to_string()));

        let encoded_no_port = urlencoding::encode(
            "https://plex.example.com/web/index.html#!/details?key=/library/metadata/456",
        );
        let result = parse_deep_link(&format!("simplex://open?url={}", encoded_no_port)).unwrap();
        assert_eq!(
            result.base_url,
            Some("https://plex.example.com".to_string())
        );
        assert_eq!(result.key, Some("/web/index.html".to_string()));

        let encoded_custom_port = urlencoding::encode(
            "http://localhost:8080/web/index.html#!/details?key=/library/metadata/789",
        );
        let result = parse_deep_link(&format!("simplex://open?url={}", encoded_custom_port)).unwrap();
        assert_eq!(result.base_url, Some("http://localhost:8080".to_string()));
    }

    #[test]
    fn test_extract_token_from_url() {
        // Test simplex:// URLs
        assert_eq!(
            extract_token_from_url("simplex://auth?token=abc123"),
            Some("abc123".to_string())
        );
        assert_eq!(
            extract_token_from_url("simplex://auth?token=abc123&other=value"),
            Some("abc123".to_string())
        );
        assert_eq!(
            extract_token_from_url("simplex://auth?other=value&token=xyz789"),
            Some("xyz789".to_string())
        );

        // Test plex-desktop:// URLs (backwards compatibility)
        assert_eq!(
            extract_token_from_url("plex-desktop://auth?token=abc123"),
            Some("abc123".to_string())
        );
        assert_eq!(
            extract_token_from_url("plex-desktop://auth?token=abc123&other=value"),
            Some("abc123".to_string())
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
        assert_eq!(extract_token_from_url("simplex://server/123"), None);
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
    fn test_extract_direct_web_url_from_deep_link() {
        let encoded = urlencoding::encode("https://192.168.1.100:32400/web/index.html#!/details?key=/library/metadata/123");
        let result = extract_direct_web_url_from_deep_link(&format!("simplex://open?url={}", encoded));
        assert_eq!(
            result,
            Some("https://192.168.1.100:32400/web/index.html#!/details?key=/library/metadata/123".to_string())
        );

        // plex-desktop backwards compatibility
        let result = extract_direct_web_url_from_deep_link(&format!("plex-desktop://open?url={}", encoded));
        assert!(result.is_some());

        // Wrong scheme
        assert!(extract_direct_web_url_from_deep_link("https://example.com").is_none());
    }
}
