//! Ops, messages, and the documents they build.
//!
//! Every name comes from a short pool ([`ENTITIES`], [`VIEWS`], ...), so
//! that in a few ops one adds what another removes, constrains or cites:
//! some ops succeed and some are refused, and both are what the
//! properties are about. Values are valid mostly and refused now and
//! then (a NaN, a dangling layer, an uncited source), so the checks on
//! every path into a document are exercised too.
//!
//! [`document`] is a start (an empty sheet, or [`two_views`] with the
//! pool's entities in place) with a short sequence of ops applied, the
//! refused ones skipped: a document reachable through the api, which is
//! every document there can be. [`with_views`] starts from
//! [`two_views`] and edits the views mostly; [`example`] gives one of
//! the crate's examples as it is.

use super::geometry::{entity, entity_of, p2, rect, transform_or_bad};
use super::values::{agent, coord, date, maybe_finite, measure, measure_or_raw, provenance};
use crate::api::model::Msg;
use crate::api::msg::{Op, SketchOp};
use crate::state::document::{Document, Note, Source, SourceKind, Title};
use crate::state::views::{aligned, in_view, Link};
use crate::value::frame::{self, Registration, Similarity};
use crate::value::geometry::{Entity, Frame, Kind, Model, Paper, PointRef, P2};
use crate::value::measure::Quantity;
use crate::value::params::Param;
use crate::value::provenance::{Check, CheckKind, Citation, ItemRef};
use crate::value::relation::{self as rel, arc, circle, line, point, Relation};
use crate::value::style::{Dash, Layer, LineStyle, Visibility};
use crate::value::text::Text;
use crate::value::view::{ModelAxis, Transform, View};
use proptest::prelude::*;
use proptest::sample::select;
use proptest::strategy::Union;
use std::sync::OnceLock;

/// Entity ids, in the sheet and in every view: each is mostly of one
/// kind (lines `a` and `b`, point `c`, circle `d`), so that relations
/// and the entities they name meet often, and now and then of another.
pub const ENTITIES: &[&str] = &["a", "b", "c", "d"];
/// The pool's lines.
const LINES: &[&str] = &["a", "b"];
/// Constraint ids, likewise.
pub const CONSTRAINTS: &[&str] = &["k1", "k2", "k3"];
/// View ids.
pub const VIEWS: &[&str] = &["front", "top"];
/// Layer ids: three standard ones and one that starts missing.
pub const LAYERS: &[&str] = &["visible", "hidden", "construction", "extra"];
/// Source ids: the first registered in some starts, the second never.
pub const SOURCES: &[&str] = &["sheet", "other"];
/// Parameter names.
pub const PARAMS: &[&str] = &["t", "u"];
/// Link ids.
pub const LINKS: &[&str] = &["x1", "x2"];
/// Text ids.
pub const TEXTS: &[&str] = &["b1", "b2"];

/// The kind entity `id` of the pool mostly is.
fn kind_of(id: &str) -> Kind {
    match id {
        "c" => Kind::Point,
        "d" => Kind::Circle,
        _ => Kind::Line,
    }
}

/// An entity for `id` of the pool: of its kind mostly, any now and then.
fn entity_for<F: Frame + 'static>(id: &'static str) -> BoxedStrategy<Entity<F>> {
    prop_oneof![9 => entity_of(kind_of(id)), 1 => entity()].boxed()
}

/// A position on an entity of the pool: of the entity's kind mostly,
/// of any kind now and then (which a sketch refuses).
pub fn point_ref() -> impl Strategy<Value = PointRef> + Clone {
    let any = (select(ENTITIES), 0..5u8).prop_map(|(id, k)| match k {
        0 => point(id),
        1 => line(id).start(),
        2 => line(id).end(),
        3 => circle(id).centre(),
        _ => arc(id).centre(),
    });
    let fitting = (select(ENTITIES), 0..3u8).prop_map(|(id, k)| match (kind_of(id), k) {
        (Kind::Point, _) => point(id),
        (Kind::Circle, _) => circle(id).centre(),
        (_, 0) => line(id).start(),
        (_, 1) => line(id).end(),
        _ => line(id).mid(),
    });
    prop_oneof![9 => fitting, 1 => any]
}

