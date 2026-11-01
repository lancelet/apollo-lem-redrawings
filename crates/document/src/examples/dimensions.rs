//! The dimensions test document: one fragment of geometry per family of
//! dimension, each showing the family's placements, built one family at
//! a time (user, 2026-10-15). Sizes are millimetres and the document
//! prints millimetres to one place, so the values read as the geometry.
//!
//! Fragment 1, LINEAR: a 60 × 30 plate with a chamfered corner and a
//! hole, drawn at 2.5:1 so the lettering sits in proportion. Below: the
//! hole's offset from the corner, with the hole's centre line as one
//! extension line, chained with the hole to the right edge (its text
//! shifted off centre); the overall
//! width outermost, as a reference, inline. Left, on the full-height
//! edge: the height as a line-to-line distance with its text beside the
//! vertical line. Above:
//! the top edge's length as a reference over an unbroken line, and the
//! chamfer's width on a shelf. Inside the part: the chamfer's length,
//! aligned with it.
//! Lines sit 12 mm from the part and from each other, the smallest
//! nearest the part, and no extension line crosses a dimension line.
//!
//! Fragment 2, VALUES: a 40 × 20 block with a notch, each dimension
//! carrying a value form: a symmetric tolerance inline, the limits
//! stacked over their bar, a plus over a minus (inline in a vertical
//! line), and a printed inch fraction set as a fraction.
//!
//! Fragment 3, RADIAL: a link plate with a rounded end and two holes:
//! the end's radius as a note landing where its run meets the arc, the
//! small hole's diameter as a note aimed at the centre, and the large
//! hole's diameter both across it through the centre and outside below
//! with extension lines from its extreme points.
//!
//! Fragment 4, ANGULAR: an isosceles triangle: the two base angles,
//! one with its text in the arc's break and one beyond the arc, and
//! the apex as a reference, the supplement of its constraint's angle.
//!
//! Fragment 5, ORDINATE: a fitting with two holes at stated positions
//! from the datum corner, called out by coordinate as the sheets call
//! out stations: one X with the standard shoulder, one Y as a straight
//! callout, one as a reference.

use super::ours;
use crate::api::model::{Model, Msg};
use crate::api::msg::{Op, SketchOp};
use crate::state::document::{Document, Source, SourceKind, Title};
use crate::value::dimension::{Axis, Dimension, End, Extension, Land, Of, Place, Within};
use crate::value::drafting::{Drafting, Units};
use crate::value::geometry::{Entity, Model as ModelFrame, P2};
use crate::value::leader::Shoulder;
use crate::value::measure::{Measure, Tolerance, Unit};
use crate::value::relation::{
    angle, arc, circle, coincident, concentric, diameter, horizontal, horizontal_distance, length,
    line, line_distance, point_line_distance, position, radius, tangent_join, vertical,
    vertical_distance, ArcEnd, LineEnd,
};
use crate::value::sketch::Sketch;
use crate::value::text::{Align, Anchor, BoxPoint, DrawingPoint, Placement, Side, Text};
use crate::value::view::{ModelAxis, Transform, View, ViewId};
use std::f64::consts::{FRAC_PI_2, PI};

fn seg(x0: f64, y0: f64, x1: f64, y1: f64) -> Entity<ModelFrame> {
    Entity::Line {
        start: P2::new(x0, y0),
        end: P2::new(x1, y1),
    }
}

fn mm(v: f64) -> Measure {
    Measure::mm(v).expect("finite")
}

