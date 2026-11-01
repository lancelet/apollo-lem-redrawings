//! Equations: the closed set of relations the constraint solver evaluates.
//!
//! Each equation is one well-defined function of its operands' values whose
//! residual is zero exactly when the relation holds. Every equation has a
//! canonical form (operands ordered, signs adjusted) so that one relation
//! has one encoding and duplicates can be refused. Equations are elementary;
//! higher-level constraints can be expressed as several of them.
//!
//! Residual units: millimetres for positional equations; for angular ones
//! (`Parallel`, `Perpendicular`, `Angle`) the residual is the sine, cosine
//! or radian measure of the angle between unit directions, which counts as
//! millimetres at a one-millimetre lever. For a consistent system the solution
//! has zero residual and the weighting does not matter.
//!
//! Residuals are generic over the scalar type so that the solver can evaluate
//! them on dual numbers and obtain exact derivatives.
//!
//! Directions are of unit vectors on purpose. An unnormalised form (the
//! cross product of two line vectors for parallel, `(p − a) × d` for a
//! point on a line) is smooth everywhere but is also zero whenever a line
//! has no length, which adds collapsed lines to every solution set and
//! makes them an attractor for the solver. The normalised forms are zero
//! only where the relation holds, and where a direction does not exist
//! (two positions coincide) the evaluation says so as a [`Degeneracy`]
//! instead of guessing one.
//!
//! Conventions, within a frame (x right, y down): a line's direction `u` runs
//! from start to end and its normal is `n = (−u.y, u.x)`, `u` turned a
//! quarter turn in the positive (clockwise on the page) sense. Signed
//! distances from a line are measured along `n`.

use super::dual::{atan2, cross, dot, norm, perp, sub, unit};
use crate::value::geometry::{
    ArcPos, EntityId, Frame, Geometry, Kind, LinePos, PointRef, SplinePos,
};
use num_dual::DualNum;
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

/// A line entity. Its positions are methods, so only positions a line has
/// can be named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LineRef(EntityId);

impl LineRef {
    /// The entity referred to.
    pub fn id(self) -> EntityId {
        self.0
    }

    pub(crate) fn with_id(self, id: EntityId) -> Self {
        LineRef(id)
    }

    /// The start.
    pub fn start(self) -> PointRef {
        PointRef::line(self.0, LinePos::Start)
    }

    /// Half way along.
    pub fn mid(self) -> PointRef {
        PointRef::line(self.0, LinePos::Mid)
    }

    /// The end.
    pub fn end(self) -> PointRef {
        PointRef::line(self.0, LinePos::End)
    }
}

/// An arc entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ArcRef(EntityId);

impl ArcRef {
    /// The entity referred to.
    pub fn id(self) -> EntityId {
        self.0
    }

    pub(crate) fn with_id(self, id: EntityId) -> Self {
        ArcRef(id)
    }

    /// The start point.
    pub fn start(self) -> PointRef {
        PointRef::arc(self.0, ArcPos::Start)
    }

    /// Half way along the sweep.
    pub fn mid(self) -> PointRef {
        PointRef::arc(self.0, ArcPos::Mid)
    }

    /// The end point.
    pub fn end(self) -> PointRef {
        PointRef::arc(self.0, ArcPos::End)
    }

    /// The centre.
    pub fn centre(self) -> PointRef {
        PointRef::arc(self.0, ArcPos::Centre)
    }
}

/// A spline entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SplineRef(EntityId);

impl SplineRef {
    /// The entity referred to.
    pub fn id(self) -> EntityId {
        self.0
    }

    /// The first knot.
    pub fn start(self) -> PointRef {
        PointRef::spline(self.0, SplinePos::Start)
    }

    /// The last knot.
    pub fn end(self) -> PointRef {
        PointRef::spline(self.0, SplinePos::End)
    }

    /// A point by index.
    pub fn point(self, i: usize) -> PointRef {
        PointRef::spline(self.0, SplinePos::Point(i))
    }

    /// The named end.
    pub fn at(self, end: SplineEnd) -> PointRef {
        PointRef::spline(self.0, end.pos())
    }

    /// The control point next to the named end: the end's tangent runs
    /// between the two.
    pub fn handle(self, end: SplineEnd) -> PointRef {
        PointRef::spline(self.0, end.handle())
    }
}

/// An end of a spline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplineEnd {
    /// The first knot.
    Start,
    /// The last knot.
    End,
}

impl SplineEnd {
    /// The position of the end.
    pub fn pos(self) -> SplinePos {
        match self {
            SplineEnd::Start => SplinePos::Start,
            SplineEnd::End => SplinePos::End,
        }
    }

    /// The position of the control point next to the end.
    pub fn handle(self) -> SplinePos {
        match self {
            SplineEnd::Start => SplinePos::StartHandle,
            SplineEnd::End => SplinePos::EndHandle,
        }
    }
}

/// A circle entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CircleRef(EntityId);

impl CircleRef {
    /// The entity referred to.
    pub fn id(self) -> EntityId {
        self.0
    }

    /// The centre.
    pub fn centre(self) -> PointRef {
        PointRef::circle(self.0)
    }
}

/// An entity with a centre and a radius: an arc or a circle. Equations that
/// accept either take this; it is made only from an `ArcRef` or a
/// `CircleRef`, so it always names a kind that has a radius.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CircRef {
    id: EntityId,
    kind: CircKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CircKind {
    Arc,
    Circle,
}

impl From<ArcRef> for CircRef {
    fn from(a: ArcRef) -> Self {
        CircRef {
            id: a.0,
            kind: CircKind::Arc,
        }
    }
}

impl From<CircleRef> for CircRef {
    fn from(c: CircleRef) -> Self {
        CircRef {
            id: c.0,
            kind: CircKind::Circle,
        }
    }
}

impl CircRef {
    /// The entity referred to.
    pub fn id(self) -> EntityId {
        self.id
    }

    pub(crate) fn with_id(self, id: EntityId) -> Self {
        CircRef { id, ..self }
    }

    /// The centre.
    pub fn centre(self) -> PointRef {
        match self.kind {
            CircKind::Arc => PointRef::arc(self.id, ArcPos::Centre),
            CircKind::Circle => PointRef::circle(self.id),
        }
    }

    /// The kind the entity must be.
    pub fn kind(self) -> Kind {
        match self.kind {
            CircKind::Arc => Kind::Arc,
            CircKind::Circle => Kind::Circle,
        }
    }
}

/// A point entity, as a position.
pub fn point(id: impl Into<EntityId>) -> PointRef {
    PointRef::point(id)
}

/// A line entity.
pub fn line(id: impl Into<EntityId>) -> LineRef {
    LineRef(id.into())
}

/// An arc entity.
pub fn arc(id: impl Into<EntityId>) -> ArcRef {
    ArcRef(id.into())
}

