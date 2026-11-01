//! What a solve or an analysis reports: the solver's public face.
//!
//! The solver itself is crate-private; these are the types its results
//! are expressed in, re-exported so that a client can read an
//! [`Analysis`](crate::value::sketch::Analysis) or a
//! [`SystemAnalysis`](crate::state::views::SystemAnalysis) and pass
//! [`Options`] to a solve.

pub use crate::solver::near::{Nearest, Prior};
pub use crate::solver::solve::{
    Backend, Dependency, Freedom, Jacobian, Motion, Options, Placement, Report, Solution,
    Termination, Value,
};
pub use crate::solver::tolerance::{self, Tolerance};
