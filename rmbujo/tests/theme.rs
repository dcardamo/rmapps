use rmbujo::theme::{css_vars, load_theme};

#[test]
fn library_palette() {
    let t = load_theme("library").unwrap();
    assert_eq!(t.get("navy").unwrap(), "#1B365D");
    assert_eq!(t.get("cover_to").unwrap(), "#0F2444");
}

#[test]
fn css_vars_sorted() {
    let t = load_theme("library").unwrap();
    let css = css_vars(&t);
    assert!(css.starts_with(":root{"));
    assert!(css.contains("--navy:#1B365D;"));
    // BTreeMap → alphabetical: brick before navy
    let bi = css.find("--brick").unwrap();
    let ni = css.find("--navy").unwrap();
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
