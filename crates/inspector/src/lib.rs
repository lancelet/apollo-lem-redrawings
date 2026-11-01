//! The inspector core: a pure, data-only Elm-style model of the viewer's
//! panes and cameras (docs/redraw-plan.md §5). No GPU, window or UI types:
//! the Leptos chrome and the native harness both turn their events into
//! [`Msg`]s, call [`update`], and draw from the cameras the [`Model`]
//! derives. A recorded `Vec<Msg>` replays to the identical model.
//!
//! [`chain`] holds the registration (paper to scan and back) and its loader
//! from a bundle's files; [`model`] is the model, the messages and `update`;
//! [`group`] maps a node to its selection group from the document's links;
//! [`constraints`] lists the constraints and lays out their icons;
//! [`layers`] classifies the redraw's parts for the layers panel and
//! [`theme`] holds the two looks; [`pick`] is the pure hover pick (owner map
//! and geometry hit-test) the shell runs and passes in with [`Msg::Hover`].

#![warn(missing_docs)]

pub mod chain;
pub mod constraints;
pub mod detail;
pub mod group;
pub mod layers;
pub mod model;
pub mod pick;
pub mod theme;

pub use chain::{Chain, ChainError, Grid};
pub use constraints::{ConstraintItem, ConstraintKind, ConstraintTrust, Constraints, Link};
pub use group::{Group, Groups, Highlight};
pub use layers::{Layer, Layers};
pub use model::{
    update, Layout, Mode, Model, Msg, NavMap, NodeId, Pane, PaperCamera, Pin, PinHead, ScanCamera,
    Sheet,
};
pub use pick::{OwnerMap, OwnerTable, OwnerTile, Picker};
pub use theme::{Role, Theme};
