use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug, Clone, Copy)]
pub struct CachePolicy {
    pub ttl_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheRecord {
    key: String,
    stored_at_unix_secs: u64,
    payload: serde_json::Value,
}

#[derive(Debug, Clone)]
struct MemoryRecord {
    stored_at_unix_secs: u64,
    payload: serde_json::Value,
}

static MEMORY_CACHE: OnceLock<Mutex<HashMap<String, MemoryRecord>>> = OnceLock::new();

fn memory_cache() -> &'static Mutex<HashMap<String, MemoryRecord>> {
    MEMORY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_fresh(stored_at_unix_secs: u64, policy: CachePolicy) -> bool {
    now_unix_secs().saturating_sub(stored_at_unix_secs) <= policy.ttl_secs
}

fn cache_dir() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("simplex").join("api-cache");
    if !dir.exists() && fs::create_dir_all(&dir).is_err() {
        return None;
    }
    #[cfg(unix)]
    {
        if let Ok(mut perms) = fs::metadata(&dir).map(|m| m.permissions()) {
            perms.set_mode(0o700);
            let _ = fs::set_permissions(&dir, perms);
        }
    }
    Some(dir)
}

fn key_hash(key: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn cache_file_path(key: &str) -> Option<PathBuf> {
    Some(cache_dir()?.join(format!("{}.json", key_hash(key))))
}

fn read_disk_record(key: &str) -> Option<CacheRecord> {
    let path = cache_file_path(key)?;
    let content = fs::read_to_string(path).ok()?;
    let record: CacheRecord = serde_json::from_str(&content).ok()?;
    if record.key == key {
        Some(record)
    } else {
        None
    }
}

fn write_disk_record(record: &CacheRecord) {
    let Some(path) = cache_file_path(&record.key) else {
        return;
    };
    let Ok(content) = serde_json::to_string(record) else {
        return;
    };
    let _ = fs::write(path, content);
    #[cfg(unix)]
    {
        if let Some(path) = cache_file_path(&record.key) {
            if let Ok(mut perms) = fs::metadata(&path).map(|m| m.permissions()) {
                perms.set_mode(0o600);
                let _ = fs::set_permissions(path, perms);
            }
        }
    }
}

pub fn get<T: DeserializeOwned>(key: &str, policy: CachePolicy) -> Option<T> {
    if policy.ttl_secs == 0 {
        return None;
    }

    if let Ok(cache) = memory_cache().lock() {
        if let Some(rec) = cache.get(key) {
            if is_fresh(rec.stored_at_unix_secs, policy) {
                if let Ok(parsed) = serde_json::from_value::<T>(rec.payload.clone()) {
                    return Some(parsed);
                }
            }
        }
    }

    if let Some(rec) = read_disk_record(key) {
        if is_fresh(rec.stored_at_unix_secs, policy) {
            if let Ok(parsed) = serde_json::from_value::<T>(rec.payload.clone()) {
                if let Ok(mut cache) = memory_cache().lock() {
                    cache.insert(
                        key.to_string(),
                        MemoryRecord {
                            stored_at_unix_secs: rec.stored_at_unix_secs,
                            payload: rec.payload.clone(),
                        },
                    );
                }
                return Some(parsed);
            }
        }
    }

    None
}

pub fn set<T: Serialize>(key: &str, value: &T) {
    let Ok(payload) = serde_json::to_value(value) else {
        return;
    };

    let stored_at_unix_secs = now_unix_secs();
    if let Ok(mut cache) = memory_cache().lock() {
        cache.insert(
            key.to_string(),
            MemoryRecord {
                stored_at_unix_secs,
                payload: payload.clone(),
            },
        );
    }

    write_disk_record(&CacheRecord {
        key: key.to_string(),
        stored_at_unix_secs,
        payload,
    });
}

pub fn invalidate_prefix(prefix: &str) {
    if let Ok(mut cache) = memory_cache().lock() {
        cache.retain(|k, _| !k.starts_with(prefix));
    }

    let Some(dir) = cache_dir() else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<CacheRecord>(&content) else {
            continue;
        };
        if record.key.starts_with(prefix) {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get_roundtrip() {
        let key = "test:set_get_roundtrip";
        set(key, &vec!["a".to_string(), "b".to_string()]);
        let got: Option<Vec<String>> = get(key, CachePolicy { ttl_secs: 60 });
        assert_eq!(got, Some(vec!["a".into(), "b".into()]));
    }

    #[test]
    fn test_ttl_expiry() {
        let key = "test:ttl_expiry";
        set(key, &"value".to_string());
        let fresh: Option<String> = get(key, CachePolicy { ttl_secs: 1 });
        assert_eq!(fresh.as_deref(), Some("value"));

        std::thread::sleep(std::time::Duration::from_secs(2));
        let stale: Option<String> = get(key, CachePolicy { ttl_secs: 1 });
        assert!(stale.is_none());
    }

    #[test]
    fn test_invalidate_prefix() {
        let key_a = "test:prefix:a";
        let key_b = "test:prefix:b";
        set(key_a, &1u64);
        set(key_b, &2u64);

        invalidate_prefix("test:prefix:");
        let a: Option<u64> = get(key_a, CachePolicy { ttl_secs: 60 });
        let b: Option<u64> = get(key_b, CachePolicy { ttl_secs: 60 });
        assert!(a.is_none());
        assert!(b.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_cache_file_permissions_are_restricted() {
        let key = "test:file_permissions";
        set(key, &"value".to_string());
        let path = cache_file_path(key).expect("cache path should exist");
        let mode = fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
