//! From a document to the drawing plane: every entity, text and leader
//! whose layer is visible for the purpose, in paper millimetres.

use crate::leader::Landing;
use crate::rounding::Rounded;
use crate::scene::{Item, Path, Plane, PlaneKind, Primitive, Run, Source, Termination};
use crate::text::Local;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::geometry::{Entity, Frame, Paper, P2};
use lmsheets_document::value::leader::{Along, DrawingEntity, Origin, Target};
use lmsheets_document::value::sketch::Sketch;
use lmsheets_document::value::style::{Dash, LineStyle, Purpose};
use lmsheets_document::value::text::{Anchor, DrawingPoint, Placement, Side, Text, TextId};
use lmsheets_document::value::view::{Transform, ViewId};
use std::collections::BTreeMap;

/// How model coordinates reach the paper: the sheet's are already there;
/// a view's go through its transform.
#[derive(Clone, Copy)]
pub(crate) enum Onto {
    Sheet,
    View(Transform),
}

impl Onto {
    pub(crate) fn point<F: Frame>(&self, p: P2<F>) -> P2<Paper> {
        match self {
            Onto::Sheet => P2::new(p.x, p.y),
            Onto::View(t) => t.to_paper(P2::new(p.x, p.y)),
        }
    }

    /// A direction in the source frame as a unit direction on the paper.
    pub(crate) fn dir(&self, x: f64, y: f64) -> P2<Paper> {
        let o = self.point(P2::<Paper>::new(0.0, 0.0));
        let p = self.point(P2::<Paper>::new(x, y));
        let (dx, dy) = (p.x - o.x, p.y - o.y);
        let len = (dx * dx + dy * dy).sqrt();
        P2::new(dx / len, dy / len)
    }

    fn length(&self, r: f64) -> f64 {
        match self {
            Onto::Sheet => r,
            Onto::View(t) => r * t.scale,
        }
    }

    /// An angle measured in the source frame, on the paper: mirrored if
    /// the view's y points up the page, then turned with the view.
    fn angle(&self, a: f64) -> f64 {
        match self {
            Onto::Sheet => a,
            Onto::View(t) => t.rotation + if t.mirror { -a } else { a },
        }
    }

    fn sweep(&self, s: f64) -> f64 {
        match self {
            Onto::Sheet => s,
            Onto::View(t) => {
                if t.mirror {
                    -s
                } else {
                    s
                }
            }
        }
    }
}

/// The curve of an entity on the paper; a point has none.
pub(crate) fn path_of<F: Frame>(e: &Entity<F>, onto: Onto) -> Option<Path> {
    Some(match *e {
        Entity::Point { .. } => return None,
        Entity::Line { start, end } => Path::Line {
            a: onto.point(start),
            b: onto.point(end),
        },
        Entity::Arc {
            centre,
            radius,
            start_angle,
            sweep,
        } => Path::Arc {
            centre: onto.point(centre),
            radius: onto.length(radius),
            start: onto.angle(start_angle),
            sweep: onto.sweep(sweep),
        },
        Entity::Circle { centre, radius } => Path::Circle {
            centre: onto.point(centre),
            radius: onto.length(radius),
        },
        // A spline is several curves; see `paths_of`.
        Entity::Spline { .. } => return None,
    })
}

/// The curves of an entity on the paper: one for most, one cubic per
/// segment for a spline, none for a point.
pub(crate) fn paths_of<F: Frame>(e: &Entity<F>, onto: Onto) -> Vec<Path> {
    match e {
        Entity::Spline { points } => points
            .windows(4)
            .step_by(3)
            .map(|w| Path::Cubic {
                a: onto.point(w[0]),
                c1: onto.point(w[1]),
                c2: onto.point(w[2]),
                b: onto.point(w[3]),
            })
            .collect(),
        _ => path_of(e, onto).into_iter().collect(),
    }
}