/// A relation over the pool's entities: geometric ones, dimensions in
/// either quantity with values that may name a parameter or be ones no
/// constructor allows, and reference (driven) dimensions.
pub fn relation() -> BoxedStrategy<Relation> {
    let (p, l) = (point_ref, || select(LINES).prop_map(line));
    let len = || measure_or_raw(Quantity::Length, PARAMS);
    let kinds: Vec<(u32, BoxedStrategy<Relation>)> = vec![
        (
            3,
            (p(), p()).prop_map(|(a, b)| rel::coincident(a, b)).boxed(),
        ),
        (2, l().prop_map(rel::horizontal).boxed()),
        (2, l().prop_map(rel::vertical).boxed()),
        (1, (l(), l()).prop_map(|(a, b)| rel::parallel(a, b)).boxed()),
        (
            1,
            (l(), l())
                .prop_map(|(a, b)| rel::perpendicular(a, b))
                .boxed(),
        ),
        (1, l().prop_map(|a| rel::tangent(a, circle("d"))).boxed()),
        (1, p().prop_map(rel::fixed).boxed()),
        (
            3,
            (p(), p(), len())
                .prop_map(|(a, b, v)| rel::distance(a, b, v))
                .boxed(),
        ),
        (2, (l(), len()).prop_map(|(a, v)| rel::length(a, v)).boxed()),
        (1, len().prop_map(|v| rel::radius(circle("d"), v)).boxed()),
        (
            1,
            (p(), len(), len())
                .prop_map(|(a, x, y)| rel::position(a, x, y))
                .boxed(),
        ),
        (
            1,
            (l(), l(), measure(Quantity::Angle, &[]))
                .prop_map(|(a, b, v)| rel::angle(a, b, v))
                .boxed(),
        ),
        (
            2,
            (p(), p(), measure(Quantity::Length, &[]))
                .prop_map(|(a, b, v)| rel::distance(a, b, v).driven().expect("a dimension"))
                .boxed(),
        ),
    ];
    Union::new_weighted(kinds).boxed()
}

/// An item of the pool a status change can name.
fn item() -> impl Strategy<Value = ItemRef> + Clone {
    prop_oneof![
        select(ENTITIES).prop_map(|e| ItemRef::Entity(e.into())),
        select(CONSTRAINTS).prop_map(|c| ItemRef::Constraint(c.into())),
    ]
}

/// A change to a sketch in frame `F`, over the pools.
pub fn sketch_op<F: Frame + 'static>() -> BoxedStrategy<SketchOp<F>> {
    let (e, k) = (|| select(ENTITIES), || select(CONSTRAINTS));
    let prov = || provenance(SOURCES);
    let kinds: Vec<(u32, BoxedStrategy<SketchOp<F>>)> = vec![
        (
            8,
            (
                e().prop_flat_map(|id| (Just(id), entity_for(id))),
                select(LAYERS),
                prov(),
            )
                .prop_map(|((id, e), layer, p)| SketchOp::add_entity(id, e, layer, p))
                .boxed(),
        ),
        (
            2,
            e().prop_flat_map(|id| (Just(id), entity_for(id)))
                .prop_map(|(id, e)| SketchOp::set_entity(id, e))
                .boxed(),
        ),
        (1, e().prop_map(SketchOp::remove_entity).boxed()),
        (
            1,
            (e(), select(LAYERS))
                .prop_map(|(id, l)| SketchOp::SetEntityLayer {
                    id: id.into(),
                    layer: l.into(),
                })
                .boxed(),
        ),
        (
            1,
            (e(), prov())
                .prop_map(|(id, p)| SketchOp::SetEntityProvenance {
                    id: id.into(),
                    provenance: p,
                })
                .boxed(),
        ),
        (
            6,
            (k(), relation(), prov())
                .prop_map(|(id, r, p)| SketchOp::add_constraint(id, r, p))
                .boxed(),
        ),
        (
            2,
            (
                k(),
                proptest::collection::vec(measure_or_raw(Quantity::Length, PARAMS), 1..=2),
            )
                .prop_map(|(id, v)| SketchOp::set_values(id, v))
                .boxed(),
        ),
        (1, k().prop_map(SketchOp::remove_constraint).boxed()),
        (1, item().prop_map(|item| SketchOp::Accept { item }).boxed()),
        (
            1,
            (item(), select(SOURCES))
                .prop_map(|(item, doc)| SketchOp::Verify {
                    item,
                    check: Check {
                        kind: CheckKind::Overlay,
                        cite: Citation::new(doc, "overlay"),
                    },
                })
                .boxed(),
        ),
        (
            1,
            (item(), item())
                .prop_map(|(item, a)| SketchOp::Dispute {
                    item,
                    against: vec![a],
                })
                .boxed(),
        ),
        (
            1,
            (item(), item())
                .prop_map(|(item, by)| SketchOp::Supersede { item, by })
                .boxed(),
        ),
    ];
    Union::new_weighted(kinds).boxed()
}

