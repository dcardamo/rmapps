//! Credential resolution: store-wins-over-env, env-fallback, missing-both, user-token-only.

use inkapp_core::secrets::{Scope, SecretStore};
use rm_cloud::Error as CloudError;
use rm_device::auth::{resolve_with, REMARKABLE_DEVICE_AUTH_NAME};

fn tmp_store() -> (tempfile::TempDir, SecretStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    (dir, store)
}

#[test]
fn resolve_store_wins_over_env() {
    let (_d, mut s) = tmp_store();
    s.set(
        Scope::DeviceAuth,
        REMARKABLE_DEVICE_AUTH_NAME,
        b"from-store",
    )
    .unwrap();
    let env = |k: &str| match k {
        "RM_CLOUD_DEVICE_TOKEN" => Some("from-env".into()),
        _ => None,
    };
    let creds = resolve_with(&s, env).unwrap();
    assert_eq!(creds.device_token.as_deref(), Some("from-store"));
    assert!(creds.user_token.is_none());
}

#[test]
fn resolve_falls_back_to_env_device_token() {
    let (_d, s) = tmp_store();
    let env = |k: &str| match k {
        "RM_CLOUD_DEVICE_TOKEN" => Some("from-env".into()),
        "RM_CLOUD_USER_TOKEN" => Some("user-from-env".into()),
        _ => None,
    };
    let creds = resolve_with(&s, env).unwrap();
    assert_eq!(creds.device_token.as_deref(), Some("from-env"));
    assert_eq!(creds.user_token.as_deref(), Some("user-from-env"));
}

#[test]
fn resolve_user_token_only_is_ok() {
    let (_d, s) = tmp_store();
    let env = |k: &str| match k {
        "RM_CLOUD_USER_TOKEN" => Some("just-user".into()),
        _ => None,
    };
    let creds = resolve_with(&s, env).unwrap();
    assert!(creds.device_token.is_none());
    assert_eq!(creds.user_token.as_deref(), Some("just-user"));
}

#[test]
fn resolve_missing_both_is_error() {
    let (_d, s) = tmp_store();
    let env = |_: &str| None;
    let err = resolve_with(&s, env).unwrap_err();
    assert!(matches!(err, CloudError::MissingCredential(_)));
}

#[test]
fn resolve_empty_strings_are_absent() {
    let (_d, mut s) = tmp_store();
    s.set(Scope::DeviceAuth, REMARKABLE_DEVICE_AUTH_NAME, b"")
        .unwrap();
    let env = |k: &str| match k {
        "RM_CLOUD_DEVICE_TOKEN" => Some("".into()),
        "RM_CLOUD_USER_TOKEN" => Some("".into()),
        _ => None,
    };
    let err = resolve_with(&s, env).unwrap_err();
    assert!(matches!(err, CloudError::MissingCredential(_)));
}

use inkapp_core::secrets::SecretStore as Store;
use rm_cloud::fake::FakeCloud;
use rm_cloud::Config;
use rm_device::auth::pair;

#[tokio::test]
async fn pair_stores_device_token_and_survives_reopen() {
    let cloud = FakeCloud::spawn().await;
    let config = Config::single_host(&cloud.base);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");

    {
        let mut secrets = Store::open(&path).unwrap();
        pair(&mut secrets, &config, "ABCD1234").await.unwrap();
        assert_eq!(
            secrets
                .get(
                    inkapp_core::secrets::Scope::DeviceAuth,
                    rm_device::auth::REMARKABLE_DEVICE_AUTH_NAME,
                )
                .unwrap()
                .unwrap(),
            b"device-token-for-ABCD1234"
        );
    }

    // Reopen from disk — same value.
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened
            .get(
                inkapp_core::secrets::Scope::DeviceAuth,
                rm_device::auth::REMARKABLE_DEVICE_AUTH_NAME,
            )
            .unwrap()
            .unwrap(),
        b"device-token-for-ABCD1234"
    );
}

#[tokio::test]
async fn pair_overwrites_previous_token() {
    let cloud = FakeCloud::spawn().await;
    let config = Config::single_host(&cloud.base);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    let mut secrets = Store::open(&path).unwrap();
    pair(&mut secrets, &config, "FIRST123").await.unwrap();
    pair(&mut secrets, &config, "SECOND12").await.unwrap();
    assert_eq!(
        secrets
            .get(
                inkapp_core::secrets::Scope::DeviceAuth,
                rm_device::auth::REMARKABLE_DEVICE_AUTH_NAME,
            )
            .unwrap()
            .unwrap(),
        b"device-token-for-SECOND12"
    );
}

use rm_device::CloudTransport;

#[tokio::test]
async fn from_secrets_succeeds_with_stored_device_token() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    let mut secrets = Store::open(&path).unwrap();
    secrets
        .set(
            inkapp_core::secrets::Scope::DeviceAuth,
            rm_device::auth::REMARKABLE_DEVICE_AUTH_NAME,
            b"stored-tok",
        )
        .unwrap();
    // Clear env so this test is independent of host shell.
    std::env::remove_var("RM_CLOUD_DEVICE_TOKEN");
    std::env::remove_var("RM_CLOUD_USER_TOKEN");
    let t = CloudTransport::from_secrets(&secrets, "/X");
    assert!(
        t.is_ok(),
        "expected Ok, got {:?}",
        t.err().map(|e| e.to_string())
    );
}

#[test]
fn from_secrets_errors_when_nothing_set() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    let secrets = Store::open(&path).unwrap();
    std::env::remove_var("RM_CLOUD_DEVICE_TOKEN");
    std::env::remove_var("RM_CLOUD_USER_TOKEN");
    match CloudTransport::from_secrets(&secrets, "/X") {
        Ok(_) => panic!("expected an error but got Ok"),
        Err(e) => assert!(
            e.to_string().contains("rm-cloud") || e.to_string().contains("credential"),
            "unexpected error: {e}"
        ),
    }
}