/// A spline entity.
pub fn spline(id: impl Into<EntityId>) -> SplineRef {
    SplineRef(id.into())
}

/// A circle entity.
pub fn circle(id: impl Into<EntityId>) -> CircleRef {
    CircleRef(id.into())
}

/// One end of an arc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcEnd {
    /// Where the arc begins (its start angle).
    Start,
    /// Where the arc ends (start angle plus sweep).
    End,
}

impl ArcEnd {
    /// The position on the arc this end is.
    pub fn pos(self) -> ArcPos {
        match self {
            ArcEnd::Start => ArcPos::Start,
            ArcEnd::End => ArcPos::End,
        }
    }
}

/// A coordinate axis of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// The x coordinate.
    X,
    /// The y coordinate.
    Y,
}

/// One relation the solver can satisfy. Each variant's doc gives its
/// residual; operand fields are named by role (a point `p`, points
/// `a` and `b`, lines `l` and `m`, circles `c` and `d`) and values
/// by unit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Equation(Rel);

/// The relations, one per variant. Private: equations are built with the
/// constructor functions below and inspected through the methods.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "eq", rename_all = "snake_case")]
enum Rel {
    /// a = b (two residuals).
    Coincide {
        /// First point.
        a: PointRef,
        /// Second point.
        b: PointRef,
    },
    /// p lies on the infinite line through l.
    OnLine {
        /// The point.
        p: PointRef,
        /// The line.
        l: LineRef,
    },
    /// p lies on the full circle of c.
    OnCircle {
        /// The point.
        p: PointRef,
        /// The circle or arc.
        c: CircRef,
    },
    /// One coordinate of `a` equals one coordinate of `b`: `a.ax = b.bx`.
    /// With the same axis on both sides this is a horizontal or vertical
    /// alignment; with different axes it relates x to y, as views sharing
    /// a model axis along different sketch axes need.
    Same {
        /// First point.
        a: PointRef,
        /// Its coordinate.
        ax: Axis,
        /// Second point.
        b: PointRef,
        /// Its coordinate.
        bx: Axis,
    },
    /// The lines' directions are parallel: their cross product is zero.
    Parallel {
        /// First line.
        l: LineRef,
        /// Second line.
        m: LineRef,
    },
    /// The lines' directions are perpendicular: their dot product is
    /// zero.
    Perpendicular {
        /// First line.
        l: LineRef,
        /// Second line.
        m: LineRef,
    },
    /// The line's locus is tangent to the circle. Which side of the line the
    /// centre lies is taken from the current state.
    LineTangentCircle {
        /// The line.
        l: LineRef,
        /// The circle or arc.
        c: CircRef,
    },
    /// The circles' loci are tangent, externally or internally; the branch
    /// is taken from the current state.
    CirclesTangent {
        /// First circle or arc.
        c: CircRef,
        /// Second circle or arc.
        d: CircRef,
    },
    /// The arc's tangent at one of its ends is parallel to the line: the
    /// radius to that end is perpendicular to the line's direction. With a
    /// `Coincide` of that end and a line end this makes a tangent join.
    /// Unlike `LineTangentCircle` it is first-order independent of the
    /// coincidence, so the join counts correctly in the rank analysis.
    LineTangentArc {
        /// The line.
        l: LineRef,
        /// The arc.
        arc: ArcRef,
        /// Which end of the arc.
        at: ArcEnd,
    },
    /// The line's named end is its point of tangency with the circle: the
    /// radius to that end is perpendicular to the line. With an `OnCircle`
    /// of the end this makes a tangent join of a line and a circle, and
    /// unlike `LineTangentCircle` plus `OnCircle` the pair stays
    /// first-order independent at the solution.
    LineTangentCircleAt {
        /// The line.
        l: LineRef,
        /// Which end of the line.
        at: LinePos,
        /// The circle or arc.
        c: CircRef,
    },
    /// The arcs' tangents at the named ends are parallel: the two radii to
    /// those ends are parallel. With a `Coincide` of the ends this
    /// makes a tangent join of two arcs.
    ArcsTangent {
        /// First arc.
        a: ArcRef,
        /// Which end of the first arc.
        at_a: ArcEnd,
        /// Second arc.
        b: ArcRef,
        /// Which end of the second arc.
        at_b: ArcEnd,
    },
    /// The lines have equal length.
    EqualLength {
        /// First line.
        l: LineRef,
        /// Second line.
        m: LineRef,
    },
    /// Equal radii.
    EqualRadius {
        /// First circle or arc.
        c: CircRef,
        /// Second circle or arc.
        d: CircRef,
    },
    /// |b − a| = mm, mm > 0.
    Distance {
        /// First point.
        a: PointRef,
        /// Second point.
        b: PointRef,
        /// Millimetres, > 0.
        mm: f64,
    },
    /// b.x − a.x = mm (signed).
    Dx {
        /// First point.
        a: PointRef,
        /// Second point.
        b: PointRef,
        /// Millimetres, signed.
        mm: f64,
    },
    /// b.y − a.y = mm (signed).
    Dy {
        /// First point.
        a: PointRef,
        /// Second point.
        b: PointRef,
        /// Millimetres, signed.
        mm: f64,
    },
    /// (p − l.start) · n = mm (signed, along the line's normal).
    LineDistance {
        /// The point.
        p: PointRef,
        /// The line.
        l: LineRef,
        /// Millimetres, signed along the line's normal.
        mm: f64,
    },
    /// The radius is mm, mm > 0.
    Radius {
        /// The circle or arc.
        c: CircRef,
        /// Millimetres, > 0.
        mm: f64,
    },
    /// The angle turned from l's direction to m's is rad, in (−π, π].
    Angle {
        /// First line.
        l: LineRef,
        /// Second line.
        m: LineRef,
        /// Radians, in (−π, π].
        rad: f64,
    },
    /// b is the reflection of a in the line l (two residuals).
    Mirror {
        /// The point reflected.
        a: PointRef,
        /// Its image.
        b: PointRef,
        /// The mirror line.
        l: LineRef,
    },
    /// p is the midpoint of a and b (two residuals).
    MidpointOf {
        /// The point.
        p: PointRef,
        /// One end.
        a: PointRef,
        /// The other end.
        b: PointRef,
    },
    /// p is midway between the lines l and m: the signed distances sum to
    /// zero when `same_way`, else are equal. Linear in the distances, with
    /// the side fixed when the relation is stated (so it is well
    /// conditioned where the lines cross, unlike a squared form).
    Equidistant {
        /// The point.
        p: PointRef,
        /// First line.
        l: LineRef,
        /// Second line.
        m: LineRef,
        /// Whether the lines run the same way (their normals agree).
        same_way: bool,
    },
    /// The midpoint of a and b lies on the perpendicular bisector of p and
    /// q (a and b may be one point). One residual: the signed distance of
    /// that midpoint from the bisector, linear in the positions like
    /// `Equidistant`'s, and well conditioned for any p and q apart.
    OnBisector {
        /// One of the pair whose midpoint is held.
        a: PointRef,
        /// The other (the same point, for a point).
        b: PointRef,
        /// One of the pair that is bisected.
        p: PointRef,
        /// The other.
        q: PointRef,
    },
    /// p stays at (x, y). The solver lowers a fixed point to this using the
    /// state at the start of the solve; nothing stored carries coordinates.
    Pinned {
        /// The point.
        p: PointRef,
        /// Target x, mm.
        x: f64,
        /// Target y, mm.
        y: f64,
    },
    /// The vectors `a → b` and `c → d` are parallel (either way round):
    /// a tangent join of a spline's end to a line, an arc's radius or
    /// another spline's handle, and smoothness at a knot.
    VectorsParallel {
        /// From.
        a: PointRef,
        /// To.
        b: PointRef,
        /// From.
        c: PointRef,
        /// To.
        d: PointRef,
    },
    /// The vectors `a → b` and `c → d` are perpendicular.
    VectorsPerpendicular {
        /// From.
        a: PointRef,
        /// To.
        b: PointRef,
        /// From.
        c: PointRef,
        /// To.
        d: PointRef,
    },
}

