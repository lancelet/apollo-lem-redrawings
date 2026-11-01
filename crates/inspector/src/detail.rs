//! The detail panel's data for a pinned item (docs/redraw-plan.md §5,
//! "Feature detail panel"): what it is, its geometry, the constraints on it,
//! the dimensions that reference it, its evidence, its style, the gaps that
//! name it and its paper trail. Pure data (sections of rows) built from the
//! document; the chrome only draws it. Every referenced item is a row with a
//! [`PinHead`], a link that pins it.
//!
//! The trail is the document's own walker ([`Document::trail_of`]), the one
//! the acceptance check A8 uses, so the panel and A8 agree by construction.
//! What the document does not record is shown as "not recorded", never
//! recomputed here: a constraint's residual (it is in `solve-report`, not in
//! the file), and a departure from the ink fit as numbers (it is in the
//! provenance note's prose, which is quoted; the rms and maximum are read
//! from that sentence when it has the usual form).

use crate::constraints::{ConstraintItem, Constraints};
use crate::group::Groups;
use crate::layers::Layers;
use crate::model::{NodeId, PinHead};
use crate::pick::OwnerTable;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::claims::Node;
use lmsheets_document::value::geometry::{Entity, EntityId, Frame, PointRef};
use lmsheets_document::value::hops::What;
use lmsheets_document::value::sketch::Sketch;
use lmsheets_document::value::view::ViewId;
use lmsheets_render::scene::Plane;
use serde_json::Value;

/// The text shown for what the document does not hold.
pub const NOT_RECORDED: &str = "not recorded";

/// One line of the panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// What it is.
    pub key: String,
    /// Its value.
    pub value: String,
    /// What a click on it pins.
    pub pin: Option<PinHead>,
    /// A web address it opens.
    pub href: Option<String>,
}

/// A titled run of rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// The title.
    pub title: &'static str,
    /// The rows.
    pub rows: Vec<Row>,
}

/// The panel for one pinned item.
#[derive(Clone, Debug, PartialEq)]
pub struct Detail {
    /// The heading: kind and id.
    pub title: String,
    /// The sections, in order.
    pub sections: Vec<Section>,
}

impl Detail {
    /// The section called `title`.
    pub fn section(&self, title: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.title == title)
    }
}

/// A dimension's residual against the ink: what was read, what the ink
/// measures, the difference in inches and in scan px, the scale it was
/// measured at, and how hard the solve was pulled to meet it.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionResidual {
    /// The read value, in words.
    pub read: String,
    /// What the ink measures, in words.
    pub ink: String,
    /// Read less ink, inches.
    pub delta_in: f64,
    /// The same, scan px.
    pub delta_px: f64,
    /// The scale this dimension alone gives, paper mm per part mm.
    pub scale_sample: f64,
    /// The scale the other dimensions give without it.
    pub scale_without: Option<f64>,
    /// The constraint's pull (its multiplier) in the solve.
    pub pull: f64,
}

/// A constraint's residual against the ink.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintResidual {
    /// How far the ink is from satisfying it, scan px.
    pub ink_px: f64,
    /// Its pull in the solve.
    pub pull: f64,
}

/// An entity's departure from its ink fit.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityDeparture {
    /// RMS, scan px.
    pub rms_px: f64,
    /// Largest, scan px.
    pub max_px: f64,
    /// What moved it most, in words (none when nothing is recorded).
    pub moved_by: Option<String>,
}

/// The residuals the document records (`Document::residuals()`), as the
/// panel asks for them. Where an answer is none, the panel says "not
/// recorded"; it never computes one.
pub trait Residuals {
    /// A dimension's, by its name.
    fn dimension(&self, name: &str) -> Option<DimensionResidual>;
    /// A constraint's, by its id in its view (none: the sheet's).
    fn constraint(&self, view: Option<&str>, id: &str) -> Option<ConstraintResidual>;
    /// An entity's departure, by its node name (`view/id`).
    fn entity(&self, name: &str) -> Option<EntityDeparture>;
}