/// The linear fragment's ops, in the view `v`.
fn linear(v: ViewId) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut add = |id: &str, layer: &str, e: Entity<ModelFrame>| {
        ops.push(SketchOp::add_entity(id, e, layer, ours()).in_view(v));
    };
    // The plate, clockwise from the bottom-left: bottom, right, chamfer,
    // top, left; the hole and its centre lines, the vertical one running
    // down past the chain to serve as its extension line (the renderer
    // phases both so they cross the centre on their short dashes).
    add("pb", "visible", seg(0.0, 0.0, 60.0, 0.0));
    add("pr", "visible", seg(60.0, 0.0, 60.0, 22.0));
    add("ch", "visible", seg(60.0, 22.0, 52.0, 30.0));
    add("pt", "visible", seg(52.0, 30.0, 0.0, 30.0));
    add("pl", "visible", seg(0.0, 30.0, 0.0, 0.0));
    add(
        "c",
        "visible",
        Entity::Circle {
            centre: P2::new(20.0, 15.0),
            radius: 4.0,
        },
    );
    add("cv", "centre", seg(20.0, -7.0, 20.0, 21.0));
    add("chz", "centre", seg(14.0, 15.0, 26.0, 15.0));
    let mut constrain = |id: &str, r| {
        ops.push(SketchOp::add_constraint(id, r, ours()).in_view(v));
    };
    for w in ["pb", "pr", "ch", "pt", "pl", "pb"].windows(2) {
        constrain(
            &format!("{}_{}", w[0], w[1]),
            coincident(line(w[0]).end(), line(w[1]).start()),
        );
    }
    constrain("pb_h", horizontal(line("pb")));
    // "h" is one equation, the distance: the edges' parallelism is stated here.
    constrain("pt_h", horizontal(line("pt")));
    constrain("pr_v", vertical(line("pr")));
    constrain("pl_v", vertical(line("pl")));
    // The constraints the dimensions show.
    constrain(
        "w",
        horizontal_distance(line("pb").start(), line("pb").end(), mm(60.0))
            .driven()
            .expect("a dimension"),
    );
    constrain("h", line_distance(line("pb"), line("pt"), mm(30.0)));
    constrain(
        "hx",
        horizontal_distance(line("pb").start(), circle("c").centre(), mm(20.0)),
    );
    constrain(
        "hr",
        point_line_distance(circle("c").centre(), line("pr"), mm(40.0)),
    );
    constrain(
        "cx",
        horizontal_distance(line("pt").start(), line("ch").start(), mm(8.0)),
    );
    constrain("cl", length(line("ch"), mm(11.3137)));
    constrain(
        "tl",
        length(line("pt"), mm(52.0)).driven().expect("a dimension"),
    );
    let of = |c: &str| Of::in_view(v, c);
    let dim = |id: &str, d: Dimension| Op::add_dimension(id, d);
    // Offsets are paper mm along the left normal of the span's direction
    // on the paper (y down): a positive offset puts the line below a span
    // running right, right of one running up, above one running left.
    ops.push(dim(
        "w",
        Dimension::linear(of("w"), 24.0, ours()).expect("finite"),
    ));
    ops.push(dim(
        "h",
        Dimension::linear(of("h"), -12.0, ours())
            .expect("finite")
            .placed(Place::Beside { side: Side::Left })
            .expect("a place"),
    ));
    ops.push(dim(
        "hx",
        Dimension::linear(of("hx"), 12.0, ours())
            .expect("finite")
            .extended([Extension::Draw, Extension::Omit])
            .expect("linear"),
    ));
    ops.push(dim(
        "hr",
        Dimension::linear(of("hr"), 49.5, ours())
            .expect("finite")
            .shifted(-12.0)
            .expect("finite")
            .extended([Extension::Omit, Extension::Draw])
            .expect("linear"),
    ));
    ops.push(dim(
        "cx",
        Dimension::linear(of("cx"), -22.0, ours())
            .expect("finite")
            .placed(Place::shelf(End::End))
            .expect("a place"),
    ));
    ops.push(dim(
        "cl",
        Dimension::linear(of("cl"), -8.0, ours())
            .expect("finite")
            .aligned(),
    ));
    ops.push(dim(
        "tl",
        Dimension::linear(of("tl"), 10.0, ours())
            .expect("finite")
            .placed(Place::Above)
            .expect("a place"),
    ));
    ops
}