/// What can be wrong with an equation.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// An equation relating a thing to itself.
    SameOperand,
    /// A distance or radius that is not finite or not positive, or an angle
    /// that is not finite.
    BadValue(f64),
    /// A weight that does not belong to an entity of the geometry, or is
    /// not a finite, symmetric, positive semi-definite matrix of the
    /// entity's size.
    BadWeight(EntityId),
    /// An operand is missing from the geometry or of the wrong kind.
    Geometry(crate::value::geometry::Error),
}

impl From<crate::value::geometry::Error> for Error {
    fn from(e: crate::value::geometry::Error) -> Self {
        Error::Geometry(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// Reduce an angle to (−π, π].
pub(crate) fn wrap_angle(a: f64) -> f64 {
    let w = (-a + PI).rem_euclid(TAU);
    PI - w
}

impl Equation {
    /// Number of residuals.
    pub(crate) fn count(&self) -> usize {
        match self.0 {
            Rel::Coincide { .. }
            | Rel::Mirror { .. }
            | Rel::MidpointOf { .. }
            | Rel::Pinned { .. } => 2,
            _ => 1,
        }
    }

    /// The entities referred to, with the kind each must be.
    pub(crate) fn operands(&self) -> Vec<(EntityId, Kind)> {
        let p = |r: &PointRef| (r.id(), r.kind());
        let l = |r: &LineRef| (r.0, Kind::Line);
        let c = |r: &CircRef| (r.id(), r.kind());
        match &self.0 {
            Rel::Coincide { a, b }
            | Rel::Same { a, b, .. }
            | Rel::Distance { a, b, .. }
            | Rel::Dx { a, b, .. }
            | Rel::Dy { a, b, .. } => vec![p(a), p(b)],
            Rel::OnLine { p: q, l: m } | Rel::LineDistance { p: q, l: m, .. } => {
                vec![p(q), l(m)]
            }
            Rel::OnCircle { p: q, c: d } => vec![p(q), c(d)],
            Rel::Parallel { l: a, m: b }
            | Rel::Perpendicular { l: a, m: b }
            | Rel::EqualLength { l: a, m: b }
            | Rel::Angle { l: a, m: b, .. } => vec![l(a), l(b)],
            Rel::LineTangentCircle { l: a, c: b } | Rel::LineTangentCircleAt { l: a, c: b, .. } => {
                vec![l(a), c(b)]
            }
            Rel::LineTangentArc { l: a, arc, .. } => {
                vec![l(a), (arc.0, Kind::Arc)]
            }
            Rel::ArcsTangent { a, b, .. } => {
                vec![(a.0, Kind::Arc), (b.0, Kind::Arc)]
            }
            Rel::CirclesTangent { c: a, d: b } | Rel::EqualRadius { c: a, d: b } => {
                vec![c(a), c(b)]
            }
            Rel::Radius { c: a, .. } => vec![c(a)],
            Rel::Mirror { a, b, l: m } => vec![p(a), p(b), l(m)],
            Rel::MidpointOf { p: q, a, b } => vec![p(q), p(a), p(b)],
            Rel::Equidistant {
                p: q, l: a, m: b, ..
            } => vec![p(q), l(a), l(b)],
            Rel::OnBisector { a, b, p: q, q: r } => {
                let mut out: Vec<(EntityId, Kind)> = Vec::new();
                for x in [a, b, q, r] {
                    if !out.contains(&p(x)) {
                        out.push(p(x));
                    }
                }
                out
            }
            Rel::Pinned { p: q, .. } => vec![p(q)],
            Rel::VectorsParallel { a, b, c, d } | Rel::VectorsPerpendicular { a, b, c, d } => {
                let mut out: Vec<(EntityId, Kind)> = Vec::new();
                for r in [a, b, c, d] {
                    if !out.contains(&p(r)) {
                        out.push(p(r));
                    }
                }
                out
            }
        }
    }

    /// Every point reference the equation holds: the only way an
    /// equation reaches into a spline, so the solver can differentiate
    /// against just those points.
    pub(crate) fn point_refs(&self) -> Vec<PointRef> {
        use Rel::*;
        match &self.0 {
            Coincide { a, b }
            | Same { a, b, .. }
            | Distance { a, b, .. }
            | Dx { a, b, .. }
            | Dy { a, b, .. } => {
                vec![*a, *b]
            }
            OnLine { p, .. } | OnCircle { p, .. } | LineDistance { p, .. } | Pinned { p, .. } => {
                vec![*p]
            }
            Mirror { a, b, .. } => vec![*a, *b],
            MidpointOf { p, a, b } => vec![*p, *a, *b],
            Equidistant { p, .. } => vec![*p],
            OnBisector { a, b, p, q } => vec![*a, *b, *p, *q],
            VectorsParallel { a, b, c, d } | VectorsPerpendicular { a, b, c, d } => {
                vec![*a, *b, *c, *d]
            }
            _ => Vec::new(),
        }
    }

    /// The same equation with every entity id passed through `f`: how a
    /// system of views names each view's entities apart.
    pub(crate) fn map_ids(&self, f: &dyn Fn(EntityId) -> EntityId) -> Equation {
        use Rel::*;
        let p = |r: &PointRef| r.with_id(f(r.id()));
        let l = |r: &LineRef| r.with_id(f(r.id()));
        let c = |r: &CircRef| r.with_id(f(r.id()));
        let a = |r: &ArcRef| r.with_id(f(r.id()));
        Equation(match &self.0 {
            Coincide { a: x, b } => Coincide { a: p(x), b: p(b) },
            OnLine { p: q, l: m } => OnLine { p: p(q), l: l(m) },
            OnCircle { p: q, c: d } => OnCircle { p: p(q), c: c(d) },
            Same { a: x, ax, b, bx } => Same {
                a: p(x),
                ax: *ax,
                b: p(b),
                bx: *bx,
            },
            Parallel { l: x, m } => Parallel { l: l(x), m: l(m) },
            Perpendicular { l: x, m } => Perpendicular { l: l(x), m: l(m) },
            LineTangentCircle { l: x, c: d } => LineTangentCircle { l: l(x), c: c(d) },
            LineTangentCircleAt { l: x, at, c: d } => LineTangentCircleAt {
                l: l(x),
                at: *at,
                c: c(d),
            },
            CirclesTangent { c: x, d } => CirclesTangent { c: c(x), d: c(d) },
            LineTangentArc { l: x, arc, at } => LineTangentArc {
                l: l(x),
                arc: a(arc),
                at: *at,
            },
            ArcsTangent {
                a: x,
                at_a,
                b,
                at_b,
            } => ArcsTangent {
                a: a(x),
                at_a: *at_a,
                b: a(b),
                at_b: *at_b,
            },
            EqualLength { l: x, m } => EqualLength { l: l(x), m: l(m) },
            EqualRadius { c: x, d } => EqualRadius { c: c(x), d: c(d) },
            Distance { a: x, b, mm } => Distance {
                a: p(x),
                b: p(b),
                mm: *mm,
            },
            Dx { a: x, b, mm } => Dx {
                a: p(x),
                b: p(b),
                mm: *mm,
            },
            Dy { a: x, b, mm } => Dy {
                a: p(x),
                b: p(b),
                mm: *mm,
            },
            LineDistance { p: q, l: m, mm } => LineDistance {
                p: p(q),
                l: l(m),
                mm: *mm,
            },
            Radius { c: d, mm } => Radius { c: c(d), mm: *mm },
            Angle { l: x, m, rad } => Angle {
                l: l(x),
                m: l(m),
                rad: *rad,
            },
            Mirror { a: x, b, l: m } => Mirror {
                a: p(x),
                b: p(b),
                l: l(m),
            },
            MidpointOf { p: q, a: x, b } => MidpointOf {
                p: p(q),
                a: p(x),
                b: p(b),
            },
            Equidistant {
                p: q,
                l: x,
                m,
                same_way,
            } => Equidistant {
                p: p(q),
                l: l(x),
                m: l(m),
                same_way: *same_way,
            },
            OnBisector { a, b, p: q, q: r } => OnBisector {
                a: p(a),
                b: p(b),
                p: p(q),
                q: p(r),
            },
            Pinned { p: q, x, y } => Pinned {
                p: p(q),
                x: *x,
                y: *y,
            },
            VectorsParallel { a: x, b, c, d } => VectorsParallel {
                a: p(x),
                b: p(b),
                c: p(c),
                d: p(d),
            },
            VectorsPerpendicular { a: x, b, c, d } => VectorsPerpendicular {
                a: p(x),
                b: p(b),
                c: p(c),
                d: p(d),
            },
        })
    }

    /// Check that every operand exists in `g` and is of the stated kind.
    pub fn check<F: Frame>(&self, g: &Geometry<F>) -> Result<(), Error> {
        use crate::value::geometry::Error as G;
        for (id, kind) in self.operands() {
            let e = g.get(id).ok_or(G::NoSuch(id))?;
            if e.kind() != kind {
                return Err(G::WrongKind {
                    id,
                    expected: kind,
                    found: e.kind(),
                }
                .into());
            }
        }
        Ok(())
    }

    /// Validate and put into canonical form: symmetric pairs ordered, signs
    /// of signed values adjusted to match, angles reduced to (−π, π]. An
    /// equation relating an operand to itself is refused: point–point
    /// equations need two different references, point–curve equations need
    /// the point to be on another entity than the curve, curve–curve
    /// equations need two different entities.
    pub fn canonical(self) -> Result<Self, Error> {
        fn finite(v: f64) -> Result<f64, Error> {
            if v.is_finite() {
                Ok(v)
            } else {
                Err(Error::BadValue(v))
            }
        }
        fn positive(v: f64) -> Result<f64, Error> {
            if v.is_finite() && v > 0.0 {
                Ok(v)
            } else {
                Err(Error::BadValue(v))
            }
        }
        fn pair<T: Ord>(a: T, b: T) -> Result<(T, T, bool), Error> {
            match a.cmp(&b) {
                std::cmp::Ordering::Equal => Err(Error::SameOperand),
                std::cmp::Ordering::Less => Ok((a, b, false)),
                std::cmp::Ordering::Greater => Ok((b, a, true)),
            }
        }
        fn apart(p: &PointRef, id: &EntityId) -> Result<(), Error> {
            if p.id() == *id {
                Err(Error::SameOperand)
            } else {
                Ok(())
            }
        }
        use Rel::*;
        Ok(Equation(match self.0 {
            Coincide { a, b } => {
                let (a, b, _) = pair(a, b)?;
                Coincide { a, b }
            }
            Same { a, ax, b, bx } => {
                // The same coordinate of the same point is a self-relation;
                // two coordinates of one point (a point on a diagonal) is not.
                let ((a, ax), (b, bx), _) = pair((a, ax), (b, bx))?;
                Same { a, ax, b, bx }
            }
            Distance { a, b, mm } => {
                let (a, b, _) = pair(a, b)?;
                Distance {
                    a,
                    b,
                    mm: positive(mm)?,
                }
            }
            Dx { a, b, mm } => {
                let (a, b, swapped) = pair(a, b)?;
                let mm = finite(mm)?;
                Dx {
                    a,
                    b,
                    mm: if swapped { -mm } else { mm },
                }
            }
            Dy { a, b, mm } => {
                let (a, b, swapped) = pair(a, b)?;
                let mm = finite(mm)?;
                Dy {
                    a,
                    b,
                    mm: if swapped { -mm } else { mm },
                }
            }
            OnLine { p, l } => {
                apart(&p, &l.0)?;
                OnLine { p, l }
            }
            LineDistance { p, l, mm } => {
                apart(&p, &l.0)?;
                LineDistance {
                    p,
                    l,
                    mm: finite(mm)?,
                }
            }
            OnCircle { p, c } => {
                apart(&p, &c.id())?;
                OnCircle { p, c }
            }
            Parallel { l, m } => {
                let (l, m, _) = pair(l, m)?;
                Parallel { l, m }
            }
            Perpendicular { l, m } => {
                let (l, m, _) = pair(l, m)?;
                Perpendicular { l, m }
            }
            EqualLength { l, m } => {
                let (l, m, _) = pair(l, m)?;
                EqualLength { l, m }
            }
            Angle { l, m, rad } => {
                let (l, m, swapped) = pair(l, m)?;
                let rad = finite(rad)?;
                Angle {
                    l,
                    m,
                    rad: wrap_angle(if swapped { -rad } else { rad }),
                }
            }
            LineTangentCircle { l, c } => LineTangentCircle { l, c },
            LineTangentCircleAt { l, at, c } => LineTangentCircleAt { l, at, c },
            LineTangentArc { l, arc, at } => LineTangentArc { l, arc, at },
            ArcsTangent { a, at_a, b, at_b } => {
                let ((a, at_a), (b, at_b), _) = pair((a, at_a), (b, at_b))?;
                if a == b {
                    return Err(Error::SameOperand);
                }
                ArcsTangent { a, at_a, b, at_b }
            }
            CirclesTangent { c, d } => {
                let (c, d, _) = pair(c, d)?;
                CirclesTangent { c, d }
            }
            EqualRadius { c, d } => {
                let (c, d, _) = pair(c, d)?;
                EqualRadius { c, d }
            }
            VectorsParallel { a, b, c, d } => {
                if a == b || c == d {
                    return Err(Error::SameOperand);
                }
                VectorsParallel { a, b, c, d }
            }
            VectorsPerpendicular { a, b, c, d } => {
                if a == b || c == d {
                    return Err(Error::SameOperand);
                }
                VectorsPerpendicular { a, b, c, d }
            }
            Radius { c, mm } => Radius {
                c,
                mm: positive(mm)?,
            },
            Mirror { a, b, l } => {
                let (a, b, _) = pair(a, b)?;
                apart(&a, &l.0)?;
                apart(&b, &l.0)?;
                Mirror { a, b, l }
            }
            MidpointOf { p, a, b } => {
                let (a, b, _) = pair(a, b)?;
                if p == a || p == b {
                    return Err(Error::SameOperand);
                }
                MidpointOf { p, a, b }
            }
            Equidistant { p, l, m, same_way } => {
                let (l, m, _) = pair(l, m)?;
                apart(&p, &l.0)?;
                apart(&p, &m.0)?;
                Equidistant { p, l, m, same_way }
            }
            OnBisector { a, b, p, q } => {
                let (p, q, _) = pair(p, q)?;
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                // The midpoint of the pair itself is on its bisector.
                if (a, b) == (p, q) {
                    return Err(Error::SameOperand);
                }
                OnBisector { a, b, p, q }
            }
            Pinned { p, x, y } => Pinned {
                p,
                x: finite(x)?,
                y: finite(y)?,
            },
        }))
    }
}

// Constructors: one lowercase function per variant, so equations read as a
// small language. Each builds the literal; `canonical` is still the caller's
// (or the store's) step.

/// `a` = `b`.
pub fn coincide(a: PointRef, b: PointRef) -> Equation {
    Equation(Rel::Coincide { a, b })
}

/// `p` on the infinite line through `l`.
pub fn on_line(p: PointRef, l: LineRef) -> Equation {
    Equation(Rel::OnLine { p, l })
}

/// `p` on the full circle of `c`.
pub fn on_circle(p: PointRef, c: impl Into<CircRef>) -> Equation {
    Equation(Rel::OnCircle { p, c: c.into() })
}

/// `a.ax` = `b.bx`: one coordinate of each point.
pub fn same(a: PointRef, ax: Axis, b: PointRef, bx: Axis) -> Equation {
    Equation(Rel::Same { a, ax, b, bx })
}

/// `a.x` = `b.x`.
pub fn same_x(a: PointRef, b: PointRef) -> Equation {
    same(a, Axis::X, b, Axis::X)
}

/// `a.y` = `b.y`.
pub fn same_y(a: PointRef, b: PointRef) -> Equation {
    same(a, Axis::Y, b, Axis::Y)
}

/// The lines are parallel.
pub fn parallel(l: LineRef, m: LineRef) -> Equation {
    Equation(Rel::Parallel { l, m })
}

/// The lines are perpendicular.
pub fn perpendicular(l: LineRef, m: LineRef) -> Equation {
    Equation(Rel::Perpendicular { l, m })
}

/// The line's locus is tangent to the circle or arc.
pub fn line_tangent_circle(l: LineRef, c: impl Into<CircRef>) -> Equation {
    Equation(Rel::LineTangentCircle { l, c: c.into() })
}

/// The two loci are tangent.
pub fn circles_tangent(c: impl Into<CircRef>, d: impl Into<CircRef>) -> Equation {
    Equation(Rel::CirclesTangent {
        c: c.into(),
        d: d.into(),
    })
}

/// The arc's tangent at `at` is parallel to the line (a tangent join with
/// the matching `coincide`).
pub fn line_tangent_arc(l: LineRef, arc: ArcRef, at: ArcEnd) -> Equation {
    Equation(Rel::LineTangentArc { l, arc, at })
}

/// The line's named end is its point of tangency with the circle (pair
/// with `on_circle` of that end for the join).
pub fn line_tangent_circle_at(l: LineRef, at: LinePos, c: impl Into<CircRef>) -> Equation {
    Equation(Rel::LineTangentCircleAt { l, at, c: c.into() })
}

/// The arcs' tangents at the named ends are parallel.
pub fn arcs_tangent(a: ArcRef, at_a: ArcEnd, b: ArcRef, at_b: ArcEnd) -> Equation {
    Equation(Rel::ArcsTangent { a, at_a, b, at_b })
}

/// The vectors `a → b` and `c → d` are parallel.
pub fn vectors_parallel(a: PointRef, b: PointRef, c: PointRef, d: PointRef) -> Equation {
    Equation(Rel::VectorsParallel { a, b, c, d })
}

/// The vectors `a → b` and `c → d` are perpendicular.
pub fn vectors_perpendicular(a: PointRef, b: PointRef, c: PointRef, d: PointRef) -> Equation {
    Equation(Rel::VectorsPerpendicular { a, b, c, d })
}

/// The lines have equal length.
pub fn equal_length(l: LineRef, m: LineRef) -> Equation {
    Equation(Rel::EqualLength { l, m })
}

/// Equal radii.
pub fn equal_radius(c: impl Into<CircRef>, d: impl Into<CircRef>) -> Equation {
    Equation(Rel::EqualRadius {
        c: c.into(),
        d: d.into(),
    })
}

/// |`b` − `a`| = `mm`.
pub fn distance(a: PointRef, b: PointRef, mm: f64) -> Equation {
    Equation(Rel::Distance { a, b, mm })
}

/// A line's own length: `distance(l.start(), l.end(), mm)`.
pub fn length(l: LineRef, mm: f64) -> Equation {
    distance(l.start(), l.end(), mm)
}

/// `b.x` − `a.x` = `mm`.
pub fn dx(a: PointRef, b: PointRef, mm: f64) -> Equation {
    Equation(Rel::Dx { a, b, mm })
}

/// `b.y` − `a.y` = `mm`.
pub fn dy(a: PointRef, b: PointRef, mm: f64) -> Equation {
    Equation(Rel::Dy { a, b, mm })
}

/// Signed distance from `p` to the line, along its normal.
pub fn line_distance(p: PointRef, l: LineRef, mm: f64) -> Equation {
    Equation(Rel::LineDistance { p, l, mm })
}

/// The radius is `mm`.
pub fn radius(c: impl Into<CircRef>, mm: f64) -> Equation {
    Equation(Rel::Radius { c: c.into(), mm })
}

/// The angle from `l` to `m` is `rad`.
pub fn angle(l: LineRef, m: LineRef, rad: f64) -> Equation {
    Equation(Rel::Angle { l, m, rad })
}

/// `b` is `a` reflected in the line.
pub fn mirror(a: PointRef, b: PointRef, l: LineRef) -> Equation {
    Equation(Rel::Mirror { a, b, l })
}

/// `p` is the midpoint of `a` and `b`.
pub fn midpoint_of(p: PointRef, a: PointRef, b: PointRef) -> Equation {
    Equation(Rel::MidpointOf { p, a, b })
}

/// `p` is midway between the lines `l` and `m`, which run the same way or
/// not (as stated).
pub fn equidistant(p: PointRef, l: LineRef, m: LineRef, same_way: bool) -> Equation {
    Equation(Rel::Equidistant { p, l, m, same_way })
}

/// The midpoint of `a` and `b` lies on the perpendicular bisector of `p`
/// and `q`.
pub fn on_bisector(a: PointRef, b: PointRef, p: PointRef, q: PointRef) -> Equation {
    Equation(Rel::OnBisector { a, b, p, q })
}

/// `p` stays at (`x`, `y`).
pub fn pinned(p: PointRef, x: f64, y: f64) -> Equation {
    Equation(Rel::Pinned { p, x, y })
}

/// Values of entities as scalars of type `T`, supplied by the solver from its
/// parameter vector (or by anything else that can evaluate positions).
pub(crate) trait Values<T> {
    fn point(&self, r: &PointRef) -> [T; 2];
    fn radius(&self, c: &CircRef) -> T;
    /// The angle from an arc's centre to one of its ends.
    fn arc_angle(&self, arc: &ArcRef, at: ArcEnd) -> T;
}

/// Where an equation's residuals have no derivative, so the solver cannot
/// linearise it there. A degeneracy of the operands' current values, not
/// an error in the equation: the same equation is well-defined elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Degeneracy {
    /// The two positions coincide, so the vector between them has no
    /// direction: a line of no length (its start and end), a point at a
    /// circle's centre, concentric circles, a distance between coincident
    /// points. A direction from it is undefined; a length of it is
    /// defined (zero) but has no derivative (a cone).
    Coincident(PointRef, PointRef),
    /// The circle or arc has no radius to divide by.
    ZeroRadius(CircRef),
}

/// One equation's operands read through `V`, keeping the first degeneracy
/// met. Every operation that is not smooth everywhere goes through here.
struct Operands<'v, V> {
    v: &'v V,
    degenerate: Option<Degeneracy>,
}

impl<V> Operands<'_, V> {
    fn at<T>(&self, p: PointRef) -> [T; 2]
    where
        V: Values<T>,
    {
        self.v.point(&p)
    }

