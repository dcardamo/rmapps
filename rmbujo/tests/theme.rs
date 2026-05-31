use rmbujo::theme::{css_vars, load_theme};

#[test]
fn library_palette() {
    let t = load_theme("library").unwrap();
    assert_eq!(t.get("primary").unwrap(), "#2A2F6B");
    assert_eq!(t.get("cover_to").unwrap(), "#1A1E48");
}

#[test]
fn css_vars_sorted() {
    let t = load_theme("library").unwrap();
    let css = css_vars(&t);
    assert!(css.starts_with(":root{"));
    assert!(css.contains("--primary:#2A2F6B;"));
    // BTreeMap → alphabetical: accent before primary
    let bi = css.find("--accent").unwrap();
    let ni = css.find("--primary").unwrap();
    assert!(bi < ni);
}

#[test]
fn unknown_theme_errors() {
    assert!(load_theme("nope").is_err());
}

#[test]
fn theme_path_loads() {
    let mut p = std::env::temp_dir();
    p.push(format!("rmbujo-theme-{}.toml", std::process::id()));
    std::fs::write(&p, "navy = \"#000080\"\n").unwrap();
    let t = load_theme(p.to_str().unwrap()).unwrap();
    assert_eq!(t.get("navy").unwrap(), "#000080");
}
