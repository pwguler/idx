use std::env;
use std::fmt;

use crate::error::{Error, Result};
use crate::stored_config::StoredConfig;
use crate::token_store::TokenStore;

pub const DEFAULT_BASE_URL: &str = "https://exodus.stockbit.com";
pub const TOKEN_ENV: &str = "STOCKBIT_BEARER_TOKEN";
pub const USER_AGENT: &str = concat!("stockbit-cli/", env!("CARGO_PKG_VERSION"));

/// Resolved runtime configuration for the Stockbit client.
#[derive(Clone)]
pub struct Config {
    pub base_url: String,
    pub token: String,
    pub user_agent: String,
    pub request_timeout: std::time::Duration,
    pub max_retries: u32,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never leak the bearer token in panic messages, tracing spans, or debug logs.
        f.debug_struct("Config")
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .field("user_agent", &self.user_agent)
            .field("request_timeout", &self.request_timeout)
            .field("max_retries", &self.max_retries)
            .finish()
    }
}

impl Config {
    /// Convenience entry point used by the binary: loads `.env`, reads the env var,
    /// the shared token store, and the stored YAML file, then delegates to
    /// [`Config::merge_with_shared`]. Tests reach
    /// for `merge` directly so they can exercise precedence without touching the
    /// process environment.
    /// # Errors
    ///
    /// Returns [`Error::MissingToken`] when no source provides a non-empty token.
    pub fn resolve(cli_token: Option<String>, cli_base_url: Option<String>) -> Result<Self> {
        let _ = dotenvy::dotenv();
        let env_token = env::var(TOKEN_ENV).ok();
        let shared = TokenStore::default_path()
            .ok()
            .and_then(|p| TokenStore::load_from(&p).ok().flatten())
            .map(|s| s.access);
        let stored = StoredConfig::load().unwrap_or_default();
        Self::merge_with_shared(cli_token, cli_base_url, env_token, shared, stored)
    }

    /// Pure precedence merger. Highest priority first:
    ///   1. CLI flag (`cli_token`, `cli_base_url`)
    ///   2. Environment value (`env_token`)
    ///   3. On-disk stored config (`stored`)
    ///   4. Defaults
    ///
    /// Whitespace-only tokens are treated as missing.
    /// # Errors
    ///
    /// Returns [`Error::MissingToken`] when no source provides a non-empty token.
    pub fn merge(
        cli_token: Option<String>,
        cli_base_url: Option<String>,
        env_token: Option<String>,
        stored: StoredConfig,
    ) -> Result<Self> {
        Self::merge_with_shared(cli_token, cli_base_url, env_token, None, stored)
    }

    /// [`Config::merge`] with the rotating token store between env and the YAML
    /// file: flag > env > shared store > `config.yaml`. A blank store token falls
    /// through to the file.
    /// # Errors
    ///
    /// Returns [`Error::MissingToken`] when no source provides a non-empty token.
    pub fn merge_with_shared(
        cli_token: Option<String>,
        cli_base_url: Option<String>,
        env_token: Option<String>,
        shared_token: Option<String>,
        stored: StoredConfig,
    ) -> Result<Self> {
        let token = cli_token
            .or(env_token)
            .or(shared_token.filter(|t| !t.trim().is_empty()))
            .or(stored.token)
            .filter(|t| !t.trim().is_empty())
            .ok_or(Error::MissingToken)?;

        let base_url = cli_base_url
            .or(stored.base_url)
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        Ok(Self {
            base_url,
            token,
            user_agent: USER_AGENT.to_string(),
            request_timeout: std::time::Duration::from_secs(30),
            max_retries: 3,
        })
    }

    /// Construct a config for tests bypassing env lookup.
    #[must_use]
    pub fn for_test(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: token.into(),
            user_agent: USER_AGENT.to_string(),
            request_timeout: std::time::Duration::from_secs(5),
            max_retries: 2,
        }
    }
}
