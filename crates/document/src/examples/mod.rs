//! Example documents and sketches: what the render crate draws and the
//! tests exercise. [`block`] and [`bracket`] are built the way a client
//! would build them, through messages to a [`Model`]; [`section_aa`] is
//! Section A-A of LDW280-23631 as a sketch.

mod bracket;
mod dimensions;
mod section_aa;
mod spline;
pub use bracket::bracket;
pub use dimensions::dimensions;
pub use section_aa::section_aa;
pub use spline::spline_contour;

use crate::api::model::{Model, Msg};
use crate::api::msg::{Op, SketchOp};
use crate::solver::equation::{circle, line, point};
use crate::state::document::{Document, Source, SourceKind, Title};
use crate::state::views::{aligned, in_view};
use crate::value::geometry::{Entity, Model as ModelFrame, Paper, Rect, P2};
use crate::value::measure::Measure;
use crate::value::provenance::{Agent, Date, Method, Provenance};
use crate::value::relation::{
    coincident, diameter, horizontal, horizontal_distance, horizontal_points, on_circle, on_line,
    position, vertical, vertical_distance, vertical_points,
};
use crate::value::sketch::Sketch;
use crate::value::view::{ModelAxis, Transform, View, ViewId};

pub(crate) fn ours() -> Provenance {
    Provenance::new(
        Method::Authored,
        Agent::Claude,
        Date::new(2026, 10, 15).expect("a date"),
    )
}

fn mm(v: f64) -> Measure {
    Measure::mm(v).expect("finite")
}

fn seg(x0: f64, y0: f64, x1: f64, y1: f64) -> Entity<ModelFrame> {
    Entity::Line {
        start: P2::new(x0, y0),
        end: P2::new(x1, y1),
    }
}

/// The ops that draw a rectangle `b, r, t, l` (bottom, right, top, left)
/// on `layer` in a view and close it with horizontals and verticals.
fn rect_ops(view: ViewId, ids: [&str; 4], w: f64, h: f64, layer: &str) -> Vec<Op> {
    let [b, r, t, l] = ids;
    let mut ops = vec![
        SketchOp::add_entity(b, seg(0.0, 0.0, w, 0.0), layer, ours()).in_view(view),
        SketchOp::add_entity(r, seg(w, 0.0, w, h), layer, ours()).in_view(view),
        SketchOp::add_entity(t, seg(w, h, 0.0, h), layer, ours()).in_view(view),
        SketchOp::add_entity(l, seg(0.0, h, 0.0, 0.0), layer, ours()).in_view(view),
    ];
    let (lb, lr, lt, ll) = (line(b), line(r), line(t), line(l));
    let joins = [
        coincident(lb.end(), lr.start()),
        coincident(lr.end(), lt.start()),
        coincident(lt.end(), ll.start()),
        coincident(ll.end(), lb.start()),
        horizontal(lb),
        vertical(lr),
        horizontal(lt),
        vertical(ll),
    ];
    for (i, c) in joins.into_iter().enumerate() {
        ops.push(SketchOp::add_constraint(format!("{b}_r{i}").as_str(), c, ours()).in_view(view));
    }
    ops
}

