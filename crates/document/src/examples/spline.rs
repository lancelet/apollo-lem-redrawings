//! A formed contour: a flat run, a bend, and a free curve, in one view.
//! The curve is a two-segment spline joined tangentially to the bend
//! arc at one end and to a straight line at the other, and smooth at
//! its knot. The example is built slightly wrong and solved: the
//! spline's ends sit off the arc and the line, its handles point
//! wrongly, and its knot has a kink; the solve pulls everything into
//! place, which is what the tests check.

use super::ours;
use crate::api::model::{Model, Msg};
use crate::api::msg::{Op, SketchOp};
use crate::state::document::{Document, Title};
use crate::value::geometry::{Entity, Model as ModelFrame, Paper, Rect, P2};
use crate::value::relation::{
    arc, fixed, horizontal, line, smooth, spline, tangent_arc_spline_join, tangent_join,
    tangent_join_spline, ArcEnd, LineEnd, SplineEnd,
};
use crate::value::sketch::Sketch;
use crate::value::view::{ModelAxis, Transform, View, ViewId};
use std::f64::consts::FRAC_PI_2;

/// The contour's view.
pub const VIEW: &str = "contour";

/// The contour document.
pub fn spline_contour() -> Document {
    let mut m = Model::new(Document::new(Title {
        drawing: "LMS-0003".into(),
        rev: "A".into(),
        sheet: "1".into(),
        name: "CONTOUR".into(),
    }));
    let v: ViewId = VIEW.into();
    let at = Transform::new(P2::new(40.0, 150.0), 1.0, 0.0, true).expect("finite");
    let region = Rect::<Paper>::new(P2::new(30.0, 60.0), P2::new(200.0, 160.0)).expect("ordered");
    let mut ops = vec![
        Op::add_view(
            v,
            View::new((ModelAxis::X, ModelAxis::Y), Sketch::new(), at).expect("distinct axes"),
        ),
        Op::set_region(v, region),
    ];
    let add =
        |id: &str, e: Entity<ModelFrame>| SketchOp::add_entity(id, e, "visible", ours()).in_view(v);
    // The flat run, held still and horizontal; the bend, a quarter turn
    // up from its end.
    ops.push(add(
        "flat",
        Entity::Line {
            start: P2::new(0.0, 0.0),
            end: P2::new(40.0, 0.0),
        },
    ));
    ops.push(add(
        "bend",
        Entity::Arc {
            centre: P2::new(40.0, 10.0),
            radius: 10.0,
            start_angle: -FRAC_PI_2,
            sweep: FRAC_PI_2,
        },
    ));
    // The free curve, from the bend's end (50, 10) up and over to the
    // outlet line at (90, 40): deliberately off at both ends, with a
    // kink at its knot.
    ops.push(add(
        "curve",
        Entity::Spline {
            points: vec![
                P2::new(51.0, 9.0),
                P2::new(53.0, 20.0),
                P2::new(60.0, 32.0),
                P2::new(70.0, 30.0),
                P2::new(78.0, 26.0),
                P2::new(84.0, 42.0),
                P2::new(89.0, 41.0),
            ],
        },
    ));
    ops.push(add(
        "outlet",
        Entity::Line {
            start: P2::new(90.0, 40.0),
            end: P2::new(130.0, 40.0),
        },
    ));
    let c = |id: &str, r| SketchOp::add_constraint(id, r, ours()).in_view(v);
    ops.push(c("flat_h", horizontal(line("flat"))));
    ops.push(c("flat_fix_a", fixed(line("flat").start())));
    ops.push(c("flat_fix_b", fixed(line("flat").end())));
    ops.push(c(
        "flat_bend",
        tangent_join(line("flat"), LineEnd::End, arc("bend"), ArcEnd::Start),
    ));
    ops.push(c(
        "bend_curve",
        tangent_arc_spline_join(arc("bend"), ArcEnd::End, spline("curve"), SplineEnd::Start),
    ));
    ops.push(c("curve_smooth", smooth(spline("curve"))));
    ops.push(c(
        "curve_outlet",
        tangent_join_spline(
            line("outlet"),
            LineEnd::Start,
            spline("curve"),
            SplineEnd::End,
        ),
    ));
    ops.push(c("outlet_h", horizontal(line("outlet"))));
    ops.push(c("outlet_fix", fixed(line("outlet").end())));
    ops.push(Op::Solve);
    m.update(Msg::Batch { ops })
        .expect("the contour is consistent");
    m.doc().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::geometry::{PointRef, SplinePos};

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn the_contour_solves_and_round_trips() {
        let d = spline_contour();
        let (_, system) = d.analyse(Default::default()).unwrap();
        assert!(system.residual_max < 1e-8, "{}", system.residual_max);
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        let g = d.views().view(VIEW).unwrap().sketch.geometry();
        let at = |pos| g.position(&PointRef::spline("curve", pos)).unwrap();
        // The curve starts where the bend ends, with its handle along the
        // bend's tangent there: perpendicular to the radius.
        let bend_end = g.position(&arc("bend").end()).unwrap();
        let centre = g.position(&arc("bend").centre()).unwrap();
        let start = at(SplinePos::Start);
        assert!(near(start.x, bend_end.x) && near(start.y, bend_end.y));
        let h0 = at(SplinePos::StartHandle);
        let radius = (bend_end.x - centre.x, bend_end.y - centre.y);
        let handle = (h0.x - start.x, h0.y - start.y);
        let cosine = (radius.0 * handle.0 + radius.1 * handle.1)
            / (radius.0.hypot(radius.1) * handle.0.hypot(handle.1));
        assert!(
            cosine.abs() < 1e-6,
            "start handle {h0:?} against radius {radius:?}"
        );
        // It ends on the outlet line's start, with its handle along the
        // line.
        let outlet = g.position(&line("outlet").start()).unwrap();
        let outlet_end = g.position(&line("outlet").end()).unwrap();
        let end = at(SplinePos::End);
        assert!(near(end.x, outlet.x) && near(end.y, outlet.y));
        let h1 = at(SplinePos::EndHandle);
        let along = (outlet_end.x - outlet.x, outlet_end.y - outlet.y);
        let handle = (end.x - h1.x, end.y - h1.y);
        let sine = (along.0 * handle.1 - along.1 * handle.0)
            / (along.0.hypot(along.1) * handle.0.hypot(handle.1));
        assert!(sine.abs() < 1e-6, "end handle {h1:?} against {along:?}");
        // And it is smooth at its knot: the handles either side of the
        // knot are collinear with it.
        let (p2, knot, p4) = (
            at(SplinePos::Point(2)),
            at(SplinePos::Point(3)),
            at(SplinePos::Point(4)),
        );
        let cross = (knot.x - p2.x) * (p4.y - knot.y) - (knot.y - p2.y) * (p4.x - knot.x);
        let scale = ((knot.x - p2.x).hypot(knot.y - p2.y)) * ((p4.x - knot.x).hypot(p4.y - knot.y));
        assert!((cross / scale).abs() < 1e-6, "kink {}", cross / scale);
        // The flat run did not move: it was held.
        let flat = g.position(&line("flat").end()).unwrap();
        assert!(near(flat.x, 40.0) && near(flat.y, 0.0));
    }

    #[test]
    fn the_spline_is_partly_free_where_nothing_holds_it() {
        // Fourteen values against two coincidences, two tangencies and one
        // smoothness leave most of the curve free to move, which the
        // analysis must say rather than hide; only the end whose
        // coincidence and tangency pin it to the held outlet is fixed.
        let d = spline_contour();
        let (_, system) = d.analyse(Default::default()).unwrap();
        let view = system.views.get(&ViewId::from(VIEW)).expect("the view");
        let curve = view
            .status
            .get(&"curve".into())
            .expect("the curve's status");
        match curve {
            crate::value::sketch::EntityStatus::Partial(free) => {
                assert!(free.len() >= 8, "{free:?}")
            }
            other => panic!("{other:?}"),
        }
    }
}