    fn radius<T>(&self, c: CircRef) -> T
    where
        V: Values<T>,
    {
        self.v.radius(&c)
    }

    /// The vector from `a` to `b`, which the caller is about to measure
    /// or normalise: noted if it is zero.
    fn between<T: DualNum<f64>>(&mut self, a: PointRef, b: PointRef) -> [T; 2]
    where
        V: Values<T>,
    {
        let d = sub(&self.at(b), &self.at(a));
        if d[0].re() == 0.0 && d[1].re() == 0.0 {
            self.degenerate.get_or_insert(Degeneracy::Coincident(a, b));
        }
        d
    }

    /// `|b − a|`.
    fn distance<T: DualNum<f64>>(&mut self, a: PointRef, b: PointRef) -> T
    where
        V: Values<T>,
    {
        norm(&self.between(a, b))
    }

    /// The unit vector from `a` to `b`.
    fn direction<T: DualNum<f64>>(&mut self, a: PointRef, b: PointRef) -> [T; 2]
    where
        V: Values<T>,
    {
        unit(&self.between(a, b))
    }

    /// A line's unit direction, start to end.
    fn line<T: DualNum<f64>>(&mut self, l: LineRef) -> [T; 2]
    where
        V: Values<T>,
    {
        self.direction(l.start(), l.end())
    }