/// The residuals the document stores ([`Document::residuals`]) as the
/// panel asks for them.
pub struct DocResiduals<'a>(pub &'a lmsheets_document::value::residuals::Residuals);

impl Residuals for DocResiduals<'_> {
    fn dimension(&self, name: &str) -> Option<DimensionResidual> {
        let d = self.0.dimension(name)?;
        Some(DimensionResidual {
            read: format!("{:.4} in", d.read_in),
            ink: format!("{:.4} in", d.ink_in),
            delta_in: d.delta_in,
            delta_px: d.delta_px,
            scale_sample: d.scale_sample,
            scale_without: d.scale_without,
            pull: d.pull,
        })
    }

    fn constraint(&self, view: Option<&str>, id: &str) -> Option<ConstraintResidual> {
        if view? != self.0.view.as_str() {
            return None;
        }
        match self.0.relation(id) {
            Some(r) => Some(ConstraintResidual {
                ink_px: r.ink_px,
                pull: r.pull,
            }),
            // A dimension's own constraint: its residual is the dimension's.
            None => self.0.dimension(id).map(|d| ConstraintResidual {
                ink_px: d.delta_px.abs(),
                pull: d.pull,
            }),
        }
    }

    fn entity(&self, name: &str) -> Option<EntityDeparture> {
        let (view, id) = name.split_once('/')?;
        if view != self.0.view.as_str() {
            return None;
        }
        let e = self.0.entity(id)?;
        Some(EntityDeparture {
            rms_px: e.rms_px,
            max_px: e.max_px,
            moved_by: e
                .moved_by
                .as_ref()
                .map(|m| format!("{} ({:.1} px of the rms)", m.constraint.as_str(), m.rms_px)),
        })
    }
}

/// What the panel reads.
pub struct Source<'a> {
    /// The document.
    pub doc: &'a Document,
    /// The scene's drawing.
    pub plane: &'a Plane,
    /// The node table.
    pub table: &'a OwnerTable,
    /// The constraint list.
    pub constraints: &'a Constraints,
    /// The layer classification.
    pub layers: &'a Layers,
    /// The selection groups.
    pub groups: &'a Groups,
    /// Crop px per paper mm at a paper point (the warp's local scale).
    pub scan_per_mm: &'a dyn Fn((f64, f64)) -> f64,
    /// The residuals the document records, if it has any.
    pub residuals: Option<&'a dyn Residuals>,
}

impl Source<'_> {
    /// The rows of a dimension's residual: its own, or one "not recorded".
    fn dimension_residual_rows(&self, name: &str) -> Vec<Row> {
        match self.residuals.and_then(|r| r.dimension(name)) {
            Some(r) => vec![
                row("read", r.read),
                row("ink", r.ink),
                row(
                    "difference",
                    format!("{:+.4} in = {:+.1} px", r.delta_in, r.delta_px),
                ),
                row(
                    "scale sample",
                    match r.scale_without {
                        Some(w) => format!("{:.4}:1 ({w:.4}:1 without it)", r.scale_sample),
                        None => format!("{:.4}:1", r.scale_sample),
                    },
                ),
                row("pull", format!("{:.4}", r.pull)),
            ],
            None => vec![row("residual", NOT_RECORDED)],
        }
    }

    /// A constraint's residual in a line.
    fn constraint_residual_text(&self, c: &ConstraintItem) -> String {
        match self
            .residuals
            .and_then(|r| r.constraint(c.view.as_deref(), &c.id))
        {
            Some(r) => format!("ink residual {:.2} px, pull {:.4}", r.ink_px, r.pull),
            None => NOT_RECORDED.to_string(),
        }
    }
}

fn row(key: &str, value: impl Into<String>) -> Row {
    Row {
        key: key.into(),
        value: value.into(),
        ..Row::default()
    }
}

fn link(key: &str, value: impl Into<String>, pin: PinHead) -> Row {
    Row {
        pin: Some(pin),
        ..row(key, value)
    }
}

/// The claim node a table node is, if the document's claims can name it:
/// the owner map's kinds are the claims' (snake case).
pub fn claim_node(kind: &str, name: &str) -> Option<Node> {
    serde_json::from_value(serde_json::json!({ "kind": kind, "name": name })).ok()
}

