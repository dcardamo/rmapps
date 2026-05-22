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
fn truncates_monster_articles_with_note() {
    // An article far over the size cap is truncated and gets an "open in Readwise"
    // note appended, so fulgur's layout time can't blow up on huge pages.
    let big = format!("<p>{}</p>", "word ".repeat(40_000)); // ~200 KB
    let f = FakeFetcher {
        map: Default::default(),
        fetched: RefCell::new(vec![]),
    };
    let out = process_html(&big, "d1", false, 80_000, &f);
    assert!(out.html.len() < big.len());
    assert!(out.html.contains("truncated"));

    // A normal-size article is left intact (no note).
    let small = "<p>just a short article</p>";
    let out2 = process_html(small, "d1", false, 80_000, &f);
    assert!(out2.html.contains("just a short article"));
    assert!(!out2.html.contains("truncated"));
}

#[test]
fn strips_inline_styles_and_presentational_attrs() {
    // Newsletter emails set inline font-family to system fonts the offline
    // renderer lacks; that override renders the text blank, so every inline
    // style (and legacy presentational attr) must be stripped while keeping text.
    let html = r##"<p style="font-family:Georgia, serif;color:#111">Hello there</p><div class="rw" width="600" align="center" bgcolor="#fff">Body text</div>"##;
    let f = FakeFetcher {
        map: Default::default(),
        fetched: RefCell::new(vec![]),
    };
    let out = process_html(html, "d1", false, 80_000, &f);
    assert!(out.html.contains("Hello there"));
    assert!(out.html.contains("Body text"));
    assert!(
        !out.html.contains("style="),
        "inline style must be stripped"
    );
    assert!(!out.html.contains("font-family"));
    assert!(!out.html.contains("align="));
    assert!(!out.html.contains("width="));
    assert!(!out.html.contains("bgcolor"));
}

#[test]
fn strips_scripts_and_handlers_keeps_text() {
    let html = r#"<p onclick="x()">Hello</p><script>evil()</script><iframe src="z"></iframe>"#;
    let f = FakeFetcher {
        map: Default::default(),
        fetched: RefCell::new(vec![]),
    };
    let out = process_html(html, "d1", true, 80_000, &f);
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
    let out = process_html(
        r#"<p><img src="https://x/p.png"></p>"#,
        "d1",
        true,
        80_000,
        &f,
    );
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
    let out = process_html(r#"<img src="https://x/track.png">"#, "d1", true, 80_000, &f);
    assert_eq!(out.assets.len(), 0);
    assert!(!out.html.contains("img"));
}

#[test]
fn images_disabled_drops_all_imgs_without_fetch() {
    let f = FakeFetcher {
        map: Default::default(),
        fetched: RefCell::new(vec![]),
    };
    let out = process_html(
        r#"<img src="https://x/p.png">text"#,
        "d1",
        false,
        80_000,
        &f,
    );
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
    let a = process_html(r#"<img src="https://x/p.png">"#, "doc-a", true, 80_000, &fa);
    let b = process_html(r#"<img src="https://x/p.png">"#, "doc-b", true, 80_000, &fb);
    assert_eq!(a.assets.len(), 1);
    assert_eq!(b.assets.len(), 1);
    assert_ne!(
        a.assets[0].0, b.assets[0].0,
        "asset keys must differ when doc ids differ to prevent AssetBundle collisions"
    );
}

/// The default `fetch_many` implementation (used by test fakes) must return
/// results in the same order as the input URL slice.
#[test]
fn fetch_many_default_preserves_order() {
    // Build a fetcher that maps each URL to a distinct image so we can tell
    // them apart by byte content after fetch_many returns.
    let url_a = "https://x/a.png".to_string();
    let url_b = "https://x/b.png".to_string();
    let url_c = "https://x/c.png".to_string();

    let make_png = |seed: u8| -> Vec<u8> {
        use image::{ImageBuffer, Rgba};
        let img: ImageBuffer<Rgba<u8>, _> =
            ImageBuffer::from_pixel(8, 8, Rgba([seed, seed, seed, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    };

    let mut map = std::collections::HashMap::new();
    map.insert(
        url_a.clone(),
        Some(FetchedImage {
            bytes: make_png(10),
            ext: "png".into(),
        }),
    );
    map.insert(
        url_b.clone(),
        Some(FetchedImage {
            bytes: make_png(20),
            ext: "png".into(),
        }),
    );
    map.insert(url_c.clone(), None); // simulates a failed fetch

    let f = FakeFetcher {
        map,
        fetched: RefCell::new(vec![]),
    };

    let urls = vec![url_a.clone(), url_b.clone(), url_c.clone()];
    let results = f.fetch_many(&urls);

    assert_eq!(results.len(), 3);
    assert!(results[0].is_some(), "first url should succeed");
    assert_eq!(results[0].as_ref().unwrap().bytes, make_png(10));
    assert!(results[1].is_some(), "second url should succeed");
    assert_eq!(results[1].as_ref().unwrap().bytes, make_png(20));
    assert!(results[2].is_none(), "third url should fail (None)");

    // Confirm the fetch calls happened in input order (sequential default).
    assert_eq!(*f.fetched.borrow(), vec![url_a, url_b, url_c]);
}
