//! Observed roundings in place of sharp corners (docs/redraw-plan.md
//! §8.3, K11).
//!
//! A drawn rounding names the two lines of a junction and a radius. The
//! design stays sharp, so the lines are the design's, meeting at the
//! theoretical corner, where its dimensions measure to. What is drawn is
//! the lines trimmed back to where an arc of that radius, tangent to both,
//! touches them, and the arc. A rounding is drawn when it says so
//! (`drawn`) and its layer is visible for the purpose; with the layer
//! hidden the corner is sharp again.
//!
//! The arc is computed here, on the paper, from the lines as they are in
//! the document and the radius: the stored centre is the record's, not
//! the drawing's, so the arc is always tangent to the lines it trims. A
//! rounding that cannot be drawn (the lines parallel, the corner not at
//! an end of both, the arc longer than a line) is left sharp.

use crate::draw::Onto;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::geometry::{Entity, EntityId, Paper, P2};
use lmsheets_document::value::observed::{Kind, ObservedId};
use lmsheets_document::value::sketch::{LayerId, Sketch};
use lmsheets_document::value::style::Purpose;
use lmsheets_document::value::view::{Transform, ViewId};
use std::collections::BTreeMap;
use std::f64::consts::PI;

/// A line's two paper ends, each possibly moved back to a tangent point:
/// `[start, end]`.
pub(crate) type Trim = [Option<P2<Paper>>; 2];

/// An arc to draw: its curve on the paper, its layer, the line whose
/// style it takes (the arc is part of the same outline).
pub(crate) struct Arc {
    pub id: ObservedId,
    pub layer: LayerId,
    pub donor: EntityId,
    pub centre: P2<Paper>,
    pub radius: f64,
    pub start: f64,
    pub sweep: f64,
}

/// What the drawn roundings of a view do: the lines they trim and the
/// arcs they add.
#[derive(Default)]
pub(crate) struct Rounded {
    pub trims: BTreeMap<EntityId, Trim>,
    pub arcs: Vec<Arc>,
}

type V = [f64; 2];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(a: V) -> f64 {
    a[0].hypot(a[1])
}

fn unit(a: V) -> Option<V> {
    let l = len(a);
    (l > 1e-12).then(|| [a[0] / l, a[1] / l])
}

fn at(p: P2<Paper>) -> V {
    [p.x, p.y]
}

fn pt(v: V) -> P2<Paper> {
    P2::new(v[0], v[1])
}

/// The paper ends of a line of the sketch.
fn ends<F: lmsheets_document::value::geometry::Frame>(
    sketch: &Sketch<F>,
    id: EntityId,
    onto: Onto,
) -> Option<[V; 2]> {
    match sketch.geometry().get(id)? {
        Entity::Line { start, end } => Some([at(onto.point(*start)), at(onto.point(*end))]),
        _ => None,
    }
}

/// The corner of two lines and the arc of `radius` tangent to both: the
/// tangent points, with which end of each line they replace, and the
/// arc's centre, start angle and sweep. `None` where it cannot be drawn.
struct Corner {
    /// Which end (0 start, 1 end) of each line is at the corner, and the
    /// tangent point it moves to.
    moves: [(usize, V); 2],
    centre: V,
    start: f64,
    sweep: f64,
}