/// `x mm (y in)`.
fn mm_in(mm: f64) -> String {
    format!("{mm:.3} mm ({:.4} in)", mm / 25.4)
}

fn point_text(p: (f64, f64)) -> String {
    format!("({:.3}, {:.3})", p.0, p.1)
}

/// The departure from the ink fit a provenance note records, in px: the
/// rms and the maximum, from "by 5.1 px rms, 5.1 px at most".
pub fn departure(note: &str) -> Option<(f64, f64)> {
    let at = note.find("by ")?;
    let rest = &note[at + 3..];
    let (rms, rest) = rest.split_once(" px rms, ")?;
    let (max, _) = rest.split_once(" px at most")?;
    Some((rms.trim().parse().ok()?, max.trim().parse().ok()?))
}

/// A string cut at `n` characters, with an ellipsis.
fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>())
    }
}

/// The geometry of a view entity, in the model's mm and on the paper.
fn entity_rows<F: Frame>(
    e: &Entity<F>,
    to_paper: &dyn Fn((f64, f64)) -> (f64, f64),
    out: &mut Vec<Row>,
) {
    let both = |key: &str, p: (f64, f64), out: &mut Vec<Row>| {
        out.push(row(
            key,
            format!("{} mm, paper {}", point_text(p), point_text(to_paper(p))),
        ));
    };
    match e {
        Entity::Point { at } => both("at", (at.x, at.y), out),
        Entity::Line { start, end } => {
            both("start", (start.x, start.y), out);
            both("end", (end.x, end.y), out);
            out.push(row(
                "length",
                mm_in((end.x - start.x).hypot(end.y - start.y)),
            ));
        }
        Entity::Arc {
            centre,
            radius,
            start_angle,
            sweep,
        } => {
            both("centre", (centre.x, centre.y), out);
            out.push(row("radius", mm_in(*radius)));
            out.push(row(
                "sweep",
                format!(
                    "{:.2} deg from {:.2} deg",
                    sweep.to_degrees(),
                    start_angle.to_degrees()
                ),
            ));
        }
        Entity::Circle { centre, radius } => {
            both("centre", (centre.x, centre.y), out);
            out.push(row("radius", mm_in(*radius)));
        }
        Entity::Spline { points } => out.push(row("points", points.len().to_string())),
    }
}

/// The solved value of a dimension's relation, mm, measured on the stored
/// geometry (not re-solved).
fn solved_mm<F: Frame>(sketch: &Sketch<F>, rel: &Value) -> Option<f64> {
    let g = sketch.geometry();
    let point = |key: &str| -> Option<(f64, f64)> {
        let r: PointRef = serde_json::from_value(rel.get(key)?.clone()).ok()?;
        g.position(&r).ok().map(|p| (p.x, p.y))
    };
    let entity = |key: &str| -> Option<&Entity<F>> {
        let v = rel.get(key)?;
        g.get(EntityId::new(v.as_str().or_else(|| v["id"].as_str())?))
    };
    match rel["rel"].as_str()? {
        "horizontal_distance" => Some((point("b")?.0 - point("a")?.0).abs()),
        "vertical_distance" => Some((point("b")?.1 - point("a")?.1).abs()),
        "distance" => {
            let (a, b) = (point("a")?, point("b")?);
            Some((b.0 - a.0).hypot(b.1 - a.1))
        }
        "radius" => match entity("c")? {
            Entity::Arc { radius, .. } | Entity::Circle { radius, .. } => Some(*radius),
            _ => None,
        },
        "diameter" => match entity("c")? {
            Entity::Arc { radius, .. } | Entity::Circle { radius, .. } => Some(radius * 2.0),
            _ => None,
        },
        "length" => match entity("l")? {
            Entity::Line { start, end } => Some((end.x - start.x).hypot(end.y - start.y)),
            _ => None,
        },
        _ => None,
    }
}

