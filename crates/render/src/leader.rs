//! Leaders on the paper: the route from the text's anchor through the
//! gap, the shoulder and the knees to every tip, and the terminators.
//!
//! The geometry is pure: the document side resolves the attachment
//! point, the direction the leader leaves in and each tip's landing
//! (a point, or a curve to land on wherever the last run meets it), and
//! this module turns those into strokes in the layer's style and
//! terminators at the drafting table's sizes. The shared trunk is drawn
//! once; each tip adds only its own runs.

use crate::scene::{Drawn, Path, PathEnd, Primitive, Termination};
use lmsheets_document::value::drafting::{Drafting, Head, Terminator};
use lmsheets_document::value::geometry::{Paper, P2};
use lmsheets_document::value::leader::{Leader, Route, Shoulder};
use lmsheets_document::value::style::LineStyle;
use lmsheets_document::value::text::Side;

/// Where a tip lands, resolved as far as the document can.
#[derive(Clone, Debug, PartialEq)]
pub enum Landing {
    /// This point.
    Point(P2<Paper>),
    /// The point of this curve nearest the last run's start.
    Nearest(Path),
    /// The point of this circle in the direction of the last run's
    /// start from its centre: the run is aimed at the centre.
    Radial {
        /// The centre.
        centre: P2<Paper>,
        /// The radius, mm.
        radius: f64,
    },
}

fn add(a: P2<Paper>, b: P2<Paper>) -> P2<Paper> {
    P2::new(a.x + b.x, a.y + b.y)
}

fn scaled(u: P2<Paper>, k: f64) -> P2<Paper> {
    P2::new(u.x * k, u.y * k)
}

fn same(a: P2<Paper>, b: P2<Paper>) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

/// One tip of a route: its own knees, where it lands, and how it ends.
pub struct RouteTip<'a> {
    /// Knees of this branch after the shared ones, offsets from the
    /// attachment point, mm.
    pub knees: &'a [P2<Paper>],
    /// Where it lands.
    pub landing: Landing,
    /// What marks the end.
    pub terminator: Terminator,
    /// A pull-back of its own, mm; the drafting table's when none.
    pub pull_back: Option<f64>,
    /// An arrowhead of its own size; the drafting table's when none.
    pub head: Option<Head>,
}

/// Where a route starts: the attachment on the paper (a text's anchor,
/// a point on a balloon's ring), the unit direction it leaves in, and
/// the gap it leaves before its first stroke (the drafting table's from
/// a text, none from a ring).
#[derive(Clone, Copy, Debug)]
pub struct Start {
    /// The attachment: a text's anchor, or a point of a balloon's ring;
    /// knees are offsets from it.
    pub attach: P2<Paper>,
    /// The unit direction away.
    pub away: P2<Paper>,
    /// The gap before the first stroke, mm.
    pub gap: f64,
}

impl Start {
    /// A start at a text's anchor, leaving the drafting table's gap.
    pub fn at_text(drafting: &Drafting, attach: P2<Paper>, away: P2<Paper>) -> Self {
        Self {
            attach,
            away,
            gap: drafting.leader_gap(),
        }
    }

    /// A start at a point of the drawing, with no gap: a leader from a
    /// point. It has no shoulder, so the direction it leaves in is its
    /// first run's.
    pub fn at_point(attach: P2<Paper>) -> Self {
        Self {
            attach,
            away: P2::new(1.0, 0.0),
            gap: 0.0,
        }
    }

    /// A start on the left or right point of a ring about `centre` of
    /// `radius`, leaving sideways with no gap: a balloon's leader.
    pub fn at_ring(centre: P2<Paper>, radius: f64, side: Side) -> Self {
        let x = match side {
            Side::Left => -1.0,
            Side::Right => 1.0,
        };
        let away = P2::new(x, 0.0);
        Self {
            attach: add(centre, scaled(away, radius)),
            away,
            gap: 0.0,
        }
    }
}

