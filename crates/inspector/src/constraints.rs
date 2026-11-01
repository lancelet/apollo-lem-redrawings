//! The constraint list the viewer shows as icons (docs/redraw-plan.md §5,
//! "Constraints shown"): every constraint of the sheet's sketch and of each
//! view's, with what it binds (as redraw nodes, so hover can light them), a
//! paper anchor for its icon, and its trust class from its provenance. Pure,
//! over the document; the glyphs and the icon layout are here too (the
//! shell only turns them into GPU instances).
//!
//! **Trust classes** (§8.2, §8.5), from the provenance:
//! - [`ConstraintTrust::Inferred`]: the method is `Inferred` (the lowest
//!   trust; its basis is the detail).
//! - [`ConstraintTrust::Read`]: the method is `Read`, a genuine constraint of
//!   the sheet; the trust ladder's level (read, overlay, second read, ...)
//!   is kept beside it.
//! - [`ConstraintTrust::Stated`]: a design statement recorded by the user
//!   or Claude (a `design_` id, or the provenance names one of them).
//! - [`ConstraintTrust::Drawn`]: what the fit estimated from the ink.
//!
//! A constraint that is a dimension's own (D1, R1, ...) is listed, flagged
//! `dimension`, and its icon is off unless the model turns them on: the
//! dimension is already drawn.

use crate::layers::Layers;
use crate::model::{Model, NodeId};
use crate::pick::OwnerTable;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::geometry::{Entity, EntityId, Frame, PointRef};
use lmsheets_document::value::provenance::{Agent, Method};
use lmsheets_document::value::sketch::Sketch;
use lmsheets_document::value::view::ViewId;
use lmsheets_render::nodes::node_key;
use lmsheets_render::scene::{Part, Plane, Source};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// How far a constraint is trusted, as the panel filters it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConstraintTrust {
    /// Stated on the sheet (a read dimension or relation).
    Read,
    /// Estimated from the ink by the fit.
    Drawn,
    /// A design statement by the user or Claude.
    Stated,
    /// An assumption that completes the drawing (K10).
    Inferred,
}

impl ConstraintTrust {
    /// Every class, in the panel's order.
    pub const ALL: [ConstraintTrust; 4] = [
        ConstraintTrust::Read,
        ConstraintTrust::Drawn,
        ConstraintTrust::Stated,
        ConstraintTrust::Inferred,
    ];

    /// The panel's label.
    pub fn label(self) -> &'static str {
        match self {
            ConstraintTrust::Read => "Read (genuine)",
            ConstraintTrust::Drawn => "Drawn (from the ink)",
            ConstraintTrust::Stated => "Stated (design)",
            ConstraintTrust::Inferred => "Inferred",
        }
    }
}

/// The type of relation, as the panel filters it and one glyph draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConstraintKind {
    /// A line or two points level.
    Horizontal,
    /// A line or two points plumb.
    Vertical,
    /// Two points at one place.
    Coincident,
    /// A line and a curve, or two curves, touching smoothly.
    Tangent,
    /// Equal lengths or radii.
    Equal,
    /// A point on a line or curve.
    OnLine,
    /// Centred, symmetric, midpoint, on a bisector.
    Centred,
    /// Parallel, perpendicular, collinear.
    Orientation,
    /// Sharing a centre.
    Concentric,
    /// A value: a distance, length, radius, angle.
    Distance,
    /// Anything else (fixed, position).
    Other,
}

impl ConstraintKind {
    /// Every type, in the panel's order.
    pub const ALL: [ConstraintKind; 11] = [
        ConstraintKind::Horizontal,
        ConstraintKind::Vertical,
        ConstraintKind::Coincident,
        ConstraintKind::Tangent,
        ConstraintKind::Equal,
        ConstraintKind::OnLine,
        ConstraintKind::Centred,
        ConstraintKind::Orientation,
        ConstraintKind::Concentric,
        ConstraintKind::Distance,
        ConstraintKind::Other,
    ];

