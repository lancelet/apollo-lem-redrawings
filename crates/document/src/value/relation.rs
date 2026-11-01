//! Relations a sketch stores: the semantic constraints of a drawing, and
//! how each lowers to the solver's elementary equations.
//!
//! A `Relation` says what the draughtsman meant: a corner, a tangent join,
//! two concentric arcs, a printed dimension. It is what a drawing's
//! annotations refer to and what a reader sees in the file. The solver
//! never sees it; `lower` turns it into one or more equations of the
//! crate-private solver, and the sketch refuses a relation whose lowered
//! equations repeat another's, so one fact is stated once however it is
//! spelled.
//!
//! Dimensional relations carry a [`Measure`] (what the sheet printed, a
//! value of ours, or a parameter) and are driving by default; a reference
//! dimension (`REF` on the sheet) is `driven`: it is not solved, only
//! checked against the solved figure. Geometric relations have no value and
//! are always driving.
//!
//! Relations are built with the lowercase functions here, in the same style
//! as the equations, and are opaque: the representation is private and the
//! file form is the only other way to see one.

use crate::solver::equation::{self as eq, Equation};
pub use crate::solver::equation::{
    arc, circle, line, point, spline, ArcEnd, ArcRef, Axis, CircRef, LineRef, SplineEnd, SplineRef,
};
use crate::value::geometry::{EntityId, Frame, Geometry, Kind, LinePos, PointRef};
use crate::value::measure::{self, Measure, ParamName, Quantity, Tolerance};
use serde::{Deserialize, Serialize};

/// One end of a line, for a join.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineEnd {
    /// The start.
    Start,
    /// The end.
    End,
}

impl LineEnd {
    fn pos(self) -> LinePos {
        match self {
            LineEnd::Start => LinePos::Start,
            LineEnd::End => LinePos::End,
        }
    }
}

/// What can be wrong with a relation.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A parameter the sketch does not have.
    UnknownParam(ParamName),
    /// A value whose unit measures the wrong quantity for the relation.
    Measure(measure::Error),
    /// `driven` or a value change on a relation that has no value.
    NotDimensional,
    /// Values given to a dimension that has a different number of them.
    WrongArity {
        /// How many the relation has.
        expected: usize,
        /// How many were given.
        found: usize,
    },
    /// An operand problem found when lowering (missing entity, wrong kind).
    Geometry(crate::value::geometry::Error),
    /// A lowered equation that is not well formed.
    Equation(eq::Error),
}

impl From<measure::Error> for Error {
    fn from(e: measure::Error) -> Self {
        Error::Measure(e)
    }
}

impl From<crate::value::geometry::Error> for Error {
    fn from(e: crate::value::geometry::Error) -> Self {
        Error::Geometry(e)
    }
}

impl From<eq::Error> for Error {
    fn from(e: eq::Error) -> Self {
        Error::Equation(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// The family of dimension that shows a relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionKind {
    /// A distance between two things, along a direction.
    Linear,
    /// A radius or a diameter.
    Radial,
    /// An angle between two lines.
    Angular,
    /// A point's coordinates from the datum.
    Ordinate,
}

/// What a radial constraint states of its circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    /// The radius.
    Radius,
    /// The diameter.
    Diameter,
}

/// The direction a linear relation measures along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// From the first point to the second.
    Direct,
    /// The sketch's x axis.
    X,
    /// The sketch's y axis.
    Y,
}

/// What a linear relation measures between.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Span {
    /// Two points, along a direction.
    Points {
        /// The first.
        a: PointRef,
        /// The second.
        b: PointRef,
        /// The direction measured along.
        along: Direction,
    },
    /// A point and its foot on a line.
    PointLine {
        /// The point.
        p: PointRef,
        /// The line.
        l: LineRef,
    },
    /// Two parallel lines: the first's start and its foot on the second.
    Lines {
        /// The first.
        l: LineRef,
        /// The second.
        m: LineRef,
    },
}

/// A relation the sketch stores, with the tolerance on its value if it
/// has one. Built by the functions below; see the module documentation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawRelation", into = "RawRelation")]
pub struct Relation(Rel, Tolerance);

/// The file form: the relation's own fields, and the tolerance when
/// one is stated.
#[derive(Serialize, Deserialize)]
struct RawRelation {
    #[serde(flatten)]
    rel: Rel,
    #[serde(default, skip_serializing_if = "Tolerance::is_none")]
    tolerance: Tolerance,
}

impl From<Relation> for RawRelation {
    fn from(r: Relation) -> Self {
        RawRelation {
            rel: r.0,
            tolerance: r.1,
        }
    }
}

impl TryFrom<RawRelation> for Relation {
    type Error = Error;

    fn try_from(r: RawRelation) -> Result<Self, Error> {
        Relation::of(r.rel).toleranced(r.tolerance)
    }
}

impl Relation {
    /// A relation with no tolerance.
    fn of(rel: Rel) -> Self {
        Relation(rel, Tolerance::None)
    }

    /// The same relation with this tolerance on its value. Fails on a
    /// relation with no value, or a tolerance of the wrong quantity.
    pub fn toleranced(mut self, tolerance: Tolerance) -> Result<Self, Error> {
        self.set_tolerance(tolerance)?;
        Ok(self)
    }

    /// Replace the tolerance, returning the old one.
    pub fn set_tolerance(&mut self, tolerance: Tolerance) -> Result<Tolerance, Error> {
        if !tolerance.is_none() {
            let q = self.quantity().ok_or(Error::NotDimensional)?;
            tolerance.check(q)?;
        }
        Ok(std::mem::replace(&mut self.1, tolerance))
    }

    /// The tolerance on the value.
    pub fn tolerance(&self) -> &Tolerance {
        &self.1
    }
}