/// The values fragment's ops, in the view `v`: a 40 × 20 block with a
/// notch, every dimension carrying a value form.
fn values(v: ViewId) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut add = |id: &str, e: Entity<ModelFrame>| {
        ops.push(SketchOp::add_entity(id, e, "visible", ours()).in_view(v));
    };
    // Clockwise from the bottom-left: bottom, right, the notch's floor
    // and wall, top, left.
    add("qb", seg(0.0, 0.0, 40.0, 0.0));
    add("qr", seg(40.0, 0.0, 40.0, 15.0));
    add("qn", seg(40.0, 15.0, 38.1, 15.0));
    add("qw", seg(38.1, 15.0, 38.1, 20.0));
    add("qt", seg(38.1, 20.0, 0.0, 20.0));
    add("ql", seg(0.0, 20.0, 0.0, 0.0));
    let mut constrain = |id: &str, r| {
        ops.push(SketchOp::add_constraint(id, r, ours()).in_view(v));
    };
    for w in ["qb", "qr", "qn", "qw", "qt", "ql", "qb"].windows(2) {
        constrain(
            &format!("{}_{}", w[0], w[1]),
            coincident(line(w[0]).end(), line(w[1]).start()),
        );
    }
    for id in ["qb", "qn"] {
        constrain(&format!("{id}_h"), horizontal(line(id)));
    }
    for id in ["qr", "qw", "ql"] {
        constrain(&format!("{id}_v"), vertical(line(id)));
    }
    let printed = |t: &str| Measure::printed(t, Unit::Mm).expect("a number");
    constrain(
        "vw",
        horizontal_distance(line("qb").start(), line("qb").end(), mm(40.0))
            .toleranced(Tolerance::symmetric(mm(0.1)))
            .expect("a length"),
    );
    constrain(
        "vh",
        line_distance(line("qb"), line("qt"), mm(20.0))
            .toleranced(Tolerance::Limits {
                upper: printed("20.05"),
                lower: printed("19.95"),
            })
            .expect("a length"),
    );
    constrain(
        "vr",
        vertical_distance(line("qr").start(), line("qr").end(), mm(15.0))
            .toleranced(Tolerance::Asymmetric {
                plus: mm(0.1),
                minus: mm(0.0),
            })
            .expect("a length"),
    );
    constrain(
        "vt",
        length(line("qt"), Measure::inches("1 1/2").expect("a fraction")),
    );
    let of = |c: &str| Of::in_view(v, c);
    let dim = |id: &str, d: Dimension| Op::add_dimension(id, d);
    ops.push(dim(
        "vw",
        Dimension::linear(of("vw"), 12.0, ours()).expect("finite"),
    ));
    ops.push(dim(
        "vh",
        Dimension::linear(of("vh"), -12.0, ours())
            .expect("finite")
            .placed(Place::Beside { side: Side::Left })
            .expect("a place"),
    ));
    ops.push(dim(
        "vr",
        Dimension::linear(of("vr"), 12.0, ours())
            .expect("finite")
            .placed(Place::Inline)
            .expect("a place"),
    ));
    ops.push(dim(
        "vt",
        Dimension::linear(of("vt"), 12.0, ours()).expect("finite"),
    ));
    ops
}