fn corner(a: [V; 2], b: [V; 2], radius: f64) -> Option<Corner> {
    // The lines' intersection: a0 + s·da = b0 + u·db.
    let (da, db) = (sub(a[1], a[0]), sub(b[1], b[0]));
    let det = da[0] * db[1] - da[1] * db[0];
    if det.abs() < 1e-9 * len(da) * len(db) {
        return None;
    }
    let w = sub(b[0], a[0]);
    let s = (w[0] * db[1] - w[1] * db[0]) / det;
    let p = [a[0][0] + s * da[0], a[0][1] + s * da[1]];
    // The end of each line at the corner (the nearer to it), and the
    // direction from the corner along the line.
    let near = |l: [V; 2]| {
        let k = usize::from(len(sub(l[1], p)) < len(sub(l[0], p)));
        (k, l[1 - k], len(sub(l[k], p)), len(sub(l[1 - k], p)))
    };
    let ((ka, far_a, gap_a, span_a), (kb, far_b, gap_b, span_b)) = (near(a), near(b));
    // The corner is where the lines end: not out in the middle of a line.
    if gap_a > 0.25 * span_a || gap_b > 0.25 * span_b {
        return None;
    }
    let (ua, ub) = (unit(sub(far_a, p))?, unit(sub(far_b, p))?);
    let cos = (ua[0] * ub[0] + ua[1] * ub[1]).clamp(-1.0, 1.0);
    // The interior angle between the lines; straight (π) or folded back
    // (0) has no rounding.
    let phi = cos.acos();
    if !((1e-3..PI - 1e-6).contains(&phi) && radius.is_finite() && radius > 0.0) {
        return None;
    }
    let t = radius / (phi / 2.0).tan();
    if t > 0.98 * span_a.min(span_b) {
        return None;
    }
    let bisector = unit([ua[0] + ub[0], ua[1] + ub[1]])?;
    let d = radius / (phi / 2.0).sin();
    let centre = [p[0] + bisector[0] * d, p[1] + bisector[1] * d];
    let (ta, tb) = (
        [p[0] + ua[0] * t, p[1] + ua[1] * t],
        [p[0] + ub[0] * t, p[1] + ub[1] * t],
    );
    let angle = |q: V| (q[1] - centre[1]).atan2(q[0] - centre[0]);
    let mut sweep = angle(tb) - angle(ta);
    while sweep > PI {
        sweep -= 2.0 * PI;
    }
    while sweep <= -PI {
        sweep += 2.0 * PI;
    }
    Some(Corner {
        moves: [(ka, ta), (kb, tb)],
        centre,
        start: angle(ta),
        sweep,
    })
}

