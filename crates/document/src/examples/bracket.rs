//! The bracket from the Navy construction manual (fig. 14250_64_1): a
//! base plate with a rounded end and two holes, a tapered upright with a
//! bossed horizontal hole, and a gusset, in three views, third-angle
//! projection as Grumman drew: top view above the front, right side to
//! the right. Sizes are ours (the figure has none); the geometry follows
//! the figure. Hidden lines carry every hole and the gusset behind the
//! upright between views; centre lines mark the holes; the tangent lines
//! of the upright are computed exactly and held by tangency.

use super::ours;
use crate::api::model::{Model, Msg};
use crate::api::msg::{Op, SketchOp};
use crate::state::document::{Document, Source, SourceKind, Title};
use crate::state::views::{aligned, in_view};
use crate::value::geometry::{Entity, Model as ModelFrame, Paper, Rect, P2};
use crate::value::leader::{Along, DrawingEntity, Leader, Target, Tip};
use crate::value::relation::{
    arc, circle, coincident, concentric, horizontal, line, on_line, tangent_at, tangent_join,
    vertical, ArcEnd, LineEnd,
};
use crate::value::sketch::Sketch;
use crate::value::text::{Align, Anchor, BoxPoint, DrawingPoint, Placement, Side, Text};
use crate::value::view::{ModelAxis, Transform, View, ViewId};
use std::f64::consts::{FRAC_PI_2, PI};

// The part, mm. Model X runs along the plate, Y across it, Z up.
const PLATE_X: f64 = 80.0; // plate length, round end at x = 0
const PLATE_Y: f64 = 50.0; // plate width
const PLATE_Z: f64 = 10.0; // plate thickness
const ROUND_R: f64 = 25.0; // the round end: a full semicircle
const HOLE_X: f64 = 25.0; // base holes, both at this x
const HOLE_Y: [f64; 2] = [12.5, 37.5];
const HOLE_R: f64 = 4.0;
const UPRIGHT_X: [f64; 2] = [70.0, 80.0]; // the upright plate, in x
const BOSS_X_END: f64 = 95.0; // the boss runs from the upright to here
const BOSS_Y: f64 = 25.0; // boss axis
const BOSS_Z: f64 = 50.0;
const BOSS_R: f64 = 12.0;
const BORE_R: f64 = 5.0;
const GUSSET_X: f64 = 30.0; // gusset foot on the plate
const GUSSET_Z: f64 = 38.0; // gusset top on the upright (= boss bottom)
const GUSSET_Y: [f64; 2] = [22.0, 28.0];

fn seg(x0: f64, y0: f64, x1: f64, y1: f64) -> Entity<ModelFrame> {
    Entity::Line {
        start: P2::new(x0, y0),
        end: P2::new(x1, y1),
    }
}

/// The point where the tangent from `p` touches the circle `(c, r)`,
/// on the left (`+1`) or right (`-1`) of the line from `p` to `c`.
fn tangent_point(p: (f64, f64), c: (f64, f64), r: f64, side: f64) -> (f64, f64) {
    let (dx, dy) = (c.0 - p.0, c.1 - p.1);
    let d = (dx * dx + dy * dy).sqrt();
    let len = (d * d - r * r).sqrt();
    let phi = dy.atan2(dx) + side * (r / d).asin();
    (p.0 + len * phi.cos(), p.1 + len * phi.sin())
}

/// A view's ops, gathered.
struct Builder {
    view: ViewId,
    ops: Vec<Op>,
}

impl Builder {
    fn new(view: ViewId) -> Self {
        Self {
            view,
            ops: Vec::new(),
        }
    }

    /// A line on a layer; horizontal or vertical ones are held so.
    fn line(&mut self, id: &str, layer: &str, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.ops
            .push(SketchOp::add_entity(id, seg(x0, y0, x1, y1), layer, ours()).in_view(self.view));
        if (y0 - y1).abs() < 1e-12 {
            self.constrain(&format!("{id}_h"), horizontal(line(id)));
        } else if (x0 - x1).abs() < 1e-12 {
            self.constrain(&format!("{id}_v"), vertical(line(id)));
        }
    }