    /// The panel's label.
    pub fn label(self) -> &'static str {
        match self {
            ConstraintKind::Horizontal => "Horizontal",
            ConstraintKind::Vertical => "Vertical",
            ConstraintKind::Coincident => "Coincident",
            ConstraintKind::Tangent => "Tangent",
            ConstraintKind::Equal => "Equal",
            ConstraintKind::OnLine => "On line / curve",
            ConstraintKind::Centred => "Centred / symmetric",
            ConstraintKind::Orientation => "Parallel / perpendicular",
            ConstraintKind::Concentric => "Concentric",
            ConstraintKind::Distance => "Distance / size",
            ConstraintKind::Other => "Other",
        }
    }

    /// The type of a relation by its file name (`"vertical_distance"`).
    pub fn of_relation(rel: &str) -> Self {
        match rel {
            "horizontal" | "horizontal_points" => ConstraintKind::Horizontal,
            "vertical" | "vertical_points" => ConstraintKind::Vertical,
            "coincident" => ConstraintKind::Coincident,
            r if r.starts_with("tangent") || r == "smooth" || r == "spline_join" => {
                ConstraintKind::Tangent
            }
            "equal_length" | "equal_radius" => ConstraintKind::Equal,
            "on_line" | "on_circle" => ConstraintKind::OnLine,
            "midpoint" | "midpoint_of" | "symmetric" | "on_bisector" | "equidistant" => {
                ConstraintKind::Centred
            }
            "parallel" | "perpendicular" | "collinear" => ConstraintKind::Orientation,
            "concentric" => ConstraintKind::Concentric,
            "distance"
            | "horizontal_distance"
            | "vertical_distance"
            | "length"
            | "radius"
            | "diameter"
            | "angle"
            | "point_line_distance"
            | "line_distance" => ConstraintKind::Distance,
            _ => ConstraintKind::Other,
        }
    }
}

/// One constraint, ready to show.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintItem {
    /// Its id in its sketch.
    pub id: String,
    /// The view whose sketch holds it (none: the sheet's).
    pub view: Option<String>,
    /// The relation's file name (`"tangent_join"`).
    pub relation: String,
    /// The type, for the glyph and the filter.
    pub kind: ConstraintKind,
    /// The trust class.
    pub trust: ConstraintTrust,
    /// For a read constraint, the trust ladder's level ("read", "overlay",
    /// "second read", ...).
    pub level: Option<String>,
    /// The basis (inferred), or the evidence and the reason, in words.
    pub detail: String,
    /// The redraw nodes it binds, sorted (a dimension's own constraint
    /// includes the dimension's node).
    pub binds: Vec<NodeId>,
    /// Where its icon sits on the paper, mm.
    pub anchor: (f64, f64),
    /// Whether it is a dimension's own constraint.
    pub dimension: bool,
    /// The dimension's node when it is.
    pub dimension_node: Option<NodeId>,
    /// The paper points it binds (the points it names, else its entities'
    /// middles), where a revealed icon's connectors end.
    pub targets: Vec<(f64, f64)>,
    /// For a relation between two points (level, plumb, a distance): the
    /// line it holds them to, drawn dashed when revealed.
    pub relation_line: Option<((f64, f64), (f64, f64))>,
    /// The entities (arcs, circles) whose centre point it names: a centre
    /// line through that centre reveals it.
    pub centre_of: Vec<NodeId>,
}

/// A relation of an item to others that no constraint states, read from the
/// document's own references: an observed rounding to the two entities whose
/// junction it rounds, a centre line to the centres it runs through.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The item.
    pub from: NodeId,
    /// What it is, in words ("rounds the junction of").
    pub what: &'static str,
    /// The nodes it relates to.
    pub to: Vec<NodeId>,
    /// Where the item is on the paper, mm (the connectors start here).
    pub anchor: (f64, f64),
    /// Where it joins them on the paper, mm; none draws no connector.
    pub targets: Vec<(f64, f64)>,
}

/// All the constraints of a document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Constraints {
    items: Vec<ConstraintItem>,
    /// For a centre line or mark, the entities whose centres it runs
    /// through.
    centres: BTreeMap<NodeId, Vec<NodeId>>,
    links: Vec<Link>,
}

/// The ladder level of a read constraint, in words.
fn level_name(t: lmsheets_document::value::provenance::Trust) -> &'static str {
    use lmsheets_document::value::provenance::Trust as T;
    match t {
        T::Inferred => "inferred",
        T::Read => "read",
        T::Overlay => "overlay checked",
        T::SecondRead => "second read",
        T::CrossSheet => "cross-sheet",
        T::Model => "model agrees",
    }
}

/// A measure written for a reader.
fn measure_text(m: &Value) -> String {
    let unit = m["unit"].as_str().unwrap_or("");
    match m["kind"].as_str() {
        Some("printed") => format!("{} {unit}", m["text"].as_str().unwrap_or("?")),
        _ => format!("{} {unit}", m["value"]),
    }
}

/// The basis of an inferred constraint, in words.
fn basis_text(basis: &Value) -> String {
    match basis["kind"].as_str() {
        Some("symmetry") => "symmetry of the genuine dimensions".into(),
        Some("scaled") => format!(
            "scaled from the ink: measured {}, to {}",
            measure_text(&basis["measured"]),
            measure_text(&basis["precision"])
        ),
        Some("standard_practice") => "standard drawing practice".into(),
        Some("other_sheet") => "another sheet says so".into(),
        _ => "unstated".into(),
    }
}