/// The roundings of one view that are drawn for `purpose`.
pub(crate) fn of_view(doc: &Document, view: ViewId, t: Transform, purpose: Purpose) -> Rounded {
    let mut out = Rounded::default();
    let Some(v) = doc.views().view(view) else {
        return out;
    };
    let onto = Onto::View(t);
    for (id, o) in doc.observed() {
        let Kind::Rounding {
            junction, radius, ..
        } = &o.kind
        else {
            continue;
        };
        let shown = doc.layer(o.layer).is_some_and(|l| l.visible.is_on(purpose));
        if o.view != view || !o.drawn || !shown {
            continue;
        }
        let (Some(a), Some(b)) = (
            ends(&v.sketch, junction[0], onto),
            ends(&v.sketch, junction[1], onto),
        ) else {
            continue;
        };
        let Some(c) = corner(a, b, radius * t.scale) else {
            continue;
        };
        for (line, (k, at)) in junction.iter().zip(c.moves) {
            out.trims.entry(*line).or_default()[k] = Some(pt(at));
        }
        out.arcs.push(Arc {
            id: *id,
            layer: o.layer,
            donor: junction[0],
            centre: pt(c.centre),
            radius: radius * t.scale,
            start: c.start,
            sweep: c.sweep,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::draw;
    use crate::scene::{Item, Path, Plane, Primitive, Source};
    use lmsheets_document::api::msg::Op;
    use lmsheets_document::examples::block;
    use lmsheets_document::value::geometry::Model;
    use lmsheets_document::value::observed::{Fit, Observed};
    use lmsheets_document::value::provenance::{Agent, Citation, Date, Method, Provenance};
    use lmsheets_document::value::style::Visibility;

    fn rounded_block(radius: f64) -> Document {
        let fit = Fit {
            ink_radius_mm: 1.0,
            pen_mm: 0.1,
            rms_sharp_mm: 0.2,
            rms_rounded_mm: 0.05,
            rms_best_mm: 0.04,
            loss_mm: 0.3,
            points: 10,
            sigma_mm: 0.0,
            own: None,
        };
        let seen = Provenance::new(
            Method::Measured { sigma_mm: 0.1 },
            Agent::Claude,
            Date::new(2026, 10, 28).unwrap(),
        )
        .cite(Citation::new("brief", "the corner"));
        let o = Observed::rounding(
            "top".into(),
            ["tb".into(), "tr".into()],
            radius,
            P2::<Model>::new(0.0, 0.0),
            fit,
            seen,
        );
        block()
            .apply(&Op::SetObserved {
                observed: [("OB1".into(), o)].into(),
            })
            .unwrap()
    }

    fn line_of(plane: &Plane, id: &str) -> (P2<Paper>, P2<Paper>) {
        plane
            .items
            .iter()
            .find_map(|i| match (&i.source, &i.primitive) {
                (
                    Source::View(v, e),
                    Primitive::Stroke {
                        path: Path::Line { a, b },
                        ..
                    },
                ) if v.as_str() == "top" && e.as_str() == id => Some((*a, *b)),
                _ => None,
            })
            .unwrap()
    }

    fn arcs(plane: &Plane) -> Vec<&Item> {
        plane
            .items
            .iter()
            .filter(|i| matches!(i.source, Source::Observed(_)))
            .collect()
    }

    /// A drawn rounding trims the two lines to the arc's tangent points
    /// and draws the arc, tangent to both, at the radius on the paper;
    /// the design (the view's geometry) is untouched.
    #[test]
    fn a_drawn_rounding_trims_the_lines_and_adds_a_tangent_arc() {
        let sharp = draw(&block(), Purpose::Export);
        let d = rounded_block(2.0);
        let plane = draw(&d, Purpose::Export);
        let top = d.views().view("top").unwrap();
        let scale = top.transform.scale;
        let found = arcs(&plane);
        assert_eq!(found.len(), 1);
        let Primitive::Stroke {
            path:
                Path::Arc {
                    centre,
                    radius,
                    start,
                    sweep,
                },
            weight,
            ..
        } = &found[0].primitive
        else {
            panic!("an arc")
        };
        assert!((radius - 2.0 * scale).abs() < 1e-9);
        assert_eq!(found[0].layer, "observed".into());
        // It has the weight of the lines it rounds.
        let Primitive::Stroke { weight: w0, .. } = &sharp
            .items
            .iter()
            .find(|i| i.source == Source::View("top".into(), "tb".into()))
            .unwrap()
            .primitive
        else {
            panic!()
        };
        assert_eq!(weight, w0);
        // A right angle: a quarter turn.
        assert!((sweep.abs() - PI / 2.0).abs() < 1e-9);
        let end = Path::Arc {
            centre: *centre,
            radius: *radius,
            start: *start,
            sweep: *sweep,
        };
        let (ta, tb) = (end.at(0.0), end.at(1.0));
        // Each line now ends on the arc's end it is tangent at, and is
        // shorter than the sharp one by the tangent length (the radius,
        // for a right angle).
        for (id, tangent) in [("tb", ta), ("tr", tb)] {
            let (a0, a1) = line_of(&sharp, id);
            let (b0, b1) = line_of(&plane, id);
            let moved = [(a0, b0), (a1, b1)];
            let changed: Vec<_> = moved
                .iter()
                .filter(|(p, q)| (p.x - q.x).hypot(p.y - q.y) > 1e-9)
                .collect();
            assert_eq!(changed.len(), 1, "{id}: one end moves");
            let (p, q) = changed[0];
            assert!((q.x - tangent.x).hypot(q.y - tangent.y) < 1e-9, "{id}");
            assert!(((p.x - q.x).hypot(p.y - q.y) - radius).abs() < 1e-9, "{id}");
            // Tangent: the radius to the tangent point is square to the line.
            let dir = [b1.x - b0.x, b1.y - b0.y];
            let rad = [q.x - centre.x, q.y - centre.y];
            assert!((dir[0] * rad[0] + dir[1] * rad[1]).abs() < 1e-9 * len(dir) * radius);
        }
        // The design is as it was.
        assert_eq!(
            d.views().view("top").unwrap().sketch,
            block().views().view("top").unwrap().sketch
        );
    }

    /// Not drawn when the observation says not, when its layer is hidden
    /// for the purpose, or when the arc would be longer than a line; the
    /// corner is then as sharp as the design.
    #[test]
    fn a_rounding_that_is_not_drawn_leaves_the_corner_sharp() {
        let sharp = draw(&block(), Purpose::Export);
        let same = |plane: &Plane| {
            assert!(arcs(plane).is_empty());
            for id in ["tb", "tr", "tt", "tl"] {
                assert_eq!(line_of(plane, id), line_of(&sharp, id), "{id}");
            }
        };
        let mut d = rounded_block(2.0);
        let mut o = d.observed().values().next().unwrap().clone();
        o.drawn = false;
        let hidden_obs = d
            .apply(&Op::SetObserved {
                observed: [("OB1".into(), o)].into(),
            })
            .unwrap();
        same(&draw(&hidden_obs, Purpose::Export));
        d = d
            .apply(&Op::SetLayerVisibility {
                id: "observed".into(),
                visible: Visibility {
                    screen: true,
                    export: false,
                },
            })
            .unwrap();
        same(&draw(&d, Purpose::Export));
        // Shown on the screen still.
        assert_eq!(arcs(&draw(&d, Purpose::Screen)).len(), 1);
        // Far too big for the lines.
        same(&draw(&rounded_block(1000.0), Purpose::Export));
    }

    /// Dimensions still measure to the sharp corner: with a corner of the
    /// dimensioned plate rounded, what the dimensions draw (their lines,
    /// lettering and witness lines) is the same as without.
    #[test]
    fn dimensions_still_measure_to_the_sharp_corner() {
        use lmsheets_document::examples::dimensions;
        let sharp_doc = dimensions();
        let fit = Fit {
            ink_radius_mm: 1.0,
            pen_mm: 0.1,
            rms_sharp_mm: 0.2,
            rms_rounded_mm: 0.05,
            rms_best_mm: 0.04,
            loss_mm: 0.3,
            points: 10,
            sigma_mm: 0.0,
            own: None,
        };
        let seen = Provenance::new(
            Method::Measured { sigma_mm: 0.1 },
            Agent::Claude,
            Date::new(2026, 10, 28).unwrap(),
        )
        .cite(Citation::new("brief", "the corner"));
        // The plate's right edge and its chamfer, a 135-degree corner.
        let o = Observed::rounding(
            "linear".into(),
            ["pr".into(), "ch".into()],
            2.0,
            P2::<Model>::new(0.0, 0.0),
            fit,
            seen,
        );
        let rounded_doc = sharp_doc
            .apply(&Op::SetObserved {
                observed: [("OB1".into(), o)].into(),
            })
            .unwrap();
        let (sharp, rounded) = (
            draw(&sharp_doc, Purpose::Export),
            draw(&rounded_doc, Purpose::Export),
        );
        assert_eq!(arcs(&rounded).len(), 1);
        let dimensioned = |p: &Plane| -> Vec<(String, String)> {
            p.items
                .iter()
                .filter(|i| matches!(i.source, Source::Dimension(..) | Source::Witness(_)))
                .map(|i| (format!("{:?}", i.source), format!("{:?}", i.primitive)))
                .collect()
        };
        assert!(dimensioned(&sharp).len() > 10);
        assert_eq!(dimensioned(&rounded), dimensioned(&sharp));
        // The lines of the corner are the ones that changed.
        let changed = sharp
            .items
            .iter()
            .filter(|i| matches!(i.source, Source::View(..)))
            .filter(|i| !rounded.items.contains(i))
            .count();
        assert_eq!(changed, 2);
    }

    /// An obtuse corner (a shallow taper): the tangent length is
    /// r·tan(deflection/2), the arc's ends are on the lines at that
    /// distance, the centre is r from both lines and the arc turns by
    /// the deflection; the corner may be named in either order and with
    /// either line reversed.
    #[test]
    fn an_obtuse_corner_has_the_right_tangent_length_and_turn() {
        let (flat, taper) = ([[0.0, 0.0], [10.0, 0.0]], [[10.0, 0.0], [20.0, 3.0]]);
        let r = 1.5;
        let c = corner(flat, taper, r).expect("drawn");
        let deflection = (3.0f64).atan2(10.0);
        let t = r * (deflection / 2.0).tan();
        let (ka, ta) = c.moves[0];
        let (kb, tb) = c.moves[1];
        assert_eq!((ka, kb), (1, 0), "the flat's end and the taper's start");
        assert!((ta[0] - (10.0 - t)).abs() < 1e-9 && ta[1].abs() < 1e-9);
        let along = [10.0 + t * deflection.cos(), t * deflection.sin()];
        assert!((tb[0] - along[0]).abs() < 1e-9 && (tb[1] - along[1]).abs() < 1e-9);
        assert!((c.sweep.abs() - deflection).abs() < 1e-9);
        let dist = |p: V, q: V| len(sub(p, q));
        assert!((dist(c.centre, ta) - r).abs() < 1e-9);
        assert!((dist(c.centre, tb) - r).abs() < 1e-9);
        // The other way round, one line reversed: the same arc.
        let again = corner(taper, [flat[1], flat[0]], r).expect("drawn");
        assert!(dist(again.centre, c.centre) < 1e-9);
        assert!((again.sweep.abs() - c.sweep.abs()).abs() < 1e-9);
        // Collinear: no corner. Too big: none.
        assert!(corner(flat, [[10.0, 0.0], [20.0, 0.0]], r).is_none());
        assert!(corner(flat, taper, 1000.0).is_none());
    }
}