    fn entity(&mut self, id: &str, layer: &str, e: Entity<ModelFrame>) {
        self.ops
            .push(SketchOp::add_entity(id, e, layer, ours()).in_view(self.view));
    }

    fn constrain(&mut self, id: &str, r: crate::value::relation::Relation) {
        self.ops
            .push(SketchOp::add_constraint(id, r, ours()).in_view(self.view));
    }

    /// Consecutive lines joined end to start; closed back to the first.
    fn chain(&mut self, ids: &[&str], closed: bool) {
        for w in ids.windows(2) {
            self.constrain(
                &format!("{}_{}", w[0], w[1]),
                coincident(line(w[0]).end(), line(w[1]).start()),
            );
        }
        if closed {
            let (last, first) = (ids[ids.len() - 1], ids[0]);
            self.constrain(
                &format!("{last}_{first}"),
                coincident(line(last).end(), line(first).start()),
            );
        }
    }
}

/// The top view, model (X, Y).
fn top(view: ViewId) -> Vec<Op> {
    let mut b = Builder::new(view);
    let h0 = UPRIGHT_X[0];
    let (by0, by1) = (BOSS_Y - BOSS_R, BOSS_Y + BOSS_R);
    // The outline, clockwise from the round end's top: the plate's far
    // edge, down the upright's outer face to the boss, round the boss,
    // back along the near edge, and the round end.
    b.entity(
        "ta",
        "visible",
        Entity::Arc {
            centre: P2::new(ROUND_R, ROUND_R),
            radius: ROUND_R,
            start_angle: FRAC_PI_2,
            sweep: PI,
        },
    );
    b.line("tt", "visible", ROUND_R, PLATE_Y, PLATE_X, PLATE_Y);
    b.line("tr2", "visible", PLATE_X, PLATE_Y, PLATE_X, by1);
    b.line("bt", "visible", PLATE_X, by1, BOSS_X_END, by1);
    b.line("br", "visible", BOSS_X_END, by1, BOSS_X_END, by0);
    b.line("bb", "visible", BOSS_X_END, by0, PLATE_X, by0);
    b.line("tr1", "visible", PLATE_X, by0, PLATE_X, 0.0);
    b.line("tb", "visible", PLATE_X, 0.0, ROUND_R, 0.0);
    b.chain(&["tt", "tr2", "bt", "br", "bb", "tr1", "tb"], false);
    // A tangent join is the coincidence and the tangency in one.
    b.constrain(
        "ta_tt_t",
        tangent_join(line("tt"), LineEnd::Start, arc("ta"), ArcEnd::Start),
    );
    b.constrain(
        "tb_ta_t",
        tangent_join(line("tb"), LineEnd::End, arc("ta"), ArcEnd::End),
    );
    // The upright's inner face and the boss's silhouette over it.
    b.line("ul", "visible", h0, 0.0, h0, PLATE_Y);
    b.line("bt2", "visible", h0, by1, PLATE_X, by1);
    b.line("bb2", "visible", h0, by0, PLATE_X, by0);
    // The gusset, seen from above: its two sides and the edge where its
    // slope meets the plate; its far end is the upright's face.
    b.line("g1", "visible", GUSSET_X, GUSSET_Y[0], h0, GUSSET_Y[0]);
    b.line("g2", "visible", GUSSET_X, GUSSET_Y[1], h0, GUSSET_Y[1]);
    b.line(
        "g3",
        "visible",
        GUSSET_X,
        GUSSET_Y[0],
        GUSSET_X,
        GUSSET_Y[1],
    );
    b.constrain("g3_g1", coincident(line("g3").start(), line("g1").start()));
    b.constrain("g3_g2", coincident(line("g3").end(), line("g2").start()));
    // The base holes and the bore through the boss (hidden).
    for (i, y) in HOLE_Y.iter().enumerate() {
        b.entity(
            &format!("c{i}"),
            "visible",
            Entity::Circle {
                centre: P2::new(HOLE_X, *y),
                radius: HOLE_R,
            },
        );
    }
    b.line(
        "hh1",
        "hidden",
        h0,
        BOSS_Y - BORE_R,
        BOSS_X_END,
        BOSS_Y - BORE_R,
    );
    b.line(
        "hh2",
        "hidden",
        h0,
        BOSS_Y + BORE_R,
        BOSS_X_END,
        BOSS_Y + BORE_R,
    );
    // Centre lines: through both holes, across each, and the boss axis.
    b.line(
        "cx",
        "centre",
        HOLE_X,
        HOLE_Y[0] - 7.0,
        HOLE_X,
        HOLE_Y[1] + 7.0,
    );
    for (i, y) in HOLE_Y.iter().enumerate() {
        b.line(
            &format!("cy{i}"),
            "centre",
            HOLE_X - 7.0,
            *y,
            HOLE_X + 7.0,
            *y,
        );
    }
    b.line("cb", "centre", h0 - 5.0, BOSS_Y, BOSS_X_END + 5.0, BOSS_Y);
    b.ops
}

