use rmreader::config::*;
use rmreader::content::{FetchedImage, ImageFetcher};
use rmreader::generate::generate;
use rmreader::readwise::{HttpResponse, HttpTransport};

struct FakeT;
impl HttpTransport for FakeT {
    fn get(&self, url: &str, _t: &str) -> anyhow::Result<HttpResponse> {
        let body = if url.contains("location=feed") {
            r#"{"nextPageCursor":null,"results":[]}"#.to_string()
        } else {
            r#"{"nextPageCursor":null,"results":[{"id":"a","title":"A","saved_at":"2026-01-01T00:00:00Z","html_content":"<p>hi <a href=\"http://x\">l</a></p>"}]}"#.to_string()
        };
        Ok(HttpResponse {
            status: 200,
            retry_after: None,
            body,
        })
    }
}
struct NoImages;
impl ImageFetcher for NoImages {
    fn fetch(&self, _u: &str) -> Option<FetchedImage> {
        None
    }
}

#[test]
fn generate_writes_pdfs_and_manifests() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config {
        device: "paper-pro-move".into(),
        output_dir: dir.path().to_str().unwrap().into(),
        theme: "reader".into(),
        readwise: ReadwiseConfig { token: "t".into() },
        library: LibraryConfig {
            locations: vec!["new".into()],
            max_items: 10,
        },
        feed: FeedConfig {
            enabled: true,
            max_items: 10,
        },
        images: ImagesConfig {
            enabled: false,
            timeout_secs: 8,
            concurrency: 12,
        },
        content: ContentConfig::default(),
        deploy: DeployConfig {
            backend: "none".into(),
            library_folder: String::new(),
            feed_folder: String::new(),
        },
        cache: CacheConfig {
            enabled: true,
            dir: Some(dir.path().join("cache").to_str().unwrap().to_string()),
            expiry_days: 7,
        },
    };
    let targets = generate(&cfg, &FakeT, &NoImages).unwrap();
    assert_eq!(targets.len(), 2);
    assert!(dir.path().join("Library.pdf").exists());
    assert!(dir.path().join("Library.manifest.json").exists());
    assert!(dir.path().join("Feed.pdf").exists());
}
