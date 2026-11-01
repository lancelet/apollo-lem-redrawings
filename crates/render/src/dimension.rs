//! Dimensions on the paper: a constraint's measured span, the dimension
//! and extension lines, the arrowheads and the value's text, placed by
//! the layout and the drafting table.
//!
//! Everything is derived from the constraint: the measured points from
//! its operands, the value from its measure (a printed measure
//! verbatim, a value or a parameter by the units policy), "(REF)" when
//! it is driven. The Auto rules are here, stated once: arrows inside
//! when the span holds both heads, the text and two gaps; else outside
//! with the text between the extension lines when it fits, else on a
//! shelf.

use crate::draw::{path_of, placement_point, Onto};
use crate::leader::{Landing, RouteTip};
use crate::scene::{
    Drawn, Item, Part, Path, PathEnd, Primitive, Run, Source, Termination, Witness, WitnessId,
};
use crate::text::Local;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::dimension::{
    Angular, Arrows, Axis, Dimension, DimensionId, Drop, End, Extension, Form, Land, Layout,
    Linear, Mark, Of, Orientation, Place, TextStyle, Within,
};
use lmsheets_document::value::drafting::{Drafting, Terminator};
use lmsheets_document::value::geometry::{Frame, Paper, PointRef, P2};
use lmsheets_document::value::leader::{Route, Shoulder};
use lmsheets_document::value::measure::{split_fraction, Measure, ParamName, Quantity, Tolerance};
use lmsheets_document::value::relation::{Direction, Relation, Round, Span as Measured};
use lmsheets_document::value::sketch::{LayerId, Sketch};
use lmsheets_document::value::style::{Layer, Purpose};
use lmsheets_document::value::text::{
    Align, Anchor, BoxPoint, DrawingPoint, Line, Side, Span, Text, TextId,
};
use lmsheets_document::value::view::ModelAxis;
use std::collections::BTreeMap;

type V = P2<Paper>;

/// An extension line: from a feature point along a direction, drawn
/// between two distances from it. Shared ones are drawn once, as one
/// witness line used by each of them.
struct ExtLine {
    origin: V,
    dir: V,
    from: f64,
    to: f64,
    /// The measured point it leaves, when a point of a sketch.
    foot: Option<DrawingPoint>,
}

/// An outside arrow's tail: from a dimension line's end outward, the
/// stroke running from `short` (inside the head) to `len`. One that
/// runs into the next dimension of a chain gives way to it.
struct Tail {
    at: V,
    dir: V,
    short: f64,
    len: f64,
}

/// The stroke a dimension's head ends: one of its pieces' items, or one
/// of its outside tails (drawn later, unless the next dimension's line
/// serves for it, when the head ends no stroke of its own).
#[derive(Clone, Copy)]
enum HeadLine {
    Item(usize),
    Tail(usize),
}

/// A head among a dimension's pieces' items and the stroke it ends.
#[derive(Clone, Copy)]
struct HeadOf {
    head: usize,
    line: HeadLine,
    end: PathEnd,
}

/// An extension line's identity: its layer, and its origin and
/// direction to the micron, so dimensions sharing one draw it once.
type ExtKey = (LayerId, i64, i64, i64, i64);

/// A dimension's drawn pieces: what is drawn as is, and what is laid
/// against the other dimensions first.
#[derive(Default)]
struct Pieces {
    items: Vec<Primitive>,
    text: Vec<Primitive>,
    /// The lettering as set lines, each with its angle and pen.
    runs: Vec<(crate::text::SetLine, f64, f64)>,
    ext: Vec<ExtLine>,
    tails: Vec<Tail>,
    extents: Vec<(V, V)>,
    /// Which stroke each head among the items ends.
    heads: Vec<HeadOf>,
}

impl Pieces {
    fn merge(&mut self, other: Pieces) {
        let (items, tails) = (self.items.len(), self.tails.len());
        self.heads.extend(other.heads.into_iter().map(|h| HeadOf {
            head: h.head + items,
            line: match h.line {
                HeadLine::Item(i) => HeadLine::Item(i + items),
                HeadLine::Tail(t) => HeadLine::Tail(t + tails),
            },
            ..h
        }));
        self.items.extend(other.items);
        self.text.extend(other.text);
        self.runs.extend(other.runs);
        self.ext.extend(other.ext);
        self.tails.extend(other.tails);
        self.extents.extend(other.extents);
    }

    /// Add a leader's route drawn with the dimension (a radius's, an
    /// ordinate's), with its terminations.
    fn add_drawn(&mut self, drawn: Drawn) {
        let by = self.items.len();
        self.heads
            .extend(drawn.terminations.into_iter().map(|t| HeadOf {
                head: t.head + by,
                line: HeadLine::Item(t.line + by),
                end: t.end,
            }));
        self.items.extend(drawn.primitives);
    }

    /// Push a head, if there is one, ending `line` (if it drew one).
    fn push_head(&mut self, head: Option<Primitive>, line: Option<(HeadLine, PathEnd)>) {
        let Some(head) = head else { return };
        if let Some((line, end)) = line {
            self.heads.push(HeadOf {
                head: self.items.len(),
                line,
                end,
            });
        }
        self.items.push(head);
    }
}

fn add(a: V, b: V) -> V {
    P2::new(a.x + b.x, a.y + b.y)
}

fn sub(a: V, b: V) -> V {
    P2::new(a.x - b.x, a.y - b.y)
}

fn mul(u: V, k: f64) -> V {
    P2::new(u.x * k, u.y * k)
}

fn dot(a: V, b: V) -> f64 {
    a.x * b.x + a.y * b.y
}

fn unit(v: V) -> V {
    let len = dot(v, v).sqrt();
    if len > 0.0 {
        mul(v, 1.0 / len)
    } else {
        P2::new(1.0, 0.0)
    }
}

/// The spans of a measure: a printed one as printed (a fraction
/// stacked), a value or a resolved parameter by the units policy, an
/// unresolved parameter by name.
fn measure_spans(
    m: &Measure,
    q: Quantity,
    drafting: &Drafting,
    resolve: &dyn Fn(ParamName) -> Option<Measure>,
) -> Vec<Span> {
    match m {
        Measure::Printed { text, .. } => match split_fraction(text) {
            Some((whole, num, den)) => {
                let mut spans = Vec::new();
                if !whole.is_empty() {
                    spans.push(Span::run(&format!("{whole} ")));
                }
                spans.push(Span::fraction(num, den));
                spans
            }
            None => vec![Span::run(text)],
        },
        Measure::Value { .. } => vec![Span::run(
            &drafting.units().print(m.base().expect("a value"), q),
        )],
        Measure::Param { name } => match resolve(*name) {
            Some(Measure::Param { .. }) | None => vec![Span::run(name.as_str())],
            Some(v) => measure_spans(&v, q, drafting, resolve),
        },
    }
}

/// A measure as one plain string, for the rows of a stack.
fn measure_plain(
    m: &Measure,
    q: Quantity,
    drafting: &Drafting,
    resolve: &dyn Fn(ParamName) -> Option<Measure>,
) -> String {
    Line::of(measure_spans(m, q, drafting, resolve)).to_plain()
}

/// The mark a radial value carries: "R" or "DIA", before or after the
/// value, and "(TYP)" after both.
#[derive(Clone, Copy, Debug)]
pub struct Marking<'a> {
    /// The mark's letters.
    pub text: &'a str,
    /// Before the value, not after.
    pub before: bool,
    /// Followed by "(TYP)".
    pub typical: bool,
}

impl<'a> Marking<'a> {
    /// A mark after the value, not typical.
    pub fn after(text: &'a str) -> Self {
        Marking {
            text,
            before: false,
            typical: false,
        }
    }
}

/// The value as written, as a line of spans: the prefix; the measure
/// (a printed one verbatim, a value or a resolved parameter by the
/// units policy); its tolerance by the form (symmetric inline, a plus
/// over a minus, or the limits in place of the nominal); the mark, and
/// "(TYP)" after it; the suffix;
/// and "(REF)" for a driven constraint.
pub fn value_line(
    rel: &Relation,
    resolve: impl Fn(ParamName) -> Option<Measure>,
    drafting: &Drafting,
    style: &TextStyle,
    mark: Option<Marking>,
) -> Line {
    let q = rel.quantity().unwrap_or(Quantity::Length);
    let resolve: &dyn Fn(ParamName) -> Option<Measure> = &resolve;
    let plain = |m: &Measure| measure_plain(m, q, drafting, resolve);
    let mut spans = Vec::new();
    let nominal = rel.values().first().copied();
    let tolerance = match style.form {
        Form::Auto => rel.tolerance().clone(),
        Form::Nominal => Tolerance::None,
    };
    match (&tolerance, nominal) {
        (Tolerance::Limits { upper, lower }, _) => {
            spans.push(Span::limits(&plain(upper), &plain(lower)));
        }
        (Tolerance::Symmetric { by }, Some(n)) => {
            spans.extend(measure_spans(n, q, drafting, resolve));
            spans.push(Span::run(" ±"));
            spans.extend(measure_spans(by, q, drafting, resolve));
        }
        (Tolerance::Asymmetric { plus, minus }, Some(n)) => {
            spans.extend(measure_spans(n, q, drafting, resolve));
            spans.push(Span::run(" "));
            // Both are magnitudes; a printed minus keeps one sign.
            spans.push(Span::plus_minus(
                &format!("+{}", plain(plus).trim_start_matches('+')),
                &format!("-{}", plain(minus).trim_start_matches('-')),
            ));
        }
        (_, Some(n)) => spans.extend(measure_spans(n, q, drafting, resolve)),
        (_, None) => {}
    }
    wrap(spans, style, rel, mark)
}

