//! Rotating Stockbit token store at `~/.stockbit-cli/tokens.json`.
//!
//! Stockbit's refresh token is one-time-use: `POST /login/refresh` returns a new
//! access token and a new refresh token, and the old refresh token stops working.
//! So a rotation must persist the new pair before anything else can use the old
//! one, and two rotators must never send the same refresh token. [`refresh`]
//! holds an exclusive lock on a sibling `.lock` file for the whole
//! read-rotate-write cycle, and re-reads the store after taking the lock.
//!
//! The file is written atomically (temp file, fsync, rename) with mode 0600 in a
//! 0700 directory, like [`crate::stored_config`].

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::stored_config::CONFIG_DIR_NAME;

pub const STORE_FILE_NAME: &str = "tokens.json";
/// Overrides the store location (tests, multi-account hosts).
pub const STORE_ENV: &str = "STOCKBIT_TOKEN_STORE";

// Cloudflare on exodus answers a bare client UA on /login/refresh with an HTML
// challenge, so the refresh call presents the web app's browser headers.
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenStore {
    pub access: String,
    pub refresh: String,
    #[serde(default)]
    pub access_expired_at: Option<String>,
    #[serde(default)]
    pub refresh_expired_at: Option<String>,
    pub updated_at: String,
}

impl std::fmt::Debug for TokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenStore")
            .field("access", &"<redacted>")
            .field("refresh", &"<redacted>")
            .field("access_expired_at", &self.access_expired_at)
            .field("refresh_expired_at", &self.refresh_expired_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl TokenStore {
    /// `$STOCKBIT_TOKEN_STORE`, else `$HOME/.stockbit-cli/tokens.json`.
    ///
    /// # Errors
    ///
    /// Fails if neither the override nor `$HOME` is set.
    pub fn default_path() -> Result<PathBuf> {
        if let Some(p) = std::env::var_os(STORE_ENV).filter(|p| !p.is_empty()) {
            return Ok(PathBuf::from(p));
        }
        let home =
            std::env::var("HOME").map_err(|_| Error::Config("$HOME is not set".to_string()))?;
        Ok(PathBuf::from(home)
            .join(CONFIG_DIR_NAME)
            .join(STORE_FILE_NAME))
    }

    /// `None` when the store does not exist yet.
    ///
    /// # Errors
    ///
    /// Fails on read errors or malformed JSON.
    pub fn load_from(path: &Path) -> Result<Option<Self>> {
        match fs::read_to_string(path) {
            Ok(s) => Ok(Some(serde_json::from_str(&s)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Atomic owner-only write: sibling temp file, fsync, rename.
    ///
    /// # Errors
    ///
    /// Fails on filesystem errors.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        ensure_private_dir(path)?;
        let tmp = path.with_extension("json.tmp");
        {
            let mut f = open_owner_only(&tmp)?;
            f.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Expiry metadata only; safe to print.
    #[must_use]
    pub fn status(&self) -> Value {
        json!({
            "access_expired_at": self.access_expired_at,
            "refresh_expired_at": self.refresh_expired_at,
            "updated_at": self.updated_at,
        })
    }
}

/// Rotate `seed` (a refresh token from a fresh login) and persist the new pair.
///
/// # Errors
///
/// See [`refresh`].
pub async fn seed(store: &Path, base_url: &str, seed: &str) -> Result<TokenStore> {
    let _lock = lock(store).await?;
    rotate(store, base_url, seed.trim()).await
}

/// Rotate the stored refresh token and persist the new pair.
///
/// Never retries once Stockbit has answered: the answer may already have consumed
/// the refresh token. A connection failure leaves the token unconsumed, so the
/// caller can run again.
///
/// # Errors
///
/// [`Error::Config`] without a stored refresh token, [`Error::Api`] when Stockbit
/// rejects the refresh, [`Error::Http`] on connection failure.
pub async fn refresh(store: &Path, base_url: &str) -> Result<TokenStore> {
    let _lock = lock(store).await?;
    let current = TokenStore::load_from(store)?.ok_or_else(|| {
        Error::Config(format!(
            "no token store at {}; run `pipeline token seed` first",
            store.display()
        ))
    })?;
    rotate(store, base_url, &current.refresh).await
}

async fn rotate(store: &Path, base_url: &str, refresh_token: &str) -> Result<TokenStore> {
    let url = crate::util::with_trailing_slash(base_url)?.join("login/refresh")?;
    let mut auth = HeaderValue::from_str(&format!("Bearer {refresh_token}"))
        .map_err(|_| Error::Config("refresh token has invalid characters".into()))?;
    auth.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, auth);
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(ORIGIN, HeaderValue::from_static("https://stockbit.com"));
    headers.insert(REFERER, HeaderValue::from_static("https://stockbit.com/"));
    headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_UA));

    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .post(url)
        .headers(headers)
        .send()
        .await?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if status != 200 {
        return Err(Error::Api {
            status,
            body: crate::util::truncate(&body, 200),
        });
    }
    let data: Value = serde_json::from_str::<Value>(&body)?["data"].take();
    let token = |kind: &str| data[kind]["token"].as_str().map(str::to_string);
    let expiry = |kind: &str| data[kind]["expired_at"].as_str().map(str::to_string);
    let (Some(access), Some(refresh)) = (token("access"), token("refresh")) else {
        return Err(Error::Config(
            "refresh response lacks an access or refresh token; the old refresh token may be \
             consumed, so re-seed"
                .into(),
        ));
    };
    let next = TokenStore {
        access,
        refresh,
        access_expired_at: expiry("access"),
        refresh_expired_at: expiry("refresh"),
        updated_at: now_rfc3339(),
    };
    next.save_to(store)?;
    Ok(next)
}

/// Exclusive lock on `<store>.lock`, held until the returned file drops.
async fn lock(store: &Path) -> Result<fs::File> {
    ensure_private_dir(store)?;
    let file = open_owner_only_keep(&store.with_extension("json.lock"))?;
    tokio::task::spawn_blocking(move || file.lock().map(|()| file))
        .await
        .map_err(|e| Error::Config(format!("token lock task failed: {e}")))?
        .map_err(Error::from)
}

/// Create the store's directory as 0700. An existing directory keeps its mode:
/// the store path can be overridden to a shared directory such as `/tmp`.
fn ensure_private_dir(store: &Path) -> Result<()> {
    let parent = store.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() && !parent.exists() {
        fs::create_dir_all(parent)?;
        set_owner_only_dir_mode(parent)?;
    }
    Ok(())
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let date = crate::yahoo::daily::unix_to_ymd(secs, 0);
    let s = secs.rem_euclid(86_400);
    format!("{date}T{:02}:{:02}:{:02}Z", s / 3600, s % 3600 / 60, s % 60)
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
fn open_owner_only_keep(path: &Path) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?)
}

#[cfg(not(unix))]
fn open_owner_only_keep(path: &Path) -> Result<fs::File> {
    Ok(fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?)
}

#[cfg(unix)]
fn set_owner_only_dir_mode(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_dir_mode(_dir: &Path) -> Result<()> {
    Ok(())
}