/// A layer row: drawn or working, perhaps tinted by a palette entry
/// that may not exist.
fn layer() -> impl Strategy<Value = Layer> + Clone {
    let tint = proptest::option::of(select(vec!["faint", "nope"]));
    (any::<bool>(), tint).prop_map(|(drawn, tint)| {
        let style = LineStyle::new(0.35, Dash::SOLID).expect("valid");
        let l = if drawn {
            Layer::drawn(style)
        } else {
            Layer::working(style)
        };
        match tint {
            Some(t) => l.tinted(t),
            None => l,
        }
    })
}

/// A view along two model axes (perhaps the same two, refused), empty.
fn view() -> impl Strategy<Value = View> + Clone {
    let axis = || select(vec![ModelAxis::X, ModelAxis::Y, ModelAxis::Z]);
    (axis(), axis(), transform_or_bad()).prop_map(|(a, b, t)| View {
        axes: (a, b),
        model_frame: None,
        sketch: Default::default(),
        transform: t,
        region: None,
    })
}

/// A link between two views of the pool (perhaps the same one).
fn link() -> impl Strategy<Value = Link> + Clone {
    let at = || (select(VIEWS), point_ref()).prop_map(|(v, p)| in_view(v, p));
    let axis = select(vec![ModelAxis::X, ModelAxis::Y, ModelAxis::Z]);
    (at(), at(), axis).prop_map(|(a, b, axis)| aligned(a, b, axis))
}

/// A note: its position not finite now and then.
fn note() -> impl Strategy<Value = Note> + Clone {
    let at = (maybe_finite(coord()), maybe_finite(coord()));
    let text = select(vec!["check this", "", "Ø.188 REF"]);
    (at, text, agent(), date()).prop_map(|((x, y), text, by, when)| Note {
        at: P2::new(x, y),
        text: text.into(),
        by,
        when,
    })
}

