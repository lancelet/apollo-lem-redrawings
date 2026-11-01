//! What a client uses. A client never holds the document: it sends
//! messages ([`Msg`], carrying [`Op`]s) to a [`Model`] and asks it
//! [`Query`]s. Everything here is serialisable, so a panel in the viewer
//! and an agent over the server's API see the same surface.

pub mod model;
pub mod msg;
#[cfg(test)]
mod properties;
pub mod query;

pub use model::{Model, Msg};
pub use msg::{Op, SketchOp};
pub use query::{Answer, Query};
