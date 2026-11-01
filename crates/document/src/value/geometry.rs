//! Geometric entities: the lowest layer of the document.
//!
//! Each entity owns exactly its own degrees of freedom and nothing is shared:
//! a line owns both its ends, an arc owns its centre, radius and angles. Any
//! values make a valid entity, so the store can never hold a half-meaningful
//! shape. Relations between entities (a line ending where an arc starts, two
//! arcs sharing a centre) are not expressed here at all; they are constraints,
//! a later layer, and the solver moves these values to satisfy them. The values
//! here are therefore the current state of the drawing, exactly as drawn.
//!
//! Other layers refer to positions through a `PointRef`: a point entity, or a
//! named position on a curve (a line's start, mid or end; an arc's start, mid,
//! end or centre; a circle's centre; a spline's ends, the control points next
//! to them, or any of its points by index).
//!
//! A spline is a chain of cubic Bézier segments, `3n + 1` points for `n`
//! segments, knots and control points interleaved. Its tangent at an end is
//! the direction from the end knot to its neighbouring control point, which
//! is what the tangent relations constrain; smoothness at an interior knot
//! is the collinearity of the control points either side of it. The chain
//! keeps its number of points for life, so references by index stay
//! meaningful.
//!
//! Units are millimetres, always. What varies is the frame the coordinates
//! are expressed in: a view's model plane, or the paper. Every type here
//! carries the frame as a marker (`P2<Model>`, `Geometry<Paper>`), so the only
//! way from one frame to another is through a transform, and mixing them is a
//! compile error. Entities themselves know nothing about frames; the container
//! (a view, the sheet) says which one applies.
//!
//! Within a frame, axes are x right, y down, and angles are in radians from
//! +x towards +y, so a positive sweep turns clockwise in that frame. Whether
//! it looks clockwise on the paper depends on the view's transform, which may
//! mirror; that is the transform's concern, not the entity's.
//!
//! # Example
//!
//! A clip's top, bend and leg, a pin, and the positions other layers would
//! refer to. [`geometry!`](macro@crate::geometry) builds through the checked
//! operations; the store is then changed only through `set` and `remove`.
//!
//! ```
//! use lmsheets_document::value::geometry::{
//!     ArcPos, Entity, Error, LinePos, Model, PointRef, P2,
//! };
//! use lmsheets_document::geometry;
//! use std::f64::consts::FRAC_PI_2;
//!
//! let mut g = geometry!(Model {
//!     top:  line (0.0, 0.0) -> (10.0, 0.0);
//!     bend: arc (10.0, 5.0), r = 5.0, from = -FRAC_PI_2, sweep = FRAC_PI_2;
//!     leg:  line (15.0, 5.0) -> (15.0, 20.0);
//!     pin:  point (3.0, 4.0);
//! });
//!
//! // Positions are functions of an entity's own values.
//! let bend_end = PointRef::arc("bend", ArcPos::End);
//! let p = g.position(&bend_end)?;
//! assert!((p.x - 15.0).abs() < 1e-12 && (p.y - 5.0).abs() < 1e-12);
//! let top_mid = PointRef::line("top", LinePos::Mid);
//! assert_eq!(g.position(&top_mid)?, P2::new(5.0, 0.0));
//!
//! // A reference of the wrong kind is refused, not misread.
//! let wrong = PointRef::circle("top");
//! assert!(matches!(g.position(&wrong), Err(Error::WrongKind { .. })));
//!
//! // Changing values keeps the kind; the file form carries the frame.
//! g.set("pin", Entity::Point { at: P2::new(3.5, 4.0) })?;
//! assert_eq!(g.remove("leg")?, Entity::Line {
//!     start: P2::new(15.0, 5.0),
//!     end: P2::new(15.0, 20.0),
//! });
//! let json = serde_json::to_string(&g).unwrap();
//! assert!(json.contains("\"frame\":\"model\""));
//! # Ok::<(), Error>(())
//! ```

use crate::solver::tolerance::Tolerance;
use approx::AbsDiffEq;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};
use std::marker::PhantomData;
use ustr::Ustr;

/// The name of an entity: a non-empty string without surrounding
/// space, unique within a geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityId(Ustr);

impl EntityId {
    /// An id from its name. Names are interned once, so an id is a `Copy`
    /// handle and the type is cheap to pass around; ordering is by name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&EntityId> for EntityId {
    fn from(id: &EntityId) -> Self {
        *id
    }
}

impl From<&str> for EntityId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An axis-aligned box in a frame, mm: `min` at or below `max` in both
/// axes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawRect<F>", into = "RawRect<F>", bound = "")]
pub struct Rect<F: Frame> {
    min: P2<F>,
    max: P2<F>,
}

#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
struct RawRect<F: Frame> {
    min: P2<F>,
    max: P2<F>,
}

impl<F: Frame> From<Rect<F>> for RawRect<F> {
    fn from(r: Rect<F>) -> Self {
        RawRect {
            min: r.min,
            max: r.max,
        }
    }
}

impl<F: Frame> TryFrom<RawRect<F>> for Rect<F> {
    type Error = Error;

    fn try_from(r: RawRect<F>) -> Result<Self, Error> {
        Rect::new(r.min, r.max)
    }
}

impl<F: Frame> Rect<F> {
    /// A box from two corners; refused unless finite and `min <= max`.
    pub fn new(min: P2<F>, max: P2<F>) -> Result<Self, Error> {
        if !(min.x.is_finite() && min.y.is_finite() && max.x.is_finite() && max.y.is_finite()) {
            return Err(Error::NotFinite);
        }
        if min.x > max.x || min.y > max.y {
            return Err(Error::BadRect);
        }
        Ok(Self { min, max })
    }

    /// The lower corner.
    pub fn min(&self) -> P2<F> {
        self.min
    }

    /// The upper corner.
    pub fn max(&self) -> P2<F> {
        self.max
    }

    /// Whether the point is inside or on the edge.
    pub fn contains(&self, p: P2<F>) -> bool {
        self.min.x <= p.x && p.x <= self.max.x && self.min.y <= p.y && p.y <= self.max.y
    }

    /// The box of one point, if it is finite.
    pub fn around(p: P2<F>) -> Option<Self> {
        p.is_finite().then_some(Self { min: p, max: p })
    }

    /// The box of the points, if there are any and all are finite.
    pub fn of_points(points: impl IntoIterator<Item = P2<F>>) -> Option<Self> {
        let mut it = points.into_iter();
        let first = Self::around(it.next()?)?;
        it.try_fold(first, |r, p| Self::around(p).map(|q| r.union(q)))
    }

    /// The box of the boxes, if there are any.
    pub fn union_all(boxes: impl IntoIterator<Item = Self>) -> Option<Self> {
        boxes.into_iter().reduce(Self::union)
    }

    /// The box around both.
    pub fn union(self, o: Self) -> Self {
        Self {
            min: P2::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            max: P2::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        }
    }