/// The kinds, one per variant. Private: relations are built with the
/// constructor functions and inspected through the methods.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "rel", rename_all = "snake_case")]
enum Rel {
    Coincident {
        a: PointRef,
        b: PointRef,
    },
    OnLine {
        p: PointRef,
        l: LineRef,
    },
    OnCircle {
        p: PointRef,
        c: CircRef,
    },
    Horizontal {
        l: LineRef,
    },
    Vertical {
        l: LineRef,
    },
    HorizontalPoints {
        a: PointRef,
        b: PointRef,
    },
    VerticalPoints {
        a: PointRef,
        b: PointRef,
    },
    Parallel {
        l: LineRef,
        m: LineRef,
    },
    Perpendicular {
        l: LineRef,
        m: LineRef,
    },
    Collinear {
        l: LineRef,
        m: LineRef,
    },
    Tangent {
        l: LineRef,
        c: CircRef,
    },
    TangentCircles {
        c: CircRef,
        d: CircRef,
    },
    TangentJoin {
        l: LineRef,
        at: LineEnd,
        arc: ArcRef,
        arc_at: ArcEnd,
    },
    TangentArcJoin {
        a: ArcRef,
        at_a: ArcEnd,
        b: ArcRef,
        at_b: ArcEnd,
    },
    TangentAt {
        l: LineRef,
        at: LineEnd,
        c: CircRef,
    },
    TangentJoinSpline {
        l: LineRef,
        at: LineEnd,
        s: SplineRef,
        s_at: SplineEnd,
    },
    TangentArcSplineJoin {
        arc: ArcRef,
        at_a: ArcEnd,
        s: SplineRef,
        s_at: SplineEnd,
    },
    SplineJoin {
        a: SplineRef,
        at_a: SplineEnd,
        b: SplineRef,
        at_b: SplineEnd,
    },
    Smooth {
        s: SplineRef,
    },
    EqualLength {
        l: LineRef,
        m: LineRef,
    },
    EqualRadius {
        c: CircRef,
        d: CircRef,
    },
    Concentric {
        c: CircRef,
        d: CircRef,
    },
    Symmetric {
        a: PointRef,
        b: PointRef,
        l: LineRef,
    },
    Midpoint {
        p: PointRef,
        l: LineRef,
    },
    MidpointOf {
        p: PointRef,
        a: PointRef,
        b: PointRef,
    },
    Equidistant {
        p: PointRef,
        l: LineRef,
        m: LineRef,
        same_way: bool,
    },
    OnBisector {
        a: PointRef,
        b: PointRef,
        p: PointRef,
        q: PointRef,
    },
    Fixed {
        p: PointRef,
    },
    Distance {
        a: PointRef,
        b: PointRef,
        value: Measure,
        driving: bool,
    },
    HorizontalDistance {
        a: PointRef,
        b: PointRef,
        value: Measure,
        driving: bool,
    },
    VerticalDistance {
        a: PointRef,
        b: PointRef,
        value: Measure,
        driving: bool,
    },
    Length {
        l: LineRef,
        value: Measure,
        driving: bool,
    },
    PointLineDistance {
        p: PointRef,
        l: LineRef,
        value: Measure,
        driving: bool,
    },
    LineDistance {
        l: LineRef,
        m: LineRef,
        value: Measure,
        driving: bool,
    },
    Radius {
        c: CircRef,
        value: Measure,
        driving: bool,
    },
    Diameter {
        c: CircRef,
        value: Measure,
        driving: bool,
    },
    Angle {
        l: LineRef,
        m: LineRef,
        value: Measure,
        driving: bool,
    },
    Position {
        p: PointRef,
        x: Measure,
        y: Measure,
        driving: bool,
    },
}

// Geometric relations.

/// `a` and `b` are the same point.
pub fn coincident(a: PointRef, b: PointRef) -> Relation {
    Relation::of(Rel::Coincident { a, b })
}

/// `p` lies on the line through `l`.
pub fn on_line(p: PointRef, l: LineRef) -> Relation {
    Relation::of(Rel::OnLine { p, l })
}

/// `p` lies on the circle of `c`.
pub fn on_circle(p: PointRef, c: impl Into<CircRef>) -> Relation {
    Relation::of(Rel::OnCircle { p, c: c.into() })
}

/// The line is horizontal.
pub fn horizontal(l: LineRef) -> Relation {
    Relation::of(Rel::Horizontal { l })
}

/// The line is vertical.
pub fn vertical(l: LineRef) -> Relation {
    Relation::of(Rel::Vertical { l })
}

/// The points are level.
pub fn horizontal_points(a: PointRef, b: PointRef) -> Relation {
    Relation::of(Rel::HorizontalPoints { a, b })
}

/// The points are plumb.
pub fn vertical_points(a: PointRef, b: PointRef) -> Relation {
    Relation::of(Rel::VerticalPoints { a, b })
}

/// The lines are parallel.
pub fn parallel(l: LineRef, m: LineRef) -> Relation {
    Relation::of(Rel::Parallel { l, m })
}

/// The lines are perpendicular.
pub fn perpendicular(l: LineRef, m: LineRef) -> Relation {
    Relation::of(Rel::Perpendicular { l, m })
}

/// The lines lie on one line.
pub fn collinear(l: LineRef, m: LineRef) -> Relation {
    Relation::of(Rel::Collinear { l, m })
}

/// The line is tangent to the circle or arc (their loci touch).
pub fn tangent(l: LineRef, c: impl Into<CircRef>) -> Relation {
    Relation::of(Rel::Tangent { l, c: c.into() })
}

/// The two circles or arcs are tangent (their loci touch).
pub fn tangent_circles(c: impl Into<CircRef>, d: impl Into<CircRef>) -> Relation {
    Relation::of(Rel::TangentCircles {
        c: c.into(),
        d: d.into(),
    })
}

/// The line's end meets the arc's end, and the two are tangent there.
pub fn tangent_join(l: LineRef, at: LineEnd, arc: ArcRef, arc_at: ArcEnd) -> Relation {
    Relation::of(Rel::TangentJoin { l, at, arc, arc_at })
}

