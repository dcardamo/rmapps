use rmreader::cache::{key, Cache};
use std::time::{Duration, SystemTime};

fn tmp_cache() -> (tempfile::TempDir, Cache) {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path().to_path_buf(), true, 7);
    (dir, cache)
}

#[test]
fn key_is_sensitive_to_each_input() {
    let base = key("d1", "<p>hi</p>", 80_000, true);
    assert_ne!(base, key("d2", "<p>hi</p>", 80_000, true), "doc_id");
    assert_ne!(base, key("d1", "<p>HI</p>", 80_000, true), "html");
    assert_ne!(base, key("d1", "<p>hi</p>", 40_000, true), "max_bytes");
    assert_ne!(
        base,
        key("d1", "<p>hi</p>", 80_000, false),
        "images_enabled"
    );
    assert_eq!(base, key("d1", "<p>hi</p>", 80_000, true), "stable");
}

#[test]
fn put_then_get_roundtrips_html_and_assets() {
    let (_d, cache) = tmp_cache();
    let assets = vec![
        ("img-d1-0.png".to_string(), vec![1u8, 2, 3]),
        ("img-d1-1.jpg".to_string(), vec![9u8, 8, 7, 6]),
    ];
    cache.put("k1", "<p>body</p>", &assets);
    let got = cache.get("k1").expect("hit");
    assert_eq!(got.html, "<p>body</p>");
    assert_eq!(got.assets, assets);
}

#[test]
fn get_misses_when_disabled_or_absent() {
    let (_d, cache) = tmp_cache();
    assert!(cache.get("nope").is_none());

    let dir = tempfile::tempdir().unwrap();
    let disabled = Cache::new(dir.path().to_path_buf(), false, 7);
    disabled.put("k", "<p>x</p>", &[]);
    assert!(disabled.get("k").is_none(), "disabled cache never hits");
}

#[test]
fn get_touches_mtime() {
    let (d, cache) = tmp_cache();
    cache.put("k", "<p>x</p>", &[]);
    let meta = d.path().join("k").join("meta.json");
    // Backdate the entry well past any test runtime.
    let old = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 10);
    std::fs::File::open(&meta)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert!(cache.get("k").is_some());
    let mtime = std::fs::metadata(&meta).unwrap().modified().unwrap();
    assert!(
        SystemTime::now().duration_since(mtime).unwrap() < Duration::from_secs(60),
        "get should have refreshed the mtime to ~now"
    );
}

#[test]
fn sweep_removes_stale_keeps_fresh_and_cleans_junk() {
    let (d, cache) = tmp_cache();
    cache.put("fresh", "<p>f</p>", &[]);
    cache.put("stale", "<p>s</p>", &[]);
    // Backdate "stale" past the 7-day expiry.
    let old = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 8);
    std::fs::File::open(d.path().join("stale").join("meta.json"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    // A leftover temp dir and a partial entry (no meta.json).
    std::fs::create_dir_all(d.path().join(".tmp-junk")).unwrap();
    std::fs::create_dir_all(d.path().join("partial")).unwrap();

    cache.sweep();

    assert!(cache.get("fresh").is_some(), "fresh kept");
    assert!(!d.path().join("stale").exists(), "stale removed");
    assert!(!d.path().join(".tmp-junk").exists(), "temp removed");
    assert!(!d.path().join("partial").exists(), "partial removed");
}