/// A value's spans with the style's prefix and suffix, a mark ("R",
/// "DIA") about the value, "(TYP)" after it, and "(REF)" when the
/// constraint is driven.
fn wrap(value: Vec<Span>, style: &TextStyle, rel: &Relation, mark: Option<Marking>) -> Line {
    let mut spans = Vec::new();
    if let Some(p) = &style.prefix {
        spans.push(Span::run(&format!("{p} ")));
    }
    match mark {
        Some(m) if m.before => {
            spans.push(Span::run(&format!("{} ", m.text)));
            spans.extend(value);
        }
        Some(m) => {
            spans.extend(value);
            spans.push(Span::run(&format!(" {}", m.text)));
        }
        None => spans.extend(value),
    }
    if mark.is_some_and(|m| m.typical) {
        spans.push(Span::run(" (TYP)"));
    }
    if let Some(s) = &style.suffix {
        spans.push(Span::run(&format!(" {s}")));
    }
    if !rel.driving() {
        spans.push(Span::run(" (REF)"));
    }
    Line::of(spans)
}

/// A station callout's line: the axis letter and the coordinate's
/// measure (the `index`th of the position's values), with the prefix,
/// suffix and "(REF)" as for any dimension.
pub fn ordinate_line(
    rel: &Relation,
    resolve: impl Fn(ParamName) -> Option<Measure>,
    drafting: &Drafting,
    style: &TextStyle,
    letter: &str,
    index: usize,
) -> Line {
    let resolve: &dyn Fn(ParamName) -> Option<Measure> = &resolve;
    let mut spans = Vec::new();
    if let Some(p) = &style.prefix {
        spans.push(Span::run(&format!("{p} ")));
    }
    spans.push(Span::run(letter));
    if let Some(m) = rel.values().get(index) {
        spans.extend(measure_spans(m, Quantity::Length, drafting, resolve));
    }
    if let Some(s) = &style.suffix {
        spans.push(Span::run(&format!(" {s}")));
    }
    if !rel.driving() {
        spans.push(Span::run(" (REF)"));
    }
    Line::of(spans)
}

/// The items of every dimension whose layer is on for the purpose.
pub(crate) fn dimension_items(
    doc: &Document,
    purpose: Purpose,
    cache: &mut BTreeMap<TextId, V>,
    out: &mut Vec<Item>,
    runs: &mut Vec<Run>,
    terminations: &mut Vec<Termination>,
    witnesses: &mut Vec<Witness>,
) {
    let mut all: Vec<(DimensionId, LayerId, &Layer, Pieces)> = Vec::new();
    for (id, dim) in doc.dimensions() {
        let layer = doc
            .layer(dim.layer())
            .expect("the document keeps every dimension's layer in its table");
        if !layer.visible.is_on(purpose) {
            continue;
        }
        let mut pieces = Pieces::default();
        match dim.of() {
            Of::Sheet { constraint } => {
                let s = doc.sheet();
                let rel = s.constraint(constraint).expect("checked").relation();
                let frame = Frame2 {
                    onto: Onto::Sheet,
                    axes: None,
                };
                items_in(doc, dim, rel, s, frame, layer, cache, &mut pieces);
            }
            Of::View { view, constraint } => {
                let v = doc.views().view(view).expect("checked");
                let rel = v.sketch.constraint(constraint).expect("checked").relation();
                let frame = Frame2 {
                    onto: Onto::View(v.transform),
                    axes: Some(v.axes),
                };
                items_in(doc, dim, rel, &v.sketch, frame, layer, cache, &mut pieces);
            }
        }
        all.push((*id, dim.layer(), layer, pieces));
    }
    let item =
        |id: DimensionId, on: LayerId, layer: &Layer, part: Part, primitive: Primitive| Item {
            primitive,
            layer: on,
            tint: layer.tint,
            source: Source::Dimension(id, part),
        };
    let stroke = |id: DimensionId, on: LayerId, layer: &Layer, a: V, b: V| {
        item(
            id,
            on,
            layer,
            Part::Line,
            Primitive::Stroke {
                path: Path::Line { a, b },
                weight: layer.style.weight,
                dash: layer.style.dash.clone(),
                anchors: Vec::new(),
            },
        )
    };
    // The heads that end an outside tail, by dimension: (the head's
    // item, the tail, its end), resolved when the tails are drawn.
    let mut tail_heads: Vec<Vec<(usize, usize, PathEnd)>> = Vec::new();
    for (id, on, layer, p) in &all {
        let by = out.len();
        let mut mine = Vec::new();
        for h in &p.heads {
            match h.line {
                HeadLine::Item(i) => terminations.push(Termination {
                    head: h.head + by,
                    line: i + by,
                    end: h.end,
                }),
                HeadLine::Tail(t) => mine.push((h.head + by, t, h.end)),
            }
        }
        tail_heads.push(mine);
        for (set, angle, weight) in &p.runs {
            runs.push(Run {
                source: Source::Dimension(*id, Part::Lettering),
                text: set.line.to_plain(),
                line: set.line.clone(),
                origin: set.origin,
                cap: set.cap,
                width: set.width,
                anchor: set.anchor,
                angle: *angle,
                weight: *weight,
            });
        }
        out.extend(
            p.items
                .iter()
                .cloned()
                .map(|pr| item(*id, *on, layer, Part::Line, pr)),
        );
        out.extend(
            p.text
                .iter()
                .cloned()
                .map(|pr| item(*id, *on, layer, Part::Lettering, pr)),
        );
    }
    // Extension lines are witness lines, nodes of their own: one shared
    // by several dimensions (a chain, a baseline) is drawn once, over the
    // union of their extents, and lists every one of them as a user.
    let key = |v: f64| (v * 1000.0).round() as i64;
    let mut shared: BTreeMap<ExtKey, usize> = BTreeMap::new();
    let mut drawn: Vec<(LayerId, &Layer, f64, f64)> = Vec::new();
    let first = witnesses.len();
    for (id, on, layer, p) in &all {
        for e in &p.ext {
            let k = (
                *on,
                key(e.origin.x),
                key(e.origin.y),
                key(e.dir.x),
                key(e.dir.y),
            );
            let w = *shared.entry(k).or_insert_with(|| {
                witnesses.push(Witness {
                    foot: e.foot,
                    origin: e.origin,
                    dir: e.dir,
                    users: Vec::new(),
                });
                drawn.push((*on, *layer, e.from, e.to));
                witnesses.len() - 1
            });
            let (_, _, from, to) = &mut drawn[w - first];
            *from = from.min(e.from);
            *to = to.max(e.to);
            let wi = &mut witnesses[w];
            if wi.foot.is_none() {
                wi.foot = e.foot;
            }
            if !wi.users.contains(id) {
                wi.users.push(*id);
            }
        }
    }
    for (k, (on, layer, from, to)) in drawn.into_iter().enumerate() {
        let w = &witnesses[first + k];
        out.push(Item {
            primitive: Primitive::Stroke {
                path: Path::Line {
                    a: add(w.origin, mul(w.dir, from)),
                    b: add(w.origin, mul(w.dir, to)),
                },
                weight: layer.style.weight,
                dash: layer.style.dash.clone(),
                anchors: Vec::new(),
            },
            layer: on,
            tint: layer.tint,
            source: Source::Witness(WitnessId((first + k) as u32)),
        });
    }
    // An outside tail that runs along another dimension's line (the
    // next of a chain) is not drawn: that line serves.
    for (i, (id, on, layer, p)) in all.iter().enumerate() {
        for (k, tail) in p.tails.iter().enumerate() {
            let probe = add(tail.at, mul(tail.dir, tail.short + 0.01));
            let covered = all.iter().enumerate().any(|(j, (_, _, _, q))| {
                j != i
                    && q.extents.iter().any(|(a, b)| {
                        let u = unit(sub(*b, *a));
                        let len = dot(sub(*b, *a), u);
                        let along = dot(sub(probe, *a), u);
                        let off = (sub(probe, *a).x * u.y - sub(probe, *a).y * u.x).abs();
                        (tail.dir.x * u.y - tail.dir.y * u.x).abs() < 1e-6
                            && off < 1e-6
                            && along >= -1e-6
                            && along <= len + 1e-6
                    })
            });
            if !covered {
                for &(head, _, end) in tail_heads[i].iter().filter(|h| h.1 == k) {
                    terminations.push(Termination {
                        head,
                        line: out.len(),
                        end,
                    });
                }
                out.push(stroke(
                    *id,
                    *on,
                    layer,
                    add(tail.at, mul(tail.dir, tail.short)),
                    add(tail.at, mul(tail.dir, tail.len)),
                ));
            }
        }
    }
}

/// A dimension's lettering strokes at the pen the text was set with.
fn lettering(set: &SetText, layer: &Layer) -> Vec<Primitive> {
    set.strokes
        .iter()
        .cloned()
        .map(|(path, share)| Primitive::Stroke {
            path,
            weight: set.weight * share,
            dash: layer.style.dash.clone(),
            anchors: Vec::new(),
        })
        .collect()
}

/// How a sketch reaches the paper, and which model axes its frame's
/// axes are (none for the sheet).
#[derive(Clone, Copy)]
struct Frame2 {
    onto: Onto,
    axes: Option<(ModelAxis, ModelAxis)>,
}