    /// Grown by `m` on every side; a negative `m` shrinks it, to its
    /// centre line at most, never past it. A non-finite `m` leaves it.
    pub fn grow(self, m: f64) -> Self {
        if !m.is_finite() {
            return self;
        }
        let c = self.centre();
        let mx = (self.width() / 2.0 + m).max(0.0);
        let my = (self.height() / 2.0 + m).max(0.0);
        Self {
            min: P2::new(c.x - mx, c.y - my),
            max: P2::new(c.x + mx, c.y + my),
        }
    }

    /// Moved by `(dx, dy)`; a non-finite move leaves it.
    pub fn translated(self, dx: f64, dy: f64) -> Self {
        if !(dx.is_finite() && dy.is_finite()) {
            return self;
        }
        Self {
            min: P2::new(self.min.x + dx, self.min.y + dy),
            max: P2::new(self.max.x + dx, self.max.y + dy),
        }
    }

    /// The extent along x, mm.
    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    /// The extent along y, mm.
    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    /// The middle.
    pub fn centre(&self) -> P2<F> {
        P2::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }

    /// Whether the two overlap or touch.
    pub fn intersects(&self, o: &Self) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
    }

    /// Whether `o` lies wholly inside, edges included.
    pub fn contains_rect(&self, o: &Self) -> bool {
        self.contains(o.min) && self.contains(o.max)
    }
}

/// A coordinate frame, as a zero-sized marker type.
pub trait Frame: Copy + std::fmt::Debug + PartialEq {
    /// The frame's name as written in files.
    const NAME: &'static str;
}

/// The model plane of one view: the part's own coordinates, mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Model;
impl Frame for Model {
    const NAME: &'static str = "model";
}

/// The physical sheet: position on the paper, mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paper;
impl Frame for Paper {
    const NAME: &'static str = "paper";
}

/// A 2D point in millimetres, in frame `F`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct P2<F: Frame> {
    /// Millimetres.
    pub x: f64,
    /// Millimetres.
    pub y: f64,
    #[serde(skip)]
    frame: PhantomData<F>,
}

impl<F: Frame> P2<F> {
    /// A point from its coordinates.
    pub const fn new(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            frame: PhantomData,
        }
    }

    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl<F: Frame> AbsDiffEq for P2<F> {
    type Epsilon = Tolerance;

    fn default_epsilon() -> Tolerance {
        Tolerance::NOISE
    }

    fn abs_diff_eq(&self, o: &Self, t: Tolerance) -> bool {
        t.length_near(self.x, o.x) && t.length_near(self.y, o.y)
    }
}

/// A geometric entity. Every variant owns all of its own values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", bound = "")]
pub enum Entity<F: Frame> {
    /// A bare point: a datum, a centre mark, a construction aid.
    Point {
        /// Position.
        at: P2<F>,
    },
    /// A straight segment from `start` to `end`.
    Line {
        /// One end.
        start: P2<F>,
        /// The other end.
        end: P2<F>,
    },
    /// From `start_angle` through `sweep` (signed, 0 < |sweep| < 2π; a
    /// closed curve is a `Circle`).
    Arc {
        /// Centre.
        centre: P2<F>,
        /// Radius, mm, > 0.
        radius: f64,
        /// Angle of the start point, radians, kept in [−π, π).
        start_angle: f64,
        /// Signed angle turned from the start, radians.
        sweep: f64,
    },
    /// A full circle.
    Circle {
        /// Centre.
        centre: P2<F>,
        /// Radius, mm, > 0.
        radius: f64,
    },
    /// A smooth curve: a chain of cubic Bézier segments sharing their
    /// end points, `3n + 1` points for `n ≥ 1` segments: a knot, two
    /// control points, a knot, two more, a knot, and so on. The curve's
    /// tangent at a knot is the direction to its neighbouring control
    /// point, which is what the tangent relations constrain. No segment
    /// may have all four of its points equal.
    Spline {
        /// The points, knots and control points interleaved.
        points: Vec<P2<F>>,
    },
}

/// A named position on a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinePos {
    /// The start.
    Start,
    /// Half way along.
    Mid,
    /// The end.
    End,
}

/// A named position on an arc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcPos {
    /// The start point.
    Start,
    /// Half way along the sweep.
    Mid,
    /// The end point.
    End,
    /// The centre.
    Centre,
    /// The extreme of the circle the arc is part of, in an axis
    /// direction of the sketch: the centre plus the radius along it.
    /// The direction must lie within the arc's sweep (checked when a
    /// constraint names it), so it is a point of the arc itself.
    Extreme(Extreme),
}

/// An axis direction of a sketch's own frame (its +x, -x, +y or -y,
/// angle 0, π, π/2 or -π/2): which extreme of a circle or arc a point
/// names. Exactly four, so a reference is exact, hashable and equal to
/// itself after a file round trip; a dimension over a rounded end
/// measures between such extremes. A slanted direction is not
/// representable (an aligned measure over a rounded end would need one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Extreme {
    /// Towards +x.
    PlusX,
    /// Towards -x.
    MinusX,
    /// Towards +y.
    PlusY,
    /// Towards -y.
    MinusY,
}

impl Extreme {
    /// The unit vector, exact (no rounding from cos and sin).
    pub fn unit(self) -> [f64; 2] {
        match self {
            Extreme::PlusX => [1.0, 0.0],
            Extreme::MinusX => [-1.0, 0.0],
            Extreme::PlusY => [0.0, 1.0],
            Extreme::MinusY => [0.0, -1.0],
        }
    }

    /// The direction's angle from +x, counter-clockwise, in (-π, π].
    pub fn angle(self) -> f64 {
        match self {
            Extreme::PlusX => 0.0,
            Extreme::MinusX => std::f64::consts::PI,
            Extreme::PlusY => std::f64::consts::FRAC_PI_2,
            Extreme::MinusY => -std::f64::consts::FRAC_PI_2,
        }
    }

    /// Whether the direction lies within an arc's sweep from
    /// `start_angle` (the sweep's sign is its direction).
    fn within(self, start_angle: f64, sweep: f64) -> bool {
        let t = std::f64::consts::TAU;
        let d = if sweep >= 0.0 {
            (self.angle() - start_angle).rem_euclid(t)
        } else {
            (start_angle - self.angle()).rem_euclid(t)
        };
        // A direction on an end counts, whatever rounding the end's angle has.
        d <= sweep.abs() + 1e-9 || d >= t - 1e-9
    }
}

/// A place on a spline, named so that a reference needs no knowledge of
/// how many points the spline has: its ends, the control points next to
/// them (the handles, whose directions are the end tangents), or any
/// point by index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplinePos {
    /// The first knot.
    Start,
    /// The control point after the start: the start tangent runs from
    /// the start to it.
    StartHandle,
    /// The control point before the end: the end tangent runs from it
    /// to the end.
    EndHandle,
    /// The last knot.
    End,
    /// A point by its index in the chain (knots at multiples of three).
    Point(usize),
}

