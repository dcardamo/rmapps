use inkapp_config::{Config, ConnectorRef, FieldKind, Namespace, Registry, SecretRef};

#[derive(serde::Deserialize, Config)]
#[serde(default)]
#[config(kind = "sample", namespace = "connector")]
struct Sample {
    /// the cap on items
    #[config(default = 100)]
    max: usize,
    enabled: bool,
    token: SecretRef,
    feed: ConnectorRef,
}

#[test]
fn defaults_apply_for_absent_fields() {
    let s: Sample = toml::from_str("").unwrap();
    assert_eq!(s.max, 100);
    assert!(!s.enabled);
    assert!(s.token.is_empty());
}

#[test]
fn partial_toml_fills_rest_with_defaults() {
    let s: Sample = toml::from_str("enabled = true\ntoken = \"tok\"").unwrap();
    assert!(s.enabled);
    assert_eq!(s.max, 100);
    assert_eq!(s.token.name(), "tok");
}

#[test]
fn schema_is_registered_with_field_kinds_and_docs() {
    assert_eq!(Sample::KIND, "sample");
    assert_eq!(Sample::NAMESPACE, Namespace::Connector);
    let schema = Registry::find(Namespace::Connector, "sample").unwrap();
    let token = schema.fields.iter().find(|f| f.name == "token").unwrap();
    assert_eq!(token.kind, FieldKind::Secret);
    let feed = schema.fields.iter().find(|f| f.name == "feed").unwrap();
    assert_eq!(feed.kind, FieldKind::Connector);
    let max = schema.fields.iter().find(|f| f.name == "max").unwrap();
    assert_eq!(max.doc, "the cap on items");
    assert_eq!(max.default, "100");
    let enabled = schema.fields.iter().find(|f| f.name == "enabled").unwrap();
    assert_eq!(
        enabled.default, "",
        "fields without #[config(default)] render an empty default string"
    );
}
