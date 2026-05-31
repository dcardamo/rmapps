use serde::{Deserialize, Deserializer};

use crate::error::ConfigError;

/// A reference to a secret by *name* in the `SecretStore` — never the value.
/// Deserializes from a plain TOML string.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct SecretRef(pub String);

impl SecretRef {
    pub fn name(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A binding to a connector instance, written `"kind.instance"` in TOML.
///
/// `Default` yields an empty `kind`/`instance` — a zero-value placeholder that
/// `#[serde(default)]` needs, not a usable binding. Construct real values via
/// [`ConnectorRef::parse`] or by setting both fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectorRef {
    pub kind: String,
    pub instance: String,
}

impl ConnectorRef {
    pub fn parse(s: &str) -> Result<Self, ConfigError> {
        let mut parts = s.splitn(2, '.');
        match (parts.next(), parts.next()) {
            (Some(k), Some(i)) if !k.is_empty() && !i.is_empty() && !i.contains('.') => {
                Ok(ConnectorRef {
                    kind: k.to_string(),
                    instance: i.to_string(),
                })
            }
            _ => Err(ConfigError::BadConnectorRef(s.to_string())),
        }
    }
}

impl<'de> Deserialize<'de> for ConnectorRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ConnectorRef::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_ref_from_string() {
        let r: SecretRef = serde_json::from_str("\"readwise-reader\"").unwrap();
        assert_eq!(r.name(), "readwise-reader");
    }

    #[test]
    fn connector_ref_parses_kind_instance() {
        let r = ConnectorRef::parse("ics.work").unwrap();
        assert_eq!((r.kind.as_str(), r.instance.as_str()), ("ics", "work"));
    }

    #[test]
    fn connector_ref_rejects_malformed() {
        assert!(ConnectorRef::parse("icswork").is_err());
        assert!(ConnectorRef::parse("ics.").is_err());
        assert!(ConnectorRef::parse("a.b.c").is_err());
    }
}
