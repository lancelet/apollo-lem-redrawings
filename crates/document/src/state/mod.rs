//! The document: readable from outside, changed only through the api.
//! [`document::Document`] holds the sheet, the views ([`views::Views`]),
//! the tables and the notes; every method that edits it is crate-private
//! and reached through [`Document::apply`](document::Document::apply).
//! [`analysis`] is what a solve reports, in the document's terms;
//! [`version`] brings an older file to this reader's form.

pub mod analysis;
pub mod document;
pub mod trail;
pub mod version;
pub mod views;