/// The primitives for an entity, placed on the paper: a marker for a
/// point, a stroke per curve otherwise (one per segment of a spline).
fn primitives<F: Frame>(e: &Entity<F>, onto: Onto, style: &LineStyle) -> Vec<Primitive> {
    let weight = style.weight;
    if let Entity::Point { at } = e {
        return vec![Primitive::Marker {
            at: onto.point(*at),
            marker: style.points,
            weight,
        }];
    }
    paths_of(e, onto)
        .into_iter()
        .map(|path| Primitive::Stroke {
            path,
            weight,
            dash: style.dash.clone(),
            anchors: Vec::new(),
        })
        .collect()
}

/// Where the circles and arcs of a sketch have their centres on the
/// line from `a` to `b`, as distances along it on the paper, in order:
/// a centre line through a circle crosses its centre on a short dash.
fn centres_on<F: Frame>(sketch: &Sketch<F>, onto: Onto, a: P2<Paper>, b: P2<Paper>) -> Vec<f64> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len <= 0.0 {
        return Vec::new();
    }
    let (ux, uy) = (dx / len, dy / len);
    let mut out: Vec<f64> = sketch
        .entities()
        .filter_map(|(_, e, _)| match e {
            Entity::Circle { centre, .. } | Entity::Arc { centre, .. } => Some(onto.point(*centre)),
            _ => None,
        })
        .filter_map(|c| {
            let (px, py) = (c.x - a.x, c.y - a.y);
            let t = px * ux + py * uy;
            let off = (px * uy - py * ux).abs();
            (off < 1e-6 && t > 1e-9 && t < len - 1e-9).then_some(t)
        })
        .collect();
    out.sort_by(f64::total_cmp);
    out.dedup_by(|p, q| (*p - *q).abs() < 1e-6);
    out
}

/// The items of one sketch whose layer is on for the purpose, each drawn
/// in its convention's style on its layer
/// ([`style_on`](lmsheets_document::value::sketch::EntityMeta::style_on)). A
/// line whose pattern has a short group (a centre or phantom line) is
/// anchored at every circle or arc centre on it.
fn sketch_items<F: Frame>(
    doc: &Document,
    sketch: &Sketch<F>,
    onto: Onto,
    source: impl Fn(lmsheets_document::value::geometry::EntityId) -> Source,
    rounded: &Rounded,
    purpose: Purpose,
    out: &mut Vec<Item>,
) {
    for (id, e, meta) in sketch.entities() {
        let layer = doc
            .layer(meta.layer)
            .expect("the document keeps every entity's layer in its table");
        if !layer.visible.is_on(purpose) {
            continue;
        }
        for mut primitive in
            primitives(e, onto, &meta.style_on(layer, doc.layers().map(|(_, l)| l)))
        {
            if let (Entity::Line { start, end }, Primitive::Stroke { anchors, dash, .. }) =
                (e, &mut primitive)
            {
                if dash.pattern().len() >= 3 {
                    *anchors = centres_on(sketch, onto, onto.point(*start), onto.point(*end));
                }
            }
            // A line whose corner is drawn rounded stops at the arc's
            // tangent point (the design is still sharp).
            if let (
                Some(trim),
                Primitive::Stroke {
                    path: Path::Line { a, b },
                    ..
                },
            ) = (rounded.trims.get(id), &mut primitive)
            {
                *a = trim[0].unwrap_or(*a);
                *b = trim[1].unwrap_or(*b);
            }
            out.push(Item {
                primitive,
                layer: meta.layer,
                tint: layer.tint,
                source: source(*id),
            });
        }
    }
    // The drawn roundings' arcs, in the style of the line they round.
    for arc in &rounded.arcs {
        let (Some(donor), Some(layer)) = (sketch.meta(arc.donor), doc.layer(arc.layer)) else {
            continue;
        };
        let Some(donor_layer) = doc.layer(donor.layer) else {
            continue;
        };
        let style = donor.style_on(donor_layer, doc.layers().map(|(_, l)| l));
        out.push(Item {
            primitive: Primitive::Stroke {
                path: Path::Arc {
                    centre: arc.centre,
                    radius: arc.radius,
                    start: arc.start,
                    sweep: arc.sweep,
                },
                weight: style.weight,
                dash: style.dash,
                anchors: Vec::new(),
            },
            layer: arc.layer,
            tint: layer.tint,
            source: Source::Observed(arc.id),
        });
    }
}