/// The model-frame centre of an entity's own: a representative point.
fn entity_point<F: Frame>(e: &Entity<F>) -> (f64, f64) {
    match e {
        Entity::Point { at } => (at.x, at.y),
        Entity::Line { start, end } => ((start.x + end.x) / 2.0, (start.y + end.y) / 2.0),
        Entity::Arc {
            centre,
            radius,
            start_angle,
            sweep,
        } => {
            let a = start_angle + sweep / 2.0;
            (centre.x + radius * a.cos(), centre.y + radius * a.sin())
        }
        Entity::Circle { centre, radius } => (
            centre.x + radius * std::f64::consts::FRAC_1_SQRT_2,
            centre.y - radius * std::f64::consts::FRAC_1_SQRT_2,
        ),
        Entity::Spline { points } => {
            let p = points[points.len() / 2];
            (p.x, p.y)
        }
    }
}

/// The end of a line or arc named `at` (`"start"` or `"end"`).
fn end_point<F: Frame>(e: &Entity<F>, at: &str) -> Option<(f64, f64)> {
    let end = at == "end";
    match e {
        Entity::Line { start, end: b } => {
            let p = if end { b } else { start };
            Some((p.x, p.y))
        }
        Entity::Arc {
            centre,
            radius,
            start_angle,
            sweep,
        } => {
            let a = if end {
                start_angle + sweep
            } else {
                *start_angle
            };
            Some((centre.x + radius * a.cos(), centre.y + radius * a.sin()))
        }
        _ => None,
    }
}

fn mean(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = points.len() as f64;
    (n > 0.0).then(|| {
        (
            points.iter().map(|p| p.0).sum::<f64>() / n,
            points.iter().map(|p| p.1).sum::<f64>() / n,
        )
    })
}

