//! The scene: primitives in paper millimetres with their source, layer
//! and tint. No colour yet; that is the theme's, at resolve.

use lmsheets_document::value::bounds::{
    arc_bounds, circle_bounds, cubic_bounds, elliptical_arc_bounds, Bounded,
};
use lmsheets_document::value::centre::{CentreLineId, CentreMarkId};
use lmsheets_document::value::dimension::DimensionId;
use lmsheets_document::value::form::{FieldId, FormItem};
use lmsheets_document::value::geometry::{EntityId, Paper, Rect, P2};
use lmsheets_document::value::leader::LeaderId;
use lmsheets_document::value::observed::ObservedId;
use lmsheets_document::value::sketch::LayerId;
use lmsheets_document::value::style::{Dash, Marker, PaletteId};
use lmsheets_document::value::text::{DrawingPoint, Line, TextId};
use lmsheets_document::value::unmodelled::UnmodelledId;
use lmsheets_document::value::view::ViewId;
use std::f64::consts::TAU;

/// The document item a primitive came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// An entity of the sheet sketch.
    Sheet(EntityId),
    /// An entity of a view.
    View(ViewId, EntityId),
    /// A text.
    Text(TextId),
    /// The figure drawn round an enclosed text: a balloon's ring.
    Enclosure(TextId),
    /// A leader.
    Leader(LeaderId),
    /// A dimension: its lines and heads, or its lettering.
    Dimension(DimensionId, Part),
    /// A centre mark.
    CentreMark(CentreMarkId),
    /// A centre line.
    CentreLine(CentreLineId),
    /// A witness (extension) line: its plane's [`Plane::witnesses`]
    /// says where it leaves from and which dimensions use it.
    Witness(WitnessId),
    /// An unmodelled mark's outline.
    Unmodelled(UnmodelledId),
    /// An observed rounding's arc, drawn in place of a sharp corner.
    Observed(ObservedId),
    /// A printed item of the form.
    Form(FormItem),
    /// A field of the placed form: its entry's lettering.
    Field(FieldId),
}

/// A witness line's place among its plane's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WitnessId(pub u32);

/// A witness (extension) line: a node of the drawing in its own right,
/// held by every dimension that measures from its foot in its
/// direction, drawn once over the union of what they need. No one of
/// them owns it.
#[derive(Clone, Debug, PartialEq)]
pub struct Witness {
    /// The measured point it leaves, when that is a point of a sketch
    /// (a foot on a line, an angle's vertex, are not).
    pub foot: Option<DrawingPoint>,
    /// Where it leaves on the paper, mm.
    pub origin: P2<Paper>,
    /// Its direction, a unit vector towards the dimension lines.
    pub dir: P2<Paper>,
    /// The dimensions using it, each once, in the document's order.
    pub users: Vec<DimensionId>,
}

/// Which part of a dimension a primitive draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The dimension line, its tails and heads, or a note's leader.
    Line,
    /// The value's lettering.
    Lettering,
}

/// A curve on the paper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Path {
    /// A segment.
    Line {
        /// One end.
        a: P2<Paper>,
        /// The other.
        b: P2<Paper>,
    },
    /// An arc from `start` through `sweep` radians, positive towards +y
    /// (clockwise on a page whose y runs down).
    Arc {
        /// The centre.
        centre: P2<Paper>,
        /// The radius, mm.
        radius: f64,
        /// The start angle, radians from +x.
        start: f64,
        /// The signed sweep, radians.
        sweep: f64,
    },
    /// A full circle.
    Circle {
        /// The centre.
        centre: P2<Paper>,
        /// The radius, mm.
        radius: f64,
    },
    /// A cubic Bézier.
    Cubic {
        /// The start.
        a: P2<Paper>,
        /// The first control point.
        c1: P2<Paper>,
        /// The second control point.
        c2: P2<Paper>,
        /// The end.
        b: P2<Paper>,
    },
    /// An arc of an ellipse (lettering under a width factor): the point
    /// at parameter `t` is `centre + R(rotation) · (rx cos t, ry sin t)`,
    /// from `start` through `sweep`.
    EllipticalArc {
        /// The centre.
        centre: P2<Paper>,
        /// The semi-axis along the rotated x, mm.
        rx: f64,
        /// The semi-axis along the rotated y, mm.
        ry: f64,
        /// The rotation of the axes, radians.
        rotation: f64,
        /// The start parameter, radians.
        start: f64,
        /// The signed sweep, radians.
        sweep: f64,
    },
}