/// The paper point of a point of the drawing (which the document keeps
/// present).
fn drawing_point(doc: &Document, point: DrawingPoint) -> P2<Paper> {
    match point {
        DrawingPoint::Sheet { point } => {
            let p = doc.sheet().geometry().position(&point).expect("checked");
            P2::new(p.x, p.y)
        }
        DrawingPoint::View { view, point } => {
            let v = doc.views().view(view).expect("checked");
            v.transform
                .to_paper(v.sketch.geometry().position(&point).expect("checked"))
        }
    }
}

/// The paper centre of a circle or an arc of the drawing (by the
/// document's invariant on centre marks).
fn drawing_centre(doc: &Document, entity: DrawingEntity) -> Option<P2<Paper>> {
    fn centre<F: Frame>(e: &Entity<F>) -> Option<P2<F>> {
        match *e {
            Entity::Arc { centre, .. } | Entity::Circle { centre, .. } => Some(centre),
            _ => None,
        }
    }
    match entity {
        DrawingEntity::Sheet { id } => {
            let c = centre(doc.sheet().geometry().get(id)?)?;
            Some(P2::new(c.x, c.y))
        }
        DrawingEntity::View { view, id } => {
            let v = doc.views().view(view)?;
            let c = centre(v.sketch.geometry().get(id)?)?;
            Some(v.transform.to_paper(c))
        }
    }
}

/// The unmodelled marks' regions, outlined (their layer is the
/// screen's: on paper they draw nothing, and the scan's ink inside is
/// theirs).
fn unmodelled_items(doc: &Document, purpose: Purpose, out: &mut Vec<Item>) {
    for (id, u) in doc.unmodelled_marks() {
        let layer = doc
            .layer(u.layer())
            .expect("the document keeps every layer held");
        if !layer.visible.is_on(purpose) {
            continue;
        }
        let r = u.region();
        for (k, a) in r.iter().enumerate() {
            out.push(Item {
                primitive: Primitive::Stroke {
                    path: Path::Line {
                        a: *a,
                        b: r[(k + 1) % r.len()],
                    },
                    weight: layer.style.weight,
                    dash: layer.style.dash.clone(),
                    anchors: Vec::new(),
                },
                layer: u.layer(),
                tint: layer.tint,
                source: Source::Unmodelled(*id),
            });
        }
    }
}

