//! HTML→Typst article content pipeline. `convert` is the pure transform;
//! `Article` is the highlightable component apps render.

pub mod convert;

pub use convert::{convert, image_key, Converted, Token};
