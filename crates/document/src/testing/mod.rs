//! Proptest strategies for the document's values, its geometry, and
//! whole documents with the op sequences that build them. Compiled for
//! this crate's tests, and for other crates' with the `testing` feature.
//!
//! Small and shrinkable on purpose: a few entities, a few ops, names from
//! short fixed pools so that ops meet each other (an op removing what
//! another added, a constraint on an entity that exists) often, and
//! values on a small integer grid first, so that failures shrink to round
//! numbers and special configurations (coincident, axis-aligned) come up.
//!
//! - [`values`]: numbers by what they measure, measures, printed text,
//!   dates, provenance;
//! - [`geometry`]: points, entities, boxes, view transforms;
//! - [`document`]: ops, messages, and documents reached by ops.
//!
//! The solver's own strategies (equations over a fixed geometry, special
//! positions) are crate-private, in `solver::testing`, and build on
//! [`values`].

pub mod document;
pub mod geometry;
pub mod values;
