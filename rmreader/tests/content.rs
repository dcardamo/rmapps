use rmreader::content::{process_html, FetchedImage, ImageFetcher};
use std::cell::RefCell;

struct FakeFetcher {
    map: std::collections::HashMap<String, Option<FetchedImage>>,
    fetched: RefCell<Vec<String>>,
}
impl ImageFetcher for FakeFetcher {
    fn fetch(&self, url: &str) -> Option<FetchedImage> {
        self.fetched.borrow_mut().push(url.to_string());
        self.map.get(url).cloned().flatten()
    }
}

fn png_1x1() -> Vec<u8> {
    // minimal valid 1x1 PNG
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
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

#[test]
fn strips_scripts_and_handlers_keeps_text() {
    let html = r#"<p onclick="x()">Hello</p><script>evil()</script><iframe src="z"></iframe>"#;
    let f = FakeFetcher {
        map: Default::default(),
        fetched: RefCell::new(vec![]),
    };
    let out = process_html(html, "d1", true, &f);
    assert!(out.html.contains("Hello"));
    assert!(!out.html.contains("script"));
    assert!(!out.html.contains("iframe"));
    assert!(!out.html.contains("onclick"));
}

#[test]
fn embeds_real_image_and_rewrites_src() {
    let mut map = std::collections::HashMap::new();
    map.insert(
        "https://x/p.png".to_string(),
        Some(FetchedImage {
            bytes: png_8x8(),
            ext: "png".into(),
        }),
    );
    let f = FakeFetcher {
        map,
        fetched: RefCell::new(vec![]),
    };
    let out = process_html(r#"<p><img src="https://x/p.png"></p>"#, "d1", true, &f);
    assert_eq!(out.assets.len(), 1);
    let key = &out.assets[0].0;
    assert!(out.html.contains(key));
    assert!(!out.html.contains("https://x/p.png"));
}

#[test]
fn drops_tracking_pixel() {
    let mut map = std::collections::HashMap::new();
    map.insert(
        "https://x/track.png".to_string(),
        Some(FetchedImage {
            bytes: png_1x1(),
            ext: "png".into(),
        }),
    );
    let f = FakeFetcher {
        map,
        fetched: RefCell::new(vec![]),
    };
    let out = process_html(r#"<img src="https://x/track.png">"#, "d1", true, &f);
    assert_eq!(out.assets.len(), 0);
    assert!(!out.html.contains("img"));
}

#[test]
fn images_disabled_drops_all_imgs_without_fetch() {
    let f = FakeFetcher {
        map: Default::default(),
        fetched: RefCell::new(vec![]),
    };
    let out = process_html(r#"<img src="https://x/p.png">text"#, "d1", false, &f);
    assert!(out.html.contains("text"));
    assert!(!out.html.contains("img"));
    assert!(f.fetched.borrow().is_empty());
}

/// Regression: asset keys must be unique across documents so that merging
/// multiple articles' assets into one AssetBundle never silently overwrites
/// an image from a different document.
#[test]
fn distinct_doc_ids_yield_distinct_asset_keys() {
    let mut map = std::collections::HashMap::new();
    map.insert(
        "https://x/p.png".to_string(),
        Some(FetchedImage {
            bytes: png_8x8(),
            ext: "png".into(),
        }),
    );
    // Two separate fetchers sharing the same image data.
    let fa = FakeFetcher {
        map: map.clone(),
        fetched: RefCell::new(vec![]),
    };
    let fb = FakeFetcher {
        map,
        fetched: RefCell::new(vec![]),
    };
    let a = process_html(r#"<img src="https://x/p.png">"#, "doc-a", true, &fa);
    let b = process_html(r#"<img src="https://x/p.png">"#, "doc-b", true, &fb);
    assert_eq!(a.assets.len(), 1);
    assert_eq!(b.assets.len(), 1);
    assert_ne!(
        a.assets[0].0, b.assets[0].0,
        "asset keys must differ when doc ids differ to prevent AssetBundle collisions"
    );
}