/// The front view, model (X, Z).
fn front(view: ViewId) -> Vec<Op> {
    let mut b = Builder::new(view);
    let (h0, h1) = (UPRIGHT_X[0], UPRIGHT_X[1]);
    let (bz0, bz1) = (BOSS_Z - BOSS_R, BOSS_Z + BOSS_R);
    // The outline, clockwise from the plate's bottom-left.
    b.line("pl", "visible", 0.0, PLATE_Z, 0.0, 0.0);
    b.line("fb", "visible", 0.0, 0.0, h1, 0.0);
    b.line("fr", "visible", h1, 0.0, h1, bz0);
    b.line("bb", "visible", h1, bz0, BOSS_X_END, bz0);
    b.line("br", "visible", BOSS_X_END, bz0, BOSS_X_END, bz1);
    b.line("bt", "visible", BOSS_X_END, bz1, h0, bz1);
    b.line("ul", "visible", h0, bz1, h0, PLATE_Z);
    b.line("pt", "visible", h0, PLATE_Z, 0.0, PLATE_Z);
    b.chain(&["pl", "fb", "fr", "bb", "br", "bt", "ul", "pt"], true);
    // The upright is a plate standing on the base, so the base's top
    // edge continues under it to the outer face (user, 2026-10-15).
    b.line("pb", "visible", h0, PLATE_Z, h1, PLATE_Z);
    b.constrain("pb_ul", coincident(line("pb").start(), line("ul").end()));
    b.constrain("pb_fr", on_line(line("pb").end(), line("fr")));
    // The gusset's slope.
    b.line("g", "visible", GUSSET_X, PLATE_Z, h0, GUSSET_Z);
    // Hidden: the bore through the boss, the base holes.
    b.line(
        "hb1",
        "hidden",
        h0,
        BOSS_Z - BORE_R,
        BOSS_X_END,
        BOSS_Z - BORE_R,
    );
    b.line(
        "hb2",
        "hidden",
        h0,
        BOSS_Z + BORE_R,
        BOSS_X_END,
        BOSS_Z + BORE_R,
    );
    b.line(
        "hk1",
        "hidden",
        HOLE_X - HOLE_R,
        0.0,
        HOLE_X - HOLE_R,
        PLATE_Z,
    );
    b.line(
        "hk2",
        "hidden",
        HOLE_X + HOLE_R,
        0.0,
        HOLE_X + HOLE_R,
        PLATE_Z,
    );
    // Centre lines: the boss axis and the base holes.
    b.line("fc", "centre", h0 - 5.0, BOSS_Z, BOSS_X_END + 5.0, BOSS_Z);
    b.line("fk", "centre", HOLE_X, -4.0, HOLE_X, PLATE_Z + 4.0);
    b.ops
}