/// A 40 × 20 × 10 block with a Ø6 hole through it, in three views on an
/// A4 sheet: the top view (X, Y) carries every dimension and a station;
/// the front (X, Z) carries the height; the side (Y, Z) carries nothing
/// but its shape and takes every size through links. Solved; the only
/// freedom left is where front and side sit in Z, together.
pub fn block() -> Document {
    let mut m = Model::new(Document::new(Title {
        drawing: "LMS-0001".into(),
        rev: "A".into(),
        sheet: "1".into(),
        name: "BLOCK - THREE VIEWS".into(),
    }));
    let region =
        |x0, y0, x1, y1| Rect::<Paper>::new(P2::new(x0, y0), P2::new(x1, y1)).expect("ordered");
    let view =
        |axes, x, y| View::new(axes, Sketch::new(), Transform::at(x, y)).expect("distinct axes");
    let (top, front, side): (ViewId, ViewId, ViewId) =
        ("top".into(), "front".into(), "side".into());
    let mut ops = vec![
        Op::add_source(
            "brief",
            Source::new(SourceKind::Other, "the three-view block brief"),
        ),
        Op::add_view(
            top,
            view((ModelAxis::X, ModelAxis::Y), 40.0, 40.0)
                .claim_model(ours())
                .expect("checked"),
        ),
        Op::add_view(front, view((ModelAxis::X, ModelAxis::Z), 40.0, 100.0)),
        Op::add_view(side, view((ModelAxis::Y, ModelAxis::Z), 120.0, 100.0)),
        Op::set_region(top, region(30.0, 30.0, 100.0, 80.0)),
        Op::set_region(front, region(30.0, 90.0, 100.0, 130.0)),
        Op::set_region(side, region(110.0, 90.0, 160.0, 130.0)),
    ];
    ops.extend(rect_ops(
        top,
        ["tb", "tr", "tt", "tl"],
        40.0,
        20.0,
        "visible",
    ));
    ops.extend(rect_ops(
        front,
        ["fb", "fr", "ft", "fl"],
        40.0,
        10.0,
        "visible",
    ));
    ops.extend(rect_ops(
        side,
        ["sb", "sr", "st", "sl"],
        20.0,
        10.0,
        "visible",
    ));
    // The hole and four construction points on it, in the top view.
    let hole = Entity::Circle {
        centre: P2::new(10.0, 10.0),
        radius: 3.0,
    };
    let pt = |x, y| Entity::Point { at: P2::new(x, y) };
    ops.extend([
        SketchOp::add_entity("hole", hole, "visible", ours()).in_view(top),
        SketchOp::add_entity("hw", pt(7.0, 10.0), "construction", ours()).in_view(top),
        SketchOp::add_entity("he", pt(13.0, 10.0), "construction", ours()).in_view(top),
        SketchOp::add_entity("hs", pt(10.0, 7.0), "construction", ours()).in_view(top),
        SketchOp::add_entity("hn", pt(10.0, 13.0), "construction", ours()).in_view(top),
    ]);
    let (tb, tr) = (line("tb"), line("tr"));
    let c = circle("hole");
    let top_constraints = [
        ("origin", position(tb.start(), mm(0.0), mm(0.0))),
        ("width", horizontal_distance(tb.start(), tb.end(), mm(40.0))),
        ("depth", vertical_distance(tr.start(), tr.end(), mm(20.0))),
        ("hx", horizontal_distance(tb.start(), c.centre(), mm(10.0))),
        ("hy", vertical_distance(tb.start(), c.centre(), mm(10.0))),
        ("hd", diameter(c, mm(6.0))),
        ("w1", on_circle(point("hw"), c)),
        ("w2", horizontal_points(point("hw"), c.centre())),
        ("e1", on_circle(point("he"), c)),
        ("e2", horizontal_points(point("he"), c.centre())),
        ("s1", on_circle(point("hs"), c)),
        ("s2", vertical_points(point("hs"), c.centre())),
        ("n1", on_circle(point("hn"), c)),
        ("n2", vertical_points(point("hn"), c.centre())),
    ];
    for (id, r) in top_constraints {
        ops.push(SketchOp::add_constraint(id, r, ours()).in_view(top));
    }
    // Hidden lines for the hole in the front and side views.
    for (v, ids, xs) in [
        (front, ["fw", "fe"], [7.0, 13.0]),
        (side, ["ss", "sn"], [7.0, 13.0]),
    ] {
        let (bottom, top_line) = if v == front {
            ("fb", "ft")
        } else {
            ("sb", "st")
        };
        for (id, x) in ids.into_iter().zip(xs) {
            ops.push(SketchOp::add_entity(id, seg(x, 0.0, x, 10.0), "hidden", ours()).in_view(v));
            ops.push(
                SketchOp::add_constraint(format!("{id}_v").as_str(), vertical(line(id)), ours())
                    .in_view(v),
            );
            ops.push(
                SketchOp::add_constraint(
                    format!("{id}_b").as_str(),
                    on_line(line(id).start(), line(bottom)),
                    ours(),
                )
                .in_view(v),
            );
            ops.push(
                SketchOp::add_constraint(
                    format!("{id}_t").as_str(),
                    on_line(line(id).end(), line(top_line)),
                    ours(),
                )
                .in_view(v),
            );
        }
    }
    ops.push(
        SketchOp::add_constraint(
            "height",
            vertical_distance(line("fr").start(), line("fr").end(), mm(10.0)),
            ours(),
        )
        .in_view(front),
    );
    // Projection lines.
    let t = |p| in_view(top, p);
    let f = |p| in_view(front, p);
    let s = |p| in_view(side, p);
    let links = [
        (
            "x0",
            aligned(f(line("fb").start()), t(tb.start()), ModelAxis::X),
        ),
        (
            "x1",
            aligned(f(line("fb").end()), t(tb.end()), ModelAxis::X),
        ),
        (
            "xw",
            aligned(f(line("fw").start()), t(point("hw")), ModelAxis::X),
        ),
        (
            "xe",
            aligned(f(line("fe").start()), t(point("he")), ModelAxis::X),
        ),
        (
            "y0",
            aligned(s(line("sb").start()), t(tb.start()), ModelAxis::Y),
        ),
        (
            "y1",
            aligned(s(line("sb").end()), t(tr.end()), ModelAxis::Y),
        ),
        (
            "ys",
            aligned(s(line("ss").start()), t(point("hs")), ModelAxis::Y),
        ),
        (
            "yn",
            aligned(s(line("sn").start()), t(point("hn")), ModelAxis::Y),
        ),
        (
            "z0",
            aligned(s(line("sr").start()), f(line("fr").start()), ModelAxis::Z),
        ),
        (
            "z1",
            aligned(s(line("sr").end()), f(line("fr").end()), ModelAxis::Z),
        ),
    ];
    for (id, l) in links {
        ops.push(Op::add_link(id, l, ours()));
    }
    ops.push(Op::Solve);
    m.update(Msg::Batch { ops })
        .expect("the block is consistent");
    m.doc().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::query::{Answer, Query};

    #[test]
    fn the_block_is_solved_and_round_trips() {
        let d = block();
        let (_, system) = d.analyse(Default::default()).unwrap();
        assert!(system.residual_max < 1e-9);
        assert!(system.groups.is_empty());
        assert_eq!((system.internal_free, system.free), (0, 1));
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        // The side view's hidden lines landed at the hole's extent.
        let g = d.views().view("side").unwrap().sketch.geometry();
        assert!((g.position(&line("ss").start()).unwrap().x - 7.0).abs() < 1e-9);
        assert!((g.position(&line("sn").start()).unwrap().x - 13.0).abs() < 1e-9);
        // A client sees it through queries.
        let m = Model::new(d);
        let Answer::Info(i) = m.query(&Query::Info).unwrap() else {
            panic!()
        };
        assert_eq!((i.views, i.links), (3, 10));
    }
}
