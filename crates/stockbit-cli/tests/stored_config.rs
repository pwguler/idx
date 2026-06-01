//! YAML config file (`~/.stockbit-cli/config.yaml`): load/save, perms, redaction.

use stockbit_cli::error::Error;
use stockbit_cli::stored_config::StoredConfig;
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
