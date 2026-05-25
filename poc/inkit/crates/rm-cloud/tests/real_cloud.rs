//! Live-cloud tests. Gated by `RM_CLOUD_DEVICE_TOKEN` and `#[ignore]` so they never run
//! by default. All work happens inside `rmrs-test/<run-id>` so parallel runs (and other
//! contributors) never collide. The run folder is deleted on success, kept on failure.
//!
//! Run live with:
//!   RM_CLOUD_DEVICE_TOKEN=… nix develop -c cargo test -p rm-cloud --features fake \
//!       -- --ignored real_cloud_lifecycle

use rm_cloud::{Client, Config, DocFiles, Metadata, Result};
use uuid::Uuid;

const ROOT_TEST_DIR: &str = "rmrs-test";

/// Build a device-token client from the env, or print a skip notice and return `None`.
fn client_or_skip() -> Option<Client> {
    match std::env::var("RM_CLOUD_DEVICE_TOKEN") {
        Ok(t) if !t.is_empty() => Some(Client::from_device_token(Config::from_env(), t)),
        _ => {
            eprintln!("skipping real-cloud test: RM_CLOUD_DEVICE_TOKEN unset");
            None
        }
    }
}

/// Find folder `name` under `parent`, creating it if absent (reMarkable allows duplicate
/// folder names, so we resolve-then-create rather than blindly `mkdir`). Returns its id.
async fn get_or_create_folder(client: &Client, name: &str, parent: &str) -> Result<String> {
    if let Some(e) = client
        .ls(parent)
        .await?
        .into_iter()
        .find(|e| e.is_folder && e.name == name)
    {
        return Ok(e.id);
    }
    client.mkdir(name, parent).await
}

#[tokio::test]
#[ignore = "hits the live reMarkable cloud; needs RM_CLOUD_DEVICE_TOKEN"]
async fn real_cloud_lifecycle() {
    let Some(client) = client_or_skip() else {
        return;
    };

    // Unique isolation folder for this run, under a shared rmrs-test root.
    let run_id = Uuid::new_v4().to_string();
    let base = get_or_create_folder(&client, ROOT_TEST_DIR, "")
        .await
        .expect("get/create test root folder");
    let run_folder = client.mkdir(&run_id, &base).await.expect("mk run folder");

    // Run the body so we can implement leave-on-failure: clean up only on Ok.
    let result = real_lifecycle_body(&client, &run_folder).await;

    match result {
        Ok(()) => {
            client
                .rm(&run_folder)
                .await
                .expect("cleanup run folder on success");
        }
        Err(e) => {
            eprintln!(
                "real_cloud_lifecycle FAILED; leaving {ROOT_TEST_DIR}/{run_id} for inspection: {e}"
            );
            panic!("real_cloud_lifecycle failed: {e}");
        }
    }
}

async fn real_lifecycle_body(client: &Client, run_folder: &str) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    let meta = Metadata {
        visible_name: "rm-cloud-it".into(),
        doc_type: "DocumentType".into(),
        parent: run_folder.into(),
        last_modified: "0".into(),
        deleted: false,
        extra: Default::default(),
    };
    let df = DocFiles {
        id: id.clone(),
        files: vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
            (format!("{id}.content"), b"{}".to_vec()),
            (format!("{id}.pdf"), b"%PDF-1.4 test".to_vec()),
        ],
    };
    client.put(df).await?;

    let listing = client.ls(run_folder).await?;
    assert!(
        listing.iter().any(|e| e.id == id),
        "uploaded doc should be listed"
    );

    let got = client.get(&id).await?;
    assert_eq!(got.get(&format!("{id}.pdf")).unwrap(), b"%PDF-1.4 test");

    client
        .put_content_only(&id, b"%PDF-1.4 updated".to_vec())
        .await?;
    let got = client.get(&id).await?;
    assert_eq!(got.get(&format!("{id}.pdf")).unwrap(), b"%PDF-1.4 updated");

    client.rm(&id).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "destructive: removes all rmrs-test/* folders from the live account"]
async fn sweep_stale_test_folders() {
    let Some(client) = client_or_skip() else {
        return;
    };
    let Some(base) = client
        .ls("")
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|e| e.is_folder && e.name == ROOT_TEST_DIR)
        .map(|e| e.id)
    else {
        eprintln!("no {ROOT_TEST_DIR} folder; nothing to sweep");
        return;
    };
    for entry in client.ls(&base).await.expect("ls test root") {
        eprintln!("sweeping {ROOT_TEST_DIR}/{}", entry.name);
        client.rm(&entry.id).await.expect("rm stale run folder");
    }
}