/// An op over the pools, any kind the document takes.
pub fn op() -> BoxedStrategy<Op> {
    let prov = || provenance(SOURCES);
    let v = || select(VIEWS);
    let kinds: Vec<(u32, BoxedStrategy<Op>)> = vec![
        (
            12,
            sketch_op::<Paper>().prop_map(SketchOp::on_sheet).boxed(),
        ),
        (
            12,
            (v(), sketch_op::<Model>())
                .prop_map(|(v, e)| e.in_view(v))
                .boxed(),
        ),
        (
            4,
            (v(), view())
                .prop_map(|(id, w)| Op::add_view(id, w))
                .boxed(),
        ),
        (
            1,
            v().prop_map(|id| Op::RemoveView { id: id.into() }).boxed(),
        ),
        (
            1,
            (v(), transform_or_bad())
                .prop_map(|(v, transform)| Op::SetTransform {
                    view: v.into(),
                    transform,
                })
                .boxed(),
        ),
        (
            1,
            (v(), rect())
                .prop_map(|(v, r)| Op::set_region(v, r))
                .boxed(),
        ),
        (
            1,
            (v(), proptest::option::of(prov()))
                .prop_map(|(v, provenance)| Op::SetModelFrame {
                    view: v.into(),
                    provenance,
                })
                .boxed(),
        ),
        (
            3,
            (select(LINKS), link(), prov())
                .prop_map(|(id, l, p)| Op::add_link(id, l, p))
                .boxed(),
        ),
        (
            1,
            select(LINKS)
                .prop_map(|id| Op::RemoveLink { id: id.into() })
                .boxed(),
        ),
        (
            2,
            (
                select(SOURCES),
                select(vec![SourceKind::Sheet, SourceKind::Photo]),
            )
                .prop_map(|(id, k)| Op::add_source(id, Source::new(k, "archive")))
                .boxed(),
        ),
        (
            1,
            select(SOURCES)
                .prop_map(|id| Op::RemoveSource { id: id.into() })
                .boxed(),
        ),
        (
            1,
            (select(LAYERS), layer())
                .prop_map(|(id, l)| Op::add_layer(id, l))
                .boxed(),
        ),
        (
            1,
            select(LAYERS)
                .prop_map(|id| Op::RemoveLayer { id: id.into() })
                .boxed(),
        ),
        (
            1,
            (select(LAYERS), any::<bool>())
                .prop_map(|(id, screen)| Op::SetLayerVisibility {
                    id: id.into(),
                    visible: Visibility {
                        screen,
                        export: !screen,
                    },
                })
                .boxed(),
        ),
        (
            2,
            (
                select(PARAMS),
                measure_or_raw(Quantity::Length, &[]),
                prov(),
            )
                .prop_filter_map("a parameter", |(n, m, p)| {
                    Some(Op::set_param(n, Param::new(m, p).ok()?))
                })
                .boxed(),
        ),
        (1, select(PARAMS).prop_map(Op::remove_param).boxed()),
        (
            1,
            (select(TEXTS), select(vec!["-15", "3", "A"]), p2(), prov())
                .prop_filter_map("a balloon", |(id, label, at, p)| {
                    Some(Op::add_text(id, Text::balloon(label, at, p).ok()?))
                })
                .boxed(),
        ),
        (
            1,
            select(TEXTS)
                .prop_map(|id| Op::RemoveText { id: id.into() })
                .boxed(),
        ),
        (2, note().prop_map(|note| Op::AddNote { note }).boxed()),
        (
            1,
            (0..3usize)
                .prop_map(|index| Op::RemoveNote { index })
                .boxed(),
        ),
        (
            1,
            select(vec!["LDW280-23631", ""])
                .prop_map(|d| Op::SetTitle {
                    title: Title {
                        drawing: d.into(),
                        ..Title::default()
                    },
                })
                .boxed(),
        ),
        (
            1,
            proptest::option::of((maybe_finite(1.0..20.0), coord(), provenance(SOURCES)))
                .prop_map(|s| Op::SetFrame {
                    id: "scan".into(),
                    frame: s.map(|(px_per_mm, rotation, how)| {
                        Box::new(frame::Frame {
                            file: "scan.png".into(),
                            size_px: [1000, 700],
                            hash: None,
                            registration: Registration::similarity(
                                [1000, 700],
                                Similarity {
                                    origin_px: [10.0, 20.0],
                                    px_per_mm,
                                    rotation,
                                },
                                how,
                            ),
                            hops: Vec::new(),
                            evidence: None,
                        })
                    }),
                })
                .boxed(),
        ),
        (3, Just(Op::Solve).boxed()),
    ];
    Union::new_weighted(kinds).boxed()
}

/// Up to `n` ops.
pub fn ops(n: usize) -> impl Strategy<Value = Vec<Op>> {
    proptest::collection::vec(op(), 0..=n)
}

/// A message to the model: mostly ops, and every other kind.
pub fn msg() -> BoxedStrategy<Msg> {
    prop_oneof![
        8 => op().prop_map(|op| Msg::Op { op }),
        2 => proptest::collection::vec(op(), 1..=3).prop_map(|ops| Msg::Batch { ops }),
        2 => op().prop_map(|op| Msg::Preview { op }),
        1 => Just(Msg::DropPreview),
        1 => Just(Msg::Commit),
        2 => Just(Msg::Undo),
        1 => Just(Msg::Redo),
    ]
    .boxed()
}

