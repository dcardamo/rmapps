use rmreader::config::{Config, ContentConfig, ImagesConfig};

fn valid_toml() -> &'static str {
    r#"
device = "paper-pro-move"
output_dir = "."
[readwise]
token = "abc123"
[library]
locations = ["new", "later", "shortlist"]
max_items = 100
[feed]
enabled = true
max_items = 100
[images]
enabled = true
[deploy]
backend = "rmapi"
library_folder = "/Reader"
feed_folder = "/Reader"
"#
}

#[test]
fn parses_and_validates() {
    let c: Config = toml::from_str(valid_toml()).unwrap();
    assert_eq!(c.device, "paper-pro-move");
    assert_eq!(c.library.locations, vec!["new", "later", "shortlist"]);
    assert_eq!(c.library.max_items, 100);
    assert!(c.validate().is_ok());
}

#[test]
fn roundtrips() {
    let c: Config = toml::from_str(valid_toml()).unwrap();
    let s = toml::to_string_pretty(&c).unwrap();
    let c2: Config = toml::from_str(&s).unwrap();
    assert_eq!(c, c2);
}

#[test]
fn rejects_empty_token() {
    let mut c: Config = toml::from_str(valid_toml()).unwrap();
    c.readwise.token = String::new();
    assert!(c.validate().is_err());
}

#[test]
fn rejects_bad_location() {
    let mut c: Config = toml::from_str(valid_toml()).unwrap();
    c.library.locations = vec!["bogus".into()];
    assert!(c.validate().is_err());
}

#[test]
fn rejects_rmapi_without_folder() {
    let mut c: Config = toml::from_str(valid_toml()).unwrap();
    c.deploy.library_folder = String::new();
    assert!(c.validate().is_err());
}

/// A config that omits the new `[content]` section and the new `[images]` fields
/// must still parse, with the new fields defaulting correctly.
#[test]
fn new_fields_have_correct_defaults() {
    // valid_toml() has `[images] enabled = true` but no timeout_secs/concurrency,
    // and no [content] section at all.
    let c: Config = toml::from_str(valid_toml()).unwrap();

    // ImagesConfig new fields
    assert_eq!(c.images.timeout_secs, 8, "images.timeout_secs default");
    assert_eq!(c.images.concurrency, 12, "images.concurrency default");

    // ContentConfig (whole section absent from TOML → default)
    assert_eq!(
        c.content.max_article_bytes, 80_000,
        "content.max_article_bytes default"
    );
}

/// A config that explicitly sets the new fields must parse them correctly.
#[test]
fn new_fields_parse_explicit_values() {
    let toml_str = r#"
device = "paper-pro-move"
output_dir = "."
[readwise]
token = "abc123"
[images]
enabled = true
timeout_secs = 15
concurrency = 4
[content]
max_article_bytes = 50000
[deploy]
backend = "none"
"#;
    let c: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(c.images.timeout_secs, 15);
    assert_eq!(c.images.concurrency, 4);
    assert_eq!(c.content.max_article_bytes, 50_000);
}

/// ImagesConfig and ContentConfig Default impls must match the serde defaults.
#[test]
fn config_struct_defaults_match_serde_defaults() {
    let images = ImagesConfig::default();
    assert!(images.enabled);
    assert_eq!(images.timeout_secs, 8);
    assert_eq!(images.concurrency, 12);

    let content = ContentConfig::default();
    assert_eq!(content.max_article_bytes, 80_000);
}