impl SplinePos {
    /// The index into a chain of `len` points, if it has one.
    pub fn index(self, len: usize) -> Option<usize> {
        let i = match self {
            SplinePos::Start => 0,
            SplinePos::StartHandle => 1,
            SplinePos::EndHandle => len.checked_sub(2)?,
            SplinePos::End => len.checked_sub(1)?,
            SplinePos::Point(i) => i,
        };
        (i < len).then_some(i)
    }
}

/// A position defined by an entity: a point itself, a named place on a line
/// or arc, or a circle's centre. Each is a function of that entity's own
/// values, so a reference is well-formed by construction; the only check left
/// is that the named entity exists and is of the stated kind. Built by the
/// kind-named constructors; the representation is private.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PointRef(pub(crate) PointAt);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PointAt {
    Point {
        id: EntityId,
    },
    Line {
        id: EntityId,
        at: LinePos,
    },
    Arc {
        id: EntityId,
        at: ArcPos,
    },
    /// A circle's centre, or its extreme if `at` is given.
    Circle {
        id: EntityId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<Extreme>,
    },
    Spline {
        id: EntityId,
        at: SplinePos,
    },
}

impl PointRef {
    /// A point entity.
    pub fn point(id: impl Into<EntityId>) -> Self {
        Self(PointAt::Point { id: id.into() })
    }

    /// A position on a line.
    pub fn line(id: impl Into<EntityId>, at: LinePos) -> Self {
        Self(PointAt::Line { id: id.into(), at })
    }

    /// A position on an arc.
    pub fn arc(id: impl Into<EntityId>, at: ArcPos) -> Self {
        Self(PointAt::Arc { id: id.into(), at })
    }

    /// A circle's centre.
    pub fn circle(id: impl Into<EntityId>) -> Self {
        Self(PointAt::Circle {
            id: id.into(),
            at: None,
        })
    }

    /// A circle's extreme in an axis direction.
    pub fn circle_extreme(id: impl Into<EntityId>, at: Extreme) -> Self {
        Self(PointAt::Circle {
            id: id.into(),
            at: Some(at),
        })
    }

    /// An arc's extreme in an axis direction (it must lie within the
    /// sweep).
    pub fn arc_extreme(id: impl Into<EntityId>, at: Extreme) -> Self {
        Self::arc(id, ArcPos::Extreme(at))
    }

    /// A position on a spline.
    pub fn spline(id: impl Into<EntityId>, at: SplinePos) -> Self {
        Self(PointAt::Spline { id: id.into(), at })
    }

    /// The entity referred to.
    pub fn id(&self) -> EntityId {
        match self.0 {
            PointAt::Point { id }
            | PointAt::Line { id, .. }
            | PointAt::Arc { id, .. }
            | PointAt::Circle { id, .. }
            | PointAt::Spline { id, .. } => id,
        }
    }

    /// The circle or arc whose centre this is, if it is one.
    pub fn centre_of(&self) -> Option<EntityId> {
        match self.0 {
            PointAt::Arc {
                id,
                at: ArcPos::Centre,
            }
            | PointAt::Circle { id, at: None } => Some(id),
            _ => None,
        }
    }

    /// The same position on another entity of the same kind.
    pub(crate) fn with_id(self, id: EntityId) -> Self {
        Self(match self.0 {
            PointAt::Point { .. } => PointAt::Point { id },
            PointAt::Line { at, .. } => PointAt::Line { id, at },
            PointAt::Arc { at, .. } => PointAt::Arc { id, at },
            PointAt::Circle { at, .. } => PointAt::Circle { id, at },
            PointAt::Spline { at, .. } => PointAt::Spline { id, at },
        })
    }

    /// The kind of entity this reference expects.
    pub fn kind(&self) -> Kind {
        match self.0 {
            PointAt::Point { .. } => Kind::Point,
            PointAt::Line { .. } => Kind::Line,
            PointAt::Arc { .. } => Kind::Arc,
            PointAt::Circle { .. } => Kind::Circle,
            PointAt::Spline { .. } => Kind::Spline,
        }
    }
}

/// The kind of an entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A point.
    Point,
    /// A line.
    Line,
    /// An arc.
    Arc,
    /// A circle.
    Circle,
    /// A spline: a chain of cubic Bézier segments.
    Spline,
}