impl Bounded<Paper> for Path {
    /// The exact box of the curve (its centre line; a pen's width is
    /// the caller's to add).
    fn bounds(&self) -> Option<Rect<Paper>> {
        match *self {
            Path::Line { a, b } => Rect::of_points([a, b]),
            Path::Circle { centre, radius } => circle_bounds(centre, radius),
            Path::Arc {
                centre,
                radius,
                start,
                sweep,
            } => arc_bounds(centre, radius, start, sweep),
            Path::Cubic { a, c1, c2, b } => cubic_bounds([a, c1, c2, b]),
            Path::EllipticalArc {
                centre,
                rx,
                ry,
                rotation,
                start,
                sweep,
            } => elliptical_arc_bounds(centre, rx, ry, rotation, start, sweep),
        }
    }
}

impl Path {
    /// The length along the curve, mm (a cubic's and an ellipse's by
    /// 16-point Gauss–Legendre quadrature).
    pub fn length(&self) -> f64 {
        match *self {
            Path::Line { a, b } => ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt(),
            Path::Arc { radius, sweep, .. } => radius * sweep.abs(),
            Path::Circle { radius, .. } => TAU * radius,
            Path::Cubic { a, c1, c2, b } => quadrature(|t| {
                let u = 1.0 - t;
                let dx = 3.0 * u * u * (c1.x - a.x)
                    + 6.0 * u * t * (c2.x - c1.x)
                    + 3.0 * t * t * (b.x - c2.x);
                let dy = 3.0 * u * u * (c1.y - a.y)
                    + 6.0 * u * t * (c2.y - c1.y)
                    + 3.0 * t * t * (b.y - c2.y);
                (dx * dx + dy * dy).sqrt()
            }),
            Path::EllipticalArc {
                rx,
                ry,
                start,
                sweep,
                ..
            } => quadrature(|t| {
                let a = start + sweep * t;
                ((rx * a.sin()).powi(2) + (ry * a.cos()).powi(2)).sqrt() * sweep.abs()
            }),
        }
    }

    /// Whether the curve closes on itself (no ends for a dash to land on).
    pub fn is_closed(&self) -> bool {
        match self {
            Path::Circle { .. } => true,
            Path::EllipticalArc { sweep, .. } => sweep.abs() >= TAU - 1e-12,
            _ => false,
        }
    }