/// Centre marks (a solid cross on the centre, arms the table's or the
/// mark's own) and centre lines (from the outermost of their points
/// past each by the overshoot, in the layer's dash, a short dash on
/// every point).
fn centre_items(doc: &Document, purpose: Purpose, out: &mut Vec<Item>) {
    let drafting = doc.drafting();
    for (id, m) in doc.centre_marks() {
        let layer = doc
            .layer(m.layer())
            .expect("the document keeps every layer held");
        if !layer.visible.is_on(purpose) {
            continue;
        }
        let Some(c) = m.of().first().and_then(|e| drawing_centre(doc, *e)) else {
            continue;
        };
        let arm = m.arm().unwrap_or(drafting.centre_arm());
        let (s, k) = m.angle().sin_cos();
        for u in [P2::<Paper>::new(k, s), P2::new(-s, k)] {
            out.push(Item {
                primitive: Primitive::Stroke {
                    path: Path::Line {
                        a: P2::new(c.x - u.x * arm, c.y - u.y * arm),
                        b: P2::new(c.x + u.x * arm, c.y + u.y * arm),
                    },
                    weight: layer.style.weight,
                    dash: Dash::SOLID,
                    anchors: Vec::new(),
                },
                layer: m.layer(),
                tint: layer.tint,
                source: Source::CentreMark(*id),
            });
        }
    }
    for (id, l) in doc.centre_lines() {
        let layer = doc
            .layer(l.layer())
            .expect("the document keeps every layer held");
        if !layer.visible.is_on(purpose) {
            continue;
        }
        let pts: Vec<P2<Paper>> = l.through().iter().map(|p| drawing_point(doc, *p)).collect();
        let Some(run) = l.run(&pts, drafting.centre_overshoot()) else {
            continue;
        };
        let (a, u, lo, hi) = (run.from, run.dir, run.lo, run.hi);
        let t = |p: &P2<Paper>| (p.x - a.x) * u.x + (p.y - a.y) * u.y;
        out.push(Item {
            primitive: Primitive::Stroke {
                path: Path::Line {
                    a: P2::new(a.x + u.x * lo, a.y + u.y * lo),
                    b: P2::new(a.x + u.x * hi, a.y + u.y * hi),
                },
                weight: layer.style.weight,
                dash: layer.style.dash.clone(),
                anchors: pts.iter().map(|p| t(p) - lo).collect(),
            },
            layer: l.layer(),
            tint: layer.tint,
            source: Source::CentreLine(*id),
        });
    }
}

/// The paper curve of an entity of the drawing (present and a curve, by
/// the document's invariant on leaders).
fn drawing_path(doc: &Document, entity: DrawingEntity) -> Path {
    match entity {
        DrawingEntity::Sheet { id } => {
            let e = doc.sheet().geometry().get(id).expect("checked");
            path_of(e, Onto::Sheet).expect("a curve")
        }
        DrawingEntity::View { view, id } => {
            let v = doc.views().view(view).expect("checked");
            let e = v.sketch.geometry().get(id).expect("checked");
            path_of(e, Onto::View(v.transform)).expect("a curve")
        }
    }
}

/// The paper point of a text's anchor: its placement resolved, through
/// the drawing or through the text it is aligned to (acyclic by the
/// document's invariant).
fn anchor_point(
    doc: &Document,
    id: TextId,
    text: &Text,
    cache: &mut BTreeMap<TextId, P2<Paper>>,
) -> P2<Paper> {
    if let Some(p) = cache.get(&id) {
        return *p;
    }
    let p = placement_point(doc, text.at(), cache);
    cache.insert(id, p);
    p
}

/// The paper point a placement names.
pub(crate) fn placement_point(
    doc: &Document,
    at: Placement,
    cache: &mut BTreeMap<TextId, P2<Paper>>,
) -> P2<Paper> {
    match at {
        Placement::Paper { point } => point,
        Placement::Beside { point, offset } => {
            let base = drawing_point(doc, point);
            P2::new(base.x + offset.x, base.y + offset.y)
        }
        Placement::AlignedTo {
            text: other,
            anchor,
            offset,
        } => {
            let p = text_anchor(doc, other, anchor, cache);
            P2::new(p.x + offset.x, p.y + offset.y)
        }
    }
}

/// The paper point of any anchor of a text.
fn text_anchor(
    doc: &Document,
    id: TextId,
    anchor: Anchor,
    cache: &mut BTreeMap<TextId, P2<Paper>>,
) -> P2<Paper> {
    let t = doc.text(id).expect("checked");
    let at = anchor_point(doc, id, t, cache);
    let (layout, frame) = crate::text::place(doc, t, at);
    frame.to_paper(layout.anchor(anchor))
}

/// The unit direction on the paper a leader leaves a text's anchor in:
/// away from the text along its baseline, on the anchor's side (a
/// leader's origin is sided, by the leader's invariant).
fn away_from(doc: &Document, id: TextId, anchor: Anchor, at: P2<Paper>) -> P2<Paper> {
    let x = match anchor.side().expect("a leader leaves from a side") {
        Side::Left => -1.0,
        Side::Right => 1.0,
    };
    let t = doc.text(id).expect("checked");
    let (layout, frame) = crate::text::place(doc, t, at);
    let a = layout.anchor(anchor);
    let p = frame.to_paper(Local { x: a.x + x, y: a.y });
    let q = frame.to_paper(a);
    P2::new(p.x - q.x, p.y - q.y)
}

