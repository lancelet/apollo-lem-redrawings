//! lmsheets-document: the sheet document, its constraint solver, and the
//! messages that change it. Pure Rust, no I/O beyond (de)serialising;
//! builds natively and for WebAssembly.
//!
//! # The contract
//!
//! The document is a value. Three things cross the crate boundary:
//!
//! - **messages in**: [`Msg`] carrying an [`Op`] (or undo,
//!   redo, preview, commit), handled by [`Model::update`]; the
//!   pure fold of one op is [`Document::apply`];
//! - **the file**: [`Document::parse`] and [`Document::to_json`],
//!   older versions upgraded first ([`state::version`]), validated on
//!   load exactly as `apply` validates;
//! - **reads out**: every accessor is `&self`; the render crate and the
//!   queries read the value, and nothing they get back can change it.
//!
//! No `&mut` to document state leaves this crate: the methods that edit a
//! sketch, the views or the tables are crate-private and reached only
//! through `apply`. Value types a message carries (entities, relations,
//! measures, provenance, a whole [`value::sketch::Sketch`]) have public
//! constructors, because building a payload is not editing a document.
//!
//! # Layout
//!
//! - [`api`]: what a client uses: [`Model`], [`Msg`] with [`Op`] and
//!   [`SketchOp`], [`Query`] and [`Answer`];
//! - [`value`]: what messages carry, plain values with public builders:
//!   geometry, relations, measures, provenance, sketches, views and the
//!   tables;
//! - [`state`]: the document, readable from outside and changed only
//!   through the api, with its analyses;
//! - `solver`: crate-private, the equations and the numerical solver;
//! - [`examples`]: documents and sketches for tests and the render crate;
//! - `testing`: proptest strategies for values, geometry, documents and
//!   op sequences, in this crate's tests and, with the `testing`
//!   feature, in other crates'.
//!
//! Every public item is documented; the lint below enforces it.

#![warn(missing_docs)]

pub mod api;
pub mod examples;
pub(crate) mod solver;
pub mod state;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod value;

pub use api::model::{Model, Msg};
pub use api::msg::{Op, SketchOp};
pub use api::query::{Answer, Query};
pub use state::document::Document;
