//! Pure precedence merger and Debug-redaction. Tests use `Config::merge` directly
//! with explicit inputs so they never touch the process environment — the binary
//! entry point `Config::resolve` is the only thing that reads env/dotenv/file.

use stockbit_cli::config::{Config, DEFAULT_BASE_URL};
use stockbit_cli::stored_config::StoredConfig;

#[test]
fn cli_token_beats_env_and_stored() {
    let stored = StoredConfig {
        token: Some("from-stored".into()),
        ..Default::default()
    };
    let cfg = Config::merge(
        Some("from-cli".into()),
        None,
        Some("from-env".into()),
        stored,
    )
    .unwrap();
    assert_eq!(cfg.token, "from-cli");
}

#[test]
fn env_token_beats_stored_when_cli_unset() {
    let stored = StoredConfig {
        token: Some("from-stored".into()),
        ..Default::default()
    };
    let cfg = Config::merge(None, None, Some("from-env".into()), stored).unwrap();
    assert_eq!(cfg.token, "from-env");
}

#[test]
fn stored_token_used_when_cli_and_env_unset() {
    let stored = StoredConfig {
        token: Some("from-stored".into()),
        ..Default::default()
    };
    let cfg = Config::merge(None, None, None, stored).unwrap();
    assert_eq!(cfg.token, "from-stored");
}

#[test]
fn whitespace_only_token_treated_as_missing() {
    let err = Config::merge(None, None, Some("   ".into()), StoredConfig::default()).unwrap_err();
    assert!(matches!(err, stockbit_cli::error::Error::MissingToken));
}

#[test]
fn missing_everywhere_errors() {
    let err = Config::merge(None, None, None, StoredConfig::default()).unwrap_err();
    assert!(matches!(err, stockbit_cli::error::Error::MissingToken));
}

#[test]
fn cli_base_url_beats_stored() {
    let stored = StoredConfig {
        base_url: Some("https://stored.invalid".into()),
        ..Default::default()
    };
    let cfg = Config::merge(
        Some("t".into()),
        Some("https://cli.invalid".into()),
        None,
        stored,
    )
    .unwrap();
    assert_eq!(cfg.base_url, "https://cli.invalid");
}

#[test]
fn stored_base_url_used_when_cli_unset() {
    let stored = StoredConfig {
        token: Some("t".into()),
        base_url: Some("https://stored.invalid".into()),
    };
    let cfg = Config::merge(None, None, None, stored).unwrap();
    assert_eq!(cfg.base_url, "https://stored.invalid");
}

#[test]
fn default_base_url_used_when_nothing_set() {
    let cfg = Config::merge(Some("t".into()), None, None, StoredConfig::default()).unwrap();
    assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
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
