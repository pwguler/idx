use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("missing bearer token: set STOCKBIT_BEARER_TOKEN or pass --token")]
    MissingToken,

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Stockbit API returned status {status}: {body}")]
    Api { status: u16, body: String },

    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),

    #[error("invalid date {input:?}: expected YYYY-MM-DD")]
    InvalidDate { input: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("config error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// True for transient failures worth retrying (5xx, 429, network-level reqwest errors).
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Api { status, .. } => *status == 429 || (500..600).contains(status),
            Self::Http(e) => e.is_timeout() || e.is_connect() || e.is_request(),
            _ => false,
        }
    }
}