/// The primitives of a leader's route from its start: the gap, the
/// shoulder, the shared knees, and each tip from the last shared knee;
/// `style` the layer's. Each terminator ends its tip's last run (when
/// the tip has one).
pub fn route(
    drafting: &Drafting,
    start: Start,
    route: &Route,
    tips: &[RouteTip],
    style: &LineStyle,
) -> Drawn {
    let Start { attach, away, gap } = start;
    let (shoulder, knees) = (route.shoulder, &route.knees);
    let stroke = |a, b| Primitive::Stroke {
        path: Path::Line { a, b },
        weight: style.weight,
        dash: style.dash.clone(),
        anchors: Vec::new(),
    };
    let mut out = Vec::new();
    let mut terminations = Vec::new();
    // The trunk: the gap point, the shoulder's knee, the shared knees.
    let start = add(attach, scaled(away, gap));
    let mut trunk = vec![start];
    let shoulder = match shoulder {
        Shoulder::Standard => Some(drafting.shoulder()),
        Shoulder::Length { mm } => Some(mm),
        Shoulder::None => None,
    };
    if let Some(len) = shoulder {
        trunk.push(add(start, scaled(away, len)));
    }
    trunk.extend(knees.iter().map(|k| add(attach, *k)));
    // The trunk's last run: what a head ends whose tip draws none of its
    // own (its run shorter than the head).
    let mut trunk_last = None;
    for w in trunk.windows(2) {
        if !same(w[0], w[1]) {
            trunk_last = Some(out.len());
            out.push(stroke(w[0], w[1]));
        }
    }
    let fork = *trunk.last().expect("the gap point at least");
    // Each tip from the fork through its own knees to its target.
    for tip in tips {
        let mut run = vec![fork];
        run.extend(tip.knees.iter().map(|k| add(attach, *k)));
        let last = *run.last().expect("the fork at least");
        let target = match &tip.landing {
            Landing::Point(p) => *p,
            Landing::Nearest(path) => path.nearest(last),
            Landing::Radial { centre, radius } => {
                let (dx, dy) = (last.x - centre.x, last.y - centre.y);
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.0 {
                    P2::new(centre.x + dx / len * radius, centre.y + dy / len * radius)
                } else {
                    P2::new(centre.x + radius, centre.y)
                }
            }
        };
        // The direction of arrival: from the last vertex distinct from
        // the target; a leader that never leaves its target has none,
        // and takes no arrowhead and no pull-back.
        let unit = run.iter().rev().find(|p| !same(**p, target)).map(|from| {
            let (dx, dy) = (target.x - from.x, target.y - from.y);
            let len = (dx * dx + dy * dy).sqrt();
            P2::new(dx / len, dy / len)
        });
        // The tip, pulled back from the target along the arrival if asked;
        // the run ends just inside the head past its base, so its stroke
        // never shows past the head and no seam shows between them.
        let pull = tip.pull_back.unwrap_or(drafting.pull_back());
        let tip_at = match unit {
            Some(u) => add(target, scaled(u, -pull)),
            None => target,
        };
        let head = tip.head.unwrap_or(drafting.arrow());
        let run_end = match (tip.terminator, unit) {
            (Terminator::Arrow, Some(u)) => add(tip_at, scaled(u, head.inset - head.length)),
            _ => tip_at,
        };
        // A last run shorter than the head already ends inside it: it
        // stops where it is, never doubling back towards the base.
        let back =
            unit.is_some_and(|u| (run_end.x - last.x) * u.x + (run_end.y - last.y) * u.y <= 0.0);
        if !back {
            run.push(run_end);
        }
        // The tip's last run (else the trunk's): what its terminator
        // ends.
        let mut last_run = trunk_last;
        for w in run.windows(2) {
            if !same(w[0], w[1]) {
                last_run = Some(out.len());
                out.push(stroke(w[0], w[1]));
            }
        }
        let towards = unit.unwrap_or(P2::new(0.0, 0.0));
        let drawn_head = match tip.head {
            Some(own) if tip.terminator == Terminator::Arrow => {
                crate::terminator::arrowhead(tip_at, towards, own, Some(style.weight))
            }
            _ => crate::terminator::terminator(
                tip.terminator,
                tip_at,
                towards,
                drafting,
                style.weight,
            ),
        };
        if let Some(head) = drawn_head {
            if let Some(line) = last_run {
                terminations.push(Termination {
                    head: out.len(),
                    line,
                    end: PathEnd::End,
                });
            }
            out.push(head);
        }
    }
    Drawn {
        primitives: out,
        terminations,
    }
}