/// The items of every leader whose layer is on for the purpose.
fn leader_items(
    doc: &Document,
    purpose: Purpose,
    cache: &mut BTreeMap<TextId, P2<Paper>>,
    out: &mut Vec<Item>,
    terminations: &mut Vec<Termination>,
) {
    for (id, leader) in doc.leaders() {
        let layer = doc
            .layer(leader.layer())
            .expect("the document keeps every leader's layer in its table");
        if !layer.visible.is_on(purpose) {
            continue;
        }
        let landings: Vec<Landing> = leader
            .tips()
            .iter()
            .map(|tip| landing(doc, tip.target, cache))
            .collect();
        let start = match leader.from() {
            Origin::Text { text, anchor } => {
                let attach = text_anchor(doc, text, anchor, cache);
                let away = away_from(doc, text, anchor, attach);
                crate::leader::Start::at_text(doc.drafting(), attach, away)
            }
            Origin::Enclosure { text, side } => {
                let (centre, radius) = ring(doc, text, cache)
                    .expect("the document checks a leader's origin is enclosed");
                crate::leader::Start::at_ring(centre, radius, side)
            }
            Origin::Point { at } => match landing(doc, at, cache) {
                Landing::Point(p) => crate::leader::Start::at_point(p),
                _ => unreachable!("the leader checks its origin is a point"),
            },
        };
        let drawn =
            crate::leader::primitives(leader, doc.drafting(), start, &landings, &layer.style);
        let by = out.len();
        terminations.extend(drawn.terminations.iter().map(|t| t.offset(by)));
        out.extend(drawn.primitives.into_iter().map(|primitive| Item {
            primitive,
            layer: leader.layer(),
            tint: layer.tint,
            source: Source::Leader(*id),
        }));
    }
}

/// Where a target lands, as far as the document resolves it: a point,
/// or a curve to land on wherever the last run meets it.
fn landing(doc: &Document, target: Target, cache: &mut BTreeMap<TextId, P2<Paper>>) -> Landing {
    match target {
        Target::Point { point } => Landing::Point(drawing_point(doc, point)),
        Target::Along { entity, at } => {
            let path = drawing_path(doc, entity);
            match at {
                Along::Fraction { t } => Landing::Point(path.at(t)),
                Along::Nearest => Landing::Nearest(path),
            }
        }
        Target::Text { text, anchor } => Landing::Point(text_anchor(doc, text, anchor, cache)),
        Target::Paper { point } => Landing::Point(point),
        Target::InView { view, point } => {
            let v = doc.views().view(view).expect("checked");
            Landing::Point(v.transform.to_paper(point))
        }
    }
}

/// The ring round an enclosed text on the paper: its centre and radius.
fn ring(
    doc: &Document,
    id: TextId,
    cache: &mut BTreeMap<TextId, P2<Paper>>,
) -> Option<(P2<Paper>, f64)> {
    let t = doc.text(id).expect("checked");
    let at = anchor_point(doc, id, t, cache);
    let (layout, frame) = crate::text::place(doc, t, at);
    crate::text::enclosure(doc, t, &layout, &frame)
}