/// The radial fragment's ops, in the view `v`: a link plate with a
/// rounded end and two holes, its radius and diameters as notes and
/// across.
fn radial(v: ViewId) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut add = |id: &str, e: Entity<ModelFrame>| {
        ops.push(SketchOp::add_entity(id, e, "visible", ours()).in_view(v));
    };
    // The plate: bottom, right, top, and the round end (an arc about
    // (0, 10) from the top edge round the left to the bottom edge);
    // a small hole on the arc's centre and a large one along the plate.
    add("lb", seg(0.0, 0.0, 50.0, 0.0));
    add("lr", seg(50.0, 0.0, 50.0, 20.0));
    add("lt", seg(50.0, 20.0, 0.0, 20.0));
    add(
        "le",
        Entity::Arc {
            centre: P2::new(0.0, 10.0),
            radius: 10.0,
            start_angle: FRAC_PI_2,
            sweep: PI,
        },
    );
    add(
        "h1",
        Entity::Circle {
            centre: P2::new(0.0, 10.0),
            radius: 4.0,
        },
    );
    add(
        "h2",
        Entity::Circle {
            centre: P2::new(35.0, 10.0),
            radius: 6.0,
        },
    );
    let mut constrain = |id: &str, r| {
        ops.push(SketchOp::add_constraint(id, r, ours()).in_view(v));
    };
    constrain("lb_lr", coincident(line("lb").end(), line("lr").start()));
    constrain("lr_lt", coincident(line("lr").end(), line("lt").start()));
    constrain(
        "lt_le",
        tangent_join(line("lt"), LineEnd::End, arc("le"), ArcEnd::Start),
    );
    constrain(
        "le_lb",
        tangent_join(line("lb"), LineEnd::Start, arc("le"), ArcEnd::End),
    );
    constrain("lb_h", horizontal(line("lb")));
    constrain("lt_h", horizontal(line("lt")));
    constrain("lr_v", vertical(line("lr")));
    constrain("h1_c", concentric(circle("h1"), arc("le")));
    constrain("r_end", radius(arc("le"), mm(10.0)));
    constrain("d1", diameter(circle("h1"), mm(8.0)));
    constrain("d2", diameter(circle("h2"), mm(12.0)));
    let of = |c: &str| Of::in_view(v, c);
    let beside = |point, dx, dy| Placement::beside_in(v, point, dx, dy);
    let dim = |id: &str, d: Dimension| Op::add_dimension(id, d);
    // The round end's radius: a note up and left of the arc, its leader
    // landing where the run meets the arc.
    ops.push(dim(
        "r_end",
        Dimension::radial(
            of("r_end"),
            beside(arc("le").centre(), -42.0, -40.0),
            Side::Right,
            ours(),
        )
        .expect("a note"),
    ));
    // The small hole's diameter: a note below, its run aimed at the
    // centre.
    ops.push(dim(
        "d1",
        Dimension::radial(
            of("d1"),
            beside(circle("h1").centre(), -36.0, 46.0),
            Side::Right,
            ours(),
        )
        .expect("a note")
        .landed(Land::Radial)
        .expect("a landing"),
    ));
    // The large hole's diameter twice: across it through the centre,
    // and outside below with extension lines from its extreme points.
    ops.push(dim(
        "d2",
        Dimension::across(of("d2"), -FRAC_PI_2 * 2.0 / 3.0, 0.0, ours()).expect("finite"),
    ));
    ops.push(dim(
        "d2_below",
        Dimension::across(of("d2"), 0.0, 40.0, ours()).expect("finite"),
    ));
    ops
}

/// The angular fragment's ops, in the view `v`: an isosceles triangle,
/// its two base angles and the apex as a reference.
fn angular(v: ViewId) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut add = |id: &str, e: Entity<ModelFrame>| {
        ops.push(SketchOp::add_entity(id, e, "visible", ours()).in_view(v));
    };
    let h = 30.0 * 30f64.to_radians().tan();
    add("tb", seg(0.0, 0.0, 60.0, 0.0));
    add("ts", seg(0.0, 0.0, 30.0, h));
    add("tc", seg(30.0, h, 60.0, 0.0));
    let mut constrain = |id: &str, r| {
        ops.push(SketchOp::add_constraint(id, r, ours()).in_view(v));
    };
    constrain("ts_tb", coincident(line("ts").start(), line("tb").start()));
    constrain("ts_tc", coincident(line("ts").end(), line("tc").start()));
    constrain("tc_tb", coincident(line("tc").end(), line("tb").end()));
    constrain("tb_h", horizontal(line("tb")));
    constrain(
        "tw",
        horizontal_distance(line("tb").start(), line("tb").end(), mm(60.0)),
    );
    let deg = |d: f64| Measure::deg(d).expect("finite");
    constrain("a_left", angle(line("tb"), line("ts"), deg(30.0)));
    constrain("a_right", angle(line("tc"), line("tb"), deg(30.0)));
    // The apex follows from the other two: a reference. The constraint
    // turns from the cap to the slant, 60°; the dimension shows the
    // sector inside the triangle, its supplement.
    constrain(
        "a_apex",
        angle(line("tc"), line("ts"), deg(60.0))
            .driven()
            .expect("a dimension"),
    );
    let of = |c: &str| Of::in_view(v, c);
    let dim = |id: &str, d: Dimension| Op::add_dimension(id, d);
    // Model y runs up the page, so a sector above the base points to
    // negative paper y.
    ops.push(dim(
        "a_left",
        Dimension::angular(of("a_left"), 30.0, P2::new(1.0, -0.3), ours()).expect("finite"),
    ));
    ops.push(dim(
        "a_right",
        Dimension::angular(of("a_right"), 30.0, P2::new(-1.0, -0.3), ours())
            .expect("finite")
            .within(Within::Outside)
            .expect("angular"),
    ));
    ops.push(dim(
        "a_apex",
        Dimension::angular(of("a_apex"), 18.0, P2::new(0.0, 1.0), ours()).expect("finite"),
    ));
    ops
}

