//! On-disk YAML config at `~/.stockbit-cli/config.yaml`.
//!
//! All fields are optional so the file can hold any subset of settings; the runtime
//! [`crate::config::Config`] merges this with the CLI flags and env vars (flag > env
//! > file > defaults).
//!
//! Files holding the bearer token are written with mode `0600` (owner-only) and
//! parent directory `0700` on Unix to keep the credential off other users' eyes.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const CONFIG_DIR_NAME: &str = ".stockbit-cli";
pub const CONFIG_FILE_NAME: &str = "config.yaml";

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl StoredConfig {
    /// Default on-disk path: `$HOME/.stockbit-cli/config.yaml`.
    pub fn default_path() -> Result<PathBuf> {
        let home =
            std::env::var("HOME").map_err(|_| Error::Config("$HOME is not set".to_string()))?;
        Ok(PathBuf::from(home)
            .join(CONFIG_DIR_NAME)
            .join(CONFIG_FILE_NAME))
    }

    /// Load from the default path. Returns [`StoredConfig::default`] if the file
    /// does not exist (a fresh install is not an error).
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::default_path()?)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(s) if s.trim().is_empty() => Ok(Self::default()),
            Ok(s) => Ok(serde_yaml::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Save to the default path, creating the parent directory if needed.
    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::default_path()?;
        self.save_to(&path)?;
        Ok(path)
    }

    /// Atomic write: serialise to a sibling temp file, fsync, rename into place.
    /// On Unix the file ends with mode 0600 and the parent directory with 0700.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            set_owner_only_dir_mode(parent)?;
        }
        let yaml = serde_yaml::to_string(self)?;

        let tmp = path.with_extension("yaml.tmp");
        {
            let mut f = open_owner_only(&tmp)?;
            f.write_all(yaml.as_bytes())?;
            f.flush()?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Convenience: update one of the known string keys (case-insensitive).
    /// Returns the previous value, if any.
    pub fn set(&mut self, key: &str, value: String) -> Result<Option<String>> {
        match key.to_ascii_lowercase().as_str() {
            "token" => Ok(self.token.replace(value)),
            "base_url" | "base-url" | "baseurl" => Ok(self.base_url.replace(value)),
            other => Err(Error::Config(format!(
                "unknown config key {other:?}; expected one of: token, base-url"
            ))),
        }
    }

    pub fn unset(&mut self, key: &str) -> Result<Option<String>> {
        match key.to_ascii_lowercase().as_str() {
            "token" => Ok(self.token.take()),
            "base_url" | "base-url" | "baseurl" => Ok(self.base_url.take()),
            other => Err(Error::Config(format!(
                "unknown config key {other:?}; expected one of: token, base-url"
            ))),
        }
    }

    /// Debug-friendly view that never reveals the token. Used by `stockbit config show`.
    #[must_use]
    pub fn redacted(&self) -> serde_json::Value {
        serde_json::json!({
            "token": self.token.as_deref().map_or("<unset>", |_| "<set>"),
            "base_url": self.base_url,
        })
    }
}

#[cfg(unix)]
fn open_owner_only(path: &Path) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?)
}

#[cfg(not(unix))]
fn open_owner_only(path: &Path) -> Result<fs::File> {
    Ok(fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?)
}

#[cfg(unix)]
fn set_owner_only_dir_mode(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(dir)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(dir, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_dir_mode(_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempdir().unwrap();
        let cfg = StoredConfig::load_from(&dir.path().join("nope.yaml")).unwrap();
        assert_eq!(cfg, StoredConfig::default());
    }

    #[test]
    fn load_empty_file_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("c.yaml");
        std::fs::write(&path, "").unwrap();
        assert_eq!(
            StoredConfig::load_from(&path).unwrap(),
            StoredConfig::default()
        );
    }

    #[test]
    fn roundtrip_writes_and_reads_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("c.yaml");
        let cfg = StoredConfig {
            token: Some("eyJtokenvalue".to_string()),
            base_url: Some("https://example.invalid/api".to_string()),
        };
        cfg.save_to(&path).unwrap();
        let loaded = StoredConfig::load_from(&path).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn empty_fields_are_omitted_from_yaml() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("c.yaml");
        let cfg = StoredConfig {
            token: Some("t".into()),
            base_url: None,
        };
        cfg.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("token: t"));
        assert!(!text.contains("base_url"));
    }

    #[test]
    fn set_then_unset_token() {
        let mut cfg = StoredConfig::default();
        assert!(cfg.set("token", "abc".into()).unwrap().is_none());
        assert_eq!(cfg.token.as_deref(), Some("abc"));
        let prev = cfg.set("TOKEN", "def".into()).unwrap();
        assert_eq!(prev.as_deref(), Some("abc"));
        let removed = cfg.unset("token").unwrap();
        assert_eq!(removed.as_deref(), Some("def"));
        assert!(cfg.token.is_none());
    }

    #[test]
    fn set_supports_base_url_variants() {
        let mut cfg = StoredConfig::default();
        cfg.set("base-url", "https://a".into()).unwrap();
        assert_eq!(cfg.base_url.as_deref(), Some("https://a"));
        cfg.set("base_url", "https://b".into()).unwrap();
        assert_eq!(cfg.base_url.as_deref(), Some("https://b"));
    }

    #[test]
    fn set_rejects_unknown_key() {
        let mut cfg = StoredConfig::default();
        let err = cfg.set("nope", "v".into()).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn redacted_hides_token_value_but_shows_presence() {
        let cfg = StoredConfig {
            token: Some("super-secret".into()),
            base_url: Some("https://x".into()),
        };
        let v = cfg.redacted();
        let s = serde_json::to_string(&v).unwrap();
        assert!(!s.contains("super-secret"));
        assert!(s.contains("<set>"));
        assert!(s.contains("https://x"));

        let empty = StoredConfig::default();
        let s = serde_json::to_string(&empty.redacted()).unwrap();
        assert!(s.contains("<unset>"));
    }

    #[cfg(unix)]
    #[test]
    fn save_sets_owner_only_perms_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let nested = dir.path().join(".stockbit-cli");
        let path = nested.join("config.yaml");
        let cfg = StoredConfig {
            token: Some("t".into()),
            base_url: None,
        };
        cfg.save_to(&path).unwrap();
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(&nested).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600, "file mode: {file_mode:o}");
        assert_eq!(dir_mode, 0o700, "dir mode: {dir_mode:o}");
    }

    #[test]
    fn malformed_yaml_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("c.yaml");
        std::fs::write(&path, "token: : :\n  - this is not yaml").unwrap();
        let err = StoredConfig::load_from(&path).unwrap_err();
        assert!(matches!(err, Error::Yaml(_)));
    }
}