/// What can go wrong changing or reading a geometry.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// An id must be a non-empty string without leading or trailing space.
    BadId(EntityId),
    /// A box whose `min` is above its `max`.
    BadRect,
    /// An entity with this id already exists.
    Duplicate(EntityId),
    /// No entity has this id.
    NoSuch(EntityId),
    /// `set` may change an entity's values, not its kind.
    KindChanged(EntityId),
    /// A reference names an entity of one kind but it is another.
    WrongKind {
        /// The entity.
        id: EntityId,
        /// The kind the reference stated.
        expected: Kind,
        /// The kind the entity is.
        found: Kind,
    },
    /// A coordinate, radius or angle that is NaN or infinite.
    NotFinite,
    /// A line whose ends coincide has no direction.
    ZeroLength,
    /// A radius that is not positive.
    BadRadius(f64),
    /// A sweep that is zero, or a full turn or more.
    BadSweep(f64),
    /// A spline must have `3n + 1` points for `n ≥ 1` segments.
    BadSplineLength(usize),
    /// A spline segment whose four points all coincide has no curve.
    DegenerateSegment(usize),
    /// `set` may move a spline's points, not change how many it has:
    /// references to its points by index must stay meaningful.
    SplineResized(EntityId),
    /// A reference names a point a spline does not have.
    NoSuchPoint {
        /// The spline.
        id: EntityId,
        /// The position asked for.
        at: SplinePos,
    },
    /// A reference names an extreme of an arc in a direction its sweep
    /// does not reach.
    ExtremeOffArc(EntityId, Extreme),
    /// A file written in one frame was read as another.
    WrongFrame {
        /// The frame being read into.
        expected: String,
        /// The frame written in the file.
        found: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

impl EntityId {
    fn validate(&self) -> Result<(), Error> {
        let name = self.as_str();
        if name.is_empty() || name.trim() != name {
            return Err(Error::BadId(*self));
        }
        Ok(())
    }
}

impl<F: Frame> Entity<F> {
    /// Check the values and put them in canonical form, so that equal shapes
    /// have equal representations: an arc's start angle is kept in [−π, π).
    pub(crate) fn canonical(mut self) -> Result<Self, Error> {
        let ok = match &self {
            Entity::Point { at } => at.is_finite(),
            Entity::Line { start, end } => start.is_finite() && end.is_finite(),
            Entity::Arc {
                centre,
                radius,
                start_angle,
                sweep,
            } => {
                centre.is_finite()
                    && radius.is_finite()
                    && start_angle.is_finite()
                    && sweep.is_finite()
            }
            Entity::Circle { centre, radius } => centre.is_finite() && radius.is_finite(),
            Entity::Spline { points } => points.iter().all(|p| p.is_finite()),
        };
        if !ok {
            return Err(Error::NotFinite);
        }
        if let Entity::Spline { points } = &self {
            if points.len() < 4 || points.len() % 3 != 1 {
                return Err(Error::BadSplineLength(points.len()));
            }
            for (k, seg) in points.windows(4).step_by(3).enumerate() {
                if seg.iter().all(|p| *p == seg[0]) {
                    return Err(Error::DegenerateSegment(k));
                }
            }
        }
        match &mut self {
            Entity::Line { start, end } if start == end => return Err(Error::ZeroLength),
            Entity::Arc { radius, .. } | Entity::Circle { radius, .. } if *radius <= 0.0 => {
                return Err(Error::BadRadius(*radius))
            }
            Entity::Arc { sweep, .. } if *sweep == 0.0 || sweep.abs() >= TAU => {
                return Err(Error::BadSweep(*sweep))
            }
            // An angle already in range is left exactly as it is, so that
            // the form is canonical once and for all (wrapping it anyway
            // would round it: (0.1 + π) − π is not 0.1).
            Entity::Arc { start_angle, .. } if !(-PI..PI).contains(start_angle) => {
                let a = (*start_angle + PI).rem_euclid(TAU) - PI;
                // Just under −π the wrap rounds to π: the same angle,
                // written as the range has it.
                *start_angle = if a < PI { a } else { -PI };
            }
            _ => {}
        }
        Ok(self)
    }

    /// The kind of this entity.
    pub fn kind(&self) -> Kind {
        match self {
            Entity::Point { .. } => Kind::Point,
            Entity::Line { .. } => Kind::Line,
            Entity::Arc { .. } => Kind::Arc,
            Entity::Circle { .. } => Kind::Circle,
            Entity::Spline { .. } => Kind::Spline,
        }
    }

    /// How many values the entity has for the solver: two per point, five
    /// for an arc, three for a circle.
    pub fn values(&self) -> usize {
        match self {
            Entity::Point { .. } => 2,
            Entity::Line { .. } => 4,
            Entity::Arc { .. } => 5,
            Entity::Circle { .. } => 3,
            Entity::Spline { points } => 2 * points.len(),
        }
    }
}

/// Entities of different kinds are never near. Angles are compared on the
/// circle; sweeps as plain differences.
impl<F: Frame> AbsDiffEq for Entity<F> {
    type Epsilon = Tolerance;

    fn default_epsilon() -> Tolerance {
        Tolerance::NOISE
    }

    fn abs_diff_eq(&self, o: &Self, t: Tolerance) -> bool {
        match (self, o) {
            (Entity::Point { at: a }, Entity::Point { at: b }) => a.abs_diff_eq(b, t),
            (Entity::Line { start: s1, end: e1 }, Entity::Line { start: s2, end: e2 }) => {
                s1.abs_diff_eq(s2, t) && e1.abs_diff_eq(e2, t)
            }
            (
                Entity::Arc {
                    centre: c1,
                    radius: r1,
                    start_angle: a1,
                    sweep: w1,
                },
                Entity::Arc {
                    centre: c2,
                    radius: r2,
                    start_angle: a2,
                    sweep: w2,
                },
            ) => {
                c1.abs_diff_eq(c2, t)
                    && t.length_near(*r1, *r2)
                    && t.angle_near(*a1, *a2)
                    && t.sweep_near(*w1, *w2)
            }
            (
                Entity::Circle {
                    centre: c1,
                    radius: r1,
                },
                Entity::Circle {
                    centre: c2,
                    radius: r2,
                },
            ) => c1.abs_diff_eq(c2, t) && t.length_near(*r1, *r2),
            (Entity::Spline { points: a }, Entity::Spline { points: b }) => {
                a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.abs_diff_eq(q, t))
            }
            _ => false,
        }
    }
}

/// Near when the same ids are present and each entity is near its namesake.
impl<F: Frame> AbsDiffEq for Geometry<F> {
    type Epsilon = Tolerance;

    fn default_epsilon() -> Tolerance {
        Tolerance::NOISE
    }

    fn abs_diff_eq(&self, o: &Self, t: Tolerance) -> bool {
        self.entities.len() == o.entities.len()
            && self
                .entities
                .iter()
                .all(|(id, e)| o.entities.get(id).is_some_and(|f| e.abs_diff_eq(f, t)))
    }
}

impl<F: Frame> Entity<F> {
    /// The position `r` names, given that `r` refers to this entity.
    fn position(&self, r: &PointRef) -> Result<P2<F>, Error> {
        match (self, &r.0) {
            (Entity::Point { at }, PointAt::Point { .. }) => Ok(*at),
            (Entity::Line { start, end }, PointAt::Line { at, .. }) => Ok(match at {
                LinePos::Start => *start,
                LinePos::End => *end,
                LinePos::Mid => P2::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0),
            }),
            (
                Entity::Arc {
                    centre,
                    radius,
                    start_angle,
                    sweep,
                },
                PointAt::Arc { at, .. },
            ) => {
                let a = match at {
                    ArcPos::Start => *start_angle,
                    ArcPos::Mid => start_angle + sweep / 2.0,
                    ArcPos::End => start_angle + sweep,
                    ArcPos::Centre => return Ok(*centre),
                    ArcPos::Extreme(d) => {
                        if !d.within(*start_angle, *sweep) {
                            return Err(Error::ExtremeOffArc(r.id(), *d));
                        }
                        let [ux, uy] = d.unit();
                        return Ok(P2::new(centre.x + radius * ux, centre.y + radius * uy));
                    }
                };
                Ok(P2::new(
                    centre.x + radius * a.cos(),
                    centre.y + radius * a.sin(),
                ))
            }
            (Entity::Circle { centre, .. }, PointAt::Circle { at: None, .. }) => Ok(*centre),
            (Entity::Circle { centre, radius }, PointAt::Circle { at: Some(d), .. }) => {
                let [ux, uy] = d.unit();
                Ok(P2::new(centre.x + radius * ux, centre.y + radius * uy))
            }
            (Entity::Spline { points }, PointAt::Spline { id, at }) => at
                .index(points.len())
                .map(|i| points[i])
                .ok_or(Error::NoSuchPoint { id: *id, at: *at }),
            _ => Err(Error::WrongKind {
                id: r.id(),
                expected: r.kind(),
                found: self.kind(),
            }),
        }
    }
}

/// The store of entities in one frame, changed only through its
/// checked operations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "", try_from = "Raw<F>", into = "Raw<F>")]
pub struct Geometry<F: Frame> {
    entities: BTreeMap<EntityId, Entity<F>>,
}

