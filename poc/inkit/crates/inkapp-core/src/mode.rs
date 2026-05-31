//! The interaction-mode axis. A component carries a `Mode` as a *field* (not a
//! trait parameter); its `render` and `decode` both branch on the same value, so
//! a `ReadOnly` render that drew no affordance cannot have a `decode` that reads
//! one. `view`/`update` chooses the mode from the backing connector's capability
//! — the component never touches a connector.

/// Whether a component exposes edit affordances and decodes structured ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Renders content, decodes nothing (Display behavior).
    ReadOnly,
    /// Renders affordances, decodes structured ink into messages (Control behavior).
    Editable,
}
