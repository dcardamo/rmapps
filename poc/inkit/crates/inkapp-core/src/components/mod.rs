pub mod calendar_view;
pub mod checkbox;
pub mod highlight_text;
pub mod notice;

/// Escape a string for a Typst string literal (`#"..."`): only `\` and `"` need
/// escaping — other markup chars (`[`, `]`, `#`) are literal inside a string
/// expression. Shared by the components that inject arbitrary text into Typst so
/// the escaping rule lives in one place rather than being re-derived per widget.
pub(crate) fn esc_typst_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