/// The items of every text whose layer is on for the purpose: glyph
/// strokes at the layer's pen (or the text's own weight) in the layer's
/// dash, on the text's layer; the ring of an enclosed text in the
/// layer's line style.
fn text_items(
    doc: &Document,
    purpose: Purpose,
    cache: &mut BTreeMap<TextId, P2<Paper>>,
    out: &mut Vec<Item>,
    runs: &mut Vec<Run>,
) {
    let font = Document::font();
    for (id, text) in doc.texts() {
        let layer = doc
            .layer(text.layer())
            .expect("the document keeps every text's layer in its table");
        if !layer.visible.is_on(purpose) {
            continue;
        }
        let at = anchor_point(doc, *id, text, cache);
        let (layout, frame) = crate::text::place(doc, text, at);
        for set in crate::text::runs(text, &layout, &frame) {
            runs.push(Run {
                source: Source::Text(*id),
                text: set.line.to_plain(),
                line: set.line,
                origin: set.origin,
                cap: set.cap,
                width: set.width,
                anchor: set.anchor,
                angle: text.angle(),
                weight: layout.weight,
            });
        }
        for (path, share) in crate::text::weighted_strokes(text, &layout, &frame, font) {
            out.push(Item {
                primitive: Primitive::Stroke {
                    path,
                    weight: layout.weight * share,
                    dash: layer.style.dash.clone(),
                    anchors: Vec::new(),
                },
                layer: text.layer(),
                tint: layer.tint,
                source: Source::Text(*id),
            });
        }
        if let (Some(kind), Some((centre, half))) = (
            text.enclosure(),
            crate::text::enclosure(doc, text, &layout, &frame),
        ) {
            for path in crate::text::enclosure_paths(kind, centre, half) {
                out.push(Item {
                    primitive: Primitive::Stroke {
                        path,
                        weight: layer.style.weight,
                        dash: layer.style.dash.clone(),
                        anchors: Vec::new(),
                    },
                    layer: text.layer(),
                    tint: layer.tint,
                    source: Source::Enclosure(*id),
                });
            }
        }
    }
}