/// The line's named end lies on the circle and the line is tangent to it
/// there: a line ending at its point of tangency.
pub fn tangent_at(l: LineRef, at: LineEnd, c: impl Into<CircRef>) -> Relation {
    Relation::of(Rel::TangentAt { l, at, c: c.into() })
}

/// The arcs' ends meet, and the two are tangent there.
pub fn tangent_arc_join(a: ArcRef, at_a: ArcEnd, b: ArcRef, at_b: ArcEnd) -> Relation {
    Relation::of(Rel::TangentArcJoin { a, at_a, b, at_b })
}

/// The line's end meets the spline's end, and the line runs along the
/// spline's tangent there.
pub fn tangent_join_spline(l: LineRef, at: LineEnd, s: SplineRef, s_at: SplineEnd) -> Relation {
    Relation::of(Rel::TangentJoinSpline { l, at, s, s_at })
}

/// The arc's end meets the spline's end, and the two are tangent there.
pub fn tangent_arc_spline_join(
    arc: ArcRef,
    at_a: ArcEnd,
    s: SplineRef,
    s_at: SplineEnd,
) -> Relation {
    Relation::of(Rel::TangentArcSplineJoin { arc, at_a, s, s_at })
}

/// The splines' ends meet, and the two are tangent there.
pub fn spline_join(a: SplineRef, at_a: SplineEnd, b: SplineRef, at_b: SplineEnd) -> Relation {
    Relation::of(Rel::SplineJoin { a, at_a, b, at_b })
}

/// The spline is smooth at every interior knot: the control points
/// either side of each knot lie on a line through it.
pub fn smooth(s: SplineRef) -> Relation {
    Relation::of(Rel::Smooth { s })
}

/// The lines have the same length.
pub fn equal_length(l: LineRef, m: LineRef) -> Relation {
    Relation::of(Rel::EqualLength { l, m })
}

/// The circles or arcs have the same radius.
pub fn equal_radius(c: impl Into<CircRef>, d: impl Into<CircRef>) -> Relation {
    Relation::of(Rel::EqualRadius {
        c: c.into(),
        d: d.into(),
    })
}

/// The circles or arcs share a centre.
pub fn concentric(c: impl Into<CircRef>, d: impl Into<CircRef>) -> Relation {
    Relation::of(Rel::Concentric {
        c: c.into(),
        d: d.into(),
    })
}

/// `b` is `a` mirrored in the line.
pub fn symmetric(a: PointRef, b: PointRef, l: LineRef) -> Relation {
    Relation::of(Rel::Symmetric { a, b, l })
}

/// `p` is the midpoint of the line.
pub fn midpoint(p: PointRef, l: LineRef) -> Relation {
    Relation::of(Rel::Midpoint { p, l })
}

/// `p` is the midpoint of the points `a` and `b`.
pub fn midpoint_of(p: PointRef, a: PointRef, b: PointRef) -> Relation {
    Relation::of(Rel::MidpointOf { p, a, b })
}

/// `p` is midway between the lines `l` and `m` (parallel): `same_way`
/// says whether they run the same way, which fixes the side of each
/// distance when the relation is stated.
pub fn equidistant(p: PointRef, l: LineRef, m: LineRef, same_way: bool) -> Relation {
    Relation::of(Rel::Equidistant { p, l, m, same_way })
}

/// The midpoint of `a` and `b` (one point, if they are the same) lies on
/// the perpendicular bisector of `p` and `q`: a point or a pair centred
/// between two points, as one row, linear in the positions.
pub fn on_bisector(a: PointRef, b: PointRef, p: PointRef, q: PointRef) -> Relation {
    Relation::of(Rel::OnBisector { a, b, p, q })
}

/// `p` stays where it is.
pub fn fixed(p: PointRef) -> Relation {
    Relation::of(Rel::Fixed { p })
}

// Dimensional relations. Driving unless made `driven`.

/// The distance between the points.
pub fn distance(a: PointRef, b: PointRef, value: Measure) -> Relation {
    Relation::of(Rel::Distance {
        a,
        b,
        value,
        driving: true,
    })
}

/// The horizontal distance from `a` to `b`, signed.
pub fn horizontal_distance(a: PointRef, b: PointRef, value: Measure) -> Relation {
    Relation::of(Rel::HorizontalDistance {
        a,
        b,
        value,
        driving: true,
    })
}

/// The vertical distance from `a` to `b`, signed.
pub fn vertical_distance(a: PointRef, b: PointRef, value: Measure) -> Relation {
    Relation::of(Rel::VerticalDistance {
        a,
        b,
        value,
        driving: true,
    })
}

/// The line's length.
pub fn length(l: LineRef, value: Measure) -> Relation {
    Relation::of(Rel::Length {
        l,
        value,
        driving: true,
    })
}

/// The signed distance from the point to the line, along its normal.
pub fn point_line_distance(p: PointRef, l: LineRef, value: Measure) -> Relation {
    Relation::of(Rel::PointLineDistance {
        p,
        l,
        value,
        driving: true,
    })
}

/// `m`'s start is this far from `l` (signed, along `l`'s normal): the
/// distance between two lines that are parallel. One equation: the
/// parallelism is a precondition the sketch states itself (both level, or
/// a `parallel` constraint), not restated here.
pub fn line_distance(l: LineRef, m: LineRef, value: Measure) -> Relation {
    Relation::of(Rel::LineDistance {
        l,
        m,
        value,
        driving: true,
    })
}

/// The radius.
pub fn radius(c: impl Into<CircRef>, value: Measure) -> Relation {
    Relation::of(Rel::Radius {
        c: c.into(),
        value,
        driving: true,
    })
}

/// The diameter.
pub fn diameter(c: impl Into<CircRef>, value: Measure) -> Relation {
    Relation::of(Rel::Diameter {
        c: c.into(),
        value,
        driving: true,
    })
}

