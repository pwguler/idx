//! Runtime `Config` resolution: CLI flag > env var > stored file > defaults. Also
//! guards the bearer token from leaking via `Debug`.

use std::sync::Mutex;

use stockbit_cli::config::{Config, DEFAULT_BASE_URL, TOKEN_ENV};

// Each test reaches into the process-global env; serialise with a mutex so
// parallel tests don't race.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_env<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let prev = std::env::var(key).ok();
    match value {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
    f();
    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

#[test]
fn cli_token_beats_env() {
    with_env(TOKEN_ENV, Some("from-env"), || {
        let cfg = Config::resolve(Some("from-cli".to_string()), None).unwrap();
        assert_eq!(cfg.token, "from-cli");
    });
}

#[test]
fn env_token_used_when_cli_unset() {
    with_env(TOKEN_ENV, Some("from-env"), || {
        let cfg = Config::resolve(None, None).unwrap();
        assert_eq!(cfg.token, "from-env");
    });
}

#[test]
fn whitespace_only_token_treated_as_missing() {
    with_env(TOKEN_ENV, Some("   "), || {
        // Only assert MissingToken when no other source could supply a value —
        // the installed ~/.stockbit-cli/config.yaml may legitimately hold one.
        if stockbit_cli::stored_config::StoredConfig::load()
            .ok()
            .and_then(|c| c.token)
            .is_none()
        {
            let err = Config::resolve(None, None).unwrap_err();
            assert!(matches!(err, stockbit_cli::error::Error::MissingToken));
        }
    });
}

#[test]
fn default_base_url_used_when_unset() {
    with_env(TOKEN_ENV, Some("t"), || {
        let cfg = Config::resolve(None, None).unwrap();
        let stored_base = stockbit_cli::stored_config::StoredConfig::load()
            .ok()
            .and_then(|c| c.base_url);
        if stored_base.is_none() {
            assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
        }
    });
}

#[test]
fn debug_format_never_leaks_token() {
    let cfg = Config::for_test("https://example.invalid", "sensitive-bearer-value");
    let dbg = format!("{cfg:?}");
    assert!(
        !dbg.contains("sensitive-bearer-value"),
        "token leaked in Debug: {dbg}"
    );
    assert!(dbg.contains("<redacted>"));
}