    /// The point at parameter `t` in `0..=1` along the curve.
    pub fn at(&self, t: f64) -> P2<Paper> {
        match *self {
            Path::Line { a, b } => P2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t),
            Path::Arc {
                centre,
                radius,
                start,
                sweep,
            } => Self::at_angle(centre, radius, start + sweep * t),
            Path::Circle { centre, radius } => Self::at_angle(centre, radius, TAU * t),
            Path::Cubic { a, c1, c2, b } => {
                let u = 1.0 - t;
                let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                P2::new(
                    w[0] * a.x + w[1] * c1.x + w[2] * c2.x + w[3] * b.x,
                    w[0] * a.y + w[1] * c1.y + w[2] * c2.y + w[3] * b.y,
                )
            }
            Path::EllipticalArc {
                centre,
                rx,
                ry,
                rotation,
                start,
                sweep,
            } => {
                let a = start + sweep * t;
                let (x, y) = (rx * a.cos(), ry * a.sin());
                let (s, c) = rotation.sin_cos();
                P2::new(centre.x + c * x - s * y, centre.y + s * x + c * y)
            }
        }
    }

    /// The point at an angle on an arc or circle.
    fn at_angle(centre: P2<Paper>, radius: f64, a: f64) -> P2<Paper> {
        P2::new(centre.x + radius * a.cos(), centre.y + radius * a.sin())
    }

    /// The point of the curve nearest `p`: where a leader aimed from `p`
    /// lands. Exact for lines, arcs and circles; a cubic's and an
    /// ellipse's by the nearest of 65 samples.
    pub fn nearest(&self, p: P2<Paper>) -> P2<Paper> {
        let d2 = |a: P2<Paper>, b: P2<Paper>| (a.x - b.x).powi(2) + (a.y - b.y).powi(2);
        match *self {
            Path::Line { a, b } => {
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let len2 = dx * dx + dy * dy;
                let t = if len2 > 0.0 {
                    (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                self.at(t)
            }
            Path::Circle { centre, radius } => {
                let a = (p.y - centre.y).atan2(p.x - centre.x);
                Self::at_angle(centre, radius, a)
            }
            Path::Arc {
                centre,
                radius,
                start,
                sweep,
            } => {
                // The angle of `p`, measured from the start in the sweep's
                // direction; within the sweep it is the foot, else the
                // nearer end.
                let a = (p.y - centre.y).atan2(p.x - centre.x);
                let turned = (a - start).rem_euclid(TAU);
                let along = if sweep >= 0.0 { turned } else { TAU - turned };
                if along <= sweep.abs() {
                    return Self::at_angle(centre, radius, a);
                }
                let (s, e) = self.arc_ends().expect("an arc");
                if d2(p, s) <= d2(p, e) {
                    s
                } else {
                    e
                }
            }
            Path::Cubic { .. } | Path::EllipticalArc { .. } => (0..=64)
                .map(|i| self.at(i as f64 / 64.0))
                .min_by(|a, b| d2(p, *a).total_cmp(&d2(p, *b)))
                .expect("65 points"),
        }
    }

    /// The arc's ends.
    pub fn arc_ends(&self) -> Option<(P2<Paper>, P2<Paper>)> {
        match *self {
            Path::Arc {
                centre,
                radius,
                start,
                sweep,
            } => Some((
                Self::at_angle(centre, radius, start),
                Self::at_angle(centre, radius, start + sweep),
            )),
            _ => None,
        }
    }
}

/// ∫₀¹ f(t) dt by 16-point Gauss–Legendre.
fn quadrature(f: impl Fn(f64) -> f64) -> f64 {
    const X: [f64; 8] = [
        0.095_012_509_837_637_44,
        0.281_603_550_779_258_9,
        0.458_016_777_657_227_4,
        0.617_876_244_402_643_7,
        0.755_404_408_355_003,
        0.865_631_202_387_831_7,
        0.944_575_023_073_232_6,
        0.989_400_934_991_649_9,
    ];
    const W: [f64; 8] = [
        0.189_450_610_455_068_5,
        0.182_603_415_044_923_6,
        0.169_156_519_395_002_5,
        0.149_595_988_816_576_7,
        0.124_628_971_255_533_9,
        0.095_158_511_682_492_78,
        0.062_253_523_938_647_89,
        0.027_152_459_411_754_095,
    ];
    let mut sum = 0.0;
    for (x, w) in X.iter().zip(&W) {
        sum += w * (f(0.5 + 0.5 * x) + f(0.5 - 0.5 * x));
    }
    sum * 0.5
}

/// A filled shape.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// A polygon through these points, closed: an arrowhead.
    Polygon {
        /// The corners, in order.
        points: Vec<P2<Paper>>,
    },
    /// A disc: a dot.
    Disc {
        /// The centre.
        centre: P2<Paper>,
        /// The radius, mm.
        radius: f64,
    },
}

impl Bounded<Paper> for Fill {
    fn bounds(&self) -> Option<Rect<Paper>> {
        match self {
            Fill::Polygon { points } => Rect::of_points(points.iter().copied()),
            Fill::Disc { centre, radius } => circle_bounds(*centre, *radius),
        }
    }
}

/// Something drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// A stroked curve.
    Stroke {
        /// The curve.
        path: Path,
        /// Line weight, mm.
        weight: f64,
        /// The layer's dash pattern, unfitted.
        dash: Dash,
        /// Distances along the curve, mm, where the pattern's short
        /// group is centred when it is fitted: a centre line crosses a
        /// circle's centre on its short dash. Empty for most strokes.
        anchors: Vec<f64>,
    },
    /// A point marker.
    Marker {
        /// Where.
        at: P2<Paper>,
        /// Its shape and size, from the layer.
        marker: Marker,
        /// Line weight, mm.
        weight: f64,
    },
    /// A filled shape in the layer's colour, outlined at a weight if one
    /// is given (an arrowhead is stroked as well as filled, so its edges
    /// are as heavy as its line).
    Fill {
        /// The shape.
        fill: Fill,
        /// The outline's weight, mm, if outlined.
        outline: Option<f64>,
    },
}