/// The primitives of a leader: its route from `attach` in the direction
/// `away`, one landing per tip in order, in the layer's style.
pub fn primitives(
    leader: &Leader,
    drafting: &Drafting,
    start: Start,
    landings: &[Landing],
    style: &LineStyle,
) -> Drawn {
    assert_eq!(
        leader.tips().len(),
        landings.len(),
        "one landing per tip, in order"
    );
    let tips: Vec<RouteTip> = leader
        .tips()
        .iter()
        .zip(landings)
        .map(|(tip, landing)| RouteTip {
            knees: &tip.knees,
            landing: landing.clone(),
            terminator: tip.terminator,
            pull_back: tip.pull_back,
            head: tip.head,
        })
        .collect();
    route(drafting, start, leader.route(), &tips, style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Fill;
    use lmsheets_document::value::leader::{Target, Tip};
    use lmsheets_document::value::provenance::{Agent, Date, Method, Provenance};
    use lmsheets_document::value::style::Dash;
    use lmsheets_document::value::text::Anchor;

    fn ours() -> Provenance {
        Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 15).unwrap(),
        )
    }

    fn free(x: f64, y: f64) -> Target {
        Target::Paper {
            point: P2::new(x, y),
        }
    }

    #[test]
    fn the_route_is_gap_shoulder_knees_and_tips() {
        let d = Drafting::standard();
        let style = LineStyle::new(0.35, Dash::hidden()).unwrap();
        let anchor = Anchor::Line {
            index: 0,
            side: Side::Right,
        };
        // Two tips from the shoulder's knee: one with a knee of its own
        // and an arrow, one a dot; a shared knee between.
        let l = Leader::new("n", anchor, Tip::arrow(free(100.0, 50.0)), ours())
            .unwrap()
            .via(vec![P2::new(20.0, 10.0)])
            .unwrap()
            .also_to(Tip::dot(free(100.0, -50.0)).via(vec![P2::new(30.0, -10.0)]))
            .unwrap();
        let attach = P2::new(10.0, 0.0);
        let right = P2::new(1.0, 0.0);
        let landings = [
            Landing::Point(P2::new(100.0, 50.0)),
            Landing::Point(P2::new(100.0, -50.0)),
        ];
        let drawn = primitives(&l, &d, Start::at_text(&d, attach, right), &landings, &style);
        // Each head ends its own tip's last run, at the run's end.
        assert_eq!(
            drawn.terminations,
            vec![
                Termination {
                    head: 3,
                    line: 2,
                    end: PathEnd::End
                },
                Termination {
                    head: 6,
                    line: 5,
                    end: PathEnd::End
                },
            ]
        );
        let out = drawn.primitives;
        // Trunk: start→shoulder knee→shared knee (2); tip 1: fork→target
        // + head (2); tip 2: fork→knee→target + dot (3).
        assert_eq!(out.len(), 7);
        let Primitive::Stroke {
            path: Path::Line { a, b },
            dash,
            ..
        } = &out[0]
        else {
            panic!()
        };
        assert!((a.x - (10.0 + d.leader_gap())).abs() < 1e-12 && a.y == 0.0);
        assert!((b.x - (10.0 + d.leader_gap() + d.shoulder())).abs() < 1e-12);
        // The runs take the layer's dash.
        assert_eq!(*dash, Dash::hidden());
        let Primitive::Fill {
            fill: Fill::Polygon { points },
            outline,
        } = &out[3]
        else {
            panic!()
        };
        assert_eq!(*outline, Some(0.35));
        assert_eq!(points[0], P2::new(100.0, 50.0));
        // The head's base is arrow length back along the run, and its
        // corners arrow width apart.
        let base: P2<Paper> = P2::new(
            (points[1].x + points[2].x) / 2.0,
            (points[1].y + points[2].y) / 2.0,
        );
        let back = ((base.x - 100.0).powi(2) + (base.y - 50.0).powi(2)).sqrt();
        assert!((back - d.arrow().length).abs() < 1e-9);
        let across =
            ((points[1].x - points[2].x).powi(2) + (points[1].y - points[2].y).powi(2)).sqrt();
        assert!((across - d.arrow().width).abs() < 1e-9);
        assert!(matches!(
            out[6],
            Primitive::Fill {
                fill: Fill::Disc { .. },
                outline: None
            }
        ));
        // No shoulder and a nearest landing on a circle: the tip lands
        // where the run from the gap point meets it.
        let l = Leader::new("n", anchor, Tip::arrow(free(0.0, 0.0)), ours())
            .unwrap()
            .shouldered(Shoulder::None)
            .unwrap();
        let circle = Path::Circle {
            centre: P2::new(50.0, 0.0),
            radius: 5.0,
        };
        let out = primitives(
            &l,
            &d,
            Start::at_text(&d, attach, right),
            &[Landing::Nearest(circle)],
            &style,
        )
        .primitives;
        assert_eq!(out.len(), 2);
        let Primitive::Stroke {
            path: Path::Line { b, .. },
            ..
        } = &out[0]
        else {
            panic!()
        };
        // The run stops just inside the head: 45 less the head's length
        // plus its inset.
        let into = d.arrow().length - d.arrow().inset;
        assert!((b.x - (45.0 - into)).abs() < 1e-9 && b.y.abs() < 1e-9);
        let Primitive::Fill {
            fill: Fill::Polygon { points },
            ..
        } = &out[1]
        else {
            panic!()
        };
        assert!((points[0].x - 45.0).abs() < 1e-9);
        // Pulled back 2 mm: the tip and the run both move back along the
        // arrival, and the head still sits on the run's end.
        let pulled = Leader::new(
            "n",
            anchor,
            Tip::arrow(free(0.0, 0.0)).pulled_back(2.0),
            ours(),
        )
        .unwrap()
        .shouldered(Shoulder::None)
        .unwrap();
        let out = primitives(
            &pulled,
            &d,
            Start::at_text(&d, attach, right),
            &[Landing::Nearest(circle)],
            &style,
        )
        .primitives;
        let Primitive::Stroke {
            path: Path::Line { b, .. },
            ..
        } = &out[0]
        else {
            panic!()
        };
        assert!((b.x - (43.0 - into)).abs() < 1e-9);
        let Primitive::Fill {
            fill: Fill::Polygon { points },
            ..
        } = &out[1]
        else {
            panic!()
        };
        assert!((points[0].x - 43.0).abs() < 1e-9 && points[0].y.abs() < 1e-9);
    }

    #[test]
    fn a_run_shorter_than_its_head_never_doubles_back() {
        let d = Drafting::standard();
        let style = LineStyle::new(0.35, Dash::SOLID).unwrap();
        let anchor = Anchor::Line {
            index: 0,
            side: Side::Right,
        };
        // The shoulder ends half a head short of the target.
        let start = Start::at_text(&d, P2::new(0.0, 0.0), P2::new(1.0, 0.0));
        let to = d.leader_gap() + d.shoulder() + d.arrow().length / 2.0;
        let l = Leader::new("n", anchor, Tip::arrow(free(to, 0.0)), ours()).unwrap();
        let drawn = primitives(&l, &d, start, &[Landing::Point(P2::new(to, 0.0))], &style);
        // The shoulder only, then the head, which ends it.
        assert_eq!(drawn.primitives.len(), 2);
        let Primitive::Stroke {
            path: Path::Line { a, b },
            ..
        } = drawn.primitives[0]
        else {
            panic!("the shoulder")
        };
        assert!(b.x > a.x);
        assert_eq!(
            drawn.terminations,
            vec![Termination {
                head: 1,
                line: 0,
                end: PathEnd::End
            }]
        );
    }

    #[test]
    fn a_ring_start_is_on_the_ring_with_no_gap() {
        let centre = P2::new(50.0, 20.0);
        let s = Start::at_ring(centre, 7.5, Side::Left);
        assert!((s.attach.x - 42.5).abs() < 1e-9 && (s.attach.y - 20.0).abs() < 1e-9);
        assert!((s.away.x + 1.0).abs() < 1e-9 && s.away.y.abs() < 1e-9);
        assert_eq!(s.gap, 0.0);
        let s = Start::at_ring(centre, 7.5, Side::Right);
        assert!((s.attach.x - 57.5).abs() < 1e-9 && (s.away.x - 1.0).abs() < 1e-9);
        // The route from it: the shoulder leaves the ring point itself.
        let d = Drafting::standard();
        let style = LineStyle::new(0.35, Dash::SOLID).unwrap();
        let l = Leader::balloon("b", Side::Right, Tip::plain(free(100.0, 20.0)), ours()).unwrap();
        let out = primitives(&l, &d, s, &[Landing::Point(P2::new(100.0, 20.0))], &style).primitives;
        let Primitive::Stroke {
            path: Path::Line { a, b },
            ..
        } = &out[0]
        else {
            panic!("a stroke first")
        };
        assert!((a.x - 57.5).abs() < 1e-9 && (a.y - 20.0).abs() < 1e-9);
        assert!((b.x - 57.5 - d.shoulder()).abs() < 1e-9);
    }

    #[test]
    #[should_panic(expected = "one landing per tip")]
    fn landings_must_match_tips() {
        let l = Leader::new(
            "n",
            Anchor::Line {
                index: 0,
                side: Side::Left,
            },
            Tip::arrow(free(0.0, 0.0)),
            ours(),
        )
        .unwrap();
        let style = LineStyle::new(0.35, Dash::SOLID).unwrap();
        let d = Drafting::standard();
        primitives(
            &l,
            &d,
            Start::at_text(&d, P2::new(0.0, 0.0), P2::new(-1.0, 0.0)),
            &[],
            &style,
        );
    }

    #[test]
    fn a_route_from_a_point_starts_there_and_a_tip_may_size_its_head() {
        let d = Drafting::standard();
        let style = LineStyle::new(0.7, Dash::SOLID).unwrap();
        let head = Head::new(10.0, 3.0, 1.0).unwrap();
        // An L: up from the point, then left to an arrowhead of its own
        // size (a cutting-plane mark).
        let l = Leader::from_point(
            free(50.0, 50.0),
            Tip::arrow(free(40.0, 30.0)).with_head(head),
            ours(),
        )
        .unwrap()
        .via(vec![P2::new(0.0, -20.0)])
        .unwrap();
        let drawn = primitives(
            &l,
            &d,
            Start::at_point(P2::new(50.0, 50.0)),
            &[Landing::Point(P2::new(40.0, 30.0))],
            &style,
        );
        let out = &drawn.primitives;
        assert_eq!(out.len(), 3, "the plane's leg, the return and the head");
        let Primitive::Stroke {
            path: Path::Line { a, b },
            ..
        } = &out[0]
        else {
            panic!()
        };
        assert_eq!((*a, *b), (P2::new(50.0, 50.0), P2::new(50.0, 30.0)));
        // The return stops inside the head's base, the head's own size.
        let Primitive::Stroke {
            path: Path::Line { b, .. },
            ..
        } = &out[1]
        else {
            panic!()
        };
        let pull = d.pull_back();
        assert!(
            (b.x - (40.0 + pull + head.length - head.inset)).abs() < 1e-9,
            "{b:?}"
        );
        let Primitive::Fill {
            fill: Fill::Polygon { points },
            ..
        } = &out[2]
        else {
            panic!()
        };
        let base = points[1].x - points[0].x;
        assert!((base - head.length).abs() < 1e-9, "{points:?}");
        assert!(((points[1].y - points[2].y).abs() - head.width).abs() < 1e-9);
        assert_eq!(
            drawn.terminations,
            vec![Termination {
                head: 2,
                line: 1,
                end: PathEnd::End
            }]
        );
    }
}
