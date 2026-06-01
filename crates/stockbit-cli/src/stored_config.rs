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