/// The right side view, model (Y, Z): looking along −X, Y across the
/// page, Z up.
fn side(view: ViewId) -> Vec<Op> {
    let mut b = Builder::new(view);
    let c = (BOSS_Y, BOSS_Z);
    let tl = tangent_point((0.0, PLATE_Z), c, BOSS_R, 1.0);
    let tr = tangent_point((PLATE_Y, PLATE_Z), c, BOSS_R, -1.0);
    // The plate, and the upright's tapered sides up to the boss.
    b.line("sb", "visible", 0.0, 0.0, PLATE_Y, 0.0);
    b.line("sr", "visible", PLATE_Y, 0.0, PLATE_Y, PLATE_Z);
    b.line("st", "visible", PLATE_Y, PLATE_Z, 0.0, PLATE_Z);
    b.line("sl", "visible", 0.0, PLATE_Z, 0.0, 0.0);
    b.chain(&["sb", "sr", "st", "sl"], true);
    b.line("tl", "visible", 0.0, PLATE_Z, tl.0, tl.1);
    b.line("tr", "visible", PLATE_Y, PLATE_Z, tr.0, tr.1);
    b.constrain("tl_sl", coincident(line("tl").start(), line("sl").start()));
    b.constrain("tr_sr", coincident(line("tr").start(), line("sr").end()));
    // The boss's end face and its bore.
    b.entity(
        "sc",
        "visible",
        Entity::Circle {
            centre: P2::new(c.0, c.1),
            radius: BOSS_R,
        },
    );
    b.entity(
        "sbore",
        "visible",
        Entity::Circle {
            centre: P2::new(c.0, c.1),
            radius: BORE_R,
        },
    );
    b.constrain("bore_conc", concentric(circle("sbore"), circle("sc")));
    b.constrain("tl_t", tangent_at(line("tl"), LineEnd::End, circle("sc")));
    b.constrain("tr_t", tangent_at(line("tr"), LineEnd::End, circle("sc")));
    // Hidden: the base holes, and the gusset behind the upright.
    for (i, y) in HOLE_Y.iter().enumerate() {
        b.line(
            &format!("hk{i}a"),
            "hidden",
            y - HOLE_R,
            0.0,
            y - HOLE_R,
            PLATE_Z,
        );
        b.line(
            &format!("hk{i}b"),
            "hidden",
            y + HOLE_R,
            0.0,
            y + HOLE_R,
            PLATE_Z,
        );
    }
    b.line("sg1", "hidden", GUSSET_Y[0], PLATE_Z, GUSSET_Y[0], GUSSET_Z);
    b.line("sg2", "hidden", GUSSET_Y[1], PLATE_Z, GUSSET_Y[1], GUSSET_Z);
    // Centre lines through the boss and the base holes.
    b.line(
        "scv",
        "centre",
        c.0,
        c.1 - BOSS_R - 5.0,
        c.0,
        c.1 + BOSS_R + 5.0,
    );
    b.line(
        "sch",
        "centre",
        c.0 - BOSS_R - 5.0,
        c.1,
        c.0 + BOSS_R + 5.0,
        c.1,
    );
    for (i, y) in HOLE_Y.iter().enumerate() {
        b.line(&format!("sk{i}"), "centre", *y, -4.0, *y, PLATE_Z + 4.0);
    }
    b.ops
}