/// The primitives of one dimension of a sketch, by its layout.
#[allow(clippy::too_many_arguments)]
fn items_in<F: Frame>(
    doc: &Document,
    dim: &Dimension,
    rel: &Relation,
    sketch: &Sketch<F>,
    frame: Frame2,
    layer: &Layer,
    cache: &mut BTreeMap<TextId, V>,
    pieces: &mut Pieces,
) {
    let onto = frame.onto;
    let pos = |p: &PointRef| onto.point(sketch.geometry().position(p).expect("checked"));
    let resolve = |name: ParamName| doc.param(name).map(|p| p.value().clone());
    let drafting = doc.drafting();
    let circle = || {
        let (circ, round) = rel.circle().expect("a radial constraint");
        let diameter = round == Round::Diameter;
        let e = sketch.geometry().get(circ.id()).expect("checked");
        let path = path_of(e, onto).expect("a circle or arc");
        let (centre, radius) = match path {
            Path::Circle { centre, radius } | Path::Arc { centre, radius, .. } => (centre, radius),
            _ => unreachable!("a radial constraint is on a circle or arc"),
        };
        (path, centre, radius, diameter)
    };
    match dim.layout() {
        Layout::Linear(lin) => {
            let text = value_line(rel, resolve, drafting, dim.text(), None);
            let span = rel.span().expect("a linear constraint has a span");
            // The measured points and the direction, in paper mm; with each
            // point, the line of the sketch it sits on, if one, whose
            // extension its extension line is.
            let (ends, d0) = match span {
                Measured::Points { a, b, along } => {
                    let (pa, pb) = (pos(&a), pos(&b));
                    let d = match along {
                        Direction::Direct => unit(sub(pb, pa)),
                        Direction::X => onto.dir(1.0, 0.0),
                        Direction::Y => onto.dir(0.0, 1.0),
                    };
                    ([(pa, None), (pb, None)], d)
                }
                Measured::PointLine { p, l } => {
                    let (p, s, e) = (pos(&p), pos(&l.start()), pos(&l.end()));
                    let u = unit(sub(e, s));
                    let foot = add(s, mul(u, dot(sub(p, s), u)));
                    ([(p, None), (foot, Some((s, e)))], unit(sub(foot, p)))
                }
                Measured::Lines { l, m } => {
                    let (s, e) = (pos(&l.start()), pos(&l.end()));
                    let (ms, me) = (pos(&m.start()), pos(&m.end()));
                    let u = unit(sub(me, ms));
                    let foot = add(ms, mul(u, dot(sub(s, ms), u)));
                    (
                        [(s, Some((s, e))), (foot, Some((ms, me)))],
                        unit(sub(foot, s)),
                    )
                }
            };
            let at = |point| match dim.of() {
                Of::Sheet { .. } => Some(DrawingPoint::Sheet { point }),
                Of::View { view, .. } => Some(DrawingPoint::View { view, point }),
            };
            let feet = match span {
                Measured::Points { a, b, .. } => [at(a), at(b)],
                Measured::PointLine { p, .. } => [at(p), None],
                Measured::Lines { .. } => [None, None],
            };
            pieces.merge(linear(
                &text,
                ends,
                feet,
                d0,
                lin,
                dim.text().orientation,
                doc,
                dim,
                layer,
            ));
        }
        Layout::Radial(rad) => {
            let (path, centre, radius, diameter) = circle();
            let mark = Marking {
                text: if diameter { "DIA" } else { "R" },
                before: rad.mark == Mark::Before,
                typical: rad.typical,
            };
            let text = value_line(rel, resolve, drafting, dim.text(), Some(mark));
            let attach = placement_point(doc, rad.at, cache);
            let anchor = Anchor::Line {
                index: 0,
                side: rad.side,
            };
            let axes = (P2::new(1.0, 0.0), P2::new(0.0, 1.0));
            let set = set_text(doc, dim, &text, attach, anchor, 0.0, axes);
            let away = P2::new(if rad.side == Side::Right { 1.0 } else { -1.0 }, 0.0);
            let landing = match rad.land {
                Land::Nearest => Landing::Nearest(path),
                Land::Fraction { t } => Landing::Point(path.at(t)),
                Land::Radial | Land::FromCentre => Landing::Radial { centre, radius },
            };
            let from_centre = rad.land == Land::FromCentre;
            let start =
                crate::leader::Start::at_text(drafting, clear_of_ink(&set, attach, away), away);
            let tip = RouteTip {
                knees: &[],
                landing,
                // From the centre the arrowhead is the centre's line's:
                // the text's run just meets the arc.
                terminator: if from_centre {
                    Terminator::None
                } else {
                    Terminator::Arrow
                },
                pull_back: from_centre.then_some(0.0),
                head: None,
            };
            pieces.add_drawn(crate::leader::route(
                drafting,
                start,
                &rad.route,
                &[tip],
                &layer.style,
            ));
            if from_centre {
                // Where the text's run met the arc: on the ray from the
                // centre through the run's start (the shoulder's end,
                // there being no knees of its own).
                let shoulder = match rad.route.shoulder {
                    Shoulder::Standard => drafting.shoulder(),
                    Shoulder::Length { mm } => mm,
                    Shoulder::None => 0.0,
                };
                let from: P2<Paper> = rad.route.knees.last().map_or(
                    P2::new(
                        attach.x + away.x * (start.gap + shoulder),
                        attach.y + away.y * (start.gap + shoulder),
                    ),
                    |k| P2::new(attach.x + k.x, attach.y + k.y),
                );
                let (dx, dy) = (from.x - centre.x, from.y - centre.y);
                let len = dx.hypot(dy).max(1e-9);
                let out = P2::new(dx / len, dy / len);
                let on_arc = P2::new(centre.x + out.x * radius, centre.y + out.y * radius);
                pieces.add_drawn(crate::leader::route(
                    drafting,
                    crate::leader::Start {
                        attach: centre,
                        away: out,
                        gap: 0.0,
                    },
                    &Route {
                        shoulder: Shoulder::None,
                        knees: Vec::new(),
                    },
                    &[RouteTip {
                        knees: &[],
                        landing: Landing::Point(on_arc),
                        terminator: Terminator::Arrow,
                        pull_back: Some(0.0),
                        head: None,
                    }],
                    &layer.style,
                ));
            }
            pieces.text.extend(lettering(&set, layer));
            pieces.runs.extend(set.runs);
        }
        Layout::Ordinate(ord) => {
            let point = rel.positioned().expect("an ordinate constraint");
            let (letter, index) = match (ord.axis, frame.axes) {
                (Axis::X, Some((a, _))) => (a.letter(), 0),
                (Axis::Y, Some((_, b))) => (b.letter(), 1),
                (Axis::X, None) => ("X", 0),
                (Axis::Y, None) => ("Y", 1),
            };
            let text = ordinate_line(rel, resolve, drafting, dim.text(), letter, index);
            let attach = placement_point(doc, ord.at, cache);
            let anchor = Anchor::Line {
                index: 0,
                side: ord.side,
            };
            let axes = (P2::new(1.0, 0.0), P2::new(0.0, 1.0));
            let set = set_text(doc, dim, &text, attach, anchor, 0.0, axes);
            let away = P2::new(if ord.side == Side::Right { 1.0 } else { -1.0 }, 0.0);
            let tip = RouteTip {
                knees: &[],
                landing: Landing::Point(pos(&point)),
                terminator: Terminator::Arrow,
                pull_back: None,
                head: None,
            };
            pieces.add_drawn(crate::leader::route(
                drafting,
                crate::leader::Start::at_text(drafting, clear_of_ink(&set, attach, away), away),
                &ord.route,
                &[tip],
                &layer.style,
            ));
            pieces.text.extend(lettering(&set, layer));
            pieces.runs.extend(set.runs);
        }
        Layout::Angular(ang) => {
            let (l, m) = rel.lines().expect("an angular constraint");
            let lines = [l, m].map(|r| (pos(&r.start()), pos(&r.end())));
            pieces.merge(angular(rel, resolve, lines, ang, doc, dim, layer));
        }
        Layout::Across(ac) => {
            let (_, centre, radius, _) = circle();
            let text = value_line(
                rel,
                resolve,
                drafting,
                dim.text(),
                Some(Marking::after("DIA")),
            );
            let d = P2::new(ac.angle.cos(), ac.angle.sin());
            let ends = [
                (sub(centre, mul(d, radius)), None),
                (add(centre, mul(d, radius)), None),
            ];
            let through = ac.offset.abs() < 1e-9;
            let ext = if through {
                [Extension::Omit, Extension::Omit]
            } else {
                [Extension::Draw, Extension::Draw]
            };
            let lin = Linear {
                offset: ac.offset,
                along: 0.0,
                place: ac.place,
                arrows: ac.arrows,
                ext,
                ext_gap: None,
            };
            pieces.merge(linear(
                &text,
                ends,
                [None, None],
                d,
                &lin,
                dim.text().orientation,
                doc,
                dim,
                layer,
            ));
        }
    }
}

