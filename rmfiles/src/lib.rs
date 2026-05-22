//! `rmfiles` — a pure-Rust reader for reMarkable `.rm` (v6) scene files.
//!
//! The initial focus is extracting ink-stroke geometry (notably highlighter
//! strokes), which is how the reMarkable Paper Pro stores PDF highlights. The
//! parser is ported from the rmscene Python reference implementation and walks
//! the v6 tagged-block stream, decoding `Line` items into [`Stroke`]s.
//!
//! ```no_run
//! let bytes = std::fs::read("page.rm").unwrap();
//! let scene = rmfiles::Scene::parse(&bytes).unwrap();
//! assert_eq!(scene.version(), 6);
//! for stroke in scene.strokes() {
//!     if stroke.is_highlighter() {
//!         println!("{} points", stroke.points.len());
//!     }
//! }
//! ```

#![warn(missing_docs)]

mod error;
mod geometry;
mod scene;

pub use error::{Error, Result};
pub use geometry::{Point, SCREEN_DPI, SCREEN_HEIGHT, SCREEN_WIDTH};
pub use scene::{Pen, PenColor, Scene, SceneItem, Stroke};