/// The angle turned from `l` to `m`.
pub fn angle(l: LineRef, m: LineRef, value: Measure) -> Relation {
    Relation::of(Rel::Angle {
        l,
        m,
        value,
        driving: true,
    })
}

/// `p` is at (`x`, `y`) in the sketch's frame: a fiducial, such as a
/// station read from the sheet. Unlike [`fixed`], which holds a point
/// wherever it is, this is a claim with values and provenance.
pub fn position(p: PointRef, x: Measure, y: Measure) -> Relation {
    Relation::of(Rel::Position {
        p,
        x,
        y,
        driving: true,
    })
}

impl Relation {
    /// A dimension's values (one, or two for a position), whether it
    /// drives, and the quantity the values must be in.
    fn dim(&self) -> Option<(Vec<&Measure>, bool, Quantity)> {
        use Rel::*;
        match &self.0 {
            Distance { value, driving, .. }
            | HorizontalDistance { value, driving, .. }
            | VerticalDistance { value, driving, .. }
            | Length { value, driving, .. }
            | PointLineDistance { value, driving, .. }
            | LineDistance { value, driving, .. }
            | Radius { value, driving, .. }
            | Diameter { value, driving, .. } => Some((vec![value], *driving, Quantity::Length)),
            Angle { value, driving, .. } => Some((vec![value], *driving, Quantity::Angle)),
            Position { x, y, driving, .. } => Some((vec![x, y], *driving, Quantity::Length)),
            _ => None,
        }
    }

    fn dim_mut(&mut self) -> Option<(Vec<&mut Measure>, &mut bool)> {
        use Rel::*;
        match &mut self.0 {
            Distance { value, driving, .. }
            | HorizontalDistance { value, driving, .. }
            | VerticalDistance { value, driving, .. }
            | Length { value, driving, .. }
            | PointLineDistance { value, driving, .. }
            | LineDistance { value, driving, .. }
            | Radius { value, driving, .. }
            | Diameter { value, driving, .. }
            | Angle { value, driving, .. } => Some((vec![value], driving)),
            Position { x, y, driving, .. } => Some((vec![x, y], driving)),
            _ => None,
        }
    }

    /// Make a dimension a reference (`REF`): checked, not solved. Fails on
    /// a relation with no value.
    pub fn driven(mut self) -> Result<Self, Error> {
        match self.dim_mut() {
            Some((_, driving)) => {
                *driving = false;
                Ok(self)
            }
            None => Err(Error::NotDimensional),
        }
    }

    /// Whether the relation is solved for. Geometric relations always are;
    /// a dimension is unless made `driven`.
    pub fn driving(&self) -> bool {
        self.dim().is_none_or(|(_, d, _)| d)
    }

    /// The values: none for a geometric relation, one for a dimension, two
    /// (x, y) for a position.
    pub fn values(&self) -> Vec<&Measure> {
        self.dim().map(|(v, _, _)| v).unwrap_or_default()
    }

    /// What quantity a dimension's values must be in.
    pub fn quantity(&self) -> Option<Quantity> {
        self.dim().map(|(_, _, q)| q)
    }

    /// Which family of dimension can show this relation, if any.
    pub fn dimension_kind(&self) -> Option<DimensionKind> {
        use Rel::*;
        Some(match &self.0 {
            Distance { .. }
            | HorizontalDistance { .. }
            | VerticalDistance { .. }
            | Length { .. }
            | PointLineDistance { .. }
            | LineDistance { .. } => DimensionKind::Linear,
            Radius { .. } | Diameter { .. } => DimensionKind::Radial,
            Angle { .. } => DimensionKind::Angular,
            Position { .. } => DimensionKind::Ordinate,
            _ => return None,
        })
    }

    /// What a radial relation measures: its circle or arc, and whether
    /// the value is a diameter (else a radius).
    pub fn circle(&self) -> Option<(CircRef, Round)> {
        match &self.0 {
            Rel::Radius { c, .. } => Some((*c, Round::Radius)),
            Rel::Diameter { c, .. } => Some((*c, Round::Diameter)),
            _ => None,
        }
    }

    /// What an angular relation measures between: the line turned from
    /// and the line turned to.
    pub fn lines(&self) -> Option<(LineRef, LineRef)> {
        match &self.0 {
            Rel::Angle { l, m, .. } => Some((*l, *m)),
            _ => None,
        }
    }

    /// What an ordinate relation measures: the positioned point.
    pub fn positioned(&self) -> Option<PointRef> {
        match &self.0 {
            Rel::Position { p, .. } => Some(*p),
            _ => None,
        }
    }

    /// What a linear relation measures between: its span.
    pub fn span(&self) -> Option<Span> {
        use Rel::*;
        Some(match &self.0 {
            Distance { a, b, .. } => Span::Points {
                a: *a,
                b: *b,
                along: Direction::Direct,
            },
            HorizontalDistance { a, b, .. } => Span::Points {
                a: *a,
                b: *b,
                along: Direction::X,
            },
            VerticalDistance { a, b, .. } => Span::Points {
                a: *a,
                b: *b,
                along: Direction::Y,
            },
            Length { l, .. } => Span::Points {
                a: l.start(),
                b: l.end(),
                along: Direction::Direct,
            },
            PointLineDistance { p, l, .. } => Span::PointLine { p: *p, l: *l },
            LineDistance { l, m, .. } => Span::Lines { l: *l, m: *m },
            _ => return None,
        })
    }

    /// Replace a dimension's values, returning the old ones. As many values
    /// as the relation has must be given.
    pub fn set_values(&mut self, values: Vec<Measure>) -> Result<Vec<Measure>, Error> {
        let (slots, _) = self.dim_mut().ok_or(Error::NotDimensional)?;
        if slots.len() != values.len() {
            return Err(Error::WrongArity {
                expected: slots.len(),
                found: values.len(),
            });
        }
        Ok(slots
            .into_iter()
            .zip(values)
            .map(|(slot, v)| std::mem::replace(slot, v))
            .collect())
    }

