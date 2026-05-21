use rmreader::config::Config;

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