/// A sheet with the pool's first source registered, its two views
/// (front along X, Y; top along X, Z), and the pool's entities in each
/// view and in the sheet: lines `a` and `b`, point `c`, circle `d`,
/// roughly placed and unconstrained. Most ops find what they name.
pub fn two_views() -> Document {
    fn entities<F: Frame>() -> [(&'static str, Entity<F>); 4] {
        let p = |x, y| P2::new(x, y);
        [
            (
                "a",
                Entity::Line {
                    start: p(0.0, 0.0),
                    end: p(10.0, 0.5),
                },
            ),
            (
                "b",
                Entity::Line {
                    start: p(10.2, 0.3),
                    end: p(9.8, 8.0),
                },
            ),
            ("c", Entity::Point { at: p(3.0, 4.0) }),
            (
                "d",
                Entity::Circle {
                    centre: p(5.0, 5.0),
                    radius: 2.0,
                },
            ),
        ]
    }
    use crate::examples::ours;
    let view = |axes, y| {
        View::new(axes, Default::default(), Transform::at(100.0, y)).expect("distinct axes")
    };
    let mut ops = vec![
        Op::add_source(SOURCES[0], Source::new(SourceKind::Sheet, "archive")),
        Op::add_view(VIEWS[0], view((ModelAxis::X, ModelAxis::Y), 100.0)),
        Op::add_view(VIEWS[1], view((ModelAxis::X, ModelAxis::Z), 200.0)),
    ];
    for (id, e) in entities::<Paper>() {
        ops.push(SketchOp::add_entity(id, e, "visible", ours()).on_sheet());
    }
    for v in VIEWS {
        for (id, e) in entities::<Model>() {
            ops.push(SketchOp::add_entity(id, e, "visible", ours()).in_view(*v));
        }
    }
    ops.iter().fold(Document::new(Title::default()), |d, op| {
        d.apply(op).expect("a consistent start")
    })
}

/// The example documents (built once): larger than the pools reach,
/// and slow to solve in a debug build, so used as they are.
pub fn examples() -> &'static [Document] {
    static EXAMPLES: OnceLock<Vec<Document>> = OnceLock::new();
    EXAMPLES.get_or_init(|| {
        use crate::examples::*;
        vec![block(), bracket(), dimensions(), spline_contour()]
    })
}

/// One of the [`examples`].
pub fn example() -> impl Strategy<Value = Document> + Clone {
    (0..examples().len()).prop_map(|i| examples()[i].clone())
}

/// The start sequences take: [`two_views`] mostly (where the pools'
/// names mean most), an empty sheet now and then.
pub fn start() -> impl Strategy<Value = Document> + Clone {
    prop_oneof![
        1 => Just(Document::new(Title::default())),
        3 => Just(two_views_once().clone()),
    ]
}

/// [`two_views`], built once.
fn two_views_once() -> &'static Document {
    static TWO_VIEWS: OnceLock<Document> = OnceLock::new();
    TWO_VIEWS.get_or_init(two_views)
}

/// `ops` applied to `doc` in turn, the refused ones skipped.
pub fn fold(doc: &Document, ops: &[Op]) -> Document {
    ops.iter()
        .fold(doc.clone(), |d, op| d.apply(op).unwrap_or(d))
}

/// A document reachable through the api: a [`start`] and up to `n` ops.
pub fn document(n: usize) -> impl Strategy<Value = Document> {
    (start(), ops(n)).prop_map(|(d, ops)| fold(&d, &ops))
}

/// A document of [`two_views`] and up to `n` ops, edits to the views'
/// sketches mostly: small, with constraints, links and solves.
pub fn with_views(n: usize) -> impl Strategy<Value = Document> {
    let edit = (select(VIEWS), sketch_op::<Model>()).prop_map(|(v, e)| e.in_view(v));
    let op = prop_oneof![3 => edit, 1 => op()];
    proptest::collection::vec(op, 0..=n).prop_map(|ops| fold(two_views_once(), &ops))
}