/// The file form: the frame is written so that a file cannot be read into the
/// wrong one, and every entity is checked on the way in exactly as `add`
/// checks it, so a file is held to the same invariants as the store.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
struct Raw<F: Frame> {
    frame: String,
    entities: BTreeMap<EntityId, Entity<F>>,
}

impl<F: Frame> From<Geometry<F>> for Raw<F> {
    fn from(g: Geometry<F>) -> Self {
        Raw {
            frame: F::NAME.to_string(),
            entities: g.entities,
        }
    }
}

impl<F: Frame> TryFrom<Raw<F>> for Geometry<F> {
    type Error = Error;

    fn try_from(raw: Raw<F>) -> Result<Self, Error> {
        if raw.frame != F::NAME {
            return Err(Error::WrongFrame {
                expected: F::NAME.to_string(),
                found: raw.frame,
            });
        }
        let mut g = Geometry::default();
        for (id, e) in raw.entities {
            g.add(id, e)?;
        }
        Ok(g)
    }
}

impl<F: Frame> Default for Geometry<F> {
    fn default() -> Self {
        Self {
            entities: BTreeMap::new(),
        }
    }
}

impl<F: Frame> Geometry<F> {
    /// The entity with this id, if any.
    pub fn get(&self, id: impl Into<EntityId>) -> Option<&Entity<F>> {
        let id = &id.into();
        self.entities.get(id)
    }

    /// All entities, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (&EntityId, &Entity<F>)> {
        self.entities.iter()
    }

    /// Number of entities.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether there are no entities.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Add an entity under a new id. The id and the values are checked
    /// and the values put in canonical form.
    pub fn add(&mut self, id: EntityId, e: Entity<F>) -> Result<(), Error> {
        id.validate()?;
        let e = e.canonical()?;
        if self.entities.contains_key(&id) {
            return Err(Error::Duplicate(id));
        }
        self.entities.insert(id, e);
        Ok(())
    }

    /// Replace an entity's values. The kind must stay the same, so point
    /// references held elsewhere stay meaningful.
    pub fn set(&mut self, id: impl Into<EntityId>, e: Entity<F>) -> Result<(), Error> {
        let id = &id.into();
        let e = e.canonical()?;
        let slot = self.entities.get_mut(id).ok_or(Error::NoSuch(*id))?;
        if slot.kind() != e.kind() {
            return Err(Error::KindChanged(*id));
        }
        if let (Entity::Spline { points: old }, Entity::Spline { points: new }) = (&*slot, &e) {
            if old.len() != new.len() {
                return Err(Error::SplineResized(*id));
            }
        }
        *slot = e;
        Ok(())
    }

    /// Remove an entity. Nothing in a geometry refers to another entity, so
    /// this is unchecked; whatever holds references to entities (equations,
    /// annotations) must refuse to remove one that is in use.
    pub fn remove(&mut self, id: impl Into<EntityId>) -> Result<Entity<F>, Error> {
        let id = &id.into();
        self.entities.remove(id).ok_or(Error::NoSuch(*id))
    }

    /// Build a geometry by adding to an empty one in `f`; the first error
    /// stops the build. What [`try_geometry!`](crate::try_geometry) expands to.
    pub fn build(
        f: impl FnOnce(&mut Geometry<F>) -> Result<(), Error>,
    ) -> Result<Geometry<F>, Error> {
        let mut g = Geometry::default();
        f(&mut g)?;
        Ok(g)
    }

    /// Evaluate a point reference. Fails if the entity is missing or is not
    /// of the kind the reference states.
    pub fn position(&self, r: &PointRef) -> Result<P2<F>, Error> {
        self.entities
            .get(&r.id())
            .ok_or_else(|| Error::NoSuch(r.id()))?
            .position(r)
    }
}

/// Build a `Geometry` declaratively. Entities are `id: kind spec`, separated
/// by semicolons; each goes through [`Geometry::add`], so the result is
/// checked exactly as if built by hand. Coordinates are any expressions.
///
/// ```
/// use lmsheets_document::value::geometry::{Entity, Model};
/// use lmsheets_document::try_geometry;
///
/// let g = try_geometry!(Model {
///     pin:  point (3.0, 4.0);
///     top:  line (0.0, 0.0) -> (10.0, 0.0);
///     hole: circle (27.0, 8.0), r = 4.0;
///     bend: arc (10.0, 5.0), r = 5.0, from = -1.6, sweep = 1.7;
/// })?;
/// assert_eq!(g.len(), 4);
/// assert!(matches!(g.get("hole"), Some(Entity::Circle { .. })));
/// # Ok::<(), lmsheets_document::value::geometry::Error>(())
/// ```
///
/// Returns `Result<Geometry<F>, Error>`;
/// [`geometry!`](macro@crate::geometry) is
/// the same but panics on error, for tests.
#[macro_export]
macro_rules! try_geometry {
    ($frame:ty { $($body:tt)* }) => {{
        $crate::value::geometry::Geometry::<$frame>::build(|g| {
            let _ = &g;
            $crate::__geometry_body!(g; $($body)*);
            Ok(())
        })
    }};
}

/// [`try_geometry!`](crate::try_geometry) that panics on error. For tests
/// and examples.
#[macro_export]
macro_rules! geometry {
    ($($t:tt)*) => {
        $crate::try_geometry!($($t)*).expect("geometry! failed")
    };
}

/// The body of a [`try_geometry!`](crate::try_geometry) invocation, one
/// entity per arm. Not for direct use.
#[doc(hidden)]
#[macro_export]
macro_rules! __geometry_body {
    ($g:ident;) => {};
    ($g:ident; $id:ident : point ($x:expr, $y:expr) $(; $($rest:tt)*)?) => {
        $g.add(
            $crate::value::geometry::EntityId::from(stringify!($id)),
            $crate::value::geometry::Entity::Point {
                at: $crate::value::geometry::P2::new($x, $y),
            },
        )?;
        $crate::__geometry_body!($g; $($($rest)*)?);
    };
    ($g:ident; $id:ident : line ($x0:expr, $y0:expr) -> ($x1:expr, $y1:expr)
        $(; $($rest:tt)*)?) => {
        $g.add(
            $crate::value::geometry::EntityId::from(stringify!($id)),
            $crate::value::geometry::Entity::Line {
                start: $crate::value::geometry::P2::new($x0, $y0),
                end: $crate::value::geometry::P2::new($x1, $y1),
            },
        )?;
        $crate::__geometry_body!($g; $($($rest)*)?);
    };
    ($g:ident; $id:ident : circle ($x:expr, $y:expr), r = $r:expr
        $(; $($rest:tt)*)?) => {
        $g.add(
            $crate::value::geometry::EntityId::from(stringify!($id)),
            $crate::value::geometry::Entity::Circle {
                centre: $crate::value::geometry::P2::new($x, $y),
                radius: $r,
            },
        )?;
        $crate::__geometry_body!($g; $($($rest)*)?);
    };
    ($g:ident; $id:ident : spline [$(($x:expr, $y:expr)),+ $(,)?] $(; $($rest:tt)*)?) => {
        $g.add(
            $crate::value::geometry::EntityId::from(stringify!($id)),
            $crate::value::geometry::Entity::Spline {
                points: vec![$($crate::value::geometry::P2::new($x, $y)),+],
            },
        )?;
        $crate::__geometry_body!($g; $($($rest)*)?);
    };
    ($g:ident; $id:ident : arc ($x:expr, $y:expr), r = $r:expr,
        from = $a:expr, sweep = $s:expr $(; $($rest:tt)*)?) => {
        $g.add(
            $crate::value::geometry::EntityId::from(stringify!($id)),
            $crate::value::geometry::Entity::Arc {
                centre: $crate::value::geometry::P2::new($x, $y),
                radius: $r,
                start_angle: $a,
                sweep: $s,
            },
        )?;
        $crate::__geometry_body!($g; $($($rest)*)?);
    };
}