/// The primitives of an angular dimension between two lines (each as
/// its two ends on the paper): the arc about their vertex in the sector
/// the layout points into, extension lines along the rays where the
/// lines fall short of the arc, heads tangent at the arc's ends, and
/// the value inside a break of the arc or beyond it. The value shown is
/// the sector's angle: the constraint's when the sector lies between
/// the lines' directions, else its supplement by the units policy.
fn angular(
    rel: &Relation,
    resolve: impl Fn(ParamName) -> Option<Measure>,
    lines: [(V, V); 2],
    ang: &Angular,
    doc: &Document,
    dim: &Dimension,
    layer: &Layer,
) -> Pieces {
    let drafting = doc.drafting();
    let weight = layer.style.weight;
    let stroke = |path: Path| Primitive::Stroke {
        path,
        weight,
        dash: layer.style.dash.clone(),
        anchors: Vec::new(),
    };
    // The vertex: where the two lines (extended) cross.
    let [(p, q), (r, t)] = lines;
    let (u, v) = (unit(sub(q, p)), unit(sub(t, r)));
    let den = u.x * v.y - u.y * v.x;
    if den.abs() < 1e-9 {
        return Pieces::default();
    }
    let w = sub(r, p);
    let k = (w.x * v.y - w.y * v.x) / den;
    let vertex = add(p, mul(u, k));
    // The rays bounding the sector: each line's direction turned towards
    // the sector point; the arc from the first ray to the second through
    // the sector.
    // Each ray is its line's direction turned to the sector point's
    // side of the other line, so the sector between the rays (less than
    // a half turn) is the one the point lies in.
    let s = P2::new(ang.sector.cos(), ang.sector.sin());
    let cross = |a: V, b: V| a.x * b.y - a.y * b.x;
    let ra = if cross(v, u) * cross(v, s) >= 0.0 {
        u
    } else {
        mul(u, -1.0)
    };
    let rb = if cross(u, v) * cross(u, s) >= 0.0 {
        v
    } else {
        mul(v, -1.0)
    };
    let turn = |a: V, b: V| cross(a, b).atan2(dot(a, b));
    let sweep = turn(ra, rb);
    let start = ra.y.atan2(ra.x);
    let radius = ang.radius;
    let arc_len = radius * sweep.abs();
    let at = |a: f64| add(vertex, P2::new(radius * a.cos(), radius * a.sin()));
    // The value: the constraint's, or its supplement for the other
    // sector, by whether the rays follow the lines' directions alike.
    let alike = (dot(ra, u) >= 0.0) == (dot(rb, v) >= 0.0);
    let own = rel.values().first().and_then(|m| match m {
        Measure::Param { name } => resolve(*name).and_then(|v| v.base()),
        _ => m.base(),
    });
    let text = match (alike, own) {
        (false, Some(own)) => {
            let supplement = std::f64::consts::PI - own.abs();
            let value = Span::run(&drafting.units().print(supplement, Quantity::Angle));
            wrap(vec![value], dim.text(), rel, None)
        }
        // An unresolved parameter prints its name, whichever sector.
        _ => value_line(rel, resolve, drafting, dim.text(), None),
    };
    // Extension lines along each ray where its line falls short:
    // witness lines from the vertex.
    let mut ext_lines = Vec::new();
    for (d, (a, b)) in [(ra, (p, q)), (rb, (r, t))] {
        let reach = dot(sub(a, vertex), d).max(dot(sub(b, vertex), d)).max(0.0);
        if reach < radius - drafting.ext_gap() {
            ext_lines.push(ExtLine {
                origin: vertex,
                dir: d,
                from: reach + drafting.ext_gap(),
                to: radius + drafting.ext_overshoot(),
                foot: None,
            });
        }
    }
    // The text, set provisionally on the arc's middle, horizontal.
    let mid = start + sweep / 2.0;
    let n = P2::new(mid.cos(), mid.sin());
    let tn = P2::new(-mid.sin(), mid.cos());
    let centre = Anchor::Box {
        at: BoxPoint::Centre,
    };
    let probe = set_text(doc, dim, &text, at(mid), centre, 0.0, (tn, n));
    let head = drafting.arrow();
    let gap = drafting.text_gap();
    let short = head.shaft_short();
    let fits_inside = arc_len >= 3.0 * head.length + probe.along + 2.0 * gap;
    let inside_text = match ang.text {
        Within::Inside => true,
        Within::Outside => false,
        Within::Auto => fits_inside,
    };
    let inside = match ang.arrows {
        Arrows::Inside => true,
        Arrows::Outside => false,
        Arrows::Auto => {
            if inside_text {
                fits_inside
            } else {
                arc_len >= 3.0 * head.length
            }
        }
    };
    let set = if inside_text {
        probe
    } else {
        let off = radius + gap + probe.across / 2.0;
        set_text(
            doc,
            dim,
            &text,
            add(vertex, mul(n, off)),
            centre,
            0.0,
            (tn, n),
        )
    };
    // The arc, its tails, and the heads.
    let arc = |a0: f64, a1: f64| Path::Arc {
        centre: vertex,
        radius,
        start: a0,
        sweep: a1 - a0,
    };
    let sg = sweep.signum();
    let end = start + sweep;
    // A head on the arc lies along the secant from its tip to the arc
    // point one head length back, so the arc crosses its base at the
    // middle: `secant(a, back)` is the arrival direction at angle `a`
    // for a head whose base sits `back` radians away along the arc.
    let secant = |a: f64, back: f64| unit(sub(at(a), at(a + back)));
    let back = head.length / radius;
    let (a0, a1) = if inside {
        (start + sg * short / radius, end - sg * short / radius)
    } else {
        (start, end)
    };
    // Each head's own run of the arc: the run entering it from its base
    // (the arc between the heads inside, the tail outside).
    let mut p = Pieces::default();
    let (mut at_start, mut at_end) = (None, None);
    let push = |p: &mut Pieces, path: Path| {
        p.items.push(stroke(path));
        HeadLine::Item(p.items.len() - 1)
    };
    if inside_text {
        let half = (set.along / 2.0 + gap) / radius;
        if (mid - sg * half - a0) * sg > 0.0 {
            at_start = Some((push(&mut p, arc(a0, mid - sg * half)), PathEnd::Start));
        }
        if (a1 - (mid + sg * half)) * sg > 0.0 {
            at_end = Some((push(&mut p, arc(mid + sg * half, a1)), PathEnd::End));
        }
    } else if (a1 - a0) * sg > 0.0 {
        let line = push(&mut p, arc(a0, a1));
        at_start = Some((line, PathEnd::Start));
        at_end = Some((line, PathEnd::End));
    }
    if inside {
        let h =
            crate::terminator::arrowhead(at(start), secant(start, sg * back), head, Some(weight));
        p.push_head(h, at_start);
        let h = crate::terminator::arrowhead(at(end), secant(end, -sg * back), head, Some(weight));
        p.push_head(h, at_end);
    } else {
        let tail = drafting.outside_tail() / radius;
        let a = push(&mut p, arc(start - sg * tail, start - sg * short / radius));
        let b = push(&mut p, arc(end + sg * short / radius, end + sg * tail));
        let h =
            crate::terminator::arrowhead(at(start), secant(start, -sg * back), head, Some(weight));
        p.push_head(h, Some((a, PathEnd::End)));
        let h = crate::terminator::arrowhead(at(end), secant(end, sg * back), head, Some(weight));
        p.push_head(h, Some((b, PathEnd::Start)));
    }
    Pieces {
        text: lettering(&set, layer),
        runs: set.runs.clone(),
        ext: ext_lines,
        ..p
    }
}

/// The box of the text as set at `at` with `anchor`, turned `angle`:
/// its strokes, and its extents along `d` and `n` and the paper's x.
struct SetText {
    strokes: Vec<(Path, f64)>,
    /// The lettering as set lines, each with its angle and pen.
    runs: Vec<(crate::text::SetLine, f64, f64)>,
    /// The pen width the text is set at, mm.
    weight: f64,
    along: f64,
    across: f64,
    wide: f64,
    /// The middle of the box of its ink, if it has any.
    ink_centre: Option<V>,
    /// The ink's extent along the dimension's direction `d`, as dot
    /// products with it (so absolute along `d`), if it has any ink.
    ink_along: Option<(f64, f64)>,
}

/// The value as set, held on its original's ink when the dimension
/// carries it: shifted so the box of its ink is where the box of the
/// drawn ink is (a stacked value has no one baseline to hold, so its
/// ink box is what lines up); the layout's place when there is no ink.
/// The shift moves the text only: the dimension's line, gaps and arrows
/// stay where the layout puts them.
fn set_text(
    doc: &Document,
    dim: &Dimension,
    text: &Line,
    at: V,
    anchor: Anchor,
    angle: f64,
    axes: (V, V),
) -> SetText {
    let set = set_text_at(doc, dim, text, at, anchor, angle, axes);
    let Some(target) = dim.drawn().and_then(drawn_centre) else {
        return set;
    };
    let Some(ink) = set.ink_centre else {
        return set;
    };
    let shifted = P2::new(at.x + target.x - ink.x, at.y + target.y - ink.y);
    set_text_at(doc, dim, text, shifted, anchor, angle, axes)
}