/// The icon's place in the sketch's own frame, by relation.
fn anchor_in<F: Frame>(sketch: &Sketch<F>, rel: &Value, relation: &str) -> Option<(f64, f64)> {
    let g = sketch.geometry();
    let ent = |key: &str| -> Option<&Entity<F>> {
        let v = &rel[key];
        let id = v.as_str().or_else(|| v["id"].as_str())?;
        g.get(EntityId::new(id))
    };
    let point = |key: &str| -> Option<(f64, f64)> {
        let v = &rel[key];
        if !v.is_object() {
            return None;
        }
        let r: PointRef = serde_json::from_value(v.clone()).ok()?;
        g.position(&r).ok().map(|p| (p.x, p.y))
    };
    let points: Vec<(f64, f64)> = ["a", "b", "p", "q"]
        .iter()
        .filter_map(|k| point(k))
        .collect();
    let reps: Vec<(f64, f64)> = ["l", "m", "c", "d", "arc", "s"]
        .iter()
        .filter_map(|k| ent(k))
        .map(entity_point)
        .collect();
    match relation {
        "tangent_join" => end_point(ent("l")?, rel["at"].as_str()?),
        "tangent_at" => end_point(ent("l")?, rel["at"].as_str()?),
        "tangent_arc_join" => end_point(ent("a")?, rel["at_a"].as_str()?),
        "tangent" => {
            // The line's foot nearest the curve's centre.
            let (Entity::Line { start, end }, Some(c)) = (ent("l")?, ent("c")) else {
                return mean(&reps);
            };
            let centre = match c {
                Entity::Arc { centre, .. } | Entity::Circle { centre, .. } => (centre.x, centre.y),
                other => entity_point(other),
            };
            let (d, w) = (
                (end.x - start.x, end.y - start.y),
                (centre.0 - start.x, centre.1 - start.y),
            );
            let l2 = d.0 * d.0 + d.1 * d.1;
            let t = if l2 > 0.0 {
                ((w.0 * d.0 + w.1 * d.1) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            Some((start.x + d.0 * t, start.y + d.1 * t))
        }
        "radius" | "diameter" => ent("c").map(entity_point),
        "concentric" => match ent("c")? {
            Entity::Arc { centre, .. } | Entity::Circle { centre, .. } => {
                Some((centre.x, centre.y))
            }
            other => Some(entity_point(other)),
        },
        "coincident" | "on_line" | "on_circle" | "midpoint" | "midpoint_of" | "symmetric" => {
            points.first().copied().or_else(|| mean(&reps))
        }
        "on_bisector" => mean(&points[points.len().saturating_sub(2)..]),
        _ => mean(&points).or_else(|| mean(&reps)),
    }
}

/// What a revealed icon connects to, in the sketch's frame: the points the
/// relation names (else the middles of its entities), the line between two
/// points it holds level, plumb or at a distance, and the entities whose
/// centre it names.
#[allow(clippy::type_complexity)]
fn geometry_in<F: Frame>(
    sketch: &Sketch<F>,
    rel: &Value,
    relation: &str,
) -> (
    Vec<(f64, f64)>,
    Option<((f64, f64), (f64, f64))>,
    Vec<EntityId>,
) {
    let g = sketch.geometry();
    let mut points = Vec::new();
    let mut centres = Vec::new();
    for key in ["a", "b", "p", "q"] {
        let v = &rel[key];
        if !v.is_object() {
            continue;
        }
        let Ok(r) = serde_json::from_value::<PointRef>(v.clone()) else {
            continue;
        };
        if let Ok(p) = g.position(&r) {
            points.push((p.x, p.y));
        }
        if r.centre_of().is_some() {
            if let Some(id) = v["id"].as_str() {
                centres.push(EntityId::new(id));
            }
        }
    }
    let line = matches!(
        relation,
        "horizontal_points"
            | "vertical_points"
            | "horizontal_distance"
            | "vertical_distance"
            | "distance"
    ) && points.len() == 2;
    let mut line = line.then(|| (points[0], points[1]));
    // A point to a line, or a line to a line: the segment square to the
    // line(s), from the point (or the first line's middle) to its foot on
    // the line.
    if matches!(relation, "point_line_distance" | "line_distance") {
        let line_of = |key: &str| match rel[key].as_str().and_then(|id| g.get(EntityId::new(id))) {
            Some(Entity::Line { start, end }) => Some(((start.x, start.y), (end.x, end.y))),
            _ => None,
        };
        let (from, onto) = if relation == "point_line_distance" {
            (points.first().copied(), line_of("l"))
        } else {
            let mid = line_of("l").map(|(s, e)| ((s.0 + e.0) / 2.0, (s.1 + e.1) / 2.0));
            (mid, line_of("m"))
        };
        if let (Some(p), Some((s, e))) = (from, onto) {
            let (u, len2) = (
                (e.0 - s.0, e.1 - s.1),
                (e.0 - s.0).powi(2) + (e.1 - s.1).powi(2),
            );
            if len2 > 0.0 {
                let t = ((p.0 - s.0) * u.0 + (p.1 - s.1) * u.1) / len2;
                line = Some((p, (s.0 + t * u.0, s.1 + t * u.1)));
            }
        }
    }
    if points.is_empty() {
        for key in ["l", "m", "c", "d", "arc", "a", "s"] {
            let v = &rel[key];
            let id = v.as_str().or_else(|| v["id"].as_str());
            if let Some(e) = id.and_then(|id| g.get(EntityId::new(id))) {
                points.push(entity_point(e));
            }
        }
    }
    (points, line, centres)
}

/// The provenance of a constraint as a class, level and detail.
fn classify(
    id: &str,
    p: &lmsheets_document::value::provenance::Provenance,
) -> (ConstraintTrust, Option<String>, String) {
    let value = serde_json::to_value(p).unwrap_or(Value::Null);
    let note = p.note_text().unwrap_or("");
    let at = p.evidence().first().map(|c| c.at.as_str()).unwrap_or("");
    let short = |s: &str| -> String {
        let s = s.trim();
        if s.chars().count() > 160 {
            format!("{}...", s.chars().take(160).collect::<String>())
        } else {
            s.to_string()
        }
    };
    match p.method() {
        Method::Inferred { .. } => (
            ConstraintTrust::Inferred,
            None,
            format!("{}. {}", basis_text(&value["method"]["basis"]), short(note)),
        ),
        Method::Read => (
            ConstraintTrust::Read,
            p.trust().map(|t| level_name(t).to_string()),
            format!("evidence: {}", short(at)),
        ),
        _ => {
            let by_person = matches!(p.by(), Agent::User | Agent::Claude);
            let class = if id.starts_with("design_") || by_person {
                ConstraintTrust::Stated
            } else {
                ConstraintTrust::Drawn
            };
            let detail = if note.is_empty() { at } else { note };
            (class, None, short(detail))
        }
    }
}

impl Constraints {
    /// The constraints of `doc`'s sheet sketch and views; `plane` is the
    /// scene's drawing and `table` the node table the binds are indexed in.
    pub fn new(doc: &Document, plane: &Plane, table: &OwnerTable) -> Self {
        // Which constraints are a dimension's, and the dimension's node.
        let mut dims: BTreeMap<(Option<String>, String), NodeId> = BTreeMap::new();
        for (id, d) in doc.dimensions() {
            use lmsheets_document::value::dimension::Of;
            let key = match d.of() {
                Of::Sheet { constraint } => (None, constraint.as_str().to_string()),
                Of::View { view, constraint } => (
                    Some(view.as_str().to_string()),
                    constraint.as_str().to_string(),
                ),
            };
            let k = node_key(plane, doc, Source::Dimension(*id, Part::Line));
            if let Some(n) = table.id(&k.0, &k.1) {
                dims.insert(key, n);
            }
        }
        let mut items = Vec::new();
        let sheet = |p: (f64, f64)| p;
        collect(
            &mut items,
            doc,
            plane,
            table,
            &dims,
            doc.sheet(),
            None,
            &sheet,
        );
        for (vid, view) in doc.views().views() {
            let t = view.transform;
            let to_paper = move |p: (f64, f64)| {
                let q = t.to_paper(lmsheets_document::value::geometry::P2::new(p.0, p.1));
                (q.x, q.y)
            };
            collect(
                &mut items,
                doc,
                plane,
                table,
                &dims,
                &view.sketch,
                Some(vid),
                &to_paper,
            );
        }
        // The centre lines and marks, and the entities whose centres they run
        // through.
        let mut centres: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        let mut through = |kind: &str, name: &str, points: &Value, key: &str| {
            let Some(node) = table.id(kind, name) else {
                return;
            };
            for p in points.as_array().into_iter().flatten() {
                let (Some(view), Some(id)) = (
                    p["view"].as_str(),
                    p[key]["id"].as_str().or_else(|| p["id"].as_str()),
                ) else {
                    continue;
                };
                let k = node_key(
                    plane,
                    doc,
                    Source::View(ViewId::new(view), EntityId::new(id)),
                );
                if let Some(e) = table.id(&k.0, &k.1) {
                    centres.entry(node).or_default().push(e);
                }
            }
        };
        for (id, l) in doc.centre_lines() {
            let v = serde_json::to_value(l).unwrap_or(Value::Null);
            through("centre_line", id.as_str(), &v["through"], "point");
        }
        for (id, m) in doc.centre_marks() {
            let v = serde_json::to_value(m).unwrap_or(Value::Null);
            through("centre_mark", id.as_str(), &v["of"], "id");
        }
        let mut links = Vec::new();
        observed_links(doc, plane, table, &mut links);
        for (from, to) in &centres {
            links.push(Link {
                from: *from,
                what: "runs through the centre of",
                to: to.clone(),
                anchor: (0.0, 0.0),
                targets: Vec::new(),
            });
        }
        Self {
            items,
            centres,
            links,
        }
    }

    /// The links (non-constraint relations) of `nodes`.
    pub fn links_of<'a>(&'a self, nodes: &'a [NodeId]) -> impl Iterator<Item = &'a Link> {
        self.links.iter().filter(move |l| nodes.contains(&l.from))
    }

    /// The constraints that bind `nodes` directly (their icons are the ones
    /// emphasised while those nodes are hovered or pinned): those that bind
    /// a node, and, for a centre line or mark, those that name the centre
    /// of an entity it runs through (the line counts as its centre).
    pub fn revealed(&self, nodes: &[NodeId]) -> Vec<usize> {
        let through: Vec<NodeId> = nodes
            .iter()
            .flat_map(|n| self.centres.get(n).into_iter().flatten().copied())
            .collect();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.binds.iter().any(|b| nodes.contains(b))
                    || c.centre_of.iter().any(|e| through.contains(e))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The connections view: the constraints to show one icon each, joined
    /// to what they bind, even with the icons off, or none.
    ///
    /// While something is pinned they are always the pin's (the pinned
    /// constraint, or the constraints that bind the pinned group); a hover
    /// on top only adds its own minimal highlight, and hovering the pinned
    /// item itself changes nothing. With nothing pinned they are the hovered
    /// group's, and only while Shift is held. While the pointer is on one of
    /// the icons shown (`prev` is what was shown) they stay.
    pub fn connections(&self, m: &Model, prev: Option<&[usize]>) -> Option<Vec<usize>> {
        if !m.hovered_constraints().is_empty() {
            return prev
                .filter(|r| m.hovered_constraints().iter().all(|i| r.contains(i)))
                .map(<[usize]>::to_vec);
        }
        if let Some(p) = m.pinned() {
            return Some(match p.head {
                crate::model::PinHead::Constraint(i) => vec![i],
                crate::model::PinHead::Node(_) => self.revealed(&p.highlight.members),
            });
        }
        (m.hovered().is_some() && m.connections()).then(|| self.revealed(&m.lit().members))
    }

    /// From ready items (tests, and shells with their own).
    pub fn from_items(items: Vec<ConstraintItem>) -> Self {
        Self {
            items,
            centres: BTreeMap::new(),
            links: Vec::new(),
        }
    }

    /// Every constraint, the sheet's first, then each view's.
    pub fn items(&self) -> &[ConstraintItem] {
        &self.items
    }

    /// How many constraints are of a trust class.
    pub fn count_trust(&self, t: ConstraintTrust) -> usize {
        self.items.iter().filter(|i| i.trust == t).count()
    }

    /// How many constraints are of a type.
    pub fn count_kind(&self, k: ConstraintKind) -> usize {
        self.items.iter().filter(|i| i.kind == k).count()
    }

    /// The indices of the constraints whose icons the model shows: their
    /// class and type on, dimensions' own only if asked for, and with "only
    /// on the hovered group" on, those that bind a lit node. A constraint
    /// whose bound nodes are all in hidden layers is not shown.
    pub fn visible(&self, m: &Model, layers: &Layers) -> Vec<usize> {
        if !m.constraints_shown() {
            return Vec::new();
        }
        let lit = m.lit();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                m.constraint_trust_visible(c.trust)
                    && m.constraint_kind_visible(c.kind)
                    && (!c.dimension || m.dimension_constraints())
                    && (!m.constraints_only_on_selection()
                        || c.binds.iter().any(|b| lit.contains(*b)))
                    && (c.binds.is_empty()
                        || c.binds
                            .iter()
                            .any(|b| layers.of_node(*b).is_none_or(|l| m.layer_visible(l))))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The constraints that bind any of `nodes`.
    pub fn binding(&self, nodes: &[NodeId]) -> BTreeSet<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, c)| c.binds.iter().any(|b| nodes.contains(b)))
            .map(|(i, _)| i)
            .collect()
    }
}

/// The observed roundings' links to the two entities of their junction.
fn observed_links(doc: &Document, plane: &Plane, table: &OwnerTable, out: &mut Vec<Link>) {
    for (id, o) in doc.observed() {
        let v = serde_json::to_value(o).unwrap_or(Value::Null);
        let Some(view) = doc.views().view(o.view) else {
            continue;
        };
        let Some(from) = table.id("observed", id.as_str()) else {
            continue;
        };
        let t = view.transform;
        let to_paper = |p: (f64, f64)| {
            let q = t.to_paper(lmsheets_document::value::geometry::P2::new(p.0, p.1));
            (q.x, q.y)
        };
        let (mut to, mut targets) = (Vec::new(), Vec::new());
        for e in v["kind"]["junction"].as_array().into_iter().flatten() {
            let Some(e) = e.as_str() else { continue };
            let eid = EntityId::new(e);
            let k = node_key(plane, doc, Source::View(o.view, eid));
            if let Some(n) = table.id(&k.0, &k.1) {
                to.push(n);
            }
            if let Some(ent) = view.sketch.geometry().get(eid) {
                targets.push(to_paper(entity_point(ent)));
            }
        }
        let c = &v["kind"]["centre"];
        let anchor = match (c["x"].as_f64(), c["y"].as_f64()) {
            (Some(x), Some(y)) => to_paper((x, y)),
            _ => continue,
        };
        out.push(Link {
            from,
            what: "rounds the junction of",
            to,
            anchor,
            targets,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn collect<F: Frame>(
    out: &mut Vec<ConstraintItem>,
    doc: &Document,
    plane: &Plane,
    table: &OwnerTable,
    dims: &BTreeMap<(Option<String>, String), NodeId>,
    sketch: &Sketch<F>,
    view: Option<&ViewId>,
    to_paper: &dyn Fn((f64, f64)) -> (f64, f64),
) {
    for (cid, c) in sketch.constraints() {
        let rel = serde_json::to_value(c.relation()).unwrap_or(Value::Null);
        let relation = rel["rel"].as_str().unwrap_or("unknown").to_string();
        let mut binds: Vec<NodeId> = c
            .relation()
            .operands()
            .iter()
            .filter_map(|(e, _)| {
                let source = match view {
                    Some(v) => Source::View(*v, *e),
                    None => Source::Sheet(*e),
                };
                let k = node_key(plane, doc, source);
                table.id(&k.0, &k.1)
            })
            .collect();
        let key = (
            view.map(|v| v.as_str().to_string()),
            cid.as_str().to_string(),
        );
        let dimension = dims.get(&key).copied();
        binds.extend(dimension);
        binds.sort();
        binds.dedup();
        let (trust, level, detail) = classify(cid.as_str(), c.provenance());
        let anchor = anchor_in(sketch, &rel, &relation).map_or((0.0, 0.0), to_paper);
        let (targets, relation_line, centre_ids) = geometry_in(sketch, &rel, &relation);
        let targets = targets.into_iter().map(to_paper).collect();
        let relation_line = relation_line.map(|(a, b)| (to_paper(a), to_paper(b)));
        let centre_of = centre_ids
            .into_iter()
            .filter_map(|e| {
                let source = match view {
                    Some(v) => Source::View(*v, e),
                    None => Source::Sheet(e),
                };
                let k = node_key(plane, doc, source);
                table.id(&k.0, &k.1)
            })
            .collect();
        out.push(ConstraintItem {
            id: cid.as_str().to_string(),
            view: key.0,
            kind: ConstraintKind::of_relation(&relation),
            relation,
            trust,
            level,
            detail,
            binds,
            anchor,
            dimension: dimension.is_some(),
            dimension_node: dimension,
            targets,
            relation_line,
            centre_of,
        });
    }
}

// ---- the icons: clusters and picking

/// The side of an icon, CSS px.
pub const ICON_PX: f64 = 22.0;

/// A cell of the decluttering grid, CSS px: icons whose anchors fall in one
/// cell are drawn as one icon with a count.
const CELL_PX: f64 = ICON_PX * 1.15;

/// One icon on the screen: its anchor (the members' mean), and who it stands
/// for.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    /// The anchor, paper mm.
    pub at: (f64, f64),
    /// The constraints it stands for, ascending.
    pub members: Vec<usize>,
    /// The glyph: the members' type if they share one, else `Other`.
    pub kind: ConstraintKind,
    /// The colour class: the lowest trust among the members (an inferred
    /// one shows).
    pub trust: ConstraintTrust,
    /// Where the icon sits relative to its anchor, CSS px (zero unless
    /// [`spread`] moved it off a neighbour).
    pub offset: (f64, f64),
}

/// The icons for `shown` constraints at `scale` screen px per mm: those
/// whose anchors fall in one grid cell of the screen are one cluster. The
/// same clusters in every pane, whatever the pane's own units. Deterministic.
pub fn cluster(items: &[ConstraintItem], shown: &[usize], scale: f64) -> Vec<Cluster> {
    let cell = CELL_PX.max(1.0);
    let mut cells: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for &i in shown {
        let a = items[i].anchor;
        let key = (
            (a.0 * scale / cell).floor() as i64,
            (a.1 * scale / cell).floor() as i64,
        );
        cells.entry(key).or_default().push(i);
    }
    cells
        .into_values()
        .map(|members| {
            let pts: Vec<(f64, f64)> = members.iter().map(|&i| items[i].anchor).collect();
            let at = mean(&pts).unwrap_or((0.0, 0.0));
            let first = items[members[0]].kind;
            let kind = if members.iter().all(|&i| items[i].kind == first) {
                first
            } else {
                ConstraintKind::Other
            };
            let trust = members
                .iter()
                .map(|&i| items[i].trust)
                .max()
                .unwrap_or(ConstraintTrust::Read);
            Cluster {
                at,
                members,
                kind,
                trust,
                offset: (0.0, 0.0),
            }
        })
        .collect()
}

/// One icon for each of `shown`, none merged (a revealed set: an entity's
/// own constraints, or an expanded cluster), moved off one another by
/// [`spread`].
pub fn singles(items: &[ConstraintItem], shown: &[usize], scale: f64) -> Vec<Cluster> {
    let mut out: Vec<Cluster> = shown
        .iter()
        .map(|&i| Cluster {
            at: items[i].anchor,
            members: vec![i],
            kind: items[i].kind,
            trust: items[i].trust,
            offset: (0.0, 0.0),
        })
        .collect();
    spread(&mut out, scale);
    out
}

/// Move icons that would overlap off one another: each, in order, takes the
/// first of its own place and then places on a growing spiral round it that
/// clears every icon already placed. In screen px at `scale` px per mm, so
/// the offsets are the same whatever the pane's units. Deterministic.
pub fn spread(clusters: &mut [Cluster], scale: f64) {
    let min = ICON_PX * 1.05;
    let mut placed: Vec<(f64, f64)> = Vec::new();
    for c in clusters.iter_mut() {
        let base = (c.at.0 * scale, c.at.1 * scale);
        let mut chosen = (0.0, 0.0);
        for k in 0..64 {
            let off = if k == 0 {
                (0.0, 0.0)
            } else {
                // A golden-angle spiral: the radius grows with the step.
                let a = k as f64 * 2.399963;
                let r = min * (0.9 + k as f64 / 5.0);
                (r * a.cos(), r * a.sin())
            };
            let p = (base.0 + off.0, base.1 + off.1);
            chosen = off;
            if placed.iter().all(|q| (p.0 - q.0).hypot(p.1 - q.1) >= min) {
                break;
            }
        }
        placed.push((base.0 + chosen.0, base.1 + chosen.1));
        c.offset = chosen;
    }
}

/// The cluster under a screen point: the nearest whose icon covers it.
/// `to_px` carries a paper point to the pane's pixels (CSS px times the
/// device ratio, which `dpr` is, as the icon is that many px across).
pub fn pick_icon(
    clusters: &[Cluster],
    to_px: impl Fn((f64, f64)) -> (f64, f64),
    at: (f64, f64),
    dpr: f64,
) -> Option<usize> {
    let r = ICON_PX * dpr / 2.0 + 2.0 * dpr;
    clusters
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let p = to_px(c.at);
            let p = (p.0 + c.offset.0 * dpr, p.1 + c.offset.1 * dpr);
            let d = (p.0 - at.0).hypot(p.1 - at.1);
            (d <= r).then_some((d, i))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, i)| i)
}

// ---- the glyphs

/// A stroke of a glyph, in units where the icon spans -1 to 1 each way (y
/// down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GlyphPart {
    /// A segment.
    Line((f64, f64), (f64, f64)),
    /// An arc: centre, radius, start angle, sweep (radians).
    Arc((f64, f64), f64, f64, f64),
    /// A filled disc: centre, radius.
    Disc((f64, f64), f64),
}

const TAU: f64 = std::f64::consts::TAU;

/// The strokes that draw a type's glyph inside the icon's ring.
pub fn glyph(kind: ConstraintKind) -> Vec<GlyphPart> {
    use GlyphPart::*;
    match kind {
        ConstraintKind::Horizontal => vec![Line((-0.55, 0.0), (0.55, 0.0))],
        ConstraintKind::Vertical => vec![Line((0.0, -0.55), (0.0, 0.55))],
        ConstraintKind::Coincident => vec![Disc((0.0, 0.0), 0.3), Arc((0.0, 0.0), 0.5, 0.0, TAU)],
        ConstraintKind::Tangent => vec![
            Arc((0.0, -0.18), 0.36, 0.0, TAU),
            Line((-0.6, 0.18), (0.6, 0.18)),
        ],
        ConstraintKind::Equal => vec![
            Line((-0.45, -0.2), (0.45, -0.2)),
            Line((-0.45, 0.2), (0.45, 0.2)),
        ],
        ConstraintKind::OnLine => vec![Line((-0.6, 0.45), (0.6, -0.45)), Disc((0.0, 0.0), 0.22)],
        ConstraintKind::Centred => vec![
            Disc((-0.5, 0.0), 0.17),
            Disc((0.5, 0.0), 0.17),
            Line((0.0, -0.55), (0.0, 0.55)),
        ],
        ConstraintKind::Orientation => vec![
            Line((-0.5, 0.5), (-0.1, -0.5)),
            Line((0.1, 0.5), (0.5, -0.5)),
        ],
        ConstraintKind::Concentric => vec![
            Arc((0.0, 0.0), 0.2, 0.0, TAU),
            Arc((0.0, 0.0), 0.5, 0.0, TAU),
        ],
        ConstraintKind::Distance => vec![
            Line((-0.6, 0.0), (0.6, 0.0)),
            Line((-0.6, -0.3), (-0.6, 0.3)),
            Line((0.6, -0.3), (0.6, 0.3)),
        ],
        ConstraintKind::Other => vec![
            Disc((-0.3, 0.0), 0.13),
            Disc((0.0, 0.0), 0.13),
            Disc((0.3, 0.0), 0.13),
        ],
    }
}

/// The seven-segment strokes of a count (up to 99, then 99), as lines in a
/// box `[0, 1] x [0, 2]` per digit, digits side by side (each 1.5 wide).
pub fn count_digits(n: usize) -> Vec<((f64, f64), (f64, f64))> {
    // Segments: top, upper-left, upper-right, middle, lower-left,
    // lower-right, bottom.
    const SEG: [((f64, f64), (f64, f64)); 7] = [
        ((0.0, 0.0), (1.0, 0.0)),
        ((0.0, 0.0), (0.0, 1.0)),
        ((1.0, 0.0), (1.0, 1.0)),
        ((0.0, 1.0), (1.0, 1.0)),
        ((0.0, 1.0), (0.0, 2.0)),
        ((1.0, 1.0), (1.0, 2.0)),
        ((0.0, 2.0), (1.0, 2.0)),
    ];
    const ON: [[bool; 7]; 10] = [
        [true, true, true, false, true, true, true],
        [false, false, true, false, false, true, false],
        [true, false, true, true, true, false, true],
        [true, false, true, true, false, true, true],
        [false, true, true, true, false, true, false],
        [true, true, false, true, false, true, true],
        [true, true, false, true, true, true, true],
        [true, false, true, false, false, true, false],
        [true, true, true, true, true, true, true],
        [true, true, true, true, false, true, true],
    ];
    let n = n.min(99);
    let digits: Vec<usize> = if n >= 10 {
        vec![n / 10, n % 10]
    } else {
        vec![n]
    };
    let mut out = Vec::new();
    for (k, d) in digits.iter().enumerate() {
        for (s, on) in ON[*d].iter().enumerate() {
            if *on {
                let (a, b) = SEG[s];
                let dx = k as f64 * 1.5;
                out.push(((a.0 + dx, a.1), (b.0 + dx, b.1)));
            }
        }
    }
    out
}
