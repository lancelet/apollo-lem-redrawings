//! lmsheets-render: from a document to a styled scene.
//!
//! ```text
//! Document ─draw(purpose)─► Scene ─resolve(theme)─► Styled ─► writers
//! ```
//!
//! [`draw`](draw::draw) reads a document and makes a [`scene::Scene`]:
//! primitives in paper millimetres, each naming its layer, its tint and
//! the entity it came from. It obeys the layer table and nothing else: a
//! scene made for a [`Purpose`](lmsheets_document::value::style::Purpose)
//! holds exactly the entities whose layer is visible for it.
//! [`resolve`](resolve::resolve) applies a [`theme::Theme`] and the
//! [`conventions::Conventions`] once, giving a [`resolve::Styled`] scene
//! with concrete colours, dash patterns fitted to each line, and the caps
//! and joins to draw with; the writers (SVG, PDF, the GPU) take that and
//! know nothing about documents. A [`page::Page`] says how paper
//! millimetres land on an output page.

#![warn(missing_docs)]

pub mod conventions;
pub mod dashes;
pub mod dimension;
pub mod draw;
pub mod form;
pub mod leader;
pub mod nodes;
pub mod page;
pub mod resolve;
mod rounding;
pub mod scene;
pub mod terminator;
pub mod text;
pub mod theme;
pub mod weights;

/// The document's value types that appear in this crate's API, so a
/// writer depends on this crate alone.
pub mod value {
    pub use lmsheets_document::value::bounds::Bounded;
    pub use lmsheets_document::value::geometry::{Paper, Rect, P2};
    pub use lmsheets_document::value::sketch::LayerId;
    pub use lmsheets_document::value::style::{Dash, Marker, MarkerKind, PaletteId, Rgba};
}
