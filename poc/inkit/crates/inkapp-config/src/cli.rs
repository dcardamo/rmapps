//! `config` subcommands, mounted by each app binary (feature = "cli").
//! Scoped to the registry of whatever config structs the binary links.

use std::path::PathBuf;

use clap::Subcommand;

use crate::schema::{FieldKind, Registry};
use crate::store::ConfigStore;

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Print the resolved config file path.
    Path,
    /// Print a starter config (all known sections, defaults, docs).
    Template,
    /// List known sections/keys/types/defaults/docs.
    Describe { kind: Option<String> },
    /// Parse + validate the config against the registry.
    Validate,
    /// Read a dotted key, e.g. connector.readwise.main.feed_enabled.
    Get { key: String },
    /// Set a dotted key to a (TOML-parsed) value.
    Set { key: String, value: String },
    /// Open the config file in $EDITOR.
    Edit,
}

/// Run a config subcommand against the file at `path`. Returns an exit code.
pub fn run(cmd: ConfigCmd, path: PathBuf) -> crate::Result<i32> {
    match cmd {
        ConfigCmd::Path => {
            println!("{}", path.display());
            Ok(0)
        }
        ConfigCmd::Template => {
            print!("{}", render_template());
            Ok(0)
        }
        ConfigCmd::Describe { kind } => {
            print!("{}", render_describe(kind.as_deref()));
            Ok(0)
        }
        ConfigCmd::Validate => match validate(&path) {
            Ok(()) => {
                println!("config OK");
                Ok(0)
            }
            Err(e) => {
                eprintln!("config invalid: {e}");
                Ok(1)
            }
        },
        ConfigCmd::Get { key } => {
            let store = ConfigStore::open(&path)?;
            match store.get_raw(&key) {
                Some(v) => {
                    println!("{v}");
                    Ok(0)
                }
                None => {
                    eprintln!("no value at {key}");
                    Ok(1)
                }
            }
        }
        ConfigCmd::Set { key, value } => {
            let mut store = ConfigStore::open(&path)?;
            store.set_raw(&key, &value)?;
            store.save()?;
            Ok(0)
        }
        ConfigCmd::Edit => {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
            let status = std::process::Command::new(editor).arg(&path).status();
            Ok(status.ok().and_then(|s| s.code()).unwrap_or(1))
        }
    }
}

fn section_header(s: &crate::ConfigSchema) -> String {
    match s.namespace {
        crate::Namespace::Framework => format!("[{}]", s.kind),
        crate::Namespace::Connector => format!("[connector.{}.<instance>]", s.kind),
        crate::Namespace::App => format!("[app.{}.<instance>]", s.kind),
    }
}

fn placeholder(f: &crate::FieldSchema) -> String {
    match f.kind {
        FieldKind::Secret => "\"<secret-name>\"".into(),
        FieldKind::Connector => "\"<kind.instance>\"".into(),
        FieldKind::Plain => {
            // Emit the default only when it is valid TOML. Complex Rust-expr
            // defaults (e.g. `vec![...]`, `String::new()`) are not, so fall back
            // to an empty string the user fills in (the doc comment guides them).
            if !f.default.is_empty() && f.default.parse::<toml_edit::Value>().is_ok() {
                f.default.to_string()
            } else {
                "\"\"".into()
            }
        }
    }
}

fn render_template() -> String {
    let mut out =
        String::from("# inkapp config — values are settings; secrets live in secrets.json.\n\n");
    for s in Registry::all() {
        out.push_str(&section_header(s));
        out.push('\n');
        for f in s.fields {
            if !f.doc.is_empty() {
                out.push_str(&format!("# {}\n", f.doc));
            }
            out.push_str(&format!("{} = {}\n", f.name, placeholder(f)));
        }
        out.push('\n');
    }
    out
}

fn render_describe(kind: Option<&str>) -> String {
    let mut out = String::new();
    for s in Registry::all().filter(|s| kind.is_none_or(|k| k == s.kind)) {
        out.push_str(&format!("{}\n", section_header(s)));
        for f in s.fields {
            out.push_str(&format!(
                "  {:<20} {:<10} default={:<12} {}\n",
                f.name, f.ty, f.default, f.doc
            ));
        }
    }
    out
}

fn validate(path: &std::path::Path) -> crate::Result<()> {
    let store = ConfigStore::open(path)?;
    store.validate_known_sections()
}

/// Test hook: render the template without capturing stdout.
#[doc(hidden)]
pub fn render_template_for_test() -> String {
    render_template()
}