/// The printed value of a relation, as the sheet states it.
fn read_value(rel: &Value) -> Option<String> {
    let v = rel.get("value")?;
    let unit = v["unit"].as_str().unwrap_or("");
    Some(match v["kind"].as_str() {
        Some("printed") => format!("{} {unit}", v["text"].as_str()?),
        _ => format!("{} {unit}", v["value"]),
    })
}

/// A dimension's read and solved values, as rows.
fn dimension_value(doc: &Document, name: &str) -> Option<(String, Option<f64>)> {
    use lmsheets_document::value::dimension::{DimensionId, Of};
    let d = doc.dimension(DimensionId::new(name))?;
    let c = doc.constraint_of(d.of())?;
    let rel = serde_json::to_value(c.relation()).ok()?;
    let read = read_value(&rel)?;
    let solved = match d.of() {
        Of::Sheet { .. } => solved_mm(doc.sheet(), &rel),
        Of::View { view, .. } => doc
            .views()
            .view(view)
            .and_then(|v| solved_mm(&v.sketch, &rel)),
    };
    Some((read, solved))
}

/// The panel for `head`.
pub fn detail(src: &Source, head: PinHead) -> Detail {
    match head {
        PinHead::Node(n) => node_detail(src, n),
        PinHead::Constraint(i) => constraint_detail(src, i),
    }
}

fn node_name(src: &Source, n: NodeId) -> String {
    src.table
        .get(n)
        .map_or(format!("node {}", n.0), |(k, name)| format!("{k} {name}"))
}

fn constraint_detail(src: &Source, i: usize) -> Detail {
    let Some(c) = src.constraints.items().get(i) else {
        return Detail {
            title: format!("constraint {i}"),
            sections: Vec::new(),
        };
    };
    let mut what = vec![
        row("relation", c.relation.clone()),
        row("type", c.kind.label()),
        row(
            "trust",
            match &c.level {
                Some(l) => format!("{} ({l})", c.trust.label()),
                None => c.trust.label().to_string(),
            },
        ),
        row("basis / evidence", c.detail.clone()),
        row("residual", src.constraint_residual_text(c)),
        row("icon at", format!("paper {}", point_text(c.anchor))),
    ];
    if let Some(v) = &c.view {
        what.insert(0, row("view", v.clone()));
    }
    let binds = c
        .binds
        .iter()
        .map(|b| link("binds", node_name(src, *b), PinHead::Node(*b)))
        .collect();
    let mut sections = vec![
        Section {
            title: "Constraint",
            rows: what,
        },
        Section {
            title: "Binds",
            rows: binds,
        },
    ];
    if let Some(d) = c.dimension_node {
        sections.push(Section {
            title: "Dimension",
            rows: vec![link("shown as", node_name(src, d), PinHead::Node(d))],
        });
    }
    Detail {
        title: format!("constraint {}", c.id),
        sections,
    }
}

