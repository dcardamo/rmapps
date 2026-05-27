use inkapp_harness::session::Session;
use tempfile::tempdir;

#[tokio::test]
async fn new_creates_dir_and_session_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s1");

    let s = Session::new_fake(&path).await.unwrap();
    assert!(path.join("session.json").exists());
    assert!(!s.id().is_empty());
    assert_eq!(s.backend(), "fake");
}

#[tokio::test]
async fn open_rehydrates_existing_session() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s1");

    let original_id = {
        let s = Session::new_fake(&path).await.unwrap();
        s.flush().unwrap();
        s.id().to_string()
    };

    let s = Session::open(&path).await.unwrap();
    assert_eq!(s.id(), original_id);
}

#[tokio::test]
async fn second_open_fails_while_first_alive() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s1");

    let _s1 = Session::new_fake(&path).await.unwrap();
    let err = Session::open(&path).await;
    assert!(err.is_err(), "expected lock contention error");
}

#[tokio::test]
async fn session_devices_add_and_list() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s");
    let mut s = Session::new_fake(&path).await.unwrap();

    let d1 = s.device_new(Some("primary")).unwrap();
    let d2 = s.device_new(None).unwrap();
    assert_eq!(d1.as_str(), "dev-1");
    assert_eq!(d2.as_str(), "dev-2");

    let listed: Vec<String> = s.device_list().unwrap().into_iter().map(|d| d.id).collect();
    assert_eq!(listed, vec!["dev-1", "dev-2"]);
}

#[tokio::test]
async fn destroy_removes_dir() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s1");
    {
        let _s = Session::new_fake(&path).await.unwrap();
    }
    Session::destroy(&path).unwrap();
    assert!(!path.exists());
}