    /// The signed distance of `p` from the line, along its normal.
    fn offset<T: DualNum<f64>>(&mut self, p: PointRef, l: LineRef) -> T
    where
        V: Values<T>,
    {
        let n = perp(&self.line(l));
        dot(&sub(&self.at(p), &self.at(l.start())), &n)
    }

    /// The radius, as a divisor: noted if it is zero.
    fn divisor_radius<T: DualNum<f64>>(&mut self, c: CircRef) -> T
    where
        V: Values<T>,
    {
        let r = self.radius(c);
        if r.re() == 0.0 {
            self.degenerate.get_or_insert(Degeneracy::ZeroRadius(c));
        }
        r
    }

    /// The unit vector from an arc's centre towards one of its ends
    /// (exactly `(p − centre) / r`, with no division).
    fn radial<T: DualNum<f64>>(&self, arc: ArcRef, at: ArcEnd) -> [T; 2]
    where
        V: Values<T>,
    {
        let t = self.v.arc_angle(&arc, at);
        [t.cos(), t.sin()]
    }
}

/// Sign of the real part, as a scalar: +1 or −1 (0 counts as +1). Picks
/// the branch of a relation that has two; each branch is smooth.
fn sign_of<T: DualNum<f64>>(v: &T) -> f64 {
    if v.re() < 0.0 {
        -1.0
    } else {
        1.0
    }
}