    /// Replace a single-valued dimension's value, returning the old one.
    pub fn set_value(&mut self, value: Measure) -> Result<Measure, Error> {
        let mut old = self.set_values(vec![value])?;
        Ok(old.remove(0))
    }

    /// The parameters the values refer to, if any.
    pub fn params(&self) -> Vec<ParamName> {
        self.values()
            .into_iter()
            .chain(self.1.parts())
            .filter_map(|m| match m {
                Measure::Param { name } => Some(*name),
                _ => None,
            })
            .collect()
    }

    /// The two circles or arcs this makes share a centre (concentric,
    /// or their centres coincident), if it does.
    pub fn same_centre(&self) -> Option<(EntityId, EntityId)> {
        match &self.0 {
            Rel::Concentric { c, d } => Some((c.id(), d.id())),
            Rel::Coincident { a, b } => Some((a.centre_of()?, b.centre_of()?)),
            _ => None,
        }
    }

    /// The entities referred to, with the kind each must be.
    pub fn operands(&self) -> Vec<(EntityId, Kind)> {
        use Rel::*;
        let p = |r: &PointRef| (r.id(), r.kind());
        let l = |r: &LineRef| (r.id(), Kind::Line);
        let c = |r: &CircRef| (r.id(), r.kind());
        let a = |r: &ArcRef| (r.id(), Kind::Arc);
        match &self.0 {
            Coincident { a: x, b }
            | HorizontalPoints { a: x, b }
            | VerticalPoints { a: x, b }
            | Distance { a: x, b, .. }
            | HorizontalDistance { a: x, b, .. }
            | VerticalDistance { a: x, b, .. } => vec![p(x), p(b)],
            OnLine { p: q, l: m }
            | Midpoint { p: q, l: m }
            | PointLineDistance { p: q, l: m, .. } => vec![p(q), l(m)],
            OnCircle { p: q, c: d } => vec![p(q), c(d)],
            Horizontal { l: m } | Vertical { l: m } | Length { l: m, .. } => vec![l(m)],
            Parallel { l: x, m }
            | Perpendicular { l: x, m }
            | Collinear { l: x, m }
            | EqualLength { l: x, m }
            | LineDistance { l: x, m, .. }
            | Angle { l: x, m, .. } => vec![l(x), l(m)],
            Tangent { l: x, c: d } => vec![l(x), c(d)],
            TangentCircles { c: x, d } | EqualRadius { c: x, d } | Concentric { c: x, d } => {
                vec![c(x), c(d)]
            }
            TangentJoin { l: x, arc, .. } => vec![l(x), a(arc)],
            TangentArcJoin { a: x, b, .. } => vec![a(x), a(b)],
            TangentAt { l: x, c: d, .. } => vec![l(x), c(d)],
            TangentJoinSpline { l: x, s, .. } => vec![l(x), (s.id(), Kind::Spline)],
            TangentArcSplineJoin { arc, s, .. } => vec![a(arc), (s.id(), Kind::Spline)],
            SplineJoin { a: x, b, .. } => vec![(x.id(), Kind::Spline), (b.id(), Kind::Spline)],
            Smooth { s } => vec![(s.id(), Kind::Spline)],
            Symmetric { a: x, b, l: m } => vec![p(x), p(b), l(m)],
            MidpointOf { p: q, a: x, b } => vec![p(q), p(x), p(b)],
            Equidistant { p: q, l: x, m, .. } => vec![p(q), l(x), l(m)],
            OnBisector { a, b, p: q, q: r } => vec![p(a), p(b), p(q), p(r)],
            Fixed { p: q } | Position { p: q, .. } => vec![p(q)],
            Radius { c: d, .. } | Diameter { c: d, .. } => vec![c(d)],
        }
    }