#[cfg(test)]
mod macro_tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn every_kind() {
        let g = geometry!(Model {
            pin: point (3.0, 4.0);
            top: line (0.0, 0.0) -> (10.0, 0.0);
            hole: circle (27.0, 8.0), r = 4.0;
            bend: arc (10.0, 5.0), r = 5.0, from = -FRAC_PI_2, sweep = FRAC_PI_2;
        });
        assert_eq!(g.len(), 4);
        assert_eq!(
            g.get("pin"),
            Some(&Entity::Point {
                at: P2::new(3.0, 4.0)
            })
        );
        assert_eq!(
            g.get("top"),
            Some(&Entity::Line {
                start: P2::new(0.0, 0.0),
                end: P2::new(10.0, 0.0)
            })
        );
        assert_eq!(
            g.get("hole"),
            Some(&Entity::Circle {
                centre: P2::new(27.0, 8.0),
                radius: 4.0
            })
        );
        assert_eq!(
            g.get("bend"),
            Some(&Entity::Arc {
                centre: P2::new(10.0, 5.0),
                radius: 5.0,
                start_angle: -FRAC_PI_2,
                sweep: FRAC_PI_2
            })
        );
    }

    #[test]
    fn same_as_building_by_hand() {
        let by_macro = geometry!(Model {
            a: line (1.0, 2.0) -> (3.0, 4.0);
            c: circle (0.0, 0.0), r = 1.0;
        });
        let mut by_hand = Geometry::<Model>::default();
        by_hand
            .add(
                "a".into(),
                Entity::Line {
                    start: P2::new(1.0, 2.0),
                    end: P2::new(3.0, 4.0),
                },
            )
            .unwrap();
        by_hand
            .add(
                "c".into(),
                Entity::Circle {
                    centre: P2::new(0.0, 0.0),
                    radius: 1.0,
                },
            )
            .unwrap();
        assert_eq!(by_macro, by_hand);
    }

    #[test]
    fn coordinates_are_expressions() {
        let (w, h) = (20.0, 10.0);
        let g = geometry!(Model {
            top: line (0.0, 0.0) -> (w, 0.0);
            right: line (w, 0.0) -> (w, h);
            mid: point (w / 2.0, h / 2.0);
            round: arc (w, h), r = h / 4.0, from = 0.0, sweep = -FRAC_PI_2 * 2.0;
        });
        assert_eq!(
            g.position(&PointRef::point("mid")).unwrap(),
            P2::new(10.0, 5.0)
        );
        assert_eq!(
            g.position(&PointRef::line("right", LinePos::End)).unwrap(),
            P2::new(20.0, 10.0)
        );
    }

    #[test]
    fn trailing_semicolon_optional_and_empty_allowed() {
        let a = geometry!(Model { p: point(0.0, 0.0) });
        let b = geometry!(Model { p: point (0.0, 0.0); });
        assert_eq!(a, b);
        let e = geometry!(Model {});
        assert!(e.is_empty());
    }

    #[test]
    fn frames() {
        let paper = geometry!(Paper { border: line (0.0, 0.0) -> (863.6, 0.0) });
        let model = geometry!(Model { border: line (0.0, 0.0) -> (863.6, 0.0) });
        // Different types; both hold the same values.
        let s = serde_json::to_string(&paper).unwrap();
        assert!(s.contains("\"frame\":\"paper\""));
        let s = serde_json::to_string(&model).unwrap();
        assert!(s.contains("\"frame\":\"model\""));
    }

    #[test]
    fn errors_are_the_store_errors() {
        let r = try_geometry!(Model {
            ok: point (0.0, 0.0);
            bad: circle (0.0, 0.0), r = 0.0;
        });
        assert_eq!(r, Err(Error::BadRadius(0.0)));
        let r = try_geometry!(Model {
            twice: point (0.0, 0.0);
            twice: point (1.0, 1.0);
        });
        assert_eq!(r, Err(Error::Duplicate("twice".into())));
        let r = try_geometry!(Model {
            flat: arc (0.0, 0.0), r = 1.0, from = 0.0, sweep = 0.0;
        });
        assert_eq!(r, Err(Error::BadSweep(0.0)));
        let r = try_geometry!(Model {
            dot: line (1.0, 1.0) -> (1.0, 1.0);
        });
        assert_eq!(r, Err(Error::ZeroLength));
        let r = try_geometry!(Model {
            nan: point (f64::NAN, 0.0);
        });
        assert_eq!(r, Err(Error::NotFinite));
    }

    #[test]
    #[should_panic(expected = "geometry! failed")]
    fn geometry_panics_on_error() {
        let _ = geometry!(Model {
            bad: circle (0.0, 0.0), r = -1.0;
        });
    }

    #[test]
    fn canonical_form_applies() {
        let g = geometry!(Model {
            a: arc (0.0, 0.0), r = 1.0, from = FRAC_PI_2 + std::f64::consts::TAU, sweep = 1.0;
        });
        match g.get("a").unwrap() {
            Entity::Arc { start_angle, .. } => {
                assert!((start_angle - FRAC_PI_2).abs() < 1e-12)
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::abs_diff_eq;
    use std::f64::consts::FRAC_PI_2;

    fn near(a: P2<Model>, b: P2<Model>) -> bool {
        abs_diff_eq!(a, b)
    }

    fn clip() -> Geometry<Model> {
        // Quarter turn from straight up (−y) round to +x: ends at (15, 5).
        geometry!(Model {
            top: line (0.0, 0.0) -> (10.0, 0.0);
            bend: arc (10.0, 5.0), r = 5.0, from = -FRAC_PI_2, sweep = FRAC_PI_2;
            pin: point (3.0, 4.0);
        })
    }

    #[test]
    fn extremes() {
        let g = clip();
        let bend = |d| g.position(&PointRef::arc_extreme("bend", d));
        // The quarter from -y round to +x holds those two and nothing else.
        assert_eq!(bend(Extreme::MinusY), Ok(P2::new(10.0, 0.0)));
        assert_eq!(bend(Extreme::PlusX), Ok(P2::new(15.0, 5.0)));
        assert_eq!(
            bend(Extreme::PlusY),
            Err(Error::ExtremeOffArc("bend".into(), Extreme::PlusY))
        );
        assert!(bend(Extreme::MinusX).is_err());
        // Backwards sweeps and wrapped angles.
        let h = geometry!(Model {
            back: arc (0.0, 0.0), r = 2.0, from = FRAC_PI_2, sweep = -3.0;
            ring: circle (1.0, 1.0), r = 0.5;
        });
        assert_eq!(
            h.position(&PointRef::arc_extreme("back", Extreme::PlusX)),
            Ok(P2::new(2.0, 0.0))
        );
        assert!(h
            .position(&PointRef::arc_extreme("back", Extreme::MinusX))
            .is_err());
        assert_eq!(
            h.position(&PointRef::circle_extreme("ring", Extreme::MinusY)),
            Ok(P2::new(1.0, 0.5))
        );
        assert_eq!(h.position(&PointRef::circle("ring")), Ok(P2::new(1.0, 1.0)));
        // The file form round-trips exactly, and a circle's centre keeps
        // its old form.
        for r in [
            PointRef::arc_extreme("a", Extreme::MinusX),
            PointRef::circle_extreme("c", Extreme::PlusY),
            PointRef::circle("c"),
        ] {
            let s = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<PointRef>(&s).unwrap(), r);
        }
        assert_eq!(
            serde_json::to_string(&PointRef::arc_extreme("a", Extreme::MinusX)).unwrap(),
            r#"{"kind":"arc","id":"a","at":{"extreme":"minus_x"}}"#
        );
        assert_eq!(
            serde_json::to_string(&PointRef::circle("c")).unwrap(),
            r#"{"kind":"circle","id":"c"}"#
        );
    }

    #[test]
    fn positions() {
        let g = clip();
        let p = |r: &PointRef| g.position(r).unwrap();
        let line = |at| PointRef::line("top", at);
        let arc = |at| PointRef::arc("bend", at);
        assert!(near(p(&line(LinePos::End)), P2::new(10.0, 0.0)));
        assert!(near(p(&line(LinePos::Mid)), P2::new(5.0, 0.0)));
        assert!(near(p(&arc(ArcPos::Start)), P2::new(10.0, 0.0)));
        assert!(near(p(&arc(ArcPos::End)), P2::new(15.0, 5.0)));
        assert!(near(p(&arc(ArcPos::Centre)), P2::new(10.0, 5.0)));
        let s = 5.0 / 2f64.sqrt();
        assert!(near(p(&arc(ArcPos::Mid)), P2::new(10.0 + s, 5.0 - s)));
        let pin = PointRef::point("pin");
        assert!(near(p(&pin), P2::new(3.0, 4.0)));
        // A reference of the wrong kind for the entity it names.
        let bad = PointRef::circle("top");
        assert_eq!(
            g.position(&bad),
            Err(Error::WrongKind {
                id: "top".into(),
                expected: Kind::Circle,
                found: Kind::Line
            })
        );
        let gone = PointRef::point("nope");
        assert_eq!(g.position(&gone), Err(Error::NoSuch("nope".into())));
    }

    #[test]
    fn validation() {
        let mut g = clip();
        let pt = Entity::Point {
            at: P2::new(1.0, 1.0),
        };
        assert_eq!(
            g.add("pin".into(), pt.clone()),
            Err(Error::Duplicate("pin".into()))
        );
        assert_eq!(g.set("top", pt), Err(Error::KindChanged("top".into())));
        assert_eq!(
            g.add(
                "c".into(),
                Entity::Circle {
                    centre: P2::new(0.0, 0.0),
                    radius: 0.0
                }
            ),
            Err(Error::BadRadius(0.0))
        );
        assert_eq!(
            g.add(
                "a".into(),
                Entity::Arc {
                    centre: P2::new(0.0, 0.0),
                    radius: 1.0,
                    start_angle: 0.0,
                    sweep: 0.0
                }
            ),
            Err(Error::BadSweep(0.0))
        );
        assert_eq!(
            g.add(
                "n".into(),
                Entity::Point {
                    at: P2::new(f64::NAN, 0.0)
                }
            ),
            Err(Error::NotFinite)
        );
        let arc = |start_angle, sweep| Entity::Arc {
            centre: P2::new(0.0, 0.0),
            radius: 1.0,
            start_angle,
            sweep,
        };
        assert_eq!(g.add("a".into(), arc(0.0, TAU)), Err(Error::BadSweep(TAU)));
        assert_eq!(
            g.add(
                "z".into(),
                Entity::Line {
                    start: P2::new(1.0, 1.0),
                    end: P2::new(1.0, 1.0)
                }
            ),
            Err(Error::ZeroLength)
        );
        for bad in ["", " x", "x "] {
            assert_eq!(g.add(bad.into(), pt_at(0.0)), Err(Error::BadId(bad.into())));
        }
        // Angles are stored canonically, so equal arcs are equal.
        g.add("a1".into(), arc(FRAC_PI_2, 1.0)).unwrap();
        g.add("a2".into(), arc(FRAC_PI_2 + TAU, 1.0)).unwrap();
        g.add("a3".into(), arc(-PI, 1.0)).unwrap();
        g.add("a4".into(), arc(PI, 1.0)).unwrap();
        assert_eq!(g.get("a1"), g.get("a2"));
        assert_eq!(g.get("a3"), g.get("a4"));
        let moved = pt_at(5.0);
        g.set("pin", moved.clone()).unwrap();
        assert_eq!(g.remove("pin"), Ok(moved));
        assert_eq!(g.remove("pin"), Err(Error::NoSuch("pin".into())));
    }

    fn pt_at(v: f64) -> Entity<Model> {
        Entity::Point { at: P2::new(v, v) }
    }

    #[test]
    fn nearness() {
        use crate::solver::tolerance::length;
        let g = clip();
        let mut h = clip();
        // Float noise on a coordinate and an angle one turn round.
        h.set(
            "bend",
            Entity::Arc {
                centre: P2::new(10.0 + 1e-9, 5.0),
                radius: 5.0,
                start_angle: -FRAC_PI_2 + TAU,
                sweep: FRAC_PI_2,
            },
        )
        .unwrap();
        assert!(abs_diff_eq!(g, h));
        // A real difference is not noise, but is within a looser tolerance.
        h.set(
            "pin",
            Entity::Point {
                at: P2::new(3.1, 4.0),
            },
        )
        .unwrap();
        assert!(!abs_diff_eq!(g, h));
        assert!(abs_diff_eq!(g, h, epsilon = length(0.2)));
        // Different kinds, or different ids, are never near.
        assert!(!abs_diff_eq!(
            pt_at(1.0),
            Entity::Circle {
                centre: P2::new(1.0, 1.0),
                radius: 1.0
            }
        ));
        h.remove("pin").unwrap();
        h.add("pin2".into(), pt_at(3.0)).unwrap();
        assert!(!abs_diff_eq!(g, h, epsilon = length(1.0)));
    }

    #[test]
    fn file_is_checked_on_load() {
        let s = serde_json::to_string(&clip()).unwrap();
        assert!(s.contains("\"frame\":\"model\""));
        let wrong = serde_json::from_str::<Geometry<Paper>>(&s);
        assert!(wrong.unwrap_err().to_string().contains("WrongFrame"));
        // A hand-edited file is held to the same rules as `add`.
        let bad = s.replace("\"radius\":5.0", "\"radius\":-5.0");
        let e = serde_json::from_str::<Geometry<Model>>(&bad).unwrap_err();
        assert!(e.to_string().contains("BadRadius"));
    }

    #[test]
    fn json_round_trip() {
        let g = clip();
        let s = serde_json::to_string_pretty(&g).unwrap();
        assert!(s.contains("\"kind\": \"arc\""));
        assert!(s.contains("\"frame\": \"model\""));
        assert_eq!(serde_json::from_str::<Geometry<Model>>(&s).unwrap(), g);
    }

    #[test]
    fn rect_is_ordered_and_finite() {
        let r = Rect::<Model>::new(P2::new(0.0, 1.0), P2::new(2.0, 3.0)).unwrap();
        assert!(r.contains(P2::new(2.0, 1.0)));
        assert!(!r.contains(P2::new(2.1, 1.0)));
        assert_eq!(
            Rect::<Model>::new(P2::new(1.0, 0.0), P2::new(0.0, 0.0)),
            Err(Error::BadRect)
        );
        assert_eq!(
            Rect::<Model>::new(P2::new(f64::NAN, 0.0), P2::new(0.0, 0.0)),
            Err(Error::NotFinite)
        );
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Rect<Model>>(&json).unwrap(), r);
        let bad = json.replace("\"max\":{\"x\":2.0", "\"max\":{\"x\":-2.0");
        assert!(serde_json::from_str::<Rect<Model>>(&bad).is_err());
    }
}