impl Equation {
    /// Push the residuals onto `out`: zero exactly when the relation
    /// holds. They are pushed in any case; `Err` says that the operands
    /// are where the residuals have no derivative (the values are then
    /// NaN if undefined, as a direction of a zero vector is).
    pub(crate) fn residuals<T: DualNum<f64>, V: Values<T>>(
        &self,
        v: &V,
        out: &mut Vec<T>,
    ) -> Result<(), Degeneracy> {
        use Rel::*;
        let mut o = Operands {
            v,
            degenerate: None,
        };
        match self.0 {
            Coincide { a, b } => out.extend(sub(&o.at(b), &o.at(a))),
            OnLine { p, l } => out.push(o.offset(p, l)),
            OnCircle { p, c } => out.push(o.distance(c.centre(), p) - o.radius(c)),
            Same { a, ax, b, bx } => {
                let coord = |p: [T; 2], axis: Axis| match axis {
                    Axis::X => p[0].clone(),
                    Axis::Y => p[1].clone(),
                };
                out.push(coord(o.at(b), bx) - coord(o.at(a), ax));
            }
            Parallel { l, m } => out.push(cross(&o.line(l), &o.line(m))),
            Perpendicular { l, m } => out.push(dot(&o.line(l), &o.line(m))),
            LineTangentCircle { l, c } => {
                let d = o.offset(c.centre(), l);
                let s = sign_of(&d);
                out.push(d * s - o.radius(c));
            }
            CirclesTangent { c, d } => {
                let dist = o.distance(c.centre(), d.centre());
                let (r1, r2) = (o.radius(c), o.radius(d));
                let outer = dist.clone() - &r1 - &r2;
                let diff = r1 - r2;
                let inner = dist - diff.clone() * sign_of(&diff);
                out.push(if outer.re().abs() <= inner.re().abs() {
                    outer
                } else {
                    inner
                });
            }
            LineTangentArc { l, arc, at } => {
                out.push(dot(&o.radial(arc, at), &o.line(l)));
            }
            LineTangentCircleAt { l, at, c } => {
                let u = o.line(l);
                let rad = sub(&o.at(PointRef::line(l.0, at)), &o.at(c.centre()));
                out.push(dot(&rad, &u) / o.divisor_radius(c));
            }
            ArcsTangent { a, at_a, b, at_b } => {
                out.push(cross(&o.radial(a, at_a), &o.radial(b, at_b)));
            }
            VectorsParallel { a, b, c, d } => {
                out.push(cross(&o.direction(a, b), &o.direction(c, d)));
            }
            VectorsPerpendicular { a, b, c, d } => {
                out.push(dot(&o.direction(a, b), &o.direction(c, d)));
            }
            EqualLength { l, m } => {
                let ll = o.distance(l.start(), l.end());
                out.push(ll - o.distance(m.start(), m.end()));
            }
            EqualRadius { c, d } => out.push(o.radius(c) - o.radius(d)),
            Distance { a, b, mm } => out.push(o.distance(a, b) - mm),
            Dx { a, b, mm } => out.push(o.at(b)[0].clone() - &o.at(a)[0] - mm),
            Dy { a, b, mm } => out.push(o.at(b)[1].clone() - &o.at(a)[1] - mm),
            LineDistance { p, l, mm } => out.push(o.offset(p, l) - mm),
            Radius { c, mm } => out.push(o.radius(c) - mm),
            Angle { l, m, rad } => {
                let (ul, um) = (o.line(l), o.line(m));
                let w = atan2(cross(&ul, &um), dot(&ul, &um)) - rad;
                // Wrap the real part to (−π, π]; the derivative is
                // unchanged. The cut is half a turn from the solution.
                let k = ((w.re() - PI) / TAU).ceil();
                out.push(w - k * TAU);
            }
            Mirror { a, b, l } => {
                let u = o.line(l);
                let (pa, pb) = (o.at(a), o.at(b));
                let mid = [
                    (pa[0].clone() + &pb[0]) * 0.5,
                    (pa[1].clone() + &pb[1]) * 0.5,
                ];
                out.push(dot(&sub(&mid, &o.at(l.start())), &perp(&u)));
                out.push(dot(&sub(&pb, &pa), &u));
            }
            MidpointOf { p, a, b } => {
                let (q, pa, pb) = (o.at(p), o.at(a), o.at(b));
                out.extend([
                    q[0].clone() - (pa[0].clone() + &pb[0]) * 0.5,
                    q[1].clone() - (pa[1].clone() + &pb[1]) * 0.5,
                ]);
            }
            Equidistant { p, l, m, same_way } => {
                let (dl, dm) = (o.offset(p, l), o.offset(p, m));
                out.push(if same_way { dl + dm } else { dl - dm });
            }
            OnBisector { a, b, p, q } => {
                let (pa, pb) = (o.at(a), o.at(b));
                let mid = [
                    (pa[0].clone() + &pb[0]) * 0.5,
                    (pa[1].clone() + &pb[1]) * 0.5,
                ];
                let (pp, pq) = (o.at(p), o.at(q));
                let centre = [
                    (pp[0].clone() + &pq[0]) * 0.5,
                    (pp[1].clone() + &pq[1]) * 0.5,
                ];
                out.push(dot(&sub(&mid, &centre), &o.direction(p, q)));
            }
            Pinned { p, x, y } => {
                let q = o.at(p);
                out.extend([q[0].clone() - x, q[1].clone() - y]);
            }
        }
        o.degenerate.map_or(Ok(()), Err)
    }

