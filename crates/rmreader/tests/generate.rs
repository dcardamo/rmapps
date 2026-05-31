use rmreader::config::*;
use rmreader::content::{FetchedImage, ImageFetcher};
use rmreader::generate::generate;
use rmreader::readwise::{HttpMethod, HttpResponse, HttpTransport};

struct FakeT;
impl HttpTransport for FakeT {
    fn request(
        &self,
        _method: HttpMethod,
        url: &str,
        _token: &str,
        _body: Option<&str>,
    ) -> anyhow::Result<HttpResponse> {
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

#[test]
fn cold_and_warm_runs_produce_identical_pdfs() {
    // Same fake inputs, cache enabled at a temp dir. Run twice; the second run is
    // fully warm (cache hits). PDFs must be byte-identical (rendering is
    // deterministic), proving the cache is transparent.
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let mk_cfg = |out: &std::path::Path| Config {
        device: "paper-pro-move".into(),
        output_dir: out.to_str().unwrap().into(),
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
            dir: Some(cache_dir.to_str().unwrap().into()),
            expiry_days: 7,
        },
    };

    let cold = dir.path().join("cold");
    std::fs::create_dir_all(&cold).unwrap();
    generate(&mk_cfg(&cold), &FakeT, &NoImages).unwrap();

    let warm = dir.path().join("warm");
    std::fs::create_dir_all(&warm).unwrap();
    generate(&mk_cfg(&warm), &FakeT, &NoImages).unwrap();

    for name in ["Library.pdf", "Feed.pdf"] {
        let a = std::fs::read(cold.join(name)).unwrap();
        let b = std::fs::read(warm.join(name)).unwrap();
        assert_eq!(a, b, "{name}: cold and warm output differ");
    }
}

fn png_8x8() -> Vec<u8> {
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

/// Library doc carries one `<img>`; feed is empty.
struct FakeTImg;
impl HttpTransport for FakeTImg {
    fn request(
        &self,
        _method: HttpMethod,
        url: &str,
        _token: &str,
        _body: Option<&str>,
    ) -> anyhow::Result<HttpResponse> {
        let body = if url.contains("location=feed") {
            r#"{"nextPageCursor":null,"results":[]}"#.to_string()
        } else {
            r#"{"nextPageCursor":null,"results":[{"id":"a","title":"A","saved_at":"2026-01-01T00:00:00Z","html_content":"<p>pic <img src=\"https://x/p.png\"></p>"}]}"#.to_string()
        };
        Ok(HttpResponse {
            status: 200,
            retry_after: None,
            body,
        })
    }
}

struct OneImage;
impl ImageFetcher for OneImage {
    fn fetch(&self, url: &str) -> Option<FetchedImage> {
        if url == "https://x/p.png" {
            Some(FetchedImage {
                bytes: png_8x8(),
                ext: "png".into(),
            })
        } else {
            None
        }
    }
}

#[test]
fn cold_and_warm_runs_identical_with_images() {
    // Like cold_and_warm but with images ENABLED and a real fetched PNG, so the
    // image-blob cache round-trip (cache.put bytes -> warm cache.get bytes ->
    // render) is exercised end-to-end and proven byte-transparent.
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let mk_cfg = |out: &std::path::Path| Config {
        device: "paper-pro-move".into(),
        output_dir: out.to_str().unwrap().into(),
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
            enabled: true,
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
            dir: Some(cache_dir.to_str().unwrap().into()),
            expiry_days: 7,
        },
    };

    let cold = dir.path().join("cold");
    std::fs::create_dir_all(&cold).unwrap();
    generate(&mk_cfg(&cold), &FakeTImg, &OneImage).unwrap();

    let warm = dir.path().join("warm");
    std::fs::create_dir_all(&warm).unwrap();
    generate(&mk_cfg(&warm), &FakeTImg, &OneImage).unwrap();

    let a = std::fs::read(cold.join("Library.pdf")).unwrap();
    let b = std::fs::read(warm.join("Library.pdf")).unwrap();
    assert_eq!(a, b, "Library.pdf: cold and warm differ with images");

    // The normalized image blob must actually be cached (proves the blob round-trip).
    let mut found_png = false;
    for e in std::fs::read_dir(&cache_dir).unwrap().flatten() {
        if e.path().is_dir() {
            for f in std::fs::read_dir(e.path()).unwrap().flatten() {
                if f.file_name().to_string_lossy().ends_with(".png") {
                    found_png = true;
                }
            }
        }
    }
    assert!(found_png, "normalized image blob should be cached");
}