/// The bracket document. Model Y and Z point up the page in their views,
/// so each view's transform mirrors.
pub fn bracket() -> Document {
    let mut m = Model::new(Document::new(Title {
        drawing: "LMS-0002".into(),
        rev: "A".into(),
        sheet: "1".into(),
        name: "BRACKET".into(),
    }));
    let at = |x, y| Transform::new(P2::new(x, y), 1.0, 0.0, true).expect("finite");
    let region =
        |x0, y0, x1, y1| Rect::<Paper>::new(P2::new(x0, y0), P2::new(x1, y1)).expect("ordered");
    let (t, f, s): (ViewId, ViewId, ViewId) = ("top".into(), "front".into(), "side".into());
    let view = |axes, tf| View::new(axes, Sketch::new(), tf).expect("distinct axes");
    let mut ops = vec![
        Op::add_source(
            "manual",
            Source::new(SourceKind::Report, "NAVEDTRA 14250, fig. 64-1"),
        ),
        Op::add_view(t, view((ModelAxis::X, ModelAxis::Y), at(40.0, 110.0))),
        Op::add_view(f, view((ModelAxis::X, ModelAxis::Z), at(40.0, 200.0))),
        Op::add_view(s, view((ModelAxis::Y, ModelAxis::Z), at(160.0, 200.0))),
        Op::set_region(t, region(30.0, 50.0, 145.0, 120.0)),
        Op::set_region(f, region(30.0, 130.0, 145.0, 210.0)),
        Op::set_region(s, region(150.0, 130.0, 220.0, 210.0)),
    ];
    ops.extend(top(t));
    ops.extend(front(f));
    ops.extend(side(s));
    // Projection lines between the views, along each shared axis.
    let tv = |p| in_view(t, p);
    let fv = |p| in_view(f, p);
    let sv = |p| in_view(s, p);
    let links = [
        // X: top ↔ front.
        (
            "x_end",
            aligned(fv(line("pl").start()), tv(arc("ta").mid()), ModelAxis::X),
        ),
        (
            "x_gusset",
            aligned(fv(line("g").start()), tv(line("g1").start()), ModelAxis::X),
        ),
        (
            "x_upright",
            aligned(fv(line("ul").start()), tv(line("ul").start()), ModelAxis::X),
        ),
        (
            "x_plate",
            aligned(
                fv(line("fr").start()),
                tv(line("tr1").start()),
                ModelAxis::X,
            ),
        ),
        (
            "x_boss",
            aligned(fv(line("br").start()), tv(line("br").start()), ModelAxis::X),
        ),
        (
            "x_hole",
            aligned(
                fv(line("fk").start()),
                tv(circle("c0").centre()),
                ModelAxis::X,
            ),
        ),
        // Y: top ↔ side.
        (
            "y_near",
            aligned(sv(line("sb").start()), tv(line("tb").end()), ModelAxis::Y),
        ),
        (
            "y_far",
            aligned(sv(line("sr").start()), tv(line("tt").start()), ModelAxis::Y),
        ),
        (
            "y_boss",
            aligned(
                sv(circle("sc").centre()),
                tv(line("cb").start()),
                ModelAxis::Y,
            ),
        ),
        (
            "y_gusset",
            aligned(
                sv(line("sg1").start()),
                tv(line("g1").start()),
                ModelAxis::Y,
            ),
        ),
        (
            "y_hole0",
            aligned(
                sv(line("sk0").start()),
                tv(circle("c0").centre()),
                ModelAxis::Y,
            ),
        ),
        (
            "y_hole1",
            aligned(
                sv(line("sk1").start()),
                tv(circle("c1").centre()),
                ModelAxis::Y,
            ),
        ),
        // Z: front ↔ side.
        (
            "z_base",
            aligned(sv(line("sb").start()), fv(line("fb").start()), ModelAxis::Z),
        ),
        (
            "z_plate",
            aligned(sv(line("st").start()), fv(line("pt").start()), ModelAxis::Z),
        ),
        (
            "z_boss",
            aligned(
                sv(circle("sc").centre()),
                fv(line("fc").start()),
                ModelAxis::Z,
            ),
        ),
        (
            "z_gusset",
            aligned(sv(line("sg1").end()), fv(line("g").end()), ModelAxis::Z),
        ),
    ];
    for (id, l) in links {
        ops.push(Op::add_link(id, l, ours()));
    }
    // The labels, in the view-title role (underlined). FRONT and SIDE sit
    // 10 mm under their views' bases, centred on them; they share a
    // baseline because the two views share the Z origin. TOP is aligned
    // to FRONT: the same column, 90 mm up, under the top view.
    let label = |label: &str| {
        Text::new(&[label], "view_title", P2::new(0.0, 0.0), ours())
            .expect("a label")
            .aligned(Align::Centre)
            .anchored(Anchor::Box {
                at: BoxPoint::TopCentre,
            })
            .expect("a box anchor")
    };
    let under = |view, line_id: &str| Placement::Beside {
        point: DrawingPoint::View {
            view,
            point: line(line_id).mid(),
        },
        offset: P2::new(0.0, 10.0),
    };
    ops.push(Op::add_text(
        "label_front",
        label("FRONT").placed(under(f, "fb")).expect("finite"),
    ));
    ops.push(Op::add_text(
        "label_side",
        label("SIDE").placed(under(s, "sb")).expect("finite"),
    ));
    ops.push(Op::add_text(
        "label_top",
        label("TOP")
            .placed(Placement::AlignedTo {
                text: "label_front".into(),
                anchor: Anchor::Box {
                    at: BoxPoint::TopCentre,
                },
                offset: P2::new(0.0, -90.0),
            })
            .expect("finite"),
    ));
    // The hole note, left of the top view between the two holes, its
    // leader leaving the first line's right end with the standard
    // shoulder and branching to each hole from the knee: one arrow lands
    // wherever its run meets the circle, as a draughtsman aims it, and
    // stays on the hole across a re-solve.
    let note = Text::new(&["8 DIA", "2 HOLES"], "text", P2::new(22.0, 85.0), ours())
        .expect("a note")
        .aligned(Align::Right)
        .anchored(Anchor::Line {
            index: 0,
            side: Side::Right,
        })
        .expect("line 0 exists");
    ops.push(Op::add_text("note_holes", note));
    let hole = |id: &str| {
        Tip::arrow(Target::Along {
            entity: DrawingEntity::View {
                view: t,
                id: id.into(),
            },
            at: Along::Nearest,
        })
    };
    let leader = Leader::new(
        "note_holes",
        Anchor::Line {
            index: 0,
            side: Side::Right,
        },
        hole("c0"),
        ours(),
    )
    .expect("a leader")
    .also_to(hole("c1"))
    .expect("a second tip");
    ops.push(Op::add_leader("note_holes", leader));
    ops.push(Op::Solve);
    m.update(Msg::Batch { ops })
        .expect("the bracket is consistent");
    m.doc().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bracket_is_consistent_and_round_trips() {
        let d = bracket();
        let (_, system) = d.analyse(Default::default()).unwrap();
        assert!(system.residual_max < 1e-9, "{}", system.residual_max);
        assert!(system.groups.is_empty(), "{:?}", system.groups);
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        // The drawing is exact as entered, so the solve moved nothing: a
        // mislinked feature would have been pulled off its place.
        let at = |view: &str, p: crate::value::geometry::PointRef| {
            let g = d.views().view(view).unwrap().sketch.geometry();
            g.position(&p).unwrap()
        };
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(near(at("side", line("hk0a").start()).x, HOLE_Y[0] - HOLE_R));
        assert!(near(at("side", line("hk1b").start()).x, HOLE_Y[1] + HOLE_R));
        assert!(near(at("top", line("hh1").start()).y, BOSS_Y - BORE_R));
        assert!(near(at("top", line("hh2").start()).y, BOSS_Y + BORE_R));
        assert!(near(at("front", line("hk1").start()).x, HOLE_X - HOLE_R));
        assert!(near(at("front", line("g").end()).y, GUSSET_Z));
        assert!(near(at("top", line("g3").start()).x, GUSSET_X));
        // The tangent points lie on the boss circle.
        let g = d.views().view("side").unwrap().sketch.geometry();
        for id in ["tl", "tr"] {
            let p = g.position(&line(id).end()).unwrap();
            let r = ((p.x - BOSS_Y).powi(2) + (p.y - BOSS_Z).powi(2)).sqrt();
            assert!((r - BOSS_R).abs() < 1e-9);
        }
    }
}