    /// The relation's kind, as an index in declaration order: a match with
    /// every variant, so a new one does not compile until it is given the
    /// next index here, `KINDS` is raised, and a strategy for it is added
    /// in `solver::testing::equation`.
    #[cfg(test)]
    pub(crate) fn kind(&self) -> usize {
        use Rel::*;
        match self.0 {
            Coincide { .. } => 0,
            OnLine { .. } => 1,
            OnCircle { .. } => 2,
            Same { .. } => 3,
            Parallel { .. } => 4,
            Perpendicular { .. } => 5,
            LineTangentCircle { .. } => 6,
            CirclesTangent { .. } => 7,
            LineTangentArc { .. } => 8,
            LineTangentCircleAt { .. } => 9,
            ArcsTangent { .. } => 10,
            EqualLength { .. } => 11,
            EqualRadius { .. } => 12,
            Distance { .. } => 13,
            Dx { .. } => 14,
            Dy { .. } => 15,
            LineDistance { .. } => 16,
            Radius { .. } => 17,
            Angle { .. } => 18,
            Mirror { .. } => 19,
            Pinned { .. } => 20,
            VectorsParallel { .. } => 21,
            VectorsPerpendicular { .. } => 22,
            MidpointOf { .. } => 23,
            Equidistant { .. } => 24,
            OnBisector { .. } => 25,
        }
    }

    /// The axes a `same` compares, the first point's and the second's.
    #[cfg(test)]
    pub(crate) fn same_axes(&self) -> Option<(Axis, Axis)> {
        match self.0 {
            Rel::Same { ax, bx, .. } => Some((ax, bx)),
            _ => None,
        }
    }
}