fn node_detail(src: &Source, n: NodeId) -> Detail {
    let doc = src.doc;
    let Some((kind, name)) = src.table.get(n) else {
        return Detail {
            title: format!("node {}", n.0),
            sections: Vec::new(),
        };
    };
    let (kind, name) = (kind.to_string(), name.to_string());
    let mut sections = Vec::new();

    // What it is.
    let mut what = vec![row("kind", kind.clone()), row("id", name.clone())];
    if let Some(l) = src.layers.of_node(n) {
        what.push(row("layer", l.label()));
    }
    let group = src.groups.highlight(n, false);
    for m in group.members.iter().filter(|m| **m != n) {
        what.push(link("with", node_name(src, *m), PinHead::Node(*m)));
    }
    for m in &group.serving {
        what.push(link("served by", node_name(src, *m), PinHead::Node(*m)));
    }
    sections.push(Section {
        title: "What it is",
        rows: what,
    });

    // Geometry, and the item's own record (layer, note) as JSON.
    let mut geometry = Vec::new();
    let mut style = Vec::new();
    let mut note: Option<String> = None;
    let mut anchor_paper: Option<(f64, f64)> = None;
    match kind.as_str() {
        "entity" | "marking_region" => {
            let key = name.split_once('/');
            let found = key.and_then(|(v, e)| {
                let view = doc.views().view(ViewId::new(v))?;
                let id = EntityId::new(e);
                let ent = view.sketch.geometry().get(id)?;
                Some((view, id, ent))
            });
            if let Some((view, id, ent)) = found {
                let t = view.transform;
                let to_paper = move |p: (f64, f64)| {
                    let q = t.to_paper(lmsheets_document::value::geometry::P2::new(p.0, p.1));
                    (q.x, q.y)
                };
                entity_rows(ent, &to_paper, &mut geometry);
                if let Some(meta) = view.sketch.meta(id) {
                    style.push(row("layer", meta.layer.as_str()));
                    if let Some(l) = doc.layer(meta.layer) {
                        style.push(row(
                            "pen",
                            format!("{:.2} mm {:?}", l.style.weight, l.style.dash),
                        ));
                        if let Some(c) = meta.convention_on(l) {
                            style.push(row("line kind", format!("{c:?}")));
                        }
                    }
                    note = meta.provenance.note_text().map(str::to_string);
                }
                anchor_paper = Some(to_paper(crate::detail::entity_mid(ent)));
            }
        }
        "observed" => {
            use lmsheets_document::value::observed::ObservedId;
            if let Some(o) = doc.observed().get(&ObservedId::new(&name)) {
                let v = serde_json::to_value(o).unwrap_or(Value::Null);
                geometry.push(row("status", "seen, not intended: not in the solve"));
                geometry.push(row("kind", v["kind"]["kind"].as_str().unwrap_or("?")));
                if let Some(r) = v["kind"]["radius"].as_f64() {
                    geometry.push(row("radius drawn", format!("{} (part)", mm_in(r))));
                }
                if let (Some(x), Some(y), Some(view)) = (
                    v["kind"]["centre"]["x"].as_f64(),
                    v["kind"]["centre"]["y"].as_f64(),
                    doc.views().view(o.view),
                ) {
                    let q = view
                        .transform
                        .to_paper(lmsheets_document::value::geometry::P2::new(x, y));
                    geometry.push(row(
                        "fitted centre",
                        format!(
                            "{} mm, paper {}",
                            point_text((x, y)),
                            point_text((q.x, q.y))
                        ),
                    ));
                    anchor_paper = Some((q.x, q.y));
                }
                let fit = &v["kind"]["fit"];
                let mm = |k: &str| fit[k].as_f64().map_or(NOT_RECORDED.to_string(), mm_in);
                geometry.push(row("fit: ink radius", mm("ink_radius_mm")));
                geometry.push(row("fit: pen taken off", mm("pen_mm")));
                geometry.push(row("fit loss if ignored", mm("loss_mm")));
                geometry.push(row("fit rms sharp", mm("rms_sharp_mm")));
                geometry.push(row("fit rms rounded", mm("rms_rounded_mm")));
                geometry.push(row("fit points", fit["points"].to_string()));
                if let Some(g) = &o.shared {
                    geometry.push(row("shared radius", format!("group {g}")));
                }
                style.push(row("layer", o.layer.as_str()));
                style.push(row(
                    "drawn",
                    if o.drawn {
                        "in place of the sharp corner"
                    } else {
                        "not drawn"
                    },
                ));
                note = o.provenance.note_text().map(str::to_string);
            }
        }
        "dimension" => {
            if let Some((read, solved)) = dimension_value(doc, &name) {
                geometry.push(row("read value", read));
                geometry.push(row(
                    "solved value",
                    solved.map_or(NOT_RECORDED.to_string(), mm_in),
                ));
                geometry.extend(src.dimension_residual_rows(&name));
            }
            if let Some(d) =
                doc.dimension(lmsheets_document::value::dimension::DimensionId::new(&name))
            {
                style.push(row("layer", d.layer().as_str()));
                note = d.provenance().note_text().map(str::to_string);
            }
        }
        "centre_line" => {
            if let Some(cl) =
                doc.centre_line(lmsheets_document::value::centre::CentreLineId::new(&name))
            {
                let v = serde_json::to_value(cl).unwrap_or(Value::Null);
                if let Some(a) = v["angle"].as_f64() {
                    geometry.push(row("angle", format!("{:.4} deg", a.to_degrees())));
                }
                geometry.push(row(
                    "extends",
                    format!(
                        "{:.2} mm before, {:.2} mm after its centre",
                        v["before"], v["after"]
                    ),
                ));
                style.push(row("layer", v["layer"].as_str().unwrap_or("?")));
                note = cl.provenance().note_text().map(str::to_string);
            }
        }
        _ => {}
    }
    if geometry.is_empty() {
        geometry.push(row("geometry", NOT_RECORDED));
    }
    sections.push(Section {
        title: "Geometry",
        rows: geometry,
    });
    if let Some(p) = anchor_paper {
        let s = (src.scan_per_mm)(p);
        sections.push(Section {
            title: "Scale sample",
            rows: vec![row(
                "scan scale here",
                format!("{s:.3} px per mm ({:.1} px per in)", s * 25.4),
            )],
        });
    }

    // What it relates to in the document where no constraint says so.
    let links: Vec<Row> = src
        .constraints
        .links_of(&[n])
        .flat_map(|l| l.to.iter().map(move |t| (l.what, *t)))
        .map(|(what, t)| link(what, node_name(src, t), PinHead::Node(t)))
        .collect();
    if !links.is_empty() {
        sections.push(Section {
            title: "Relates to",
            rows: links,
        });
    }

    // The constraints on it, and the dimensions that reference it.
    let mut on = Vec::new();
    let mut dims = Vec::new();
    for (i, c) in src.constraints.items().iter().enumerate() {
        if c.dimension_node == Some(n) {
            on.push(constraint_row(src, c, i));
        } else if c.binds.contains(&n) {
            on.push(constraint_row(src, c, i));
            if let Some(d) = c.dimension_node {
                let (_, dname) = src.table.get(d).unwrap_or(("", ""));
                let value = dimension_value(doc, dname).map_or(String::new(), |(read, solved)| {
                    let residual = match src.residuals.and_then(|r| r.dimension(dname)) {
                        Some(r) => format!("{:+.1} px from the ink", r.delta_px),
                        None => format!("residual {NOT_RECORDED}"),
                    };
                    format!(
                        "read {read}, solved {}, {residual}",
                        solved.map_or(NOT_RECORDED.to_string(), mm_in)
                    )
                });
                dims.push(link(dname, value, PinHead::Node(d)));
            }
        }
    }
    if on.is_empty() {
        on.push(row("constraints", "none bind it"));
    }
    sections.push(Section {
        title: "Constraints",
        rows: on,
    });
    if !dims.is_empty() {
        sections.push(Section {
            title: "Dimensions referencing it",
            rows: dims,
        });
    }

    // Evidence: its claims, and the departure from the fit.
    let claim = claim_node(&kind, &name);
    let mut evidence = Vec::new();
    if let Some(node) = &claim {
        for t in doc.trail_of(node) {
            let (px, segs) = t.ink();
            evidence.push(row(
                "claim",
                format!(
                    "{:?}{}: {segs} segment(s) and {px} px in runs, of frame {}",
                    t.claim.part,
                    t.claim.by.map_or(String::new(), |b| format!(" by {b:?}")),
                    t.claim.evidence.as_str()
                ),
            ));
        }
    }
    if evidence.is_empty() {
        evidence.push(row("claims", "none on this node"));
    }
    match (
        src.residuals.and_then(|r| r.entity(&name)),
        note.as_deref().and_then(departure),
    ) {
        (Some(d), _) => {
            evidence.push(row("departure rms", format!("{:.2} px", d.rms_px)));
            evidence.push(row("departure max", format!("{:.2} px", d.max_px)));
            if let Some(m) = d.moved_by {
                evidence.push(row("moved most by", m));
            }
        }
        (None, Some((rms, max))) => {
            evidence.push(row("departure rms", format!("{rms} px (from the note)")));
            evidence.push(row("departure max", format!("{max} px")));
        }
        (None, None) => evidence.push(row(
            "departure",
            if note.is_some() {
                "see the reason"
            } else {
                NOT_RECORDED
            },
        )),
    }
    if let Some(n) = &note {
        evidence.push(row("reason", cut(n, 600)));
    }
    sections.push(Section {
        title: "Evidence",
        rows: evidence,
    });
    if !style.is_empty() {
        sections.push(Section {
            title: "Style",
            rows: style,
        });
    }

    // Gaps that name it.
    if let Some(node) = &claim {
        let gaps: Vec<Row> = doc
            .gaps()
            .iter()
            .filter(|(_, g)| g.nodes.contains(node))
            .map(|(id, g)| {
                row(
                    id.as_str(),
                    format!("{:?}: {}", g.status, cut(&g.what, 280)),
                )
            })
            .collect();
        if !gaps.is_empty() {
            sections.push(Section {
                title: "Open gaps",
                rows: gaps,
            });
        }
    }

    // The paper trail.
    if let Some(node) = &claim {
        let mut trail = Vec::new();
        for t in doc.trail_of(node) {
            trail.push(row(
                "claim",
                format!("{:?} of {} {}", t.claim.part, kind, name),
            ));
            match t.frame {
                Some((id, f)) => {
                    trail.push(row("frame", format!("{} ({})", id.as_str(), f.file)));
                    let r = &f.registration;
                    trail.push(row(
                        "crop",
                        format!("{:?} px of the frame", r.crop.map.box_px),
                    ));
                    let fit =
                        |label: &str, res: Option<lmsheets_document::value::frame::Residual>| {
                            row(
                                label,
                                res.map_or(NOT_RECORDED.to_string(), |x| {
                                    format!("rms {:.3} px over {} samples", x.rms_px, x.samples)
                                }),
                            )
                        };
                    trail.push(fit("warp fit", r.warp.residual));
                    trail.push(fit("paper fit", r.paper.residual));
                    trail.push(row(
                        "frame hash",
                        f.hash.clone().unwrap_or_else(|| NOT_RECORDED.into()),
                    ));
                    for h in &f.hops {
                        trail.push(match &h.what {
                            What::Card { nara_box } => {
                                row("hop: card", format!("NARA box {nara_box}"))
                            }
                            What::Upload(u) => row(
                                "hop: upload",
                                format!(
                                    "{} / {} ({})",
                                    u.item,
                                    cut(&u.member, 60),
                                    u.original.hash
                                ),
                            ),
                            What::Derived(d) => {
                                row("hop: derived", format!("{} ({})", d.made, d.file))
                            }
                        });
                    }
                }
                None => trail.push(row("frame", NOT_RECORDED)),
            }
            match t.upload {
                Ok(u) => trail.push(Row {
                    key: "archive".into(),
                    value: u.original.url.clone(),
                    href: Some(u.original.url.clone()),
                    ..Row::default()
                }),
                Err(e) => trail.push(row("archive", format!("the hops do not reach it: {e:?}"))),
            }
        }
        if !trail.is_empty() {
            sections.push(Section {
                title: "Paper trail",
                rows: trail,
            });
        }
    }

    Detail {
        title: format!("{kind} {name}"),
        sections,
    }
}

fn constraint_row(src: &Source, c: &ConstraintItem, i: usize) -> Row {
    let trust = match &c.level {
        Some(l) => format!("{} ({l})", c.trust.label()),
        None => c.trust.label().to_string(),
    };
    link(
        c.kind.label(),
        format!(
            "{} [{}] {}; residual: {}",
            c.id,
            trust,
            cut(&c.detail, 120),
            src.constraint_residual_text(c)
        ),
        PinHead::Constraint(i),
    )
}

/// A representative point of an entity in its own frame.
pub fn entity_mid<F: Frame>(e: &Entity<F>) -> (f64, f64) {
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
        Entity::Circle { centre, .. } => (centre.x, centre.y),
        Entity::Spline { points } => {
            let p = points[points.len() / 2];
            (p.x, p.y)
        }
    }
}
