use crate::config::Namespace;

/// Whether a field is a plain value, a secret reference, or a connector binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Plain,
    Secret,
    Connector,
}

/// One field's static metadata (for `describe`, `template`, validation).
#[derive(Debug, Clone, Copy)]
pub struct FieldSchema {
    pub name: &'static str,
    pub ty: &'static str,
    /// Rendered default expression, or "" if the field has no explicit default.
    pub default: &'static str,
    pub doc: &'static str,
    pub kind: FieldKind,
}

/// A registered config section's schema.
#[derive(Debug, Clone, Copy)]
pub struct ConfigSchema {
    pub kind: &'static str,
    pub namespace: Namespace,
    pub fields: &'static [FieldSchema],
}

inventory::collect!(ConfigSchema);

/// Read-only access to all schemas linked into this binary.
pub struct Registry;

impl Registry {
    pub fn all() -> impl Iterator<Item = &'static ConfigSchema> {
        inventory::iter::<ConfigSchema>.into_iter()
    }
    pub fn find(namespace: Namespace, kind: &str) -> Option<&'static ConfigSchema> {
        Self::all().find(|s| s.namespace == namespace && s.kind == kind)
    }
    /// Field names a section legitimately accepts (for unknown-key validation).
    pub fn field_names(namespace: Namespace, kind: &str) -> Option<Vec<&'static str>> {
        Self::find(namespace, kind).map(|s| s.fields.iter().map(|f| f.name).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A hand-written registration, standing in for the derive (Task 2).
    inventory::submit! {
        ConfigSchema {
            kind: "sample",
            namespace: Namespace::Connector,
            fields: &[FieldSchema {
                name: "max",
                ty: "usize",
                default: "100",
                doc: "the cap",
                kind: FieldKind::Plain,
            }],
        }
    }

    #[test]
    fn registry_finds_submitted_schema() {
        let s = Registry::find(Namespace::Connector, "sample").expect("registered");
        assert_eq!(s.fields.len(), 1);
        assert_eq!(s.fields[0].name, "max");
        assert_eq!(
            Registry::field_names(Namespace::Connector, "sample").unwrap(),
            vec!["max"]
        );
        assert!(Registry::find(Namespace::App, "sample").is_none());
    }
}