/// How many relation kinds there are: one more than the last
/// `Equation::kind`.
#[cfg(test)]
pub(crate) const KINDS: usize = 26;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::geometry::{ArcPos, LinePos};

    #[test]
    fn canonical_orders_and_signs() {
        let (a, b, l, m) = (point("a"), point("b"), line("l"), line("m"));
        assert_eq!(dx(b, a, 3.0).canonical().unwrap(), dx(a, b, -3.0));
        assert_eq!(angle(m, l, 3.0).canonical().unwrap(), angle(l, m, -3.0));
        assert_eq!(angle(l, m, -PI).canonical().unwrap(), angle(l, m, PI));
        assert_eq!(coincide(b, a).canonical().unwrap(), coincide(a, b));
        // `same` orders by (point, axis), so x-of-b = y-of-a is written as
        // y-of-a = x-of-b and the two spellings are one equation.
        assert_eq!(
            same(b, Axis::X, a, Axis::Y).canonical().unwrap(),
            same(a, Axis::Y, b, Axis::X)
        );
        assert_eq!(parallel(m, l).canonical().unwrap(), parallel(l, m));
    }

    #[test]
    fn refuses_self_relations_and_bad_values() {
        let (a, b, l) = (point("a"), point("b"), line("l"));
        assert_eq!(coincide(a, a).canonical(), Err(Error::SameOperand));
        assert_eq!(same_x(a, a).canonical(), Err(Error::SameOperand));
        // A point on the diagonal: its own x equals its own y. Allowed.
        assert!(same(a, Axis::X, a, Axis::Y).canonical().is_ok());
        // Two different positions of one entity are fine (a line's length).
        assert!(length(l, 2.0).canonical().is_ok());
        // But a line's own point on the line is not.
        assert_eq!(on_line(l.start(), l).canonical(), Err(Error::SameOperand));
        assert_eq!(radius(arc("a"), 0.0).canonical(), Err(Error::BadValue(0.0)));
        assert!(matches!(
            dx(a, b, f64::NAN).canonical(),
            Err(Error::BadValue(_))
        ));
        assert!(matches!(
            pinned(a, f64::INFINITY, 0.0).canonical(),
            Err(Error::BadValue(_))
        ));
    }

    #[test]
    fn constructors_build_the_literals() {
        let (p, l, m, a, b, c) = (
            point("p"),
            line("l"),
            line("m"),
            arc("a"),
            arc("b"),
            circle("c"),
        );
        assert_eq!(l.start(), PointRef::line("l", LinePos::Start));
        assert_eq!(l.mid(), PointRef::line("l", LinePos::Mid));
        assert_eq!(l.end(), PointRef::line("l", LinePos::End));
        assert_eq!(a.centre(), PointRef::arc("a", ArcPos::Centre));
        assert_eq!(a.mid(), PointRef::arc("a", ArcPos::Mid));
        assert_eq!(c.centre(), PointRef::circle("c"));
        assert_eq!(CircRef::from(a), CircRef::from(arc("a")));
        assert_eq!(CircRef::from(c), CircRef::from(circle("c")));
        use Rel::*;
        let ca = CircRef::from(arc("a"));
        let cc = CircRef::from(circle("c"));
        assert_eq!(coincide(p, l.end()).0, Coincide { a: p, b: l.end() });
        assert_eq!(on_line(p, l).0, OnLine { p, l });
        assert_eq!(on_circle(p, a).0, OnCircle { p, c: ca });
        assert_eq!(
            same_x(p, l.end()).0,
            Same {
                a: p,
                ax: Axis::X,
                b: l.end(),
                bx: Axis::X
            }
        );
        assert_eq!(
            same_y(p, l.end()).0,
            Same {
                a: p,
                ax: Axis::Y,
                b: l.end(),
                bx: Axis::Y
            }
        );
        assert_eq!(
            same(p, Axis::X, l.end(), Axis::Y).0,
            Same {
                a: p,
                ax: Axis::X,
                b: l.end(),
                bx: Axis::Y
            }
        );
        assert_eq!(parallel(l, m).0, Parallel { l, m });
        assert_eq!(perpendicular(l, m).0, Perpendicular { l, m });
        assert_eq!(line_tangent_circle(l, c).0, LineTangentCircle { l, c: cc });
        assert_eq!(circles_tangent(a, c).0, CirclesTangent { c: ca, d: cc });
        assert_eq!(
            line_tangent_arc(l, a, ArcEnd::End).0,
            LineTangentArc {
                l,
                arc: a,
                at: ArcEnd::End
            }
        );
        assert_eq!(
            arcs_tangent(a, ArcEnd::Start, b, ArcEnd::End).0,
            ArcsTangent {
                a,
                at_a: ArcEnd::Start,
                b,
                at_b: ArcEnd::End
            }
        );
        assert_eq!(equal_length(l, m).0, EqualLength { l, m });
        assert_eq!(equal_radius(a, c).0, EqualRadius { c: ca, d: cc });
        assert_eq!(
            distance(p, l.end(), 2.0).0,
            Distance {
                a: p,
                b: l.end(),
                mm: 2.0
            }
        );
        assert_eq!(
            length(l, 2.0).0,
            Distance {
                a: l.start(),
                b: l.end(),
                mm: 2.0
            }
        );
        assert_eq!(
            dx(p, l.end(), 2.0).0,
            Dx {
                a: p,
                b: l.end(),
                mm: 2.0
            }
        );
        assert_eq!(
            dy(p, l.end(), 2.0).0,
            Dy {
                a: p,
                b: l.end(),
                mm: 2.0
            }
        );
        assert_eq!(line_distance(p, l, 1.0).0, LineDistance { p, l, mm: 1.0 });
        assert_eq!(radius(a, 1.5).0, Radius { c: ca, mm: 1.5 });
        assert_eq!(angle(l, m, 0.3).0, Angle { l, m, rad: 0.3 });
        assert_eq!(
            mirror(p, l.end(), m).0,
            Mirror {
                a: p,
                b: l.end(),
                l: m
            }
        );
        assert_eq!(pinned(p, 1.0, 2.0).0, Pinned { p, x: 1.0, y: 2.0 });
        assert_eq!(
            midpoint_of(p, l.start(), l.end()).0,
            MidpointOf {
                p,
                a: l.start(),
                b: l.end()
            }
        );
        assert_eq!(
            on_bisector(p, l.start(), l.end(), m.start()).0,
            OnBisector {
                a: p,
                b: l.start(),
                p: l.end(),
                q: m.start()
            }
        );
        assert_eq!(
            equidistant(p, l, m, true).0,
            Equidistant {
                p,
                l,
                m,
                same_way: true
            }
        );
    }

    #[test]
    fn wrap() {
        assert_eq!(wrap_angle(PI), PI);
        assert!((wrap_angle(-PI) - PI).abs() < 1e-12);
        assert!((wrap_angle(3.0 * PI) - PI).abs() < 1e-12);
        assert!((wrap_angle(0.5) - 0.5).abs() < 1e-12);
        assert!((wrap_angle(-0.5) + 0.5).abs() < 1e-12);
    }
}