/// The ordinate fragment's ops, in the view `v`: a fitting with two
/// holes at stated positions from the datum corner, called out by
/// coordinate as the sheets call out stations.
fn ordinate(v: ViewId) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut add = |id: &str, layer: &str, e: Entity<ModelFrame>| {
        ops.push(SketchOp::add_entity(id, e, layer, ours()).in_view(v));
    };
    add("ob", "visible", seg(0.0, 0.0, 70.0, 0.0));
    add("or", "visible", seg(70.0, 0.0, 70.0, 30.0));
    add("ot", "visible", seg(70.0, 30.0, 0.0, 30.0));
    add("ol", "visible", seg(0.0, 30.0, 0.0, 0.0));
    for (id, x) in [("o1", 25.0), ("o2", 55.0)] {
        add(
            id,
            "visible",
            Entity::Circle {
                centre: P2::new(x, 15.0),
                radius: 3.0,
            },
        );
        add(&format!("{id}v"), "centre", seg(x, 10.0, x, 20.0));
        add(
            &format!("{id}h"),
            "centre",
            seg(x - 5.0, 15.0, x + 5.0, 15.0),
        );
    }
    let mut constrain = |id: &str, r| {
        ops.push(SketchOp::add_constraint(id, r, ours()).in_view(v));
    };
    for w in ["ob", "or", "ot", "ol", "ob"].windows(2) {
        constrain(
            &format!("{}_{}", w[0], w[1]),
            coincident(line(w[0]).end(), line(w[1]).start()),
        );
    }
    constrain("ob_h", horizontal(line("ob")));
    constrain("ot_h", horizontal(line("ot")));
    constrain("or_v", vertical(line("or")));
    constrain("ol_v", vertical(line("ol")));
    constrain("datum", position(line("ob").start(), mm(0.0), mm(0.0)));
    constrain("p1", position(circle("o1").centre(), mm(25.0), mm(15.0)));
    constrain(
        "p2",
        position(circle("o2").centre(), mm(55.0), mm(15.0))
            .driven()
            .expect("a dimension"),
    );
    let of = |c: &str| Of::in_view(v, c);
    let beside = |point, dx, dy| Placement::beside_in(v, point, dx, dy);
    let dim = |id: &str, d: Dimension| Op::add_dimension(id, d);
    // The first hole's X from above with the standard shoulder, its Y
    // from the left as a straight callout (no shoulder, text level with
    // the point); the second hole's X as a reference.
    ops.push(dim(
        "p1x",
        Dimension::ordinate(
            of("p1"),
            Axis::X,
            beside(circle("o1").centre(), -30.0, -32.0),
            Side::Right,
            ours(),
        )
        .expect("a callout"),
    ));
    ops.push(dim(
        "p1y",
        Dimension::ordinate(
            of("p1"),
            Axis::Y,
            beside(circle("o1").centre(), -95.0, 0.0),
            Side::Right,
            ours(),
        )
        .expect("a callout")
        .shouldered(Shoulder::None)
        .expect("a route"),
    ));
    ops.push(dim(
        "p2x",
        Dimension::ordinate(
            of("p2"),
            Axis::X,
            beside(circle("o2").centre(), -10.0, -32.0),
            Side::Right,
            ours(),
        )
        .expect("a callout"),
    ));
    ops
}