#[cfg(test)]
mod spline_tests {
    use super::*;
    use crate::geometry;

    fn two_segments() -> Geometry<Model> {
        geometry!(Model {
            s: spline [(0.0, 0.0), (1.0, 2.0), (3.0, 2.0), (4.0, 0.0), (5.0, -2.0), (7.0, -2.0), (8.0, 0.0)];
        })
    }

    #[test]
    fn a_spline_has_three_n_plus_one_points_and_no_flat_segment() {
        let bad = |n: usize| {
            Geometry::<Model>::build(|g| {
                g.add(
                    "s".into(),
                    Entity::Spline {
                        points: (0..n).map(|i| P2::new(i as f64, 0.0)).collect(),
                    },
                )
            })
        };
        assert_eq!(bad(3).unwrap_err(), Error::BadSplineLength(3));
        assert_eq!(bad(5).unwrap_err(), Error::BadSplineLength(5));
        assert!(bad(4).is_ok());
        assert!(bad(7).is_ok());
        let flat = Geometry::<Model>::build(|g| {
            g.add(
                "s".into(),
                Entity::Spline {
                    points: vec![
                        P2::new(0.0, 0.0),
                        P2::new(1.0, 1.0),
                        P2::new(2.0, 0.0),
                        P2::new(3.0, 0.0),
                        P2::new(3.0, 0.0),
                        P2::new(3.0, 0.0),
                        P2::new(3.0, 0.0),
                    ],
                },
            )
        });
        assert_eq!(flat.unwrap_err(), Error::DegenerateSegment(1));
        let nan = Geometry::<Model>::build(|g| {
            g.add(
                "s".into(),
                Entity::Spline {
                    points: vec![
                        P2::new(0.0, 0.0),
                        P2::new(f64::NAN, 1.0),
                        P2::new(2.0, 0.0),
                        P2::new(3.0, 0.0),
                    ],
                },
            )
        });
        assert_eq!(nan.unwrap_err(), Error::NotFinite);
    }

