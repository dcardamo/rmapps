#![cfg(feature = "fake")]

use rm_cloud::fake::FakeCloud;
use tempfile::tempdir;

#[tokio::test]
async fn round_trip_state_through_disk() {
    let dir = tempdir().unwrap();

    let cloud_a = FakeCloud::spawn().await;
    {
        let mut s = cloud_a.state.lock().unwrap();
        s.blobs.insert("abc".into(), b"hello".to_vec());
        s.root_hash = "abc".into();
        s.generation = 1;
    }
    cloud_a.dump_to_dir(dir.path()).unwrap();
    drop(cloud_a);

    let cloud_b = FakeCloud::from_dir(dir.path()).await.unwrap();
    let s = cloud_b.state.lock().unwrap();
    assert_eq!(
        s.blobs.get("abc").map(|v| v.as_slice()),
        Some(b"hello".as_slice())
    );
    assert_eq!(s.root_hash, "abc");
    assert_eq!(s.generation, 1);
}

#[tokio::test]
async fn from_dir_empty_when_missing() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("does-not-exist");
    let cloud = FakeCloud::from_dir(&missing).await.unwrap();
    let s = cloud.state.lock().unwrap();
    assert!(s.blobs.is_empty());
    assert_eq!(s.root_hash, "");
    assert_eq!(s.generation, 0);
}
