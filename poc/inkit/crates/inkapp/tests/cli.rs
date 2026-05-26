//! Parse + behavior tests for the operator CLI (`pair`, `secret`).

use clap::Parser;
use inkapp::cli::{run_op, OpCmd, ScopeArg, SecretCmd};
use inkapp_core::secrets::{Scope, SecretStore};

#[derive(Parser, Debug)]
#[command(name = "test-op")]
struct TestCli {
    #[command(subcommand)]
    op: OpCmd,
}

#[test]
fn parses_pair_code() {
    let cli = TestCli::try_parse_from(["test-op", "pair", "ABCD1234"]).unwrap();
    match cli.op {
        OpCmd::Pair { code } => assert_eq!(code, "ABCD1234"),
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn parses_secret_set_connector() {
    let cli = TestCli::try_parse_from([
        "test-op",
        "secret",
        "set",
        "connector",
        "readwise-reader",
        "rw-token-xyz",
    ])
    .unwrap();
    match cli.op {
        OpCmd::Secret(SecretCmd::Set { scope, name, value }) => {
            assert!(matches!(scope, ScopeArg::Connector));
            assert_eq!(name, "readwise-reader");
            assert_eq!(value, "rw-token-xyz");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn parses_secret_set_device_auth() {
    let cli = TestCli::try_parse_from([
        "test-op",
        "secret",
        "set",
        "device-auth",
        "remarkable",
        "tok",
    ])
    .unwrap();
    assert!(matches!(
        cli.op,
        OpCmd::Secret(SecretCmd::Set {
            scope: ScopeArg::DeviceAuth,
            ..
        })
    ));
}

#[tokio::test]
async fn secret_set_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    run_op(
        OpCmd::Secret(SecretCmd::Set {
            scope: ScopeArg::Connector,
            name: "readwise-reader".into(),
            value: "tok".into(),
        }),
        path.clone(),
    )
    .await
    .unwrap();

    let s = SecretStore::open(&path).unwrap();
    assert_eq!(
        s.get(Scope::ConnectorCred, "readwise-reader")
            .unwrap()
            .unwrap(),
        b"tok"
    );
}

#[tokio::test]
async fn secret_list_returns_zero_after_set() {
    // We don't capture stdout here — just confirm exit code and that List
    // doesn't error on a populated store.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    run_op(
        OpCmd::Secret(SecretCmd::Set {
            scope: ScopeArg::DeviceAuth,
            name: "remarkable".into(),
            value: "tok".into(),
        }),
        path.clone(),
    )
    .await
    .unwrap();
    let code = run_op(OpCmd::Secret(SecretCmd::List), path).await.unwrap();
    assert_eq!(code, 0);
}