/// A fragment's title under it, in the view-title role: `down` below
/// the middle of the line `under`, shifted `across` on the paper.
fn title(id: &str, view: ViewId, under: &str, label: &str, across: f64, down: f64) -> Op {
    Op::add_text(
        id,
        Text::new(&[label], "view_title", P2::new(0.0, 0.0), ours())
            .expect("a label")
            .aligned(Align::Centre)
            .anchored(Anchor::Box {
                at: BoxPoint::TopCentre,
            })
            .expect("a box anchor")
            .placed(Placement::Beside {
                point: DrawingPoint::View {
                    view,
                    point: line(under).mid(),
                },
                offset: P2::new(across, down),
            })
            .expect("finite"),
    )
}

/// The dimensions document. Model Y points up the page in every view.
pub fn dimensions() -> Document {
    let mut m = Model::new(Document::new(Title {
        drawing: "LMS-0003".into(),
        rev: "A".into(),
        sheet: "1".into(),
        name: "DIMENSIONS".into(),
    }));
    let at = |x, y| Transform::new(P2::new(x, y), 2.5, 0.0, true).expect("finite");
    let view = |axes, tf| View::new(axes, Sketch::new(), tf).expect("distinct axes");
    let lin: ViewId = "linear".into();
    let mut ops = vec![
        Op::add_source(
            "brief",
            Source::new(SourceKind::Other, "the dimensions brief"),
        ),
        Op::SetDrafting {
            drafting: Drafting::standard()
                .with_units(Units {
                    unit: Unit::Mm,
                    decimals: 1,
                    leading_zero: true,
                })
                .expect("a length unit"),
        },
        Op::add_view(lin, view((ModelAxis::X, ModelAxis::Y), at(30.0, 120.0))),
    ];
    ops.extend(linear(lin));
    ops.push(title("title_linear", lin, "pb", "LINEAR", 0.0, 44.0));
    // The values block shares the plate's centre line (x = 105).
    let val: ViewId = "values".into();
    ops.push(Op::add_view(
        val,
        view((ModelAxis::X, ModelAxis::Y), at(55.0, 260.0)),
    ));
    ops.extend(values(val));
    ops.push(title("title_values", val, "qb", "VALUES", 0.0, 32.0));
    // The link plate spans 60 model units from the arc's left to the
    // right edge: 150 paper, centred on x = 105 with the arc at the left.
    let rad: ViewId = "radial".into();
    ops.push(Op::add_view(
        rad,
        view((ModelAxis::X, ModelAxis::Y), at(55.0, 400.0)),
    ));
    ops.extend(radial(rad));
    // The plate's centre is 5 model units left of the bottom edge's
    // middle: the round end reaches 10 further left.
    ops.push(title("title_radial", rad, "lb", "RADIAL", -12.5, 40.0));
    let ang: ViewId = "angular".into();
    ops.push(Op::add_view(
        ang,
        view((ModelAxis::X, ModelAxis::Y), at(30.0, 560.0)),
    ));
    ops.extend(angular(ang));
    ops.push(title("title_angular", ang, "tb", "ANGULAR", 0.0, 24.0));
    let ord: ViewId = "ordinate".into();
    ops.push(Op::add_view(
        ord,
        view((ModelAxis::X, ModelAxis::Y), at(17.5, 700.0)),
    ));
    ops.extend(ordinate(ord));
    ops.push(title("title_ordinate", ord, "ob", "ORDINATE", 0.0, 24.0));
    ops.push(Op::Solve);
    m.update(Msg::Batch { ops })
        .expect("the dimensions document is consistent");
    m.doc().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dimensions_document_is_consistent_and_round_trips() {
        let d = dimensions();
        let (_, system) = d.analyse(Default::default()).unwrap();
        assert!(system.residual_max < 1e-9, "{}", system.residual_max);
        assert!(system.groups.is_empty(), "{:?}", system.groups);
        assert_eq!(d.dimensions().count(), 21);
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
    }
}