impl Bounded<Paper> for Primitive {
    /// A stroke's centre line; a marker's square; a fill's shape and
    /// half its outline.
    fn bounds(&self) -> Option<Rect<Paper>> {
        match self {
            Primitive::Stroke { path, .. } => path.bounds(),
            Primitive::Marker { at, marker, .. } => {
                Rect::around(*at).map(|r| r.grow(marker.size / 2.0))
            }
            Primitive::Fill { fill, outline } => {
                fill.bounds().map(|r| r.grow(outline.unwrap_or(0.0) / 2.0))
            }
        }
    }
}

impl Primitive {
    /// The box of the ink: a stroke's centre line grown by half its
    /// weight; otherwise as [`Bounded::bounds`].
    pub fn ink_bounds(&self) -> Option<Rect<Paper>> {
        match self {
            Primitive::Stroke { path, weight, .. } => path.bounds().map(|r| r.grow(weight / 2.0)),
            _ => self.bounds(),
        }
    }
}

/// A primitive with where it came from and how its layer styles it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// What is drawn.
    pub primitive: Primitive,
    /// Its layer.
    pub layer: LayerId,
    /// Its layer's tint, if any.
    pub tint: Option<PaletteId>,
    /// The document item it draws.
    pub source: Source,
}

/// What a plane of the scene is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaneKind {
    /// The drawing itself.
    Drawing,
}

/// One end of a stroke's path: where its parameter starts or ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathEnd {
    /// The path's start (a line's `a`, an arc's `start`).
    Start,
    /// The path's end.
    End,
}

/// A terminator and the stroke it ends, as the drawing made them: the
/// arrowhead or dot at a leader's tip or a dimension line's end, and
/// the run of line that arrives at it (the leader's last run, the
/// dimension line or the outside arrow's tail). A line that only
/// touches a head (the dimension line between two outside arrows,
/// whose tips are its ends) is not listed: it is not the head's line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Termination {
    /// The terminator's item: its index among the plane's items (in a
    /// [`crate::resolve::Styled`], among the styled items).
    pub head: usize,
    /// The stroke's item, indexed the same way.
    pub line: usize,
    /// Which end of the stroke's path arrives at the head.
    pub end: PathEnd,
}

impl Termination {
    /// The same with both items moved on by `by` (a list of items
    /// appended after `by` others).
    pub fn offset(self, by: usize) -> Termination {
        Termination {
            head: self.head + by,
            line: self.line + by,
            ..self
        }
    }
}

/// Primitives drawn together, and which stroke among them each
/// terminator among them ends (by index in `primitives`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawn {
    /// The primitives, in draw order.
    pub primitives: Vec<Primitive>,
    /// The terminations among them.
    pub terminations: Vec<Termination>,
}

impl Drawn {
    /// Append `other`'s primitives and terminations after these.
    pub fn append(&mut self, other: Drawn) {
        let by = self.primitives.len();
        self.primitives.extend(other.primitives);
        self.terminations
            .extend(other.terminations.into_iter().map(|t| t.offset(by)));
    }
}

/// One plane of the scene: items in draw order.
#[derive(Clone, Debug, PartialEq)]
pub struct Plane {
    /// What it is for.
    pub kind: PlaneKind,
    /// The items, first drawn first.
    pub items: Vec<Item>,
    /// Every line of lettering as a run of characters, for a renderer
    /// that writes text its own way (a simulated hand) in place of the
    /// lettering strokes among the items.
    pub runs: Vec<Run>,
    /// Which stroke each terminator among the items ends.
    pub terminations: Vec<Termination>,
    /// The witness lines, by [`WitnessId`].
    pub witnesses: Vec<Witness>,
}

/// One line of lettering as set: its characters and where they go.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// The document item it letters.
    pub source: Source,
    /// The characters, a stack written `upper/lower`
    /// ([`Line::to_plain`]).
    pub text: String,
    /// The line as the document writes it: its runs and stacks, so a
    /// writer can stack a fraction over its bar.
    pub line: Line,
    /// The left end of the baseline on the paper, mm.
    pub origin: P2<Paper>,
    /// Cap height, mm.
    pub cap: f64,
    /// The line's width as set, mm.
    pub width: f64,
    /// Where along the line the text is anchored, 0 at its left end to
    /// 1 at its right: what must stay put if the line is written at
    /// another width.
    pub anchor: f64,
    /// The line's angle on the paper, radians, counter-clockwise on the
    /// page.
    pub angle: f64,
    /// Pen width, mm.
    pub weight: f64,
}