/// The middle of the box of ink as drawn: its centre-lines grown by
/// half their widths.
fn drawn_centre(d: &lmsheets_document::value::ink::AsDrawn) -> Option<V> {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for s in d.strokes() {
        for (p, w) in s.points.iter().zip(&s.widths) {
            lo = [lo[0].min(p.x - w / 2.0), lo[1].min(p.y - w / 2.0)];
            hi = [hi[0].max(p.x + w / 2.0), hi[1].max(p.y + w / 2.0)];
        }
    }
    (lo[0] <= hi[0]).then(|| P2::new((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0))
}

/// Where a leader leaves its text: the attachment, or the end of the
/// text's ink where that reaches past it (a value held on its
/// original's ink may), so the line starts a text gap clear of the ink.
fn clear_of_ink(set: &SetText, attach: V, away: V) -> V {
    let Some((lo, hi)) = set.ink_along else {
        return attach;
    };
    let x = if away.x > 0.0 {
        attach.x.max(hi)
    } else {
        attach.x.min(lo)
    };
    P2::new(x, attach.y)
}

fn set_text_at(
    doc: &Document,
    dim: &Dimension,
    text: &Line,
    at: V,
    anchor: Anchor,
    angle: f64,
    axes: (V, V),
) -> SetText {
    let (d, n) = axes;
    use lmsheets_document::value::dimension::{CALLOUT_ROLE, ROLE};
    let role = if matches!(dim.layout(), Layout::Radial(_) | Layout::Across(_))
        && doc.lettering().role(CALLOUT_ROLE).is_some()
    {
        CALLOUT_ROLE
    } else {
        ROLE
    };
    let t = Text::with_lines(vec![text.clone()], role, at, dim.provenance().clone())
        .expect("a value is one line")
        .aligned(Align::Centre)
        .anchored(anchor)
        .expect("a box anchor")
        .turned(angle)
        .expect("finite")
        .on_layer(dim.layer());
    let (layout, frame) = crate::text::place(doc, &t, at);
    let (bottom, top) = (-layout.descent, layout.height);
    let corners = [
        Local { x: 0.0, y: bottom },
        Local {
            x: layout.width,
            y: bottom,
        },
        Local {
            x: layout.width,
            y: top,
        },
        Local { x: 0.0, y: top },
    ]
    .map(|c| frame.to_paper(c));
    let extent = |u: V| {
        let p: Vec<f64> = corners.iter().map(|c| dot(*c, u)).collect();
        p.iter().cloned().fold(f64::MIN, f64::max) - p.iter().cloned().fold(f64::MAX, f64::min)
    };
    let ink = crate::text::ink_bounds(&t, &layout, &frame, Document::font());
    SetText {
        runs: crate::text::runs(&t, &layout, &frame)
            .into_iter()
            .map(|set| (set, angle, layout.weight))
            .collect(),
        strokes: crate::text::weighted_strokes(&t, &layout, &frame, Document::font()),
        weight: layout.weight,
        along: extent(d),
        across: extent(n),
        wide: extent(P2::new(1.0, 0.0)),
        ink_centre: ink
            .map(|r| P2::new((r.min().x + r.max().x) / 2.0, (r.min().y + r.max().y) / 2.0)),
        ink_along: ink.map(|r| {
            let p = [
                (r.min().x, r.min().y),
                (r.max().x, r.min().y),
                (r.min().x, r.max().y),
                (r.max().x, r.max().y),
            ]
            .map(|(x, y)| dot(P2::new(x, y), d));
            (
                p.iter().cloned().fold(f64::MAX, f64::min),
                p.iter().cloned().fold(f64::MIN, f64::max),
            )
        }),
    }
}

/// The primitives of a linear dimension between two measured points
/// along `d0`, each point with the line it sits on if any, in the
/// layer's style.
#[allow(clippy::too_many_arguments)]
fn linear(
    text: &Line,
    ends: [(V, Option<(V, V)>); 2],
    feet: [Option<DrawingPoint>; 2],
    d0: V,
    lin: &Linear,
    orientation: Orientation,
    doc: &Document,
    dim: &Dimension,
    layer: &Layer,
) -> Pieces {
    let drafting = doc.drafting();
    let weight = layer.style.weight;
    let stroke = |a: V, b: V| Primitive::Stroke {
        path: Path::Line { a, b },
        weight,
        dash: layer.style.dash.clone(),
        anchors: Vec::new(),
    };
    let mut out = Vec::new();
    let mut ext_lines = Vec::new();
    let mut tails = Vec::new();
    let mut shelf_line = None;
    let [(pa, on_a), (pb, on_b)] = ends;
    // The direction from a towards b, and its left normal; the dimension
    // line `offset` along the normal from a.
    let d = if dot(sub(pb, pa), d0) < 0.0 {
        mul(d0, -1.0)
    } else {
        d0
    };
    let n = P2::new(-d.y, d.x);
    let ea = add(pa, mul(n, lin.offset));
    let span = dot(sub(pb, pa), d);
    let eb = add(ea, mul(d, span));
    let mid = add(ea, mul(d, span / 2.0 + lin.along));
    let far = if lin.offset >= 0.0 { 1.0 } else { -1.0 };
    // Extension lines, from each point towards and past the dimension
    // line. A point on a line of the sketch (a foot, a line's start) has
    // that line for its extension line: the drawn part starts at the
    // line's end nearest the dimension line, with the gap there, and
    // there is none when the dimension line crosses the line itself.
    for (end, ((p, on), e, ext, foot)) in [
        ((pa, on_a), ea, lin.ext[0], feet[0]),
        ((pb, on_b), eb, lin.ext[1], feet[1]),
    ]
    .into_iter()
    .enumerate()
    {
        if ext != Extension::Draw {
            continue;
        }
        let dist = dot(sub(e, p), n);
        let from = match on {
            Some((s0, s1)) => {
                let (t0, t1) = (dot(sub(s0, p), n), dot(sub(s1, p), n));
                let (lo, hi) = (t0.min(t1), t0.max(t1));
                if dist > hi {
                    hi
                } else if dist < lo {
                    lo
                } else {
                    continue;
                }
            }
            None => 0.0,
        };
        // The line runs from where it starts towards the dimension line,
        // whichever side of the start that is.
        let s = if dist - from >= 0.0 { 1.0 } else { -1.0 };
        let ext_gap = lin.ext_gap.map_or_else(|| drafting.ext_gap(), |g| g[end]);
        if (dist - from).abs() > ext_gap {
            // An extension line that continues a measured line is that
            // line's: keyed by the line (its start) and the way it runs, so
            // dimensions measuring the same line from different points of
            // it (a foot, an end) share one, over the union of their extents.
            let (origin, shift) = match on {
                Some((s0, _)) => (s0, dot(sub(p, s0), mul(n, s))),
                None => (p, 0.0),
            };
            ext_lines.push(ExtLine {
                origin,
                dir: mul(n, s),
                from: s * from + ext_gap + shift,
                to: s * dist + drafting.ext_overshoot() + shift,
                foot,
            });
        }
    }
    // The text's size, set provisionally at the middle.
    let angle = match orientation {
        Orientation::Horizontal => 0.0,
        Orientation::Aligned => {
            let a = d.y.atan2(d.x);
            if d.x < -1e-9 || (d.x.abs() <= 1e-9 && d.y > 0.0) {
                a + std::f64::consts::PI
            } else {
                a
            }
        }
    };
    let centre = Anchor::Box {
        at: BoxPoint::Centre,
    };
    let probe = set_text(doc, dim, text, mid, centre, angle, (d, n));
    let head = drafting.arrow();
    let gap = drafting.text_gap();
    let short = head.shaft_short();
    // Inside needs the two heads, the text and its gaps, and a visible
    // shaft of at least half a head between each head and the text.
    // The text's room is the extent of the original's ink along the
    // line where the dimension carries it (the value is held on that
    // ink, and the original fitted it between its extension lines),
    // else our ink's, else its box.
    let drawn_room = dim.drawn().and_then(|ink| {
        let along: Vec<(f64, f64)> = ink
            .strokes()
            .iter()
            .flat_map(|s| s.points.iter().zip(&s.widths))
            .map(|(p, w)| (dot(*p, d) - w / 2.0, dot(*p, d) + w / 2.0))
            .collect();
        let lo = along.iter().map(|a| a.0).fold(f64::MAX, f64::min);
        let hi = along.iter().map(|a| a.1).fold(f64::MIN, f64::max);
        (lo <= hi).then_some(hi - lo)
    });
    let room = drawn_room
        .or(probe.ink_along.map(|(lo, hi)| hi - lo))
        .unwrap_or(probe.along);
    let fits_inline = span >= 3.0 * head.length + room + 2.0 * gap;
    let inside = match lin.arrows {
        Arrows::Inside => true,
        Arrows::Outside => false,
        Arrows::Auto => match lin.place {
            Place::Auto | Place::Inline | Place::Between => fits_inline,
            _ => span >= 2.0 * head.length + gap,
        },
    };
    let place = match lin.place {
        Place::Auto if inside => Place::Inline,
        Place::Auto if span >= room + 2.0 * gap => Place::Between,
        Place::Auto => Place::shelf(End::End),
        p => p,
    };
    // The text, placed.
    let set = match place {
        Place::Auto | Place::Inline | Place::Between => probe,
        Place::Above => set_text(
            doc,
            dim,
            text,
            add(mid, mul(n, far * (gap + probe.across / 2.0))),
            centre,
            angle,
            (d, n),
        ),
        Place::Beside { side } => {
            let sx = if side == Side::Right { 1.0 } else { -1.0 };
            set_text(
                doc,
                dim,
                text,
                add(mid, P2::new(sx * (gap + probe.wide / 2.0), 0.0)),
                centre,
                angle,
                (d, n),
            )
        }
        Place::Shelf { end, run, drop } => {
            let (e, od) = match end {
                End::Start => (ea, mul(d, -1.0)),
                End::End => (eb, d),
            };
            let run = run.unwrap_or(drafting.shelf_run());
            let start = if inside { e } else { add(e, mul(od, short)) };
            // Outside, the shelf starts inside the head and is its line;
            // inside, it leaves from the tip, and the head's line is the
            // dimension line.
            if !inside {
                shelf_line = Some(HeadLine::Item(out.len()));
            }
            let (text_at, anchor) = match drop {
                None => {
                    let shelf_end = add(e, mul(od, run));
                    out.push(stroke(start, shelf_end));
                    let at = add(shelf_end, mul(od, gap));
                    let anchor = if od.x > 1e-9 {
                        Anchor::Line {
                            index: 0,
                            side: Side::Left,
                        }
                    } else if od.x < -1e-9 {
                        Anchor::Line {
                            index: 0,
                            side: Side::Right,
                        }
                    } else if od.y < 0.0 {
                        Anchor::Box {
                            at: BoxPoint::BottomCentre,
                        }
                    } else {
                        Anchor::Box {
                            at: BoxPoint::TopCentre,
                        }
                    };
                    (at, anchor)
                }
                Some(Drop { along, side }) => {
                    let sx = if side == Side::Right { 1.0 } else { -1.0 };
                    let knee = add(e, mul(od, along));
                    let shelf_end = add(knee, P2::new(sx * run, 0.0));
                    out.push(stroke(start, knee));
                    out.push(stroke(knee, shelf_end));
                    let at = add(shelf_end, P2::new(sx * gap, 0.0));
                    let anchor = Anchor::Line {
                        index: 0,
                        side: if sx > 0.0 { Side::Left } else { Side::Right },
                    };
                    (at, anchor)
                }
            };
            set_text(doc, dim, text, text_at, anchor, angle, (d, n))
        }
    };
    // The dimension line, the tails, and the heads.
    // Each head's own line: the one entering it from its base (inside,
    // the dimension line; outside, the tail or the shelf). The line
    // between outside heads ends at their tips and is neither's.
    let shelf_at = |which: End| matches!(place, Place::Shelf { end, .. } if end == which);
    let mut p = Pieces {
        items: out,
        ..Pieces::default()
    };
    let (mut at_a, mut at_b) = (None, None);
    if inside {
        let (a, b) = (add(ea, mul(d, short)), add(eb, mul(d, -short)));
        if matches!(place, Place::Inline | Place::Auto | Place::Between) {
            // The line breaks round the whole held text: its ink (the
            // whole number, the fraction and its bar, as drawn, where
            // held on its original's ink) and a text gap beyond it each
            // side; with no ink, round its box.
            let (l, r) = match set.ink_along {
                Some((lo, hi)) => {
                    let m = dot(mid, d);
                    (
                        add(mid, mul(d, lo - gap - m)),
                        add(mid, mul(d, hi + gap - m)),
                    )
                }
                None => (
                    add(mid, mul(d, -(set.along / 2.0 + gap))),
                    add(mid, mul(d, set.along / 2.0 + gap)),
                ),
            };
            if dot(sub(l, a), d) > 0.0 {
                at_a = Some((HeadLine::Item(p.items.len()), PathEnd::Start));
                p.items.push(stroke(a, l));
            }
            if dot(sub(b, r), d) > 0.0 {
                at_b = Some((HeadLine::Item(p.items.len()), PathEnd::End));
                p.items.push(stroke(r, b));
            }
        } else {
            let line = HeadLine::Item(p.items.len());
            at_a = Some((line, PathEnd::Start));
            at_b = Some((line, PathEnd::End));
            p.items.push(stroke(a, b));
        }
        let h = crate::terminator::arrowhead(ea, mul(d, -1.0), head, Some(weight));
        p.push_head(h, at_a);
        let h = crate::terminator::arrowhead(eb, d, head, Some(weight));
        p.push_head(h, at_b);
    } else {
        let tail = drafting.outside_tail();
        // A tail and a shelf both run outward from inside the head: the
        // path's start.
        if !shelf_at(End::Start) {
            at_a = Some((HeadLine::Tail(tails.len()), PathEnd::Start));
            tails.push(Tail {
                at: ea,
                dir: mul(d, -1.0),
                short,
                len: tail,
            });
        } else {
            at_a = shelf_line.map(|l| (l, PathEnd::Start));
        }
        if !shelf_at(End::End) {
            at_b = Some((HeadLine::Tail(tails.len()), PathEnd::Start));
            tails.push(Tail {
                at: eb,
                dir: d,
                short,
                len: tail,
            });
        } else {
            at_b = shelf_line.map(|l| (l, PathEnd::Start));
        }
        if !matches!(place, Place::Inline | Place::Auto | Place::Between) && span > 0.0 {
            p.items.push(stroke(ea, eb));
        }
        let h = crate::terminator::arrowhead(ea, d, head, Some(weight));
        p.push_head(h, at_a);
        let h = crate::terminator::arrowhead(eb, mul(d, -1.0), head, Some(weight));
        p.push_head(h, at_b);
    }
    // The inner line serves a neighbour's tail only when it is drawn.
    let inner_drawn =
        inside || (!matches!(place, Place::Inline | Place::Auto | Place::Between) && span > 0.0);
    Pieces {
        text: lettering(&set, layer),
        runs: set.runs.clone(),
        ext: ext_lines,
        tails,
        extents: if inner_drawn {
            vec![(ea, eb)]
        } else {
            Vec::new()
        },
        ..p
    }
}

/// What [`clearances`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clearances {
    /// Line/text pairs checked.
    pub checked: usize,
    /// The smallest margin over the clearance, mm (negative: a breach).
    pub least_margin: f64,
    /// Each pair closer than the clearance.
    pub violations: Vec<String>,
}

