use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct OrganAcl {
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NatsCredsFile {
    #[allow(dead_code)]
    pub version: Option<u32>,
    #[allow(dead_code)]
    pub port: Option<u16>,
    pub tls_enabled: Option<bool>,
    pub organs: HashMap<String, OrganAcl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthSource {
    ExplicitUrl,
    Environment,
    CredsFile(String),
    None,
}

impl AuthSource {
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::ExplicitUrl => "URL credentials",
            Self::Environment => "environment variables",
            Self::CredsFile(_) => "creds.json auto-discovery",
            Self::None => "unauthenticated",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedNatsConfig {
    pub connect_url: String,
    pub display_url: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub auth_source: AuthSource,
    pub tls_enabled: bool,
    pub ca_path: Option<PathBuf>,
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn autonomic_root() -> PathBuf {
    std::env::var("AUTONOMIC_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".autonomic"))
}

pub fn creds_path() -> PathBuf {
    autonomic_root().join("broker").join("creds.json")
}

pub fn tls_dir() -> PathBuf {
    autonomic_root().join("broker").join("tls")
}

fn mask_url_credentials(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://")
        && let Some((creds, host_port)) = rest.split_once('@')
    {
        let masked_user = creds.split_once(':').map_or(creds, |(u, _)| u);
        return format!("{scheme}://{masked_user}:***@{host_port}");
    }
    url.to_string()
}

pub fn load_organ_credentials(path: &Path, organ: &str) -> Option<(String, String)> {
    let raw = fs::read_to_string(path).ok()?;
    let creds: NatsCredsFile = serde_json::from_str(&raw).ok()?;
    let acl = creds.organs.get(organ)?;
    Some((acl.user.clone(), acl.password.clone()))
}

#[must_use]
pub fn resolve_nats_config() -> ResolvedNatsConfig {
    resolve_nats_config_with_lookup(|key| std::env::var(key).ok(), &creds_path(), &tls_dir())
}

pub fn resolve_nats_config_with_lookup<F>(
    lookup: F,
    creds_file: &Path,
    tls_directory: &Path,
) -> ResolvedNatsConfig
where
    F: Fn(&str) -> Option<String>,
{
    let raw_url =
        lookup("AUTONOMIC_NATS_URL").unwrap_or_else(|| "nats://127.0.0.1:4222".to_string());

    let insecure = lookup("AUTONOMIC_NATS_INSECURE")
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

    // Case 1: URL contains embedded credentials (e.g. nats://user:pass@host:port)
    if let Some((_scheme, rest)) = raw_url.split_once("://")
        && rest.contains('@')
    {
        let display_url = mask_url_credentials(&raw_url);
        return ResolvedNatsConfig {
            connect_url: raw_url,
            display_url,
            user: None,
            password: None,
            auth_source: AuthSource::ExplicitUrl,
            tls_enabled: false,
            ca_path: None,
            cert_path: None,
            key_path: None,
        };
    }

    let mut user = None;
    let mut password = None;
    let mut auth_source = AuthSource::None;

    if !insecure {
        // Case 2: Environment variables AUTONOMIC_NATS_USER / AUTONOMIC_NATS_PASSWORD
        let env_user = lookup("AUTONOMIC_NATS_USER").unwrap_or_default();
        let env_pass = lookup("AUTONOMIC_NATS_PASSWORD").unwrap_or_default();
        if !env_user.is_empty() {
            user = Some(env_user);
            password = Some(env_pass);
            auth_source = AuthSource::Environment;
        } else {
            // Case 3: Auto-discover from ~/.autonomic/broker/creds.json
            // Default organ is "nerves" because it has pub/sub permissions for events.>
            let organ = lookup("AUTONOMIC_ORGAN").unwrap_or_else(|| "nerves".to_string());
            if let Some((u, p)) = load_organ_credentials(creds_file, &organ) {
                user = Some(u);
                password = Some(p);
                auth_source = AuthSource::CredsFile(organ);
            }
        }
    }

    // TLS resolution
    let tls_forced = lookup("AUTONOMIC_NATS_TLS").as_deref() == Some("1");
    let mut tls_enabled = tls_forced;
    let mut ca_path = lookup("AUTONOMIC_NATS_CA").map(PathBuf::from);
    let mut cert_path = lookup("AUTONOMIC_NATS_CERT").map(PathBuf::from);
    let mut key_path = lookup("AUTONOMIC_NATS_KEY").map(PathBuf::from);

    if !tls_enabled
        && !insecure
        && let Ok(raw) = fs::read_to_string(creds_file)
        && let Ok(creds) = serde_json::from_str::<NatsCredsFile>(&raw)
        && creds.tls_enabled == Some(true)
    {
        tls_enabled = true;
    }

    if tls_enabled {
        if ca_path.is_none() {
            let ca = tls_directory.join("ca.pem");
            if ca.exists() {
                ca_path = Some(ca);
            }
        }
        if cert_path.is_none() && key_path.is_none() {
            let organ = match &auth_source {
                AuthSource::CredsFile(org) => org.as_str(),
                _ => "nerves",
            };
            let cert = tls_directory.join(format!("client-{organ}.pem"));
            let key = tls_directory.join(format!("client-{organ}-key.pem"));
            if cert.exists() && key.exists() {
                cert_path = Some(cert);
                key_path = Some(key);
            }
        }
    }

    let display_url = if let Some(ref u) = user {
        if let Some((scheme, rest)) = raw_url.split_once("://") {
            format!("{scheme}://{u}@{rest}")
        } else {
            raw_url.clone()
        }
    } else {
        raw_url.clone()
    };

    ResolvedNatsConfig {
        connect_url: raw_url,
        display_url,
        user,
        password,
        auth_source,
        tls_enabled,
        ca_path,
        cert_path,
        key_path,
    }
}

impl ResolvedNatsConfig {
    pub async fn connect(&self) -> Result<async_nats::Client, async_nats::ConnectError> {
        let mut opts = async_nats::ConnectOptions::new();

        if let (Some(u), Some(p)) = (&self.user, &self.password) {
            opts = opts.user_and_password(u.clone(), p.clone());
        }

        if self.tls_enabled {
            opts = opts.require_tls(true);
            if let Some(ca) = &self.ca_path {
                opts = opts.add_root_certificates(ca.clone());
            }
            if let (Some(cert), Some(key)) = (&self.cert_path, &self.key_path) {
                opts = opts.add_client_certificate(cert.clone(), key.clone());
            }
        }

        opts.connect(&self.connect_url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_url_credentials() {
        assert_eq!(
            mask_url_credentials("nats://nerves:secret123@127.0.0.1:4222"),
            "nats://nerves:***@127.0.0.1:4222"
        );
        assert_eq!(
            mask_url_credentials("nats://127.0.0.1:4222"),
            "nats://127.0.0.1:4222"
        );
    }

    #[test]
    fn test_auth_source_description() {
        assert_eq!(AuthSource::ExplicitUrl.description(), "URL credentials");
        assert_eq!(
            AuthSource::Environment.description(),
            "environment variables"
        );
        assert_eq!(
            AuthSource::CredsFile("nerves".into()).description(),
            "creds.json auto-discovery"
        );
        assert_eq!(AuthSource::None.description(), "unauthenticated");
    }

    #[test]
    fn test_load_organ_credentials_from_json() {
        let temp_dir = std::env::temp_dir().join(format!("test_nats_creds_{}", uuid_test_str()));
        let _ = fs::create_dir_all(&temp_dir);
        let creds_file = temp_dir.join("creds.json");

        let json = r#"{
            "version": 1,
            "port": 4222,
            "organs": {
                "nerves": {
                    "user": "nerves_user",
                    "password": "nerves_password"
                }
            }
        }"#;
        fs::write(&creds_file, json).expect("write creds json");

        let creds = load_organ_credentials(&creds_file, "nerves");
        assert_eq!(
            creds,
            Some(("nerves_user".to_string(), "nerves_password".to_string()))
        );

        let none_creds = load_organ_credentials(&creds_file, "unknown_organ");
        assert_eq!(none_creds, None);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_resolve_explicit_url_credentials() {
        let dummy = PathBuf::from("/nonexistent");
        let config = resolve_nats_config_with_lookup(
            |key| match key {
                "AUTONOMIC_NATS_URL" => Some("nats://user1:pass1@10.0.0.1:4222".to_string()),
                _ => None,
            },
            &dummy,
            &dummy,
        );
        assert_eq!(config.connect_url, "nats://user1:pass1@10.0.0.1:4222");
        assert_eq!(config.display_url, "nats://user1:***@10.0.0.1:4222");
        assert_eq!(config.auth_source, AuthSource::ExplicitUrl);
        assert!(!config.tls_enabled);
    }

    #[test]
    fn test_resolve_env_credentials() {
        let dummy = PathBuf::from("/nonexistent");
        let config = resolve_nats_config_with_lookup(
            |key| match key {
                "AUTONOMIC_NATS_USER" => Some("env_user".to_string()),
                "AUTONOMIC_NATS_PASSWORD" => Some("env_pass".to_string()),
                _ => None,
            },
            &dummy,
            &dummy,
        );
        assert_eq!(config.user.as_deref(), Some("env_user"));
        assert_eq!(config.password.as_deref(), Some("env_pass"));
        assert_eq!(config.auth_source, AuthSource::Environment);
        assert_eq!(config.display_url, "nats://env_user@127.0.0.1:4222");
    }

    #[test]
    fn test_resolve_insecure_mode_skips_auth() {
        let dummy = PathBuf::from("/nonexistent");
        let config = resolve_nats_config_with_lookup(
            |key| match key {
                "AUTONOMIC_NATS_INSECURE" => Some("true".to_string()),
                "AUTONOMIC_NATS_USER" => Some("ignored_user".to_string()),
                _ => None,
            },
            &dummy,
            &dummy,
        );
        assert_eq!(config.user, None);
        assert_eq!(config.auth_source, AuthSource::None);
    }

    #[test]
    fn test_resolve_creds_file_auto_discovery() {
        let temp_dir = std::env::temp_dir().join(format!("test_nats_disco_{}", uuid_test_str()));
        let _ = fs::create_dir_all(&temp_dir);
        let creds_file = temp_dir.join("creds.json");
        let json = r#"{
            "organs": {
                "heart": { "user": "heart_user", "password": "heart_secret" }
            }
        }"#;
        fs::write(&creds_file, json).expect("write");

        let config = resolve_nats_config_with_lookup(
            |key| match key {
                "AUTONOMIC_ORGAN" => Some("heart".to_string()),
                _ => None,
            },
            &creds_file,
            &temp_dir,
        );
        assert_eq!(config.user.as_deref(), Some("heart_user"));
        assert_eq!(config.password.as_deref(), Some("heart_secret"));
        assert_eq!(config.auth_source, AuthSource::CredsFile("heart".into()));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_resolve_tls_forced_and_paths() {
        let dummy = PathBuf::from("/nonexistent");
        let config = resolve_nats_config_with_lookup(
            |key| match key {
                "AUTONOMIC_NATS_TLS" => Some("1".to_string()),
                "AUTONOMIC_NATS_CA" => Some("/custom/ca.pem".to_string()),
                _ => None,
            },
            &dummy,
            &dummy,
        );
        assert!(config.tls_enabled);
        assert_eq!(config.ca_path, Some(PathBuf::from("/custom/ca.pem")));
    }

    #[tokio::test]
    async fn test_auto_auth_connect_if_broker_running() {
        let config = resolve_nats_config();
        // If broker is running locally, test that connection with auto-discovered auth succeeds without error
        if let Ok(client) = config.connect().await {
            assert!(config.auth_source != AuthSource::None || config.connect_url.contains('@'));
            // Verify nerves user can subscribe to events.>
            let sub = client.subscribe("events.>").await;
            assert!(sub.is_ok());
        }
    }

    fn uuid_test_str() -> String {
        format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }
}