/// The scene: planes in draw order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Scene {
    /// The planes, first drawn first.
    pub planes: Vec<Plane>,
}

impl Scene {
    /// A scene of one plane.
    pub fn of(plane: Plane) -> Self {
        Self {
            planes: vec![plane],
        }
    }

    /// Every item, in draw order.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.planes.iter().flat_map(|p| p.items.iter())
    }
}

impl Bounded<Paper> for Item {
    fn bounds(&self) -> Option<Rect<Paper>> {
        self.primitive.bounds()
    }
}

impl Bounded<Paper> for Run {
    /// The box of the line as set: its width along the baseline from
    /// its origin and its cap height above, turned by its angle.
    fn bounds(&self) -> Option<Rect<Paper>> {
        // Along the baseline and up the page, y down.
        let (s, c) = self.angle.sin_cos();
        let along = (c, -s);
        let up = (-s, -c);
        let at = |u: f64, v: f64| {
            P2::new(
                self.origin.x + u * along.0 + v * up.0,
                self.origin.y + u * along.1 + v * up.1,
            )
        };
        Rect::of_points([
            at(0.0, 0.0),
            at(self.width, 0.0),
            at(0.0, self.cap),
            at(self.width, self.cap),
        ])
    }
}

impl Bounded<Paper> for Plane {
    /// Its items and its lines of lettering.
    fn bounds(&self) -> Option<Rect<Paper>> {
        Rect::union_all(self.items.bounds().into_iter().chain(self.runs.bounds()))
    }
}

