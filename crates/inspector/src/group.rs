//! Selection groups: what a hover takes together (docs/redraw-plan.md §5,
//! "Selection units"). Pure, over the document's own links.
//!
//! - **A dimension**: its line, arrowheads and value (one node), its drawn
//!   extension lines (witness nodes naming it among their users), and for
//!   a radial note its leader (drawn as the dimension).
//! - **A centre**: the centre lines through the same point, and the centre
//!   marks of that point's circle or arc.
//! - Anything else is a group of its own.
//!
//! A witness line used by several dimensions is a member of each of their
//! groups, so hovering it highlights them all. A line that stands in for a
//! dimension's extension line without being drawn by it (`Extension::By`:
//! CL1 for D1) is *serving*: shown in a lighter tint, and not a member, so
//! hovering CL1 does not take D1 with it.

use crate::model::NodeId;
use crate::pick::OwnerTable;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::dimension::{Extension, Layout, Of, Serving};
use lmsheets_document::value::leader::DrawingEntity;
use lmsheets_document::value::text::DrawingPoint;
use lmsheets_render::nodes::node_key;
use lmsheets_render::scene::{Part, Plane, Source, WitnessId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// What is lit for a hover: the nodes tinted fully, and the nodes that
/// serve the group without belonging to it, tinted lightly. Sorted, and
/// disjoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Highlight {
    /// The group's nodes (every group the picked node belongs to).
    pub members: Vec<NodeId>,
    /// Nodes that serve a group without belonging to it.
    pub serving: Vec<NodeId>,
}

impl Highlight {
    /// A single piece.
    pub fn piece(node: NodeId) -> Self {
        Self {
            members: vec![node],
            serving: Vec::new(),
        }
    }

    /// Whether nothing is lit.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty() && self.serving.is_empty()
    }

    /// The highlight with the nodes `keep` rejects taken out (hidden layers
    /// are not lit).
    pub fn retaining(&self, keep: impl Fn(NodeId) -> bool) -> Self {
        Self {
            members: self.members.iter().copied().filter(|n| keep(*n)).collect(),
            serving: self.serving.iter().copied().filter(|n| keep(*n)).collect(),
        }
    }

    /// Whether `node` is lit, as a member or serving.
    pub fn contains(&self, node: NodeId) -> bool {
        self.members.binary_search(&node).is_ok() || self.serving.binary_search(&node).is_ok()
    }
}

/// One group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The node that names the group: a dimension, or a centre's first
    /// centre line (or its mark).
    pub head: NodeId,
    /// Its nodes.
    pub members: BTreeSet<NodeId>,
    /// Lines that serve it without belonging to it.
    pub serving: BTreeSet<NodeId>,
}

/// All the document's groups.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Groups {
    groups: Vec<Group>,
    of_node: BTreeMap<NodeId, Vec<usize>>,
}

/// The centre entity a point is the centre of, as a drawing entity.
fn centre_entity(p: &DrawingPoint) -> Option<DrawingEntity> {
    match *p {
        DrawingPoint::Sheet { point } => point.centre_of().map(|id| DrawingEntity::Sheet { id }),
        DrawingPoint::View { view, point } => {
            point.centre_of().map(|id| DrawingEntity::View { view, id })
        }
    }
}

impl Groups {
    /// The groups of `doc`, whose drawing is `plane` (witness lines name
    /// their users there); nodes the table lacks are left out.
    pub fn new(doc: &Document, plane: &Plane, table: &OwnerTable) -> Self {
        let node = |s: Source| {
            let (kind, name) = node_key(plane, doc, s);
            table.id(&kind, &name)
        };
        let mut groups: Vec<Group> = Vec::new();

        // Dimensions.
        for (did, dim) in doc.dimensions() {
            let Some(head) = node(Source::Dimension(*did, Part::Line)) else {
                continue;
            };
            let mut members = BTreeSet::from([head]);
            for (i, w) in plane.witnesses.iter().enumerate() {
                if w.users.contains(did) {
                    members.extend(node(Source::Witness(WitnessId(i as u32))));
                }
            }
            let mut serving = BTreeSet::new();
            if let Layout::Linear(l) = dim.layout() {
                for e in l.ext {
                    let Extension::By(by) = e else { continue };
                    let n = match (by, dim.of()) {
                        (Serving::CentreLine { line }, _) => node(Source::CentreLine(line)),
                        (Serving::CentreMark { mark }, _) => node(Source::CentreMark(mark)),
                        (Serving::Line { line }, Of::View { view, .. }) => {
                            node(Source::View(view, line))
                        }
                        (Serving::Line { line }, Of::Sheet { .. }) => node(Source::Sheet(line)),
                    };
                    serving.extend(n);
                }
            }
            groups.push(Group {
                head,
                members,
                serving,
            });
        }

        // Centres: centre lines sharing a through-point are one group.
        let lines: Vec<_> = doc.centre_lines().collect();
        let mut parent: Vec<usize> = (0..lines.len()).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        for i in 0..lines.len() {
            for j in 0..i {
                if lines[i]
                    .1
                    .through()
                    .iter()
                    .any(|p| lines[j].1.through().contains(p))
                {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
        let mut marks_used: BTreeSet<_> = BTreeSet::new();
        let mut clusters: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..lines.len() {
            let r = find(&mut parent, i);
            clusters.entry(r).or_default().push(i);
        }
        for members in clusters.into_values() {
            let mut set = BTreeSet::new();
            let mut head = None;
            let centres: Vec<DrawingEntity> = members
                .iter()
                .flat_map(|&i| lines[i].1.through().iter().filter_map(centre_entity))
                .collect();
            for &i in &members {
                if let Some(n) = node(Source::CentreLine(*lines[i].0)) {
                    head.get_or_insert(n);
                    set.insert(n);
                }
            }
            for (mid, mark) in doc.centre_marks() {
                if mark.of().iter().any(|e| centres.contains(e)) {
                    set.extend(node(Source::CentreMark(*mid)));
                    marks_used.insert(*mid);
                }
            }
            if let Some(head) = head {
                groups.push(Group {
                    head,
                    members: set,
                    serving: BTreeSet::new(),
                });
            }
        }
        for (mid, _) in doc.centre_marks() {
            if marks_used.contains(mid) {
                continue;
            }
            if let Some(n) = node(Source::CentreMark(*mid)) {
                groups.push(Group {
                    head: n,
                    members: BTreeSet::from([n]),
                    serving: BTreeSet::new(),
                });
            }
        }

        let mut of_node: BTreeMap<NodeId, Vec<usize>> = BTreeMap::new();
        for (i, g) in groups.iter().enumerate() {
            for &n in &g.members {
                of_node.entry(n).or_default().push(i);
            }
        }
        Self { groups, of_node }
    }

    /// Every group.
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// The groups a node is a member of.
    pub fn groups_of(&self, node: NodeId) -> impl Iterator<Item = &Group> {
        self.of_node
            .get(&node)
            .into_iter()
            .flatten()
            .map(|&i| &self.groups[i])
    }

    /// What a hover over `node` lights: every group it belongs to (the
    /// node alone if none, or if `single`, the Alt modifier's piece).
    pub fn highlight(&self, node: NodeId, single: bool) -> Highlight {
        if single {
            return Highlight::piece(node);
        }
        let mut members = BTreeSet::from([node]);
        let mut serving = BTreeSet::new();
        for g in self.groups_of(node) {
            members.extend(&g.members);
            serving.extend(&g.serving);
        }
        Highlight {
            serving: serving.difference(&members).copied().collect(),
            members: members.into_iter().collect(),
        }
    }
}