/// Distance from a segment to a box (zero when they meet).
fn segment_to_box(a: V, b: V, lo: V, hi: V) -> f64 {
    let at = |t: f64| P2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
    let to_box = |p: V| {
        let dx = (lo.x - p.x).max(0.0).max(p.x - hi.x);
        let dy = (lo.y - p.y).max(0.0).max(p.y - hi.y);
        dx.hypot(dy)
    };
    // The distance to a box is convex along a segment: ternary search.
    let (mut t0, mut t1) = (0.0, 1.0);
    for _ in 0..80 {
        let (m0, m1) = (t0 + (t1 - t0) / 3.0, t1 - (t1 - t0) / 3.0);
        if to_box(at(m0)) < to_box(at(m1)) {
            t1 = m1;
        } else {
            t0 = m0;
        }
    }
    to_box(at((t0 + t1) / 2.0))
}

/// Every line of every dimension (its line, tails, heads' stems and
/// shelves or leaders) and every witness line, as the sheet draws them
/// for export, against the ink of every text and dimension value: none
/// comes within the drafting table's text gap of any ink.
pub fn clearances(d: &Document) -> Clearances {
    let plane = crate::draw::draw(d, Purpose::Export);
    let mut by: Vec<(Source, [f64; 4])> = Vec::new();
    for i in &plane.items {
        if !matches!(
            i.source,
            Source::Dimension(_, Part::Lettering) | Source::Text(_)
        ) {
            continue;
        }
        let (Some(b), Primitive::Stroke { .. }) = (i.primitive.ink_bounds(), &i.primitive) else {
            continue;
        };
        let r = [b.min().x, b.min().y, b.max().x, b.max().y];
        match by.iter_mut().find(|(x, _)| *x == i.source) {
            Some((_, u)) => {
                *u = [
                    u[0].min(r[0]),
                    u[1].min(r[1]),
                    u[2].max(r[2]),
                    u[3].max(r[3]),
                ]
            }
            None => by.push((i.source, r)),
        }
    }
    let clear = d.drafting().text_gap() - 1e-9;
    let mut out = Clearances {
        least_margin: f64::MAX,
        ..Clearances::default()
    };
    for i in &plane.items {
        if !matches!(
            i.source,
            Source::Dimension(_, Part::Line) | Source::Witness(_)
        ) {
            continue;
        }
        let Primitive::Stroke {
            path: Path::Line { a, b },
            ..
        } = i.primitive
        else {
            continue;
        };
        for (src, u) in &by {
            let gap = segment_to_box(a, b, P2::new(u[0], u[1]), P2::new(u[2], u[3]));
            out.checked += 1;
            out.least_margin = out.least_margin.min(gap - clear);
            if gap < clear - 1e-6 {
                out.violations.push(format!(
                    "{:?} runs {gap:.3} mm from {src:?} (needs {clear:.3})",
                    i.source
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::draw;
    use crate::scene::Plane;
    use lmsheets_document::examples::dimensions;
    use lmsheets_document::value::relation::{horizontal_distance, line};
    use lmsheets_document::value::style::Purpose;
    use lmsheets_document::value::text::StackKind;

    /// The strokes of one dimension's items, and of the witness lines
    /// it uses.
    fn lines_of(plane: &Plane, id: &str) -> Vec<(V, V)> {
        let id = DimensionId::new(id);
        plane
            .items
            .iter()
            .filter(|i| match i.source {
                Source::Dimension(d, Part::Line) => d == id,
                Source::Witness(w) => plane.witnesses[w.0 as usize].users.contains(&id),
                _ => false,
            })
            .filter_map(|i| match i.primitive {
                Primitive::Stroke {
                    path: Path::Line { a, b },
                    ..
                } => Some((a, b)),
                _ => None,
            })
            .collect()
    }

    /// A witness line is one node however many dimensions use it: the
    /// chain's shared one is drawn once and lists both, and every witness
    /// drawn is used by some dimension.
    #[test]
    fn shared_witness_lines_list_every_user() {
        let plane = draw(&dimensions(), Purpose::Export);
        let drawn: Vec<usize> = plane
            .items
            .iter()
            .filter_map(|i| match i.source {
                Source::Witness(w) => Some(w.0 as usize),
                _ => None,
            })
            .collect();
        assert_eq!(drawn.len(), plane.witnesses.len());
        assert!(plane.witnesses.iter().all(|w| !w.users.is_empty()));
        let shared: Vec<_> = plane
            .witnesses
            .iter()
            .filter(|w| w.users.len() > 1)
            .collect();
        assert!(!shared.is_empty(), "{:?}", plane.witnesses);
        // A point's witness knows its foot.
        assert!(plane.witnesses.iter().any(|w| w.foot.is_some()));
    }

    /// A distance between two lines, or from a point to a line, is drawn
    /// square to the lines with extension lines that continue the measured
    /// line(s): the lines' own directions, on the lines themselves (the
    /// "h" plate edges 30 model mm = 75 paper mm apart; the hole's centre
    /// 40 model mm = 100 paper mm from the right edge).
    #[test]
    fn a_distance_to_lines_has_extension_lines_along_them() {
        let plane = draw(&dimensions(), Purpose::Export);
        let strokes = |id: &str| {
            let id = DimensionId::new(id);
            let pick = |part: bool| {
                plane
                    .items
                    .iter()
                    .filter(|i| match i.source {
                        Source::Dimension(d, Part::Line) => part && d == id,
                        Source::Witness(w) => {
                            !part && plane.witnesses[w.0 as usize].users.contains(&id)
                        }
                        _ => false,
                    })
                    .filter_map(|i| match i.primitive {
                        Primitive::Stroke {
                            path: Path::Line { a, b },
                            ..
                        } => Some((a, b)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            (pick(true), pick(false))
        };
        // "h": the plate's bottom and top edges (level), 75 mm apart.
        let (dim, ext) = strokes("h");
        assert!(!dim.is_empty());
        assert!(
            dim.iter().all(|(a, b)| (a.x - b.x).abs() < 1e-6),
            "plumb: {dim:?}"
        );

        assert_eq!(ext.len(), 2, "{ext:?}");
        for (s, e) in &ext {
            assert!(
                (s.y - e.y).abs() < 1e-9,
                "an extension line continues a level edge"
            );
        }
        // Each on its own line: 75 mm apart.
        assert!(
            ((ext[0].0.y - ext[1].0.y).abs() - 75.0).abs() < 1e-6,
            "{ext:?}"
        );
        // "hr": the hole's centre to the right edge (plumb), 100 mm.
        let (dim, ext) = strokes("hr");
        assert!(
            dim.iter().all(|(a, b)| (a.y - b.y).abs() < 1e-6),
            "level: {dim:?}"
        );
        // The point's end is omitted; the right edge's continues it
        // (plumb), and the dimension line runs from there 100 mm to the
        // centre's level (less its head).
        assert_eq!(ext.len(), 1, "{ext:?}");
        let (s0, e0) = ext[0];
        assert!((s0.x - e0.x).abs() < 1e-9, "{ext:?}");
        assert!(
            dim.iter()
                .all(|(a, b)| a.x <= s0.x && b.x <= s0.x + 1e-9 && a.x >= s0.x - 100.0 - 1e-6),
            "{dim:?}"
        );
    }

    /// A dimension's own gap sets where its extension lines start,
    /// instead of the drafting table's: the line is shorter by the
    /// difference.
    #[test]
    fn a_dimensions_own_gap_shortens_its_extension_lines() {
        use lmsheets_document::api::msg::Op;
        let d = dimensions();
        let witness_length = |d: &lmsheets_document::state::document::Document| {
            let plane = draw(d, Purpose::Export);
            let id = DimensionId::new("w");
            plane
                .items
                .iter()
                .filter(|i| match i.source {
                    Source::Witness(w) => plane.witnesses[w.0 as usize].users.contains(&id),
                    _ => false,
                })
                .filter_map(|i| match i.primitive {
                    Primitive::Stroke {
                        path: Path::Line { a, b },
                        ..
                    } => Some(dot(sub(b, a), sub(b, a)).sqrt()),
                    _ => None,
                })
                .sum::<f64>()
        };
        let table = d.drafting().ext_gap();
        let before = witness_length(&d);
        // Dimensions measuring the same edge share its extension line (over
        // the union of their extents): every linear one is given the gap.
        let shorter = |gap: [f64; 2]| {
            let mut e = d.clone();
            let ids: Vec<_> = d.dimensions().map(|(id, _)| *id).collect();
            for id in ids {
                if let Ok(dim) = e.dimension(id).unwrap().clone().gapped(gap) {
                    e = e.apply(&Op::set_dimension(id, dim)).unwrap();
                }
            }
            before - witness_length(&e)
        };
        // Each shared extension line is shorter by the gap over the table's.
        let (a, b) = (
            shorter([table + 2.0, table + 2.0]),
            shorter([table + 5.0, table + 5.0]),
        );
        // (two lines of "w"'s are shared: each is shorter by the gap)
        assert!((a - 4.0).abs() < 1e-6 && (b - 10.0).abs() < 1e-6, "{a} {b}");
    }

    /// Check every termination of a plane against the geometry: the head
    /// is a dimension's or leader's arrowhead, its line a stroke of the
    /// same item whose named end lies inside the head, short of its tip,
    /// and runs in from its base along its direction; each head ends at
    /// most one line. Returns how many heads end a line, and how many
    /// heads there are.
    fn check_terminations(plane: &crate::scene::Plane) -> (usize, usize) {
        let mut seen = std::collections::BTreeSet::new();
        for t in &plane.terminations {
            assert!(seen.insert(t.head), "a head ends two lines");
            let (h, l) = (&plane.items[t.head], &plane.items[t.line]);
            assert_eq!(h.source, l.source);
            let Primitive::Fill {
                fill: crate::scene::Fill::Polygon { points },
                ..
            } = &h.primitive
            else {
                panic!("a head is an arrowhead")
            };
            let Primitive::Stroke { path, .. } = &l.primitive else {
                panic!("a line is a stroke")
            };
            let (tip, base) = (points[0], mul(add(points[1], points[2]), 0.5));
            let len = dot(sub(tip, base), sub(tip, base)).sqrt();
            let dir = unit(sub(tip, base));
            let (e, inner) = match t.end {
                PathEnd::Start => (path.at(0.0), path.at(1e-3)),
                PathEnd::End => (path.at(1.0), path.at(1.0 - 1e-3)),
            };
            let along = dot(sub(e, base), dir);
            assert!(along > -1e-6 && along < len - 1e-6, "{along} of {len}");
            // Into the head: from the path's inside towards its end.
            assert!(dot(unit(sub(e, inner)), dir) > 0.9, "{:?}", t);
        }
        let heads = plane
            .items
            .iter()
            .filter(|i| {
                matches!(i.source, Source::Dimension(..) | Source::Leader(_))
                    && matches!(i.primitive, Primitive::Fill { .. })
            })
            .count();
        (plane.terminations.len(), heads)
    }

    #[test]
    fn each_head_ends_its_own_line() {
        use lmsheets_document::api::msg::Op;
        let d = dimensions();
        let (ended, heads) = check_terminations(&draw(&d, Purpose::Export));
        assert!(ended > 0 && ended <= heads, "{ended} of {heads}");
        // Arrows outside: each ends its tail, never the line between
        // the extension lines, whose ends are the tips.
        for id in ["w", "h"] {
            let dim = d.dimension(id).unwrap().clone();
            let out = dim.arrowed(Arrows::Outside).unwrap();
            let d2 = d.apply(&Op::set_dimension(id, out)).unwrap();
            let plane = draw(&d2, Purpose::Export);
            check_terminations(&plane);
            let mine: Vec<_> = plane
                .terminations
                .iter()
                .filter(|t| {
                    matches!(plane.items[t.head].source, Source::Dimension(x, _) if x == id.into())
                })
                .collect();
            assert_eq!(mine.len(), 2, "{id}: {mine:?}");
            assert!(mine.iter().all(|t| t.end == PathEnd::Start), "{id}");
        }
    }

    /// A radius drawn from the centre (an explicit option; the convention
    /// is `Land::Radial`) adds a line from the centre to the arc, with
    /// the arrowhead, at the leader's weight and no other.
    #[test]
    fn a_radius_from_the_centre_is_drawn_at_the_leaders_weight() {
        use lmsheets_document::api::msg::Op;
        let d = dimensions();
        let id = DimensionId::new("d1");
        // The weights of the note's strokes: how many, the least, the most.
        let weights = |d: &lmsheets_document::Document| {
            let w: Vec<f64> = draw(d, Purpose::Export)
                .items
                .iter()
                .filter_map(|i| match (&i.source, &i.primitive) {
                    (Source::Dimension(x, Part::Line), Primitive::Stroke { weight, .. })
                        if *x == id =>
                    {
                        Some(*weight)
                    }
                    _ => None,
                })
                .collect();
            let least = w.iter().copied().fold(f64::MAX, f64::min);
            let most = w.iter().copied().fold(0.0, f64::max);
            (w.len(), least, most)
        };
        let (n0, lo0, hi0) = weights(&d);
        assert_eq!(lo0, hi0, "one weight on the leader");
        let from = d
            .dimension("d1")
            .unwrap()
            .clone()
            .landed(Land::FromCentre)
            .unwrap();
        let d2 = d.apply(&Op::set_dimension("d1", from)).unwrap();
        let (n1, lo1, hi1) = weights(&d2);
        assert!(n1 > n0, "the line from the centre is drawn: {n0} then {n1}");
        assert_eq!((lo1, hi1), (lo0, lo0), "at the leader's weight");
    }

    #[test]
    fn an_obtuse_sector_is_drawn_as_itself_not_its_reflex() {
        use lmsheets_document::api::msg::Op;
        use std::f64::consts::PI;
        // The apex angle is 120°; a sector point near one of its rays
        // must still give that sector, not the 240° around the outside.
        let d = dimensions();
        let of = Of::in_view("angular", "a_apex");
        let provenance = d.dimension("a_apex").unwrap().provenance().clone();
        let near_ray = Dimension::angular(of, 18.0, P2::new(-0.7, 1.0), provenance).unwrap();
        let d = d.apply(&Op::set_dimension("a_apex", near_ray)).unwrap();
        let plane = draw(&d, Purpose::Export);
        let sweeps: Vec<f64> = plane
            .items
            .iter()
            .filter(
                |i| matches!(i.source, Source::Dimension(id, Part::Line) if id == "a_apex".into()),
            )
            .filter_map(|i| match i.primitive {
                Primitive::Stroke {
                    path: Path::Arc { sweep, .. },
                    ..
                } => Some(sweep),
                _ => None,
            })
            .collect();
        assert!(!sweeps.is_empty());
        let total: f64 = sweeps.iter().map(|s| s.abs()).sum();
        // The arc (with the text's break taken out) is less than the
        // sector, never more than a half turn.
        assert!(
            total < 2.0 * PI / 3.0 + 1e-6 && total > PI / 3.0,
            "{sweeps:?}"
        );
    }

    #[test]
    fn the_linear_fragment_draws_every_dimension_where_it_was_put() {
        let d = dimensions();
        let plane = draw(&d, Purpose::Export);
        // The solve leaves placements within its tolerance, not exact.
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        for id in ["w", "h", "hx", "hr", "cx", "cl", "tl"] {
            assert!(!lines_of(&plane, id).is_empty(), "{id} drew nothing");
            let heads = plane
                .items
                .iter()
                .filter(|i| {
                    matches!(i.source, Source::Dimension(d, _) if d == id.into())
                        && matches!(i.primitive, Primitive::Fill { .. })
                })
                .count();
            assert_eq!(heads, 2, "{id} has {heads} heads");
        }
        // The plate's bottom is at y = 120 on the paper; the chain sits 12
        // below it and the overall 24, on horizontal lines.
        let on_y = |id: &str, y: f64| {
            lines_of(&plane, id)
                .iter()
                .any(|(a, b)| near(a.y, y) && near(b.y, y))
        };
        for (id, y) in [("hx", 132.0), ("hr", 132.0), ("w", 144.0)] {
            assert!(on_y(id, y), "{id}: {:?}", lines_of(&plane, id));
        }
        // The height's line is vertical at 12 left of the plate (x = 18).
        assert!(lines_of(&plane, "h")
            .iter()
            .any(|(a, b)| near(a.x, 18.0) && near(b.x, 18.0)));
        // The hole-to-edge extension line continues the right edge from
        // its corner (y = 120) with the gap, not from the foot at y = 82.5.
        let ext = lines_of(&plane, "hr")
            .into_iter()
            .find(|(a, b)| near(a.x, 180.0) && near(b.x, 180.0))
            .expect("an extension line on the right edge");
        assert!(near(ext.0.y.min(ext.1.y), 120.0 + 1.27), "{ext:?}");
        // The chamfer's line lies inside the part: 8 down-left of it.
        let cl = lines_of(&plane, "cl");
        let chamfer_mid = P2::<Paper>::new(30.0 + 56.0 * 2.5, 120.0 - 26.0 * 2.5);
        // Its strokes along the chamfer's direction lie on that line.
        let n = P2::<Paper>::new(0.5f64.sqrt(), -(0.5f64.sqrt()));
        assert!(cl.iter().any(|(a, b)| {
            let along = unit(sub(*b, *a));
            let m = P2::<Paper>::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            along.x.abs() > 0.7 && (dot(sub(m, chamfer_mid), n) + 8.0).abs() < 1e-3
        }));
    }

    #[test]
    fn values_print_with_their_marks() {
        use lmsheets_document::value::measure::Unit;
        let d = Drafting::standard();
        let style = TextStyle {
            orientation: Orientation::Horizontal,
            form: Form::Auto,
            prefix: None,
            suffix: Some("(TYP)".into()),
        };
        let printed = horizontal_distance(
            line("a").start(),
            line("a").end(),
            Measure::inches(".188").unwrap(),
        );
        assert_eq!(
            value_line(&printed, |_| None, &d, &style, None).to_plain(),
            ".188 (TYP)"
        );
        let ours = horizontal_distance(
            line("a").start(),
            line("a").end(),
            Measure::mm(25.4).unwrap(),
        )
        .driven()
        .unwrap();
        assert_eq!(
            value_line(&ours, |_| None, &d, &style, None).to_plain(),
            "1.000 (TYP) (REF)"
        );
        let param = horizontal_distance(
            line("a").start(),
            line("a").end(),
            Measure::param(ParamName::new("w")).unwrap(),
        );
        let resolve = |_| Some(Measure::mm(50.8).unwrap());
        assert_eq!(
            value_line(&param, resolve, &d, &style, None).to_plain(),
            "2.000 (TYP)"
        );
        assert_eq!(
            value_line(&param, |_| None, &d, &style, None).to_plain(),
            "w (TYP)"
        );
        // Tolerances by the form: symmetric inline, a plus over a minus,
        // limits in place of the nominal; a printed fraction stacked.
        let sym = horizontal_distance(
            line("a").start(),
            line("a").end(),
            Measure::mm(25.4).unwrap(),
        )
        .toleranced(Tolerance::Symmetric {
            by: Measure::printed(".010", Unit::In).unwrap(),
        })
        .unwrap();
        assert_eq!(
            value_line(&sym, |_| None, &d, &style, None).to_plain(),
            "1.000 ±.010 (TYP)"
        );
        let asym = horizontal_distance(
            line("a").start(),
            line("a").end(),
            Measure::mm(25.4).unwrap(),
        )
        .toleranced(Tolerance::Asymmetric {
            plus: Measure::printed(".003", Unit::In).unwrap(),
            minus: Measure::printed(".001", Unit::In).unwrap(),
        })
        .unwrap();
        assert_eq!(
            value_line(&asym, |_| None, &d, &style, None).to_plain(),
            "1.000 +.003/-.001 (TYP)"
        );
        let lim = horizontal_distance(
            line("a").start(),
            line("a").end(),
            Measure::mm(25.4).unwrap(),
        )
        .toleranced(Tolerance::Limits {
            upper: Measure::printed("1.266", Unit::In).unwrap(),
            lower: Measure::printed("1.234", Unit::In).unwrap(),
        })
        .unwrap();
        assert_eq!(
            value_line(&lim, |_| None, &d, &style, None).to_plain(),
            "1.266/1.234 (TYP)"
        );
        let nominal = TextStyle {
            form: Form::Nominal,
            ..style.clone()
        };
        assert_eq!(
            value_line(&lim, |_| None, &d, &nominal, None).to_plain(),
            "1.000 (TYP)"
        );
        let frac = horizontal_distance(
            line("a").start(),
            line("a").end(),
            Measure::inches("2 5/16").unwrap(),
        );
        let l = value_line(&frac, |_| None, &d, &style, Some(Marking::after("R")));
        assert_eq!(l.to_plain(), "2 5/16 R (TYP)");
        let typical = Marking {
            before: true,
            typical: true,
            ..Marking::after("R")
        };
        let plain = TextStyle {
            suffix: None,
            ..style.clone()
        };
        assert_eq!(
            value_line(&frac, |_| None, &d, &plain, Some(typical)).to_plain(),
            "R 2 5/16 (TYP)"
        );
        assert!(matches!(
            l.spans[1],
            Span::Stack {
                stack: StackKind::Fraction,
                ..
            }
        ));
        // A station callout: the letter, the coordinate, the marks.
        let st = lmsheets_document::value::relation::position(
            line("a").start(),
            Measure::inches("185.278").unwrap(),
            Measure::inches("27.00").unwrap(),
        )
        .driven()
        .unwrap();
        assert_eq!(
            ordinate_line(&st, |_| None, &d, &style, "X", 0).to_plain(),
            "X185.278 (TYP) (REF)"
        );
        assert_eq!(
            ordinate_line(&st, |_| None, &d, &style, "Z", 1).to_plain(),
            "Z27.00 (TYP) (REF)"
        );
    }
    /// No dimension or extension line comes within the drafting
    /// table's text gap of any text's ink, on the whole of the 17619 sheet (the dimension's
    /// line breaks clear of its whole held value, and nothing else
    /// runs into a value).
    #[test]
    fn no_dimension_line_runs_into_a_text() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../document/tests/fixtures/v4/LDW280-17619-sh3.document.json");
        let d = Document::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let c = clearances(&d);
        assert!(c.checked > 0);
        assert!(c.violations.is_empty(), "{:#?}", c.violations);
        println!(
            "checked {} line/text pairs; least margin over the clearance {:.3} mm",
            c.checked, c.least_margin
        );
    }
}
