//! Rotating token store (`~/.stockbit-cli/tokens.json`): precedence in the CLI's
//! token resolution, owner-only atomic writes, and the single-rotator refresh.

use std::sync::Arc;

use serde_json::json;
use stockbit_cli::config::Config;
use stockbit_cli::error::Error;
use stockbit_cli::stored_config::StoredConfig;
use stockbit_cli::token_store::{self, TokenStore};
use tempfile::tempdir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn stored(token: &str) -> StoredConfig {
    StoredConfig {
        token: Some(token.into()),
        ..Default::default()
    }
}

#[test]
fn shared_store_beats_config_yaml() {
    let cfg = Config::merge_with_shared(
        None,
        None,
        None,
        Some("from-store".into()),
        stored("from-yaml"),
    )
    .unwrap();
    assert_eq!(cfg.token, "from-store");
}

#[test]
fn env_beats_shared_store_and_flag_beats_env() {
    let env = Config::merge_with_shared(
        None,
        None,
        Some("from-env".into()),
        Some("from-store".into()),
        stored("from-yaml"),
    )
    .unwrap();
    assert_eq!(env.token, "from-env");
    let flag = Config::merge_with_shared(
        Some("from-flag".into()),
        None,
        Some("from-env".into()),
        Some("from-store".into()),
        stored("from-yaml"),
    )
    .unwrap();
    assert_eq!(flag.token, "from-flag");
}

#[test]
fn blank_shared_store_token_falls_through_to_yaml() {
    let cfg = Config::merge_with_shared(None, None, None, Some("  ".into()), stored("from-yaml"))
        .unwrap();
    assert_eq!(cfg.token, "from-yaml");
}

fn sample(access: &str, refresh: &str) -> TokenStore {
    TokenStore {
        access: access.into(),
        refresh: refresh.into(),
        access_expired_at: Some("2026-11-12T00:00:00Z".into()),
        refresh_expired_at: Some("2026-12-11T00:00:00Z".into()),
        updated_at: "2026-11-11T00:00:00Z".into(),
    }
}

#[test]
fn store_roundtrips_with_owner_only_mode_and_no_temp_left_behind() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sub").join("tokens.json");
    sample("A1", "R1").save_to(&path).unwrap();
    sample("A2", "R2").save_to(&path).unwrap();
    assert_eq!(
        TokenStore::load_from(&path).unwrap(),
        Some(sample("A2", "R2"))
    );
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n != "tokens.json")
        .collect();
    assert!(leftovers.is_empty(), "leftover files: {leftovers:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn missing_store_loads_as_none() {
    let dir = tempdir().unwrap();
    assert_eq!(
        TokenStore::load_from(&dir.path().join("nope.json")).unwrap(),
        None
    );
}

#[test]
fn debug_output_redacts_both_tokens() {
    let shown = format!("{:?}", sample("secret-access", "secret-refresh"));
    assert!(!shown.contains("secret-access") && !shown.contains("secret-refresh"));
}

#[test]
fn status_reports_expiry_without_tokens() {
    let status = sample("secret-access", "secret-refresh").status();
    assert_eq!(
        status,
        json!({
            "access_expired_at": "2026-11-12T00:00:00Z",
            "refresh_expired_at": "2026-12-11T00:00:00Z",
            "updated_at": "2026-11-11T00:00:00Z",
        })
    );
}

fn rotated(access: &str, refresh: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "data": {
            "access": {"token": access, "expired_at": "2026-11-12T00:00:00Z"},
            "refresh": {"token": refresh, "expired_at": "2026-12-11T00:00:00Z"},
        }
    }))
}

#[tokio::test]
async fn seed_rotates_the_seed_and_persists_the_new_pair() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/refresh"))
        .and(header("authorization", "Bearer SEED"))
        .respond_with(rotated("A1", "R1"))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempdir().unwrap();
    let store = dir.path().join("tokens.json");
    let got = token_store::seed(&store, &server.uri(), "SEED")
        .await
        .unwrap();
    assert_eq!((got.access.as_str(), got.refresh.as_str()), ("A1", "R1"));
    let saved = TokenStore::load_from(&store).unwrap().unwrap();
    assert_eq!(
        (saved.access.as_str(), saved.refresh.as_str()),
        ("A1", "R1")
    );
}

#[tokio::test]
async fn refresh_without_a_store_is_a_config_error() {
    let dir = tempdir().unwrap();
    let err = token_store::refresh(&dir.path().join("tokens.json"), "http://127.0.0.1:9")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Config(_)), "{err:?}");
}

#[tokio::test]
async fn rejected_refresh_keeps_the_old_store_and_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/refresh"))
        .respond_with(ResponseTemplate::new(500).set_body_string("down"))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempdir().unwrap();
    let store = dir.path().join("tokens.json");
    sample("A1", "R1").save_to(&store).unwrap();
    let err = token_store::refresh(&store, &server.uri())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Api { status: 500, .. }), "{err:?}");
    assert_eq!(
        TokenStore::load_from(&store).unwrap(),
        Some(sample("A1", "R1"))
    );
}

#[tokio::test]
async fn concurrent_refreshes_rotate_in_sequence_and_never_reuse_a_refresh_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/refresh"))
        .and(header("authorization", "Bearer R1"))
        .respond_with(rotated("A2", "R2"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/login/refresh"))
        .and(header("authorization", "Bearer R2"))
        .respond_with(rotated("A3", "R3"))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempdir().unwrap();
    let store = Arc::new(dir.path().join("tokens.json"));
    sample("A1", "R1").save_to(&store).unwrap();

    let uri = server.uri();
    let a = tokio::spawn({
        let (store, uri) = (store.clone(), uri.clone());
        async move { token_store::refresh(&store, &uri).await }
    });
    let b = tokio::spawn({
        let (store, uri) = (store.clone(), uri.clone());
        async move { token_store::refresh(&store, &uri).await }
    });
    a.await.unwrap().unwrap();
    b.await.unwrap().unwrap();
    let saved = TokenStore::load_from(&store).unwrap().unwrap();
    assert_eq!(
        (saved.access.as_str(), saved.refresh.as_str()),
        ("A3", "R3")
    );
}