impl Bounded<Paper> for Scene {
    /// Every plane's; none if empty.
    fn bounds(&self) -> Option<Rect<Paper>> {
        self.planes.bounds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// The box of many points along a path.
    fn sampled(p: &Path) -> Rect<Paper> {
        Rect::of_points((0..=20_000).map(|i| p.at(i as f64 / 20_000.0))).unwrap()
    }

    #[test]
    fn path_bounds_are_exact() {
        let paths = [
            Path::Line {
                a: P2::new(3.0, -1.0),
                b: P2::new(-2.0, 4.0),
            },
            Path::Arc {
                centre: P2::new(1.0, 1.0),
                radius: 2.0,
                start: 2.5,
                sweep: -4.0,
            },
            Path::Circle {
                centre: P2::new(0.0, 0.0),
                radius: 3.0,
            },
            Path::Cubic {
                a: P2::new(0.0, 0.0),
                c1: P2::new(4.0, -3.0),
                c2: P2::new(-3.0, 5.0),
                b: P2::new(1.0, 1.0),
            },
            Path::EllipticalArc {
                centre: P2::new(2.0, -1.0),
                rx: 3.0,
                ry: 1.0,
                rotation: 0.6,
                start: -1.0,
                sweep: 4.5,
            },
        ];
        for p in &paths {
            let exact = p.bounds().unwrap();
            let s = sampled(p);
            assert!(exact.grow(1e-9).contains_rect(&s), "{p:?}");
            assert!(
                s.grow(1e-5).contains_rect(&exact),
                "{p:?}: {exact:?} vs {s:?}"
            );
        }
        // A path with a coordinate that is not finite has no box.
        let bad = Path::Circle {
            centre: P2::new(f64::NAN, 0.0),
            radius: 1.0,
        };
        assert_eq!(bad.bounds(), None);
    }

    #[test]
    fn primitives_runs_planes_and_scenes() {
        let stroke = Primitive::Stroke {
            path: Path::Line {
                a: P2::new(0.0, 0.0),
                b: P2::new(10.0, 0.0),
            },
            weight: 0.5,
            dash: Dash::SOLID,
            anchors: Vec::new(),
        };
        let b = stroke.bounds().unwrap();
        assert_eq!((b.width(), b.height()), (10.0, 0.0));
        // The ink takes half the weight all round.
        let ink = stroke.ink_bounds().unwrap();
        assert!((ink.height() - 0.5).abs() < 1e-12 && (ink.width() - 10.5).abs() < 1e-12);
        let disc = Primitive::Fill {
            fill: Fill::Disc {
                centre: P2::new(5.0, 5.0),
                radius: 1.0,
            },
            outline: Some(0.2),
        };
        let d = disc.bounds().unwrap();
        assert!((d.width() - 2.2).abs() < 1e-12);
        // A run: 30 mm wide, 5 mm cap, its baseline from (10, 20), y
        // down: the box rises from the baseline.
        let mut run = Run {
            source: Source::Text("t".into()),
            text: "PLATE".into(),
            line: Line::plain("PLATE"),
            origin: P2::new(10.0, 20.0),
            cap: 5.0,
            width: 30.0,
            anchor: 0.0,
            angle: 0.0,
            weight: 0.35,
        };
        let r = run.bounds().unwrap();
        assert_eq!(
            (r.min(), r.max()),
            (P2::new(10.0, 15.0), P2::new(40.0, 20.0))
        );
        // Turned a quarter counter-clockwise on the page, it reads up the
        // page: the baseline runs up (y falling), the cap leftwards.
        run.angle = std::f64::consts::FRAC_PI_2;
        let r = run.bounds().unwrap();
        let near = |a: P2<Paper>, x: f64, y: f64| (a.x - x).abs() < 1e-9 && (a.y - y).abs() < 1e-9;
        assert!(
            near(r.min(), 5.0, -10.0) && near(r.max(), 10.0, 20.0),
            "{r:?}"
        );
        // A plane is its items and runs; a scene its planes; nothing has
        // no box.
        let plane = Plane {
            kind: PlaneKind::Drawing,
            items: Vec::new(),
            runs: vec![run.clone()],
            terminations: Vec::new(),
            witnesses: Vec::new(),
        };
        assert_eq!(plane.bounds(), run.bounds());
        assert_eq!(Scene::default().bounds(), None);
        assert_eq!(Scene::of(plane.clone()).bounds(), plane.bounds());
    }

    #[test]
    fn arc_bounds_include_the_quadrants_passed() {
        // The top half of the unit circle at the origin, y down: from +x
        // round through (0, 1) to −x. Its box is x ±1, y 0..1.
        let arc = Path::Arc {
            centre: P2::new(0.0, 0.0),
            radius: 1.0,
            start: 0.0,
            sweep: PI,
        };
        let b = arc.bounds().unwrap();
        assert!((b.min().x + 1.0).abs() < 1e-12 && (b.max().x - 1.0).abs() < 1e-12);
        assert!(b.min().y.abs() < 1e-12 && (b.max().y - 1.0).abs() < 1e-12);
        // A quarter arc that passes no quadrant line: the chord's box.
        let arc = Path::Arc {
            centre: P2::new(0.0, 0.0),
            radius: 1.0,
            start: 0.1,
            sweep: 1.0,
        };
        let b = arc.bounds().unwrap();
        assert!((b.max().x - 0.1f64.cos()).abs() < 1e-12);
        assert!((b.min().x - 1.1f64.cos()).abs() < 1e-12);
        assert!((arc.length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn nearest_points() {
        let near =
            |a: P2<Paper>, x: f64, y: f64| (a.x - x).abs() < 1e-12 && (a.y - y).abs() < 1e-12;
        let line = Path::Line {
            a: P2::new(0.0, 0.0),
            b: P2::new(10.0, 0.0),
        };
        assert!(near(line.nearest(P2::new(4.0, 3.0)), 4.0, 0.0));
        assert!(near(line.nearest(P2::new(14.0, 3.0)), 10.0, 0.0));
        let circle = Path::Circle {
            centre: P2::new(0.0, 0.0),
            radius: 2.0,
        };
        assert!(near(circle.nearest(P2::new(0.0, 5.0)), 0.0, 2.0));
        // The top half (y down): a point above lands on the foot; a point
        // below, outside the sweep, lands on the nearer end.
        let arc = Path::Arc {
            centre: P2::new(0.0, 0.0),
            radius: 1.0,
            start: 0.0,
            sweep: PI,
        };
        assert!(near(arc.nearest(P2::new(0.0, 5.0)), 0.0, 1.0));
        assert!(near(arc.nearest(P2::new(0.5, -5.0)), 1.0, 0.0));
        assert!(near(arc.nearest(P2::new(-0.5, -5.0)), -1.0, 0.0));
        let back = Path::Arc {
            centre: P2::new(0.0, 0.0),
            radius: 1.0,
            start: PI,
            sweep: -PI,
        };
        assert!(near(back.nearest(P2::new(0.0, 5.0)), 0.0, 1.0));
    }
}