    /// Check the operands against a geometry and the value's unit against
    /// the relation's quantity (a parameter's unit is checked when it is
    /// resolved, in `lower`).
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
        if let Some((values, _, q)) = self.dim() {
            for v in values {
                v.check(q)?;
            }
        }
        Ok(())
    }

    /// A dimension's values in the base unit (mm or rad), resolving
    /// parameters through `params`.
    fn resolve(&self, params: &dyn Fn(ParamName) -> Option<Measure>) -> Result<Vec<f64>, Error> {
        let (values, _, q) = self.dim().expect("only called on dimensions");
        values
            .into_iter()
            .map(|value| {
                let m = match value {
                    Measure::Param { name } => match params(*name) {
                        // A parameter's value is never itself a parameter.
                        Some(Measure::Param { .. }) | None => {
                            return Err(Error::UnknownParam(*name))
                        }
                        Some(m) => m,
                    },
                    m => m.clone(),
                };
                m.check(q)?;
                Ok(m.base().expect("a resolved measure has a base value"))
            })
            .collect()
    }

    /// The elementary equations this relation states, in canonical form.
    /// `fixed` is lowered at the current position in `g`; parameters are
    /// resolved through `params`. Lowering does not depend on `driving`:
    /// the sketch decides what to solve and what only to check.
    pub fn lower<F: Frame>(
        &self,
        g: &Geometry<F>,
        params: &dyn Fn(ParamName) -> Option<Measure>,
    ) -> Result<Vec<Equation>, Error> {
        use Rel::*;
        let v = |r: &Relation| r.resolve(params).map(|vals| vals[0]);
        let eqs = match &self.0 {
            Coincident { a, b } => vec![eq::coincide(*a, *b)],
            OnLine { p, l } => vec![eq::on_line(*p, *l)],
            OnCircle { p, c } => vec![eq::on_circle(*p, *c)],
            Horizontal { l } => vec![eq::same_y(l.start(), l.end())],
            Vertical { l } => vec![eq::same_x(l.start(), l.end())],
            HorizontalPoints { a, b } => vec![eq::same_y(*a, *b)],
            VerticalPoints { a, b } => vec![eq::same_x(*a, *b)],
            Parallel { l, m } => vec![eq::parallel(*l, *m)],
            Perpendicular { l, m } => vec![eq::perpendicular(*l, *m)],
            Collinear { l, m } => vec![eq::parallel(*l, *m), eq::on_line(m.start(), *l)],
            Tangent { l, c } => vec![eq::line_tangent_circle(*l, *c)],
            TangentCircles { c, d } => vec![eq::circles_tangent(*c, *d)],
            TangentJoin { l, at, arc, arc_at } => {
                let line_end = PointRef::line(l.id(), at.pos());
                let arc_end = PointRef::arc(arc.id(), arc_at.pos());
                vec![
                    eq::coincide(line_end, arc_end),
                    eq::line_tangent_arc(*l, *arc, *arc_at),
                ]
            }
            TangentAt { l, at, c } => {
                let end = PointRef::line(l.id(), at.pos());
                vec![
                    eq::on_circle(end, *c),
                    eq::line_tangent_circle_at(*l, at.pos(), *c),
                ]
            }
            TangentArcJoin { a, at_a, b, at_b } => vec![
                eq::coincide(
                    PointRef::arc(a.id(), at_a.pos()),
                    PointRef::arc(b.id(), at_b.pos()),
                ),
                eq::arcs_tangent(*a, *at_a, *b, *at_b),
            ],
            // A spline's tangent at an end is the direction between the
            // end knot and its handle.
            TangentJoinSpline { l, at, s, s_at } => vec![
                eq::coincide(PointRef::line(l.id(), at.pos()), s.at(*s_at)),
                eq::vectors_parallel(l.start(), l.end(), s.at(*s_at), s.handle(*s_at)),
            ],
            TangentArcSplineJoin { arc, at_a, s, s_at } => vec![
                eq::coincide(PointRef::arc(arc.id(), at_a.pos()), s.at(*s_at)),
                // An arc's tangent is perpendicular to its radius.
                eq::vectors_perpendicular(
                    arc.centre(),
                    PointRef::arc(arc.id(), at_a.pos()),
                    s.at(*s_at),
                    s.handle(*s_at),
                ),
            ],
            SplineJoin { a, at_a, b, at_b } => vec![
                eq::coincide(a.at(*at_a), b.at(*at_b)),
                eq::vectors_parallel(a.at(*at_a), a.handle(*at_a), b.at(*at_b), b.handle(*at_b)),
            ],
            Smooth { s } => {
                // One equation per interior knot, by index: the geometry
                // says how many points the spline has.
                use crate::value::geometry::{Entity, Error as GeomError};
                let n = match g.get(s.id()) {
                    Some(Entity::Spline { points }) => points.len(),
                    Some(e) => {
                        return Err(GeomError::WrongKind {
                            id: s.id(),
                            expected: Kind::Spline,
                            found: e.kind(),
                        }
                        .into())
                    }
                    None => return Err(GeomError::NoSuch(s.id()).into()),
                };
                (3..n.saturating_sub(1))
                    .step_by(3)
                    .map(|k| {
                        eq::vectors_parallel(s.point(k - 1), s.point(k), s.point(k), s.point(k + 1))
                    })
                    .collect()
            }
            EqualLength { l, m } => vec![eq::equal_length(*l, *m)],
            EqualRadius { c, d } => vec![eq::equal_radius(*c, *d)],
            Concentric { c, d } => vec![eq::coincide(centre(*c), centre(*d))],
            Symmetric { a, b, l } => vec![eq::mirror(*a, *b, *l)],
            Midpoint { p, l } => vec![eq::coincide(*p, l.mid())],
            MidpointOf { p, a, b } => vec![eq::midpoint_of(*p, *a, *b)],
            Equidistant { p, l, m, same_way } => {
                vec![eq::equidistant(*p, *l, *m, *same_way)]
            }
            OnBisector { a, b, p, q } => vec![eq::on_bisector(*a, *b, *p, *q)],
            Fixed { p } => {
                let at = g.position(p)?;
                vec![eq::pinned(*p, at.x, at.y)]
            }
            Distance { a, b, .. } => vec![eq::distance(*a, *b, v(self)?)],
            HorizontalDistance { a, b, .. } => vec![eq::dx(*a, *b, v(self)?)],
            VerticalDistance { a, b, .. } => vec![eq::dy(*a, *b, v(self)?)],
            Length { l, .. } => vec![eq::length(*l, v(self)?)],
            PointLineDistance { p, l, .. } => vec![eq::line_distance(*p, *l, v(self)?)],
            // One equation: the distance. That the lines are parallel is a
            // precondition the sketch states itself (both level, or a
            // `Parallel`); stating it here too made it redundant whenever
            // it was stated, and the multipliers non-unique.
            LineDistance { l, m, .. } => vec![eq::line_distance(m.start(), *l, v(self)?)],
            Radius { c, .. } => vec![eq::radius(*c, v(self)?)],
            Diameter { c, .. } => vec![eq::radius(*c, v(self)? / 2.0)],
            Angle { l, m, .. } => vec![eq::angle(*l, *m, v(self)?)],
            Position { p, .. } => {
                let xy = self.resolve(params)?;
                vec![eq::pinned(*p, xy[0], xy[1])]
            }
        };
        eqs.into_iter()
            .map(|e| e.canonical().map_err(Error::from))
            .collect()
    }
}