    #[test]
    fn positions_on_a_spline() {
        let g = two_segments();
        let at = |pos| g.position(&PointRef::spline("s", pos)).unwrap();
        assert_eq!(at(SplinePos::Start), P2::new(0.0, 0.0));
        assert_eq!(at(SplinePos::StartHandle), P2::new(1.0, 2.0));
        assert_eq!(at(SplinePos::EndHandle), P2::new(7.0, -2.0));
        assert_eq!(at(SplinePos::End), P2::new(8.0, 0.0));
        assert_eq!(at(SplinePos::Point(3)), P2::new(4.0, 0.0));
        assert_eq!(
            g.position(&PointRef::spline("s", SplinePos::Point(7))),
            Err(Error::NoSuchPoint {
                id: "s".into(),
                at: SplinePos::Point(7)
            })
        );
        assert_eq!(
            g.position(&PointRef::arc("s", ArcPos::Start)),
            Err(Error::WrongKind {
                id: "s".into(),
                expected: Kind::Arc,
                found: Kind::Spline
            })
        );
        assert_eq!(g.get("s").unwrap().values(), 14);
    }

    #[test]
    fn a_spline_keeps_its_point_count_under_set() {
        let mut g = two_segments();
        let moved = Entity::Spline {
            points: (0..7).map(|i| P2::new(i as f64, 1.0)).collect(),
        };
        assert!(g.set("s", moved).is_ok());
        let shorter = Entity::Spline {
            points: (0..4).map(|i| P2::new(i as f64, 1.0)).collect(),
        };
        assert_eq!(g.set("s", shorter), Err(Error::SplineResized("s".into())));
    }

    #[test]
    fn a_spline_round_trips_through_json() {
        let g = two_segments();
        let json = serde_json::to_string(&g).unwrap();
        assert!(json.contains("\"kind\":\"spline\""));
        assert_eq!(serde_json::from_str::<Geometry<Model>>(&json).unwrap(), g);
        // A file with a flat segment is refused on the way in.
        let bad = json.replace("{\"x\":8.0,\"y\":0.0}", "{\"x\":7.0,\"y\":-2.0}");
        assert!(serde_json::from_str::<Geometry<Model>>(&bad).is_ok());
        let flat = json.replace(
            "{\"x\":5.0,\"y\":-2.0},{\"x\":7.0,\"y\":-2.0},{\"x\":8.0,\"y\":0.0}",
            "{\"x\":4.0,\"y\":0.0},{\"x\":4.0,\"y\":0.0},{\"x\":4.0,\"y\":0.0}",
        );
        assert!(serde_json::from_str::<Geometry<Model>>(&flat).is_err());
    }
}
