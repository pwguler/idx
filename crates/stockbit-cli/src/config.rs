use std::env;
use std::fmt;

use crate::error::{Error, Result};
use crate::stored_config::StoredConfig;

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
    /// Resolve the runtime config. Precedence (highest first):
    ///   1. CLI flag (`cli_token`, `base_url`)
    ///   2. Environment variable (`STOCKBIT_BEARER_TOKEN`, loaded from `.env` if present)
    ///   3. On-disk config at `~/.stockbit-cli/config.yaml`
    ///   4. Defaults
    pub fn resolve(cli_token: Option<String>, base_url: Option<String>) -> Result<Self> {
        let _ = dotenvy::dotenv();
        let stored = StoredConfig::load().unwrap_or_default();

        let token = cli_token
            .or_else(|| env::var(TOKEN_ENV).ok())
            .or(stored.token)
            .filter(|t| !t.trim().is_empty())
            .ok_or(Error::MissingToken)?;

        let base_url = base_url
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