/// The centre of a circle or arc, as a position.
fn centre(c: CircRef) -> PointRef {
    match c.kind() {
        Kind::Arc => eq::arc(c.id()).centre(),
        _ => eq::circle(c.id()).centre(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry;
    use crate::solver::equation::{arc, circle, line, point};
    use crate::value::geometry::{ArcPos, Model, P2};
    use std::f64::consts::FRAC_PI_2;

    fn g() -> Geometry<Model> {
        geometry!(Model {
            p: point (1.0, 2.0);
            l: line (0.0, 0.0) -> (10.0, 0.0);
            m: line (0.0, 5.0) -> (10.0, 5.0);
            a: arc (10.0, 5.0), r = 5.0, from = -FRAC_PI_2, sweep = FRAC_PI_2;
            b: arc (20.0, 5.0), r = 3.0, from = 0.0, sweep = 1.0;
            c: circle (-2.0, -1.5), r = 0.9;
        })
    }

    fn no_params(_: ParamName) -> Option<Measure> {
        None
    }

    fn mm(v: f64) -> Measure {
        Measure::mm(v).unwrap()
    }

    fn lowered(r: &Relation) -> Vec<Equation> {
        r.lower(&g(), &no_params).unwrap()
    }

    fn canon(e: Equation) -> Equation {
        e.canonical().unwrap()
    }

    #[test]
    fn each_relation_lowers_to_its_equations() {
        let (p, l, m, a, b, c) = (
            point("p"),
            line("l"),
            line("m"),
            arc("a"),
            arc("b"),
            circle("c"),
        );
        let cases: Vec<(Relation, Vec<Equation>)> = vec![
            (coincident(p, l.end()), vec![eq::coincide(p, l.end())]),
            (on_line(p, l), vec![eq::on_line(p, l)]),
            (on_circle(p, a), vec![eq::on_circle(p, a)]),
            (horizontal(l), vec![eq::same_y(l.start(), l.end())]),
            (vertical(l), vec![eq::same_x(l.start(), l.end())]),
            (
                horizontal_points(p, a.centre()),
                vec![eq::same_y(p, a.centre())],
            ),
            (
                vertical_points(p, a.centre()),
                vec![eq::same_x(p, a.centre())],
            ),
            (parallel(l, m), vec![eq::parallel(l, m)]),
            (perpendicular(l, m), vec![eq::perpendicular(l, m)]),
            (
                collinear(l, m),
                vec![eq::parallel(l, m), eq::on_line(m.start(), l)],
            ),
            (tangent(l, c), vec![eq::line_tangent_circle(l, c)]),
            (tangent_circles(a, c), vec![eq::circles_tangent(a, c)]),
            (
                tangent_join(l, LineEnd::End, a, ArcEnd::Start),
                vec![
                    eq::coincide(l.end(), a.start()),
                    eq::line_tangent_arc(l, a, ArcEnd::Start),
                ],
            ),
            (
                tangent_arc_join(a, ArcEnd::End, b, ArcEnd::Start),
                vec![
                    eq::coincide(a.end(), b.start()),
                    eq::arcs_tangent(a, ArcEnd::End, b, ArcEnd::Start),
                ],
            ),
            (equal_length(l, m), vec![eq::equal_length(l, m)]),
            (equal_radius(a, c), vec![eq::equal_radius(a, c)]),
            (concentric(a, c), vec![eq::coincide(a.centre(), c.centre())]),
            (
                symmetric(p, a.centre(), l),
                vec![eq::mirror(p, a.centre(), l)],
            ),
            (midpoint(p, l), vec![eq::coincide(p, l.mid())]),
            (
                midpoint_of(p, l.start(), a.centre()),
                vec![eq::midpoint_of(p, l.start(), a.centre())],
            ),
            (
                equidistant(p, l, m, false),
                vec![eq::equidistant(p, l, m, false)],
            ),
            (
                on_bisector(p, l.start(), a.centre(), l.end()),
                vec![eq::on_bisector(p, l.start(), a.centre(), l.end())],
            ),
            (fixed(p), vec![eq::pinned(p, 1.0, 2.0)]),
            (fixed(a.end()), vec![eq::pinned(a.end(), 15.0, 5.0)]),
            (
                distance(p, l.end(), mm(3.0)),
                vec![eq::distance(p, l.end(), 3.0)],
            ),
            (
                horizontal_distance(p, l.end(), mm(3.0)),
                vec![eq::dx(p, l.end(), 3.0)],
            ),
            (
                vertical_distance(p, l.end(), mm(-3.0)),
                vec![eq::dy(p, l.end(), -3.0)],
            ),
            (length(l, mm(10.0)), vec![eq::length(l, 10.0)]),
            (
                point_line_distance(p, l, mm(2.0)),
                vec![eq::line_distance(p, l, 2.0)],
            ),
            (
                line_distance(l, m, mm(5.0)),
                vec![eq::line_distance(m.start(), l, 5.0)],
            ),
            (radius(a, mm(4.0)), vec![eq::radius(a, 4.0)]),
            (diameter(c, mm(4.0)), vec![eq::radius(c, 2.0)]),
            (
                angle(l, m, Measure::deg(90.0).unwrap()),
                vec![eq::angle(l, m, FRAC_PI_2)],
            ),
            (
                position(p, mm(3.0), mm(-4.0)),
                vec![eq::pinned(p, 3.0, -4.0)],
            ),
        ];
        assert_eq!(cases.len(), 34);
        for (r, expect) in cases {
            let want: Vec<Equation> = expect.into_iter().map(canon).collect();
            assert_eq!(lowered(&r), want, "{r:?}");
            r.check(&g()).unwrap();
        }
    }

    #[test]
    fn printed_and_parameter_values() {
        let a = arc("a");
        // Printed inches convert exactly.
        let r = radius(a, Measure::inches(".188").unwrap());
        assert_eq!(lowered(&r), vec![canon(eq::radius(a, 4.7752))]);
        // A parameter resolves through the sketch's table.
        let r = radius(a, Measure::param("r_bend".into()).unwrap());
        assert_eq!(r.params(), vec!["r_bend".into()]);
        let table = |n: ParamName| (n.as_str() == "r_bend").then(|| mm(4.0));
        assert_eq!(
            r.lower(&g(), &table).unwrap(),
            vec![canon(eq::radius(a, 4.0))]
        );
        assert_eq!(
            r.lower(&g(), &no_params),
            Err(Error::UnknownParam("r_bend".into()))
        );
        // A parameter of the wrong quantity is caught when resolved.
        let deg = |_: ParamName| Some(Measure::deg(45.0).unwrap());
        assert!(matches!(
            r.lower(&g(), &deg),
            Err(Error::Measure(measure::Error::WrongQuantity { .. }))
        ));
    }

    #[test]
    fn units_are_checked_against_the_kind() {
        let (l, m, a) = (line("l"), line("m"), arc("a"));
        let deg = Measure::deg(45.0).unwrap();
        assert!(matches!(
            radius(a, deg.clone()).check(&g()),
            Err(Error::Measure(measure::Error::WrongQuantity { .. }))
        ));
        assert!(matches!(
            angle(l, m, mm(1.0)).check(&g()),
            Err(Error::Measure(measure::Error::WrongQuantity { .. }))
        ));
        assert!(angle(l, m, deg).check(&g()).is_ok());
        assert_eq!(radius(a, mm(1.0)).quantity(), Some(Quantity::Length));
        assert_eq!(angle(l, m, mm(1.0)).quantity(), Some(Quantity::Angle));
        assert_eq!(parallel(l, m).quantity(), None);
    }

    #[test]
    fn driven_dimensions() {
        let l = line("l");
        let r = length(l, mm(10.0));
        assert!(r.driving());
        let r = r.driven().unwrap();
        assert!(!r.driving());
        // Still lowers: the sketch uses the equation to check the value.
        assert_eq!(lowered(&r), vec![canon(eq::length(l, 10.0))]);
        assert_eq!(horizontal(l).driven(), Err(Error::NotDimensional));
        assert!(horizontal(l).driving());
        let mut r = length(l, mm(10.0));
        assert_eq!(r.set_value(mm(12.0)), Ok(mm(10.0)));
        assert_eq!(r.values(), vec![&mm(12.0)]);
        assert_eq!(horizontal(l).set_value(mm(1.0)), Err(Error::NotDimensional));
    }

    #[test]
    fn position_is_a_two_valued_dimension() {
        let p = point("p");
        // A station read from the sheet, in inches.
        let r = position(
            p,
            Measure::inches("180.470").unwrap(),
            Measure::inches("27.750").unwrap(),
        );
        assert_eq!(r.values().len(), 2);
        assert_eq!(r.quantity(), Some(Quantity::Length));
        assert_eq!(
            lowered(&r),
            vec![canon(eq::pinned(p, 180.470 * 25.4, 27.750 * 25.4))]
        );
        // Either value may be a parameter.
        let mut r = position(p, Measure::param("x0".into()).unwrap(), mm(1.0));
        assert_eq!(r.params(), vec!["x0".into()]);
        let table = |n: ParamName| (n.as_str() == "x0").then(|| mm(2.0));
        assert_eq!(
            r.lower(&g(), &table).unwrap(),
            vec![canon(eq::pinned(p, 2.0, 1.0))]
        );
        // Values are replaced together, with the right arity.
        assert_eq!(
            r.set_values(vec![mm(5.0)]),
            Err(Error::WrongArity {
                expected: 2,
                found: 1
            })
        );
        assert!(r.set_value(mm(5.0)).is_err());
        let old = r.set_values(vec![mm(5.0), mm(6.0)]).unwrap();
        assert_eq!(old.len(), 2);
        assert_eq!(lowered(&r), vec![canon(eq::pinned(p, 5.0, 6.0))]);
        // `fixed` holds a point where it is; `position` says where it is.
        assert_eq!(fixed(p).values().len(), 0);
        assert!(fixed(p).driving());
        assert!(!position(p, mm(0.0), mm(0.0)).driven().unwrap().driving());
    }

    #[test]
    fn operands_and_checks() {
        let (l, a) = (line("l"), arc("a"));
        assert_eq!(
            tangent_join(l, LineEnd::End, a, ArcEnd::Start).operands(),
            vec![("l".into(), Kind::Line), ("a".into(), Kind::Arc)]
        );
        assert_eq!(fixed(a.end()).operands(), vec![("a".into(), Kind::Arc)]);
        // Missing and wrong-kind operands are refused.
        assert!(matches!(
            horizontal(line("nope")).check(&g()),
            Err(Error::Geometry(crate::value::geometry::Error::NoSuch(_)))
        ));
        assert!(matches!(
            horizontal(line("a")).check(&g()),
            Err(Error::Geometry(
                crate::value::geometry::Error::WrongKind { .. }
            ))
        ));
        // A self-relation surfaces from the equation layer when lowered.
        assert!(matches!(
            coincident(l.end(), l.end()).lower(&g(), &no_params),
            Err(Error::Equation(eq::Error::SameOperand))
        ));
    }

    #[test]
    fn one_fact_one_equation_set() {
        // Two spellings of one fact lower to the same canonical equations,
        // which is what lets the sketch refuse the second.
        let (a, c, l) = (arc("a"), circle("c"), line("l"));
        assert_eq!(
            lowered(&concentric(a, c)),
            lowered(&coincident(c.centre(), a.centre()))
        );
        assert_eq!(
            lowered(&horizontal(l)),
            lowered(&horizontal_points(l.end(), l.start()))
        );
        assert_eq!(
            lowered(&length(l, mm(10.0))),
            lowered(&distance(l.end(), l.start(), mm(10.0)))
        );
        let _ = (ArcPos::Mid, P2::<Model>::new(0.0, 0.0));
    }

    #[test]
    fn file_form() {
        let r = tangent_join(line("l"), LineEnd::End, arc("a"), ArcEnd::Start);
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.starts_with(r#"{"rel":"tangent_join""#));
        assert_eq!(serde_json::from_str::<Relation>(&s).unwrap(), r);
        let r = radius(arc("a"), Measure::inches(".188").unwrap())
            .driven()
            .unwrap();
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""driving":false"#));
        assert!(s.contains(r#""text":".188""#));
        assert_eq!(serde_json::from_str::<Relation>(&s).unwrap(), r);
    }
}