/// The drawing plane of a document for a purpose: the sheet sketch, then
/// each view in id order, then the texts, the leaders, the dimensions,
/// the centre marks and lines, the unmodelled marks and the placed form
/// (its printed items under the format layer, its entries on their own
/// layers), each whose layer is visible for the purpose. Nothing else
/// is consulted.
pub fn draw(doc: &Document, purpose: Purpose) -> Plane {
    let mut items = Vec::new();
    sketch_items(
        doc,
        doc.sheet(),
        Onto::Sheet,
        Source::Sheet,
        &Rounded::default(),
        purpose,
        &mut items,
    );
    for (vid, view) in doc.views().views() {
        let vid: ViewId = *vid;
        let rounded = crate::rounding::of_view(doc, vid, view.transform, purpose);
        sketch_items(
            doc,
            &view.sketch,
            Onto::View(view.transform),
            move |id| Source::View(vid, id),
            &rounded,
            purpose,
            &mut items,
        );
    }
    let mut cache = BTreeMap::new();
    let mut runs = Vec::new();
    text_items(doc, purpose, &mut cache, &mut items, &mut runs);
    let mut terminations = Vec::new();
    let mut witnesses = Vec::new();
    leader_items(doc, purpose, &mut cache, &mut items, &mut terminations);
    crate::dimension::dimension_items(
        doc,
        purpose,
        &mut cache,
        &mut items,
        &mut runs,
        &mut terminations,
        &mut witnesses,
    );
    centre_items(doc, purpose, &mut items);
    unmodelled_items(doc, purpose, &mut items);
    crate::form::items(doc, purpose, &mut items, &mut runs);
    let mut plane = Plane {
        kind: PlaneKind::Drawing,
        items,
        runs,
        terminations,
        witnesses,
    };
    crate::weights::apply(doc, &mut plane);
    plane
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmsheets_document::examples::block;

    /// An entity's own convention, not its layer's, decides how it is
    /// drawn: a hidden line on the visible layer is dashed and thin.
    #[test]
    fn an_entity_is_drawn_in_its_own_convention() {
        use lmsheets_document::api::msg::{Op, SketchOp};
        use lmsheets_document::value::provenance::{Agent, Date, Method, Provenance};
        use lmsheets_document::value::sketch::EntityMeta;
        use lmsheets_document::value::style::Convention;
        let mut d = block();
        let ours = Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 27).unwrap(),
        );
        let line = |y| Entity::Line {
            start: P2::new(0.0, y),
            end: P2::new(50.0, y),
        };
        for (id, y, meta) in [
            ("plain", 300.0, EntityMeta::new("visible", ours.clone())),
            (
                "behind",
                310.0,
                EntityMeta::new("visible", ours.clone()).with_convention(Convention::Hidden),
            ),
        ] {
            let edit = SketchOp::AddEntity {
                id: id.into(),
                entity: line(y),
                meta,
            };
            d.apply_in_place(&Op::Sheet { edit }).unwrap();
        }
        let drawn = draw(&d, Purpose::Export);
        let stroke = |id: &str| {
            let i = drawn
                .items
                .iter()
                .find(|i| i.source == Source::Sheet(id.into()))
                .expect("drawn");
            match &i.primitive {
                Primitive::Stroke { weight, dash, .. } => (i.layer, *weight, dash.clone()),
                p => panic!("{p:?}"),
            }
        };
        assert_eq!(stroke("plain"), ("visible".into(), 0.7, Dash::SOLID));
        assert_eq!(stroke("behind"), ("visible".into(), 0.35, Dash::hidden()));
    }

    #[test]
    fn the_block_draws_by_purpose() {
        let d = block();
        let export = draw(&d, Purpose::Export);
        let screen = draw(&d, Purpose::Screen);
        // Three rectangles, a circle, four hidden lines: 17 strokes for
        // export; the screen adds the four construction points.
        assert_eq!(export.items.len(), 17);
        assert_eq!(screen.items.len(), 21);
        assert!(export
            .items
            .iter()
            .all(|i| matches!(i.primitive, Primitive::Stroke { .. })));
        let hidden = export
            .items
            .iter()
            .filter(|i| i.layer == "hidden".into())
            .count();
        assert_eq!(hidden, 4);
        // The side view's transform placed its origin at (120, 100).
        let side_origin = export
            .items
            .iter()
            .find(|i| i.source == Source::View("side".into(), "sb".into()))
            .map(|i| match i.primitive {
                Primitive::Stroke {
                    path: Path::Line { a, .. },
                    ..
                } => a,
                _ => unreachable!(),
            })
            .unwrap();
        assert!((side_origin.x - 120.0).abs() < 1e-9 && (side_origin.y - 100.0).abs() < 1e-9);
    }

    #[test]
    fn the_bracket_leader_lands_on_both_holes() {
        use crate::scene::Fill;
        use lmsheets_document::examples::bracket;
        let d = bracket();
        let plane = draw(&d, Purpose::Export);
        let leader: Vec<&Item> = plane
            .items
            .iter()
            .filter(|i| i.source == Source::Leader("note_holes".into()))
            .collect();
        // The shoulder, two runs, two arrowheads.
        assert_eq!(leader.len(), 5);
        let top = d.views().view("top").unwrap();
        let heads: Vec<&Fill> = leader
            .iter()
            .filter_map(|i| match &i.primitive {
                Primitive::Fill { fill, .. } => Some(fill),
                _ => None,
            })
            .collect();
        assert_eq!(heads.len(), 2);
        for (head, hole) in heads.iter().zip(["c0", "c1"]) {
            let Fill::Polygon { points } = head else {
                panic!()
            };
            let Entity::Circle { centre, radius } = top.sketch.geometry().get(hole).unwrap() else {
                panic!()
            };
            let c = top.transform.to_paper(*centre);
            let r = ((points[0].x - c.x).powi(2) + (points[0].y - c.y).powi(2)).sqrt();
            assert!((r - radius).abs() < 1e-9, "tip {r} off the hole");
        }
        // Every arrowhead and run is on the leader layer at its weight.
        assert!(leader.iter().all(|i| i.layer == "leader".into()));
    }

    #[test]
    fn a_balloon_rings_its_text_and_its_leader_leaves_the_ring() {
        use lmsheets_document::api::msg::Op;
        use lmsheets_document::examples::bracket;
        use lmsheets_document::value::leader::{Leader, Tip};
        use lmsheets_document::value::provenance::{Agent, Date, Method, Provenance};
        use lmsheets_document::value::text::Text;
        let ours = Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 17).unwrap(),
        );
        let at = P2::new(60.0, 20.0);
        let d = bracket()
            .apply(&Op::add_text(
                "b",
                Text::balloon("-15", at, ours.clone()).unwrap(),
            ))
            .unwrap()
            .apply(&Op::add_leader(
                "bl",
                Leader::balloon(
                    "b",
                    Side::Right,
                    Tip::arrow(Target::Paper {
                        point: P2::new(90.0, 80.0),
                    }),
                    ours,
                )
                .unwrap()
                .via(vec![P2::new(10.0, 20.0)])
                .unwrap(),
            ))
            .unwrap();
        let plane = draw(&d, Purpose::Export);
        let ring = plane
            .items
            .iter()
            .find(|i| i.source == Source::Enclosure("b".into()))
            .expect("the ring is drawn");
        let Primitive::Stroke {
            path: Path::Circle { centre, radius },
            weight,
            ..
        } = &ring.primitive
        else {
            panic!("the ring is a circle")
        };
        // The ring takes the balloon layer's heavy pen, is centred on
        // the text, and is the standard balloon: "-15" fits inside it.
        assert_eq!(ring.layer, "balloon".into());
        assert!((weight - 0.7).abs() < 1e-9);
        assert!((centre.x - at.x).abs() < 1e-9 && (centre.y - at.y).abs() < 1e-9);
        let run = plane
            .runs
            .iter()
            .find(|r| r.source == Source::Text("b".into()))
            .unwrap();
        let half_diagonal = (run.width.powi(2) + run.cap.powi(2)).sqrt() / 2.0;
        assert!(half_diagonal + d.drafting().balloon_clearance() < *radius);
        assert!((radius - d.drafting().balloon_diameter() / 2.0).abs() < 1e-9);
        // The leader leaves the ring's right point with no gap, runs the
        // shoulder, and its knee is an offset from that point.
        let lines: Vec<(P2<Paper>, P2<Paper>)> = plane
            .items
            .iter()
            .filter_map(|i| match (&i.source, &i.primitive) {
                (
                    Source::Leader(id),
                    Primitive::Stroke {
                        path: Path::Line { a, b },
                        ..
                    },
                ) if *id == "bl".into() => Some((*a, *b)),
                _ => None,
            })
            .collect();
        let attach: P2<Paper> = P2::new(centre.x + radius, centre.y);
        let (a, b) = lines[0];
        assert!((a.x - attach.x).abs() < 1e-9 && (a.y - attach.y).abs() < 1e-9);
        assert!((b.x - attach.x - d.drafting().shoulder()).abs() < 1e-9);
        assert!((b.y - attach.y).abs() < 1e-9);
        let (_, knee) = lines[1];
        assert!((knee.x - attach.x - 10.0).abs() < 1e-9 && (knee.y - attach.y - 20.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod spline_tests {
    use super::*;
    use crate::scene::{Path, Primitive, Source};
    use lmsheets_document::examples::spline_contour;

    #[test]
    fn a_spline_draws_as_one_cubic_per_segment() {
        let d = spline_contour();
        let plane = draw(&d, Purpose::Export);
        let curve: Vec<&Item> = plane
            .items
            .iter()
            .filter(|i| i.source == Source::View("contour".into(), "curve".into()))
            .collect();
        assert_eq!(curve.len(), 2);
        let mut last_end = None;
        for item in curve {
            let Primitive::Stroke {
                path: Path::Cubic { a, b, .. },
                ..
            } = &item.primitive
            else {
                panic!("{:?}", item.primitive)
            };
            // The segments join: the second starts where the first ends.
            if let Some(end) = last_end {
                assert_eq!(*a, end);
            }
            last_end = Some(*b);
        }
    }
}
