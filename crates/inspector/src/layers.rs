//! The layers panel's categories (docs/redraw-plan.md §5): which kind of
//! drawing part each redraw item and each node is. Derived from what the
//! scene carries (an item's layer and its source), not declared: the
//! document's layer table has too many fine layers (`visible`, `hidden`,
//! `phantom`, `added`, ...) for a panel, and a source says more than a layer
//! (a witness line is a dimension's, whatever layer it was drawn on).
//!
//! Which layers are hidden is model state ([`crate::Model::layer_visible`]);
//! this module only classifies, once, when the bundle is loaded.

use crate::model::NodeId;
use crate::pick::OwnerTable;
use lmsheets_document::state::document::Document;
use lmsheets_render::nodes::node_key;
use lmsheets_render::resolve::StyledItem;
use lmsheets_render::scene::{Plane, Source};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A kind of drawing part the panel shows and hides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Layer {
    /// The part's own lines: outline, hidden, phantom, our added geometry.
    Part,
    /// Centre lines and centre marks.
    Centre,
    /// Genuine dimensions (read from the drawing) and their witness lines.
    Dimensions,
    /// Inferred dimensions (K10): the constraints the ink does not state.
    Inferred,
    /// Construction geometry.
    Constructions,
    /// The marking region (the rectangle and diagonals).
    Marking,
    /// The form: the title block's rules, captions and entries.
    Form,
    /// Annotation texts, their enclosures and leaders.
    Texts,
    /// Observed features and unmodelled marks: seen, not intended.
    Observed,
}

impl Layer {
    /// Every layer, in the panel's order.
    pub const ALL: [Layer; 9] = [
        Layer::Part,
        Layer::Centre,
        Layer::Dimensions,
        Layer::Inferred,
        Layer::Constructions,
        Layer::Marking,
        Layer::Form,
        Layer::Texts,
        Layer::Observed,
    ];

    /// The panel's label.
    pub fn label(self) -> &'static str {
        match self {
            Layer::Part => "Part geometry",
            Layer::Centre => "Centre lines",
            Layer::Dimensions => "Dimensions",
            Layer::Inferred => "Inferred dimensions",
            Layer::Constructions => "Constructions",
            Layer::Marking => "Marking region",
            Layer::Form => "Form (title block)",
            Layer::Texts => "Annotation texts",
            Layer::Observed => "Observed features",
        }
    }
}

/// The kind of part a scene item is, from its layer and its source.
fn classify(doc: &Document, plane: &Plane, layer: &str, source: Source) -> Layer {
    let dimension = |inferred: bool| {
        if inferred {
            Layer::Inferred
        } else {
            Layer::Dimensions
        }
    };
    match source {
        Source::View(v, e) if doc.marking_region_of(v.as_str(), e.as_str()).is_some() => {
            Layer::Marking
        }
        Source::Form(_) | Source::Field(_) => Layer::Form,
        Source::Text(_) | Source::Enclosure(_) | Source::Leader(_) => Layer::Texts,
        Source::Dimension(d, _) => dimension(
            doc.dimension(d)
                .is_some_and(|d| d.layer().as_str() == "inferred"),
        ),
        // A witness line is the dimensions' that use it: inferred only if
        // every user is.
        Source::Witness(w) => {
            let users = &plane.witnesses[w.0 as usize].users;
            dimension(
                !users.is_empty()
                    && users.iter().all(|d| {
                        doc.dimension(*d)
                            .is_some_and(|d| d.layer().as_str() == "inferred")
                    }),
            )
        }
        Source::CentreLine(_) | Source::CentreMark(_) => Layer::Centre,
        Source::Observed(_) | Source::Unmodelled(_) => Layer::Observed,
        Source::Sheet(_) | Source::View(..) => match layer {
            "construction" => Layer::Constructions,
            "centre" => Layer::Centre,
            "dimension" => Layer::Dimensions,
            "inferred" => Layer::Inferred,
            "format" => Layer::Form,
            "text" | "notes" | "balloon" | "leader" => Layer::Texts,
            _ => Layer::Part,
        },
    }
}

/// The classification of a loaded drawing: every styled item's layer, every
/// node's, and how many nodes each layer holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layers {
    items: Vec<Layer>,
    nodes: Vec<Option<Layer>>,
    counts: BTreeMap<Layer, usize>,
}

impl Layers {
    /// Classify `items` (the styled scene, in order) of `doc`, whose drawing
    /// is `plane`; `table` is the node table the nodes are indexed in.
    pub fn new(doc: &Document, plane: &Plane, items: &[StyledItem], table: &OwnerTable) -> Self {
        let mut nodes = vec![None; table.len()];
        let mut seen: BTreeMap<Layer, BTreeSet<(String, String)>> = BTreeMap::new();
        let kinds: Vec<Layer> = items
            .iter()
            .map(|it| {
                let layer = classify(doc, plane, it.layer.as_str(), it.source);
                let key = node_key(plane, doc, it.source);
                if let Some(id) = table.id(&key.0, &key.1) {
                    nodes[id.0 as usize].get_or_insert(layer);
                }
                seen.entry(layer).or_default().insert(key);
                layer
            })
            .collect();
        Self {
            items: kinds,
            nodes,
            counts: seen.into_iter().map(|(l, s)| (l, s.len())).collect(),
        }
    }

    /// A layer of each item, for tests and shells with their own.
    pub fn of_items(items: Vec<Layer>, nodes: Vec<Option<Layer>>) -> Self {
        let mut counts = BTreeMap::new();
        for l in &items {
            *counts.entry(*l).or_insert(0) += 1;
        }
        Self {
            items,
            nodes,
            counts,
        }
    }

    /// The layer of styled item `i`.
    pub fn of_item(&self, i: usize) -> Option<Layer> {
        self.items.get(i).copied()
    }

    /// The layer of a node (none for a node the redraw does not draw).
    pub fn of_node(&self, n: NodeId) -> Option<Layer> {
        self.nodes.get(n.0 as usize).copied().flatten()
    }

    /// How many nodes a layer holds.
    pub fn count(&self, layer: Layer) -> usize {
        self.counts.get(&layer).copied().unwrap_or(0)
    }

    /// The styled items in a layer.
    pub fn items_in(&self, layer: Layer) -> impl Iterator<Item = usize> + '_ {
        self.items
            .iter()
            .enumerate()
            .filter(move |(_, l)| **l == layer)
            .map(|(i, _)| i)
    }
}
