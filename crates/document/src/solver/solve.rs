//! The constraint solver: moves a geometry's values until a set of equations
//! is satisfied, by Levenberg–Marquardt on the residuals with an exact
//! Jacobian from dual numbers.
//!
//! The solver never mutates in place. It takes a `Geometry` and equations
//! and returns a new geometry and a report: residual size, the rank of the
//! Jacobian at the solution, and from it the number of freedoms left
//! (parameters − rank) and of redundant equations (equations − rank). The
//! starting geometry chooses the branch wherever the equations admit several
//! solutions: the solver finds the one nearest to where it starts.
//!
//! Parameters are the entities' own values in `Geometry` order: a point
//! (x, y), a line (start, end), an arc (centre, radius, start angle, sweep),
//! a circle (centre, radius).
//!
//! # Example
//!
//! A clip: a top line, a bend and a leg, drawn roughly, then driven to a
//! 10 mm top with a 4 mm bend radius and tangent joins.
//!
//! (`tests::a_clip_driven_to_size`, at the end of this file.)

use super::equation::{ArcEnd, ArcRef, CircRef, Degeneracy, Equation, Error, Values};
use crate::value::geometry::{
    ArcPos, Entity, EntityId, Extreme, Frame, Geometry, Kind, LinePos, Model, PointAt, PointRef, P2,
};
use levenberg_marquardt::{LeastSquaresProblem, LevenbergMarquardt};
use nalgebra::{DMatrix, DVector, Dyn, Owned};
use num_dual::{DualDVec64, DualNum};
use std::collections::BTreeMap;

/// One of an entity's values, for naming a parameter in a report.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Value {
    /// A point's x.
    X,
    /// A point's y.
    Y,
    /// A line's start x.
    StartX,
    /// A line's start y.
    StartY,
    /// A line's end x.
    EndX,
    /// A line's end y.
    EndY,
    /// An arc's or circle's centre x.
    CentreX,
    /// An arc's or circle's centre y.
    CentreY,
    /// An arc's or circle's radius.
    Radius,
    /// An arc's start angle.
    StartAngle,
    /// An arc's sweep.
    Sweep,
    /// A spline point's x, by index.
    SplineX(usize),
    /// A spline point's y, by index.
    SplineY(usize),
}

/// Where each entity's values sit in the parameter vector.
pub(super) struct Layout {
    /// Each entity's offset, kind and number of values.
    pub(super) slots: BTreeMap<EntityId, (usize, Kind, usize)>,
    /// The entity and value at each parameter index.
    pub(super) names: Vec<(EntityId, Value)>,
    pub(super) n: usize,
}

impl Layout {
    pub(super) fn of<F: Frame>(g: &Geometry<F>) -> Self {
        use Value::*;
        let mut slots = BTreeMap::new();
        let mut names = Vec::new();
        for (id, e) in g.iter() {
            slots.insert(*id, (names.len(), e.kind(), e.values()));
            let vals: Vec<Value> = match e {
                Entity::Point { .. } => vec![X, Y],
                Entity::Line { .. } => vec![StartX, StartY, EndX, EndY],
                Entity::Arc { .. } => vec![CentreX, CentreY, Radius, StartAngle, Sweep],
                Entity::Circle { .. } => vec![CentreX, CentreY, Radius],
                Entity::Spline { points } => (0..points.len())
                    .flat_map(|i| [SplineX(i), SplineY(i)])
                    .collect(),
            };
            names.extend(vals.iter().map(|v| (*id, *v)));
        }
        let n = names.len();
        Layout { slots, names, n }
    }

    pub(super) fn flatten<F: Frame>(&self, g: &Geometry<F>) -> DVector<f64> {
        let mut x = DVector::zeros(self.n);
        for (id, e) in g.iter() {
            let o = self.slots[id].0;
            match e {
                Entity::Point { at } => {
                    x[o] = at.x;
                    x[o + 1] = at.y;
                }
                Entity::Line { start, end } => {
                    x[o] = start.x;
                    x[o + 1] = start.y;
                    x[o + 2] = end.x;
                    x[o + 3] = end.y;
                }
                Entity::Arc {
                    centre,
                    radius,
                    start_angle,
                    sweep,
                } => {
                    x[o] = centre.x;
                    x[o + 1] = centre.y;
                    x[o + 2] = *radius;
                    x[o + 3] = *start_angle;
                    x[o + 4] = *sweep;
                }
                Entity::Circle { centre, radius } => {
                    x[o] = centre.x;
                    x[o + 1] = centre.y;
                    x[o + 2] = *radius;
                }
                Entity::Spline { points } => {
                    for (i, p) in points.iter().enumerate() {
                        x[o + 2 * i] = p.x;
                        x[o + 2 * i + 1] = p.y;
                    }
                }
            }
        }
        x
    }

    /// A geometry with the same entities and these values. Fails if the
    /// values break an entity's invariants (a radius driven to zero, say).
    pub(super) fn unflatten<F: Frame>(
        &self,
        g: &Geometry<F>,
        x: &DVector<f64>,
    ) -> Result<Geometry<F>, Error> {
        let mut out = g.clone();
        for (id, _) in g.iter() {
            out.set(id, self.entity(id, x))?;
        }
        Ok(out)
    }

    /// Whether the values make every entity one a geometry accepts (no
    /// radius at or below zero, no line of no length, ...): the part of
    /// the solver's domain the entities set, besides the equations'.
    pub(super) fn admits(&self, x: &DVector<f64>) -> bool {
        self.slots
            .keys()
            .all(|id| self.entity::<Model>(id, x).canonical().is_ok())
    }

    /// The entity `id` with the values in `x`.
    fn entity<F: Frame>(&self, id: &EntityId, x: &DVector<f64>) -> Entity<F> {
        let (o, kind, n) = self.slots[id];
        let p = |i: usize| P2::new(x[o + i], x[o + i + 1]);
        match kind {
            Kind::Point => Entity::Point { at: p(0) },
            Kind::Line => Entity::Line {
                start: p(0),
                end: p(2),
            },
            Kind::Arc => Entity::Arc {
                centre: p(0),
                radius: x[o + 2],
                start_angle: x[o + 3],
                sweep: x[o + 4],
            },
            Kind::Circle => Entity::Circle {
                centre: p(0),
                radius: x[o + 2],
            },
            Kind::Spline => Entity::Spline {
                points: (0..n / 2).map(|i| p(2 * i)).collect(),
            },
        }
    }
}

/// The parameters seen through the layout, as entity values of any scalar
/// type: f64 for residuals, dual numbers for the Jacobian. `get` supplies
/// parameter `i`; a local Jacobian gives duals only for the parameters one
/// equation touches and plain values for the rest.
struct View<'a, T> {
    get: &'a dyn Fn(usize) -> T,
    layout: &'a Layout,
}

impl<T: DualNum<f64>> View<'_, T> {
    fn at(&self, id: &EntityId, kind: Kind) -> usize {
        let (o, k, _) = self.layout.slots[id];
        debug_assert_eq!(k, kind, "equations are checked before solving");
        o
    }

    /// A spline's offset and number of points.
    fn spline(&self, id: &EntityId) -> (usize, usize) {
        let (o, k, w) = self.layout.slots[id];
        debug_assert_eq!(k, Kind::Spline, "equations are checked before solving");
        (o, w / 2)
    }

    fn x(&self, i: usize) -> T {
        (self.get)(i)
    }

    /// The angle at a fraction `k` of an arc's sweep.
    fn arc_angle_at(&self, o: usize, k: f64) -> T {
        self.x(o + 3) + self.x(o + 4) * k
    }

    /// The centre (at `o`) plus the radius (at `o + 2`) along an axis
    /// direction: linear in the values, so exact and smooth.
    fn extreme(&self, o: usize, d: Extreme) -> [T; 2] {
        let [ux, uy] = d.unit();
        let r = self.x(o + 2);
        [self.x(o) + r.clone() * ux, self.x(o + 1) + r * uy]
    }

    fn arc_point(&self, o: usize, k: f64) -> [T; 2] {
        let a = self.arc_angle_at(o, k);
        let r = self.x(o + 2);
        [self.x(o) + a.cos() * &r, self.x(o + 1) + a.sin() * &r]
    }
}

impl<T: DualNum<f64>> Values<T> for View<'_, T> {
    fn point(&self, r: &PointRef) -> [T; 2] {
        match &r.0 {
            PointAt::Point { id } => {
                let o = self.at(id, Kind::Point);
                [self.x(o), self.x(o + 1)]
            }
            PointAt::Line { id, at } => {
                let o = self.at(id, Kind::Line);
                match at {
                    LinePos::Start => [self.x(o), self.x(o + 1)],
                    LinePos::End => [self.x(o + 2), self.x(o + 3)],
                    LinePos::Mid => [
                        (self.x(o) + &self.x(o + 2)) * 0.5,
                        (self.x(o + 1) + &self.x(o + 3)) * 0.5,
                    ],
                }
            }
            PointAt::Arc { id, at } => {
                let o = self.at(id, Kind::Arc);
                match at {
                    ArcPos::Centre => [self.x(o), self.x(o + 1)],
                    ArcPos::Start => self.arc_point(o, 0.0),
                    ArcPos::Mid => self.arc_point(o, 0.5),
                    ArcPos::End => self.arc_point(o, 1.0),
                    ArcPos::Extreme(d) => self.extreme(o, *d),
                }
            }
            PointAt::Circle { id, at } => {
                let o = self.at(id, Kind::Circle);
                match at {
                    None => [self.x(o), self.x(o + 1)],
                    Some(d) => self.extreme(o, *d),
                }
            }
            PointAt::Spline { id, at } => {
                let (o, len) = self.spline(id);
                let i = at
                    .index(len)
                    .expect("spline positions are checked before solving");
                [self.x(o + 2 * i), self.x(o + 2 * i + 1)]
            }
        }
    }

    fn radius(&self, c: &CircRef) -> T {
        self.x(self.at(&c.id(), c.kind()) + 2)
    }

    fn arc_angle(&self, arc: &ArcRef, at: ArcEnd) -> T {
        let o = self.at(&arc.id(), Kind::Arc);
        self.arc_angle_at(o, if at == ArcEnd::Start { 0.0 } else { 1.0 })
    }
}

#[cfg(test)]
impl Layout {
    /// Where a position is at `x`.
    pub(super) fn position(&self, x: &DVector<f64>, p: &PointRef) -> [f64; 2] {
        let get = |i: usize| x[i];
        View {
            get: &get,
            layout: self,
        }
        .point(p)
    }

    /// The index of a position's x (its y follows) if the position is
    /// one of its entity's values rather than computed from them.
    pub(super) fn slot(&self, p: &PointRef) -> Option<usize> {
        let (o, _, w) = self.slots[&p.id()];
        match p.0 {
            PointAt::Point { .. } | PointAt::Circle { at: None, .. } => Some(o),
            PointAt::Circle { at: Some(_), .. } => None,
            PointAt::Line { at, .. } => match at {
                LinePos::Start => Some(o),
                LinePos::End => Some(o + 2),
                LinePos::Mid => None,
            },
            PointAt::Arc { at, .. } => (at == ArcPos::Centre).then_some(o),
            PointAt::Spline { at, .. } => at.index(w / 2).map(|i| o + 2 * i),
        }
    }

    /// The index of a circle's or arc's radius.
    pub(super) fn radius_slot(&self, c: &CircRef) -> usize {
        self.slots[&c.id()].0 + 2
    }
}

/// An equation, by index, whose residuals have no derivative at the
/// current values, and why.
pub(super) type Singular = (usize, Degeneracy);

/// The residuals at `x`, and every equation that is not differentiable
/// there (its residuals are still given, NaN where undefined).
pub(super) fn evaluate<T: DualNum<f64>>(
    layout: &Layout,
    eqs: &[Equation],
    x: &DVector<T>,
) -> (DVector<T>, Vec<Singular>) {
    let get = |i: usize| x[i].clone();
    let v = View { get: &get, layout };
    let mut out = Vec::new();
    let mut singular = Vec::new();
    for (i, e) in eqs.iter().enumerate() {
        if let Err(d) = e.residuals(&v, &mut out) {
            singular.push((i, d));
        }
    }
    (DVector::from_vec(out), singular)
}

/// Which linear algebra does the solving and the analysis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    /// Our Levenberg–Marquardt on `faer`'s factorisations (blocked, SIMD),
    /// and `faer`'s column-pivoting QR for the analysis. The default.
    #[default]
    Faer,
    /// The `levenberg-marquardt` crate (MINPACK's lmder on nalgebra's QR)
    /// and nalgebra's column-pivoting QR for the analysis. The reference
    /// the default is tested against.
    Crate,
}

/// How the Jacobian is evaluated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Jacobian {
    /// Each equation on dual numbers over only the parameters it touches,
    /// scattered into the matrix: cost proportional to the equations'
    /// own size, not the sketch's. The default.
    #[default]
    Local,
    /// Every equation on dual numbers over all parameters at once. Simple,
    /// and the reference the local form is tested against.
    Dense,
}

/// Residuals and the exact Jacobian at `x`, every equation differentiated
/// against every parameter.
fn linearise_dense(
    layout: &Layout,
    eqs: &[Equation],
    x: &DVector<f64>,
) -> (DVector<f64>, DMatrix<f64>, Vec<Singular>) {
    let mut singular = Vec::new();
    let (r, j) = num_dual::jacobian(
        |xd: DVector<DualDVec64>| {
            let (r, s) = evaluate(layout, eqs, &xd);
            singular = s;
            r
        },
        x.clone(),
    );
    (r, j, singular)
}

/// The parameter indices an equation's value depends on: every value of
/// each operand entity, ascending.
pub(super) fn touched(layout: &Layout, e: &Equation) -> Vec<usize> {
    let points = e.point_refs();
    let mut out: Vec<usize> = e
        .operands()
        .iter()
        .flat_map(|(id, kind)| {
            let (o, _, w) = layout.slots[id];
            if *kind == Kind::Spline {
                // Only the points the equation names, not the whole curve.
                points
                    .iter()
                    .filter(|p| p.id() == *id)
                    .filter_map(|p| match p.0 {
                        PointAt::Spline { at, .. } => at.index(w / 2),
                        _ => None,
                    })
                    .flat_map(|i| [o + 2 * i, o + 2 * i + 1])
                    .collect::<Vec<_>>()
            } else {
                (o..o + w).collect()
            }
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The most parameters one equation can touch: four positions (two
/// vectors' ends) on entities of up to five values each (arcs). Local
/// duals carry this many derivatives on the stack, so no operation
/// allocates.
const LOCAL: usize = 20;

/// Residuals and the exact Jacobian at `x`, each equation differentiated
/// only against the parameters it touches, on fixed-width duals.
fn linearise_local(
    layout: &Layout,
    eqs: &[Equation],
    x: &DVector<f64>,
) -> (DVector<f64>, DMatrix<f64>, Vec<Singular>) {
    use nalgebra::{Const, U1};
    use num_dual::{Derivative, DualSVec64};
    let m: usize = eqs.iter().map(Equation::count).sum();
    let mut r = DVector::zeros(m);
    let mut j = DMatrix::zeros(m, layout.n);
    let mut singular = Vec::new();
    let mut res = Vec::with_capacity(2);
    let mut row = 0;
    for (i, e) in eqs.iter().enumerate() {
        let cols = touched(layout, e);
        assert!(
            cols.len() <= LOCAL,
            "an equation touching more than {LOCAL} parameters"
        );
        let get = |i: usize| match cols.iter().position(|&c| c == i) {
            Some(l) => DualSVec64::<LOCAL>::new(
                x[i],
                Derivative::derivative_generic(Const::<LOCAL>, U1, l),
            ),
            None => DualSVec64::<LOCAL>::from_re(x[i]),
        };
        let v = View { get: &get, layout };
        res.clear();
        if let Err(d) = e.residuals(&v, &mut res) {
            singular.push((i, d));
        }
        for d in &res {
            r[row] = d.re;
            let eps = d.eps.unwrap_generic(Const::<LOCAL>, U1);
            for (l, &c) in cols.iter().enumerate() {
                j[(row, c)] = eps[l];
            }
            row += 1;
        }
    }
    (r, j, singular)
}

/// Residuals and the exact Jacobian at `x`, and the first equation that is
/// not differentiable there. The rows of every such equation are zero:
/// it has no linearisation, so it takes no part in the analysis.
pub(super) fn linearise(
    layout: &Layout,
    eqs: &[Equation],
    x: &DVector<f64>,
    how: Jacobian,
) -> (DVector<f64>, DMatrix<f64>, Option<Singular>) {
    let (r, mut j, singular) = match how {
        Jacobian::Local => linearise_local(layout, eqs, x),
        Jacobian::Dense => linearise_dense(layout, eqs, x),
    };
    if !singular.is_empty() {
        let mut first = 0;
        let rows: Vec<usize> = eqs
            .iter()
            .map(|e| {
                first += e.count();
                first - e.count()
            })
            .collect();
        for &(i, _) in &singular {
            j.rows_mut(rows[i], eqs[i].count()).fill(0.0);
        }
    }
    (r, j, singular.first().copied())
}

struct Problem<'a> {
    layout: &'a Layout,
    eqs: &'a [Equation],
    x: DVector<f64>,
    jacobian: Jacobian,
}

impl Problem<'_> {
    /// The residuals at `x` if `x` is in the problem's domain: values
    /// the geometry accepts, every residual finite and differentiable.
    /// Outside it the least-squares objective is taken as +∞ (an
    /// extended-value function), so a step there is rejected like any
    /// step that makes things worse.
    fn residuals_in_domain(&self, x: &DVector<f64>) -> Option<DVector<f64>> {
        if !self.layout.admits(x) {
            return None;
        }
        let (r, singular) = evaluate(self.layout, self.eqs, x);
        (singular.is_empty() && r.iter().all(|v| v.is_finite())).then_some(r)
    }
}

impl super::lm::Residuals for Problem<'_> {
    fn residuals(&self, x: &DVector<f64>) -> Option<DVector<f64>> {
        self.residuals_in_domain(x)
    }

    fn residuals_and_jacobian(
        &self,
        x: &DVector<f64>,
    ) -> Result<(DVector<f64>, DMatrix<f64>), Singular> {
        match linearise(self.layout, self.eqs, x, self.jacobian) {
            (_, _, Some(s)) => Err(s),
            (r, j, None) => Ok((r, j)),
        }
    }
}

impl LeastSquaresProblem<f64, Dyn, Dyn> for Problem<'_> {
    type ResidualStorage = Owned<f64, Dyn>;
    type JacobianStorage = Owned<f64, Dyn, Dyn>;
    type ParameterStorage = Owned<f64, Dyn>;

    fn set_params(&mut self, x: &DVector<f64>) {
        self.x = x.clone();
    }

    fn params(&self) -> DVector<f64> {
        self.x.clone()
    }

    fn residuals(&self) -> Option<DVector<f64>> {
        self.residuals_in_domain(&self.x)
    }

    fn jacobian(&self) -> Option<DMatrix<f64>> {
        match linearise(self.layout, self.eqs, &self.x, self.jacobian) {
            (_, _, Some(_)) => None,
            (_, j, None) => Some(j),
        }
    }
}

/// Why the solver stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Termination {
    /// No equations, or no parameters: the geometry is returned as given.
    NothingToSolve,
    /// The residuals are exactly zero.
    ResidualsZero,
    /// The step or the objective stopped changing within tolerance, at a
    /// point where the residuals vanish or the gradient does (a least-squares
    /// minimum, where equations that conflict leave a residual).
    Converged,
    /// The step or the objective stopped changing, or no further improvement
    /// was possible at the tolerance asked for, but not at a solution or a
    /// stationary point: the residuals are not zero and could still fall.
    Stalled,
    /// The evaluation budget ran out before converging.
    OutOfPatience,
    /// An equation has no derivative at the values reached (at the start,
    /// or after the last step), so the solve cannot go on from there: two
    /// of its positions coincide, say a line of no length whose direction
    /// it needs. `equation` indexes the equations given.
    Degenerate {
        /// Which equation.
        equation: usize,
        /// What is degenerate.
        what: Degeneracy,
    },
    /// A numerical problem (NaN, infinity) or an internal failure.
    Failed(&'static str),
}

impl Termination {
    pub(super) fn degenerate((equation, what): Singular) -> Self {
        Termination::Degenerate { equation, what }
    }

    fn from_lm(t: levenberg_marquardt::TerminationReason) -> Self {
        use levenberg_marquardt::TerminationReason as R;
        match t {
            R::ResidualsZero => Termination::ResidualsZero,
            R::Converged { .. } | R::Orthogonal => Termination::Converged,
            R::NoImprovementPossible(_) => Termination::Stalled,
            R::LostPatience => Termination::OutOfPatience,
            R::NoParameters | R::NoResiduals => Termination::NothingToSolve,
            R::User(s) | R::Numerical(s) | R::WrongDimensions(s) => Termination::Failed(s),
        }
    }
}

/// A group of equations that are not independent at the solution: each
/// one follows from the others to first order. With zero residual the group
/// is redundant and any one member can be dropped; with a residual that is
/// not zero the members conflict and cannot all hold.
#[derive(Clone, Debug, PartialEq)]
pub struct Dependency {
    /// Indices into the equations given to `solve`, ascending.
    pub equations: Vec<usize>,
    /// The largest residual magnitude among the group, mm.
    pub residual: f64,
}

/// A direction the equations leave free: these values can move together
/// without breaking any equation (to first order).
#[derive(Clone, Debug, PartialEq)]
pub struct Freedom {
    /// The values that move, by entity.
    pub values: Vec<(EntityId, Value)>,
}

/// How one entity can move at the solution, independent of any choice of
/// basis for the freedoms: `free` is the dimension of its share of the
/// freedom space; `beyond_placement` whether that share exceeds what the
/// rigid motions of its body alone would give it; `values` the
/// coordinates that take part in the motion beyond placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Motion {
    /// The dimension of the entity's share of the freedom space.
    pub free: usize,
    /// Whether it can move other than with its body's placement.
    pub beyond_placement: bool,
    /// The coordinates moving beyond placement (empty if none).
    pub values: Vec<Value>,
}

/// What the figure is free to do as a whole. A sketch with no fiducial is
/// known only up to where it sits; that is placement, not under-constraint,
/// and is reported apart from the internal freedoms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Placement {
    /// Free to translate in x.
    pub x: bool,
    /// Free to translate in y.
    pub y: bool,
    /// Free to rotate (about some point).
    pub rotation: bool,
}

impl Placement {
    /// How many rigid motions are free.
    pub fn count(self) -> usize {
        self.x as usize + self.y as usize + self.rotation as usize
    }
}

/// What the solver found. Counts refer to the solution: `free` and
/// `redundant` come from the rank of the Jacobian there, and
/// `dependencies` and `freedoms` name the equations and values involved.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Report {
    /// Entity values the solver could move.
    pub parameters: usize,
    /// Residuals (an equation may contribute more than one).
    pub equations: usize,
    /// Rank of the Jacobian at the solution.
    pub rank: usize,
    /// Parameters the equations leave undetermined.
    pub free: usize,
    /// Equations that add nothing beyond the others (at the solution).
    pub redundant: usize,
    /// Root mean square of the residuals, mm.
    pub residual_rms: f64,
    /// Largest residual magnitude, mm.
    pub residual_max: f64,
    /// Residual and Jacobian evaluations used.
    pub evaluations: usize,
    /// Why the solver stopped.
    pub termination: Termination,
    /// The largest residual magnitude of each equation, mm, in the order
    /// the equations were given.
    pub residuals: Vec<f64>,
    /// Groups of equations that are not independent, redundant or in
    /// conflict according to their residual.
    pub dependencies: Vec<Dependency>,
    /// What the figure is free to do as a whole: a motion every body is
    /// free to make.
    pub placement: Placement,
    /// What each body is free to do on its own, in the order the bodies
    /// were given (one entry, the whole figure, for `solve`).
    pub bodies: Vec<Placement>,
    /// Freedoms other than placement: `free` less the dimension of the
    /// placement space (bodies tied together count once).
    pub internal_free: usize,
    /// The internal freedoms, one per undetermined direction, with the
    /// rigid motions projected out.
    pub freedoms: Vec<Freedom>,
    /// How each entity can move, independent of the basis chosen above.
    pub motion: BTreeMap<EntityId, Motion>,
}

/// The solved geometry and the report on it.
#[derive(Clone, Debug)]
#[must_use = "the solved geometry is returned, not applied"]
pub struct Solution<F: Frame> {
    /// The input geometry with its values moved to satisfy the equations.
    pub geometry: Geometry<F>,
    /// How well, and with what left over.
    pub report: Report,
}

/// Solver settings. Start from `Options::default()` and adjust.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub(super) patience: usize,
    pub(super) tol: f64,
    pub(super) rank_tol: f64,
    pub(super) jacobian: Jacobian,
    pub(super) backend: Backend,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            patience: 200,
            tol: 1e-14,
            rank_tol: 1e-9,
            jacobian: Jacobian::Local,
            backend: Backend::Faer,
        }
    }
}

impl Options {
    /// How the Jacobian is evaluated.
    pub fn with_jacobian(self, jacobian: Jacobian) -> Self {
        Self { jacobian, ..self }
    }

    /// Which linear algebra does the work.
    pub fn with_backend(self, backend: Backend) -> Self {
        Self { backend, ..self }
    }

    /// Residual and Jacobian evaluations allowed (at least 1).
    pub fn with_patience(self, patience: usize) -> Self {
        Self {
            patience: patience.max(1),
            ..self
        }
    }

    /// Relative tolerance on the objective and the step; must be positive
    /// and finite, else unchanged.
    pub fn with_tol(self, tol: f64) -> Self {
        if tol.is_finite() && tol > 0.0 {
            Self { tol, ..self }
        } else {
            self
        }
    }

    /// Singular values below this fraction of the largest count as zero in
    /// the rank analysis; must be in (0, 1), else unchanged.
    pub fn with_rank_tol(self, rank_tol: f64) -> Self {
        if rank_tol.is_finite() && rank_tol > 0.0 && rank_tol < 1.0 {
            Self { rank_tol, ..self }
        } else {
            self
        }
    }
}

/// Rank of `a` and a basis of its null space (vectors `v` with `a v = 0`),
/// as the columns of an `n × (n − rank)` matrix, from a column-pivoting QR.
/// Diagonal entries of R below `tol` times the largest count as zero.
pub(crate) fn null_space_nalgebra(a: &DMatrix<f64>, tol: f64) -> (usize, DMatrix<f64>) {
    let n = a.ncols();
    if a.nrows() == 0 || n == 0 {
        return (0, DMatrix::identity(n, n));
    }
    let qr = a.clone().col_piv_qr();
    let r = qr.r();
    let lead = r[(0, 0)].abs();
    let rank = if lead <= 0.0 {
        0
    } else {
        (0..r.nrows().min(n))
            .take_while(|&i| r[(i, i)].abs() > tol * lead)
            .count()
    };
    // In the pivoted order, R = [R11 R12; 0 0]; each dependent column j
    // gives the null vector [−R11⁻¹ R12[:, j]; e_j].
    let mut basis = DMatrix::zeros(n, n - rank);
    for (k, j) in (rank..n).enumerate() {
        if rank > 0 {
            let r11 = r.view((0, 0), (rank, rank));
            let r12 = r.view((0, j), (rank, 1));
            let z = r11
                .solve_upper_triangular(&r12)
                .expect("R11 is nonsingular by construction");
            for i in 0..rank {
                basis[(i, k)] = -z[i];
            }
        }
        basis[(j, k)] = 1.0;
    }
    qr.p().inv_permute_rows(&mut basis);
    // What was dropped is R22, whose columns (by the pivoting) are no
    // longer than tol · lead: A v is within √n · tol · lead · |v| of zero.
    debug_assert!({
        let bound = (n as f64).sqrt() * tol * lead.max(1.0);
        let av = a * &basis;
        (0..basis.ncols()).all(|k| av.column(k).amax() <= bound * basis.column(k).norm())
    });
    (rank, basis)
}

/// The unique basis of the space the columns of `basis` span: its
/// reduced row echelon form (on the transposed basis, with partial
/// pivoting and a relative zero tolerance). Any backend's null-space
/// basis then reports the same freedoms, and each vector isolates a
/// pivot parameter, which reads better.
fn canonical_basis(basis: &DMatrix<f64>, tol: f64) -> DMatrix<f64> {
    let (n, k) = basis.shape();
    if k == 0 {
        return basis.clone();
    }
    let mut m = basis.transpose(); // k × n: one freedom per row
    let scale = m.iter().fold(0.0, |a: f64, v| a.max(v.abs())).max(1e-300);
    let mut row = 0;
    for col in 0..n {
        if row >= k {
            break;
        }
        // Partial pivoting within the column.
        let (pivot, best) = (row..k)
            .map(|i| (i, m[(i, col)].abs()))
            .fold((row, 0.0), |a, b| if b.1 > a.1 { b } else { a });
        if best <= tol * scale {
            continue;
        }
        m.swap_rows(row, pivot);
        let p = m[(row, col)];
        for j in 0..n {
            m[(row, j)] /= p;
        }
        for i in 0..k {
            if i != row {
                let f = m[(i, col)];
                if f != 0.0 {
                    for j in 0..n {
                        m[(i, j)] -= f * m[(row, j)];
                    }
                }
            }
        }
        row += 1;
    }
    // Rows beyond `row` are numerically zero: drop them.
    m.rows(0, row).transpose().into_owned()
}

/// Rank and null space on the chosen backend.
fn null_space(a: &DMatrix<f64>, tol: f64, backend: Backend) -> (usize, DMatrix<f64>) {
    match backend {
        Backend::Faer => super::faer_backend::null_space(a, tol),
        Backend::Crate => null_space_nalgebra(a, tol),
    }
}

/// Indices with a component that is not negligible against the largest.
fn support(v: nalgebra::DVectorView<f64>) -> Vec<usize> {
    let max = v.iter().fold(0.0, |a: f64, x| a.max(x.abs()));
    v.iter()
        .enumerate()
        .filter(|(_, x)| x.abs() > 1e-8 * max)
        .map(|(i, _)| i)
        .collect()
}

/// Each entity's share of the freedom space `right` against the free
/// rigid motions of its body: a subspace comparison, so whether it moves
/// beyond placement does not depend on which basis of the freedoms was
/// chosen. The coordinates taking part are read off the canonical
/// internal basis, which is unique.
fn motions(
    layout: &Layout,
    x: &DVector<f64>,
    right: &DMatrix<f64>,
    internal: &DMatrix<f64>,
    bodies: &[Vec<EntityId>],
    flags: &[Placement],
    tol: f64,
) -> BTreeMap<EntityId, Motion> {
    let mut out = BTreeMap::new();
    for (body, flag) in bodies.iter().zip(flags) {
        let [tx, ty, rot] = rigid_generators(layout, x, body);
        let gens: Vec<DVector<f64>> = [(flag.x, tx), (flag.y, ty), (flag.rotation, rot)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, v)| v)
            .collect();
        for id in body {
            let Some(&(start, _, w)) = layout.slots.get(id) else {
                continue;
            };
            let a = right.rows(start, w).clone_owned();
            let mut g = DMatrix::zeros(w, gens.len());
            for (c, v) in gens.iter().enumerate() {
                g.set_column(c, &v.rows(start, w));
            }
            let free = orthonormal(&a, tol).ncols();
            let rank_g = orthonormal(&g, tol).ncols();
            let mut both = DMatrix::zeros(w, a.ncols() + g.ncols());
            both.view_mut((0, 0), (w, a.ncols())).copy_from(&a);
            both.view_mut((0, a.ncols()), (w, g.ncols())).copy_from(&g);
            let beyond_placement = orthonormal(&both, tol).ncols() > rank_g;
            let values = if beyond_placement {
                let rows = internal.rows(start, w);
                let max = rows.iter().fold(0.0, |m: f64, v| m.max(v.abs()));
                (0..w)
                    .filter(|&r| rows.row(r).norm() > 1e-8 * max)
                    .map(|r| layout.names[start + r].1)
                    .collect()
            } else {
                Vec::new()
            };
            out.insert(
                *id,
                Motion {
                    free,
                    beyond_placement,
                    values,
                },
            );
        }
    }
    out
}

/// The rigid-motion generators of one body at the state `x`: translate
/// x, translate y, rotate about the centroid of the body's coordinates,
/// each a direction in parameter space that moves the body's entities
/// only. The centre of the turn changes nothing about the span with the
/// translations, but a turn about a far origin is a huge vector beside
/// them and the placement test, relative to the largest, would then
/// depend on where the figure sits.
fn rigid_generators(layout: &Layout, x: &DVector<f64>, body: &[EntityId]) -> [DVector<f64>; 3] {
    use Value::*;
    let n = layout.n;
    let (mut tx, mut ty, mut rot) = (DVector::zeros(n), DVector::zeros(n), DVector::zeros(n));
    let (mut sum, mut count) = ([0.0f64; 2], 0.0f64);
    for (i, (id, v)) in layout.names.iter().enumerate() {
        if let (true, X | StartX | EndX | CentreX | SplineX(_)) = (body.contains(id), v) {
            sum = [sum[0] + x[i], sum[1] + x[i + 1]];
            count += 1.0;
        }
    }
    let centre = [sum[0] / count.max(1.0), sum[1] / count.max(1.0)];
    for (i, (id, v)) in layout.names.iter().enumerate() {
        if !body.contains(id) {
            continue;
        }
        match v {
            X | StartX | EndX | CentreX | SplineX(_) => {
                tx[i] = 1.0;
                // Rotating (x, y) by a small angle moves x by −y.
                rot[i] = -(x[i + 1] - centre[1]);
            }
            Y | StartY | EndY | CentreY | SplineY(_) => {
                ty[i] = 1.0;
                rot[i] = x[i - 1] - centre[0];
            }
            StartAngle => rot[i] = 1.0,
            Radius | Sweep => {}
        }
    }
    [tx, ty, rot]
}

/// The SVD of `a` (U always, V if asked), or `None` if `a` is not finite
/// or the iteration does not converge. nalgebra's plain `svd` iterates
/// until convergence with no bound, and on a NaN it never converges.
pub(super) fn bounded_svd(a: &DMatrix<f64>, v: bool) -> Option<nalgebra::SVD<f64, Dyn, Dyn>> {
    if !a.iter().all(|x| x.is_finite()) {
        return None;
    }
    // Golub–Kahan needs a few sweeps per singular value; this is ample.
    let max_niter = 100 * a.nrows().max(a.ncols()).max(1);
    a.clone().try_svd(true, v, f64::EPSILON, max_niter)
}

/// Whether `v` lies in the column space of `basis` (relative tolerance):
/// its distance from an orthonormal basis of that space. The columns may
/// be far from orthogonal (a freedom basis from a pivoted QR is), and a
/// least-squares solve on them directly would decide by their
/// conditioning rather than by the space.
fn in_span(basis: &DMatrix<f64>, v: &DVector<f64>, tol: f64) -> bool {
    let norm = v.norm();
    if norm == 0.0 {
        return true;
    }
    let q = orthonormal(basis, tol);
    if q.ncols() == 0 {
        return false;
    }
    (v - &q * (q.transpose() * v)).norm() <= tol.sqrt() * norm
}

/// An orthonormal basis of the column space of `a`.
fn orthonormal(a: &DMatrix<f64>, tol: f64) -> DMatrix<f64> {
    if a.ncols() == 0 {
        return a.clone();
    }
    let Some(svd) = bounded_svd(a, false) else {
        return DMatrix::zeros(a.nrows(), 0);
    };
    let u = svd.u.expect("requested");
    let max = svd.singular_values.max();
    let keep: Vec<usize> = (0..svd.singular_values.len())
        .filter(|&i| svd.singular_values[i] > tol * max.max(1.0))
        .collect();
    let mut q = DMatrix::zeros(a.nrows(), keep.len());
    for (k, &i) in keep.iter().enumerate() {
        q.set_column(k, &u.column(i));
    }
    q
}

/// An orthonormal basis of the `k` dominant directions of the column
/// space of `a`: its first `k` left singular vectors.
fn dominant(a: &DMatrix<f64>, k: usize) -> DMatrix<f64> {
    let svd = match k {
        0 => None,
        _ => bounded_svd(a, false),
    };
    let Some(svd) = svd else {
        return DMatrix::zeros(a.nrows(), 0);
    };
    let u = svd.u.expect("requested");
    let s = &svd.singular_values;
    let mut order: Vec<usize> = (0..s.len()).collect();
    order.sort_by(|&i, &j| s[j].total_cmp(&s[i]));
    let mut q = DMatrix::zeros(a.nrows(), k.min(s.len()));
    for (c, &i) in order.iter().take(k).enumerate() {
        q.set_column(c, &u.column(i));
    }
    q
}

/// Split the freedom basis into placement and internal freedoms.
///
/// Placement is the part of the freedom space that is a rigid motion of
/// bodies: the intersection of the span of the freedom basis with the
/// span of every body's translation and rotation generators. Bodies that
/// are tied together (two views linked along an axis) can move as one,
/// which counts once. Each body's flags say whether it can translate or
/// turn, alone or together with others. The internal freedoms are the
/// basis with the placement directions projected out.
fn placement(
    layout: &Layout,
    x: &DVector<f64>,
    basis: &DMatrix<f64>,
    bodies: &[Vec<EntityId>],
    tol: f64,
) -> (Vec<Placement>, usize, DMatrix<f64>) {
    let n = layout.n;
    let none = || (vec![Placement::default(); bodies.len()], 0, basis.clone());
    if basis.ncols() == 0 || n == 0 || bodies.is_empty() {
        return none();
    }
    let gens: Vec<[DVector<f64>; 3]> = bodies
        .iter()
        .map(|b| rigid_generators(layout, x, b))
        .collect();
    let translations: Vec<DVector<f64>> = gens
        .iter()
        .flat_map(|[tx, ty, _]| [tx.clone(), ty.clone()])
        .collect();
    let all: Vec<DVector<f64>> = gens
        .iter()
        .flat_map(|[tx, ty, rot]| [tx.clone(), ty.clone(), rot.clone()])
        .collect();
    let with = |extra: &[&DVector<f64>]| {
        let mut m = DMatrix::zeros(n, basis.ncols() + extra.len());
        m.view_mut((0, 0), (n, basis.ncols())).copy_from(basis);
        for (k, v) in extra.iter().enumerate() {
            m.set_column(basis.ncols() + k, v);
        }
        m
    };
    // Per body: translation alone or with other bodies' translations;
    // rotation about any point, so with every translation.
    let mut flags = Vec::with_capacity(bodies.len());
    for (k, [tx, ty, rot]) in gens.iter().enumerate() {
        let others: Vec<&DVector<f64>> = translations
            .iter()
            .enumerate()
            .filter(|(i, _)| i / 2 != k)
            .map(|(_, v)| v)
            .collect();
        let m = with(&others);
        let px = in_span(&m, tx, tol);
        let py = in_span(&m, ty, tol);
        let every: Vec<&DVector<f64>> = translations.iter().collect();
        let pr = in_span(&with(&every), rot, tol);
        flags.push(Placement {
            x: px,
            y: py,
            rotation: pr,
        });
    }
    // The placement space: combinations of generators that lie in the
    // freedom space. With Q an orthonormal basis of the freedoms, those
    // are the null vectors of (I − QQᵀ)R.
    let q = orthonormal(basis, tol);
    let r = DMatrix::from_columns(&all);
    let residual = &r - &q * (q.transpose() * &r);
    let (_, combos) = null_space_nalgebra(&residual, tol.sqrt());
    let placement_space = orthonormal(&(&r * &combos), tol);
    let count = placement_space.ncols();
    if count == 0 {
        return (flags, 0, basis.clone());
    }
    // The projection takes exactly `count` directions out; what rounding
    // leaves of them is the smallest part of the rest, and is dropped.
    let projected = basis - &placement_space * (placement_space.transpose() * basis);
    let internal = dominant(&projected, basis.ncols().saturating_sub(count));
    (flags, count, internal)
}

/// The residuals of `eqs` at `g`, signed, per equation (an equation may
/// have more than one). Nothing is solved.
pub fn residuals<F: Frame>(g: &Geometry<F>, eqs: &[Equation]) -> Result<Vec<Vec<f64>>, Error> {
    for e in eqs {
        e.check(g)?;
    }
    let layout = Layout::of(g);
    let x = layout.flatten(g);
    let (r, _) = evaluate(&layout, eqs, &x);
    let mut out = Vec::with_capacity(eqs.len());
    let mut at = 0;
    for e in eqs {
        out.push(r.as_slice()[at..at + e.count()].to_vec());
        at += e.count();
    }
    Ok(out)
}

/// The report for `eqs` at the state `g` is in, without solving: rank,
/// freedoms, placement, dependencies and residuals as they stand.
pub fn analyse<F: Frame>(
    g: &Geometry<F>,
    eqs: &[Equation],
    o: Options,
) -> Result<Solution<F>, Error> {
    let all: Vec<EntityId> = g.iter().map(|(id, _)| *id).collect();
    analyse_bodies(g, eqs, &[all], o)
}

/// [`analyse`] with the entities grouped into bodies whose placement is
/// judged separately (one per view of a system).
pub fn analyse_bodies<F: Frame>(
    g: &Geometry<F>,
    eqs: &[Equation],
    bodies: &[Vec<EntityId>],
    o: Options,
) -> Result<Solution<F>, Error> {
    for e in eqs {
        e.check(g)?;
    }
    let layout = Layout::of(g);
    let x = layout.flatten(g);
    report_at(
        g,
        &layout,
        eqs,
        bodies,
        x,
        o,
        0,
        Termination::NothingToSolve,
    )
}

/// Solve `eqs` on `g` and return the result as a new geometry; `g` is not
/// changed. Every equation is checked against `g` first (operands exist and
/// are of the stated kind). Where the equations admit several solutions the
/// one nearest the starting values is found. See the module documentation
/// for an example.
pub fn solve<F: Frame>(
    g: &Geometry<F>,
    eqs: &[Equation],
    o: Options,
) -> Result<Solution<F>, Error> {
    let all: Vec<EntityId> = g.iter().map(|(id, _)| *id).collect();
    solve_bodies(g, eqs, &[all], o)
}

/// [`solve`] with the entities grouped into bodies whose placement is
/// judged separately (one per view of a system). `Report::bodies` then
/// has one entry per body, in the order given.
pub fn solve_bodies<F: Frame>(
    g: &Geometry<F>,
    eqs: &[Equation],
    bodies: &[Vec<EntityId>],
    o: Options,
) -> Result<Solution<F>, Error> {
    for e in eqs {
        e.check(g)?;
    }
    let layout = Layout::of(g);
    let x0 = layout.flatten(g);
    let m: usize = eqs.iter().map(Equation::count).sum();
    let (x, evaluations, termination) = if m == 0 || layout.n == 0 {
        (x0, 0, Termination::NothingToSolve)
    } else {
        let problem = Problem {
            layout: &layout,
            eqs,
            x: x0,
            jacobian: o.jacobian,
        };
        match o.backend {
            Backend::Faer => {
                let settings = super::lm::Settings {
                    patience: o.patience,
                    ftol: o.tol,
                    xtol: o.tol,
                    gtol: 0.0,
                };
                super::lm::minimize(&problem, problem.x.clone(), &settings)
            }
            // The crate stops at a step outside the domain (its
            // `User("residuals")`); a start outside it is reported here.
            Backend::Crate => match linearise(&layout, eqs, &problem.x, o.jacobian).2 {
                Some(s) => (problem.x, 0, Termination::degenerate(s)),
                None => {
                    let (problem, rep) = LevenbergMarquardt::new()
                        .with_patience(o.patience)
                        .with_tol(o.tol)
                        .minimize(problem);
                    (
                        problem.x,
                        rep.number_of_evaluations,
                        Termination::from_lm(rep.termination),
                    )
                }
            },
        }
    };
    report_at(g, &layout, eqs, bodies, x, o, evaluations, termination)
}

/// The geometry at `x` and the full report there.
#[allow(clippy::too_many_arguments)]
pub(super) fn report_at<F: Frame>(
    g: &Geometry<F>,
    layout: &Layout,
    eqs: &[Equation],
    bodies: &[Vec<EntityId>],
    x: DVector<f64>,
    o: Options,
    evaluations: usize,
    termination: Termination,
) -> Result<Solution<F>, Error> {
    let m: usize = eqs.iter().map(Equation::count).sum();
    let geometry = layout.unflatten(g, &x)?;
    let (r, mut j, singular) = linearise(layout, eqs, &x, o.jacobian);
    // The analysis below is linear algebra on `j`, which must be finite:
    // a NaN spreads through the QR into the freedom basis, and nalgebra's
    // SVD never converges on a NaN. A degenerate equation's rows are
    // already zero; a derivative that overflowed is read as zero, and the
    // report says the analysis failed.
    let mut termination = termination;
    if let Some(s) = singular {
        termination = Termination::degenerate(s);
    } else if !r.iter().chain(j.iter()).all(|v| v.is_finite()) {
        j.iter_mut()
            .filter(|v| !v.is_finite())
            .for_each(|v| *v = 0.0);
        termination = Termination::Failed(super::lm::NOT_FINITE);
    }
    // Which residual row belongs to which equation.
    let row_eq: Vec<usize> = eqs
        .iter()
        .enumerate()
        .flat_map(|(i, e)| std::iter::repeat_n(i, e.count()))
        .collect();
    let residuals: Vec<f64> = eqs
        .iter()
        .enumerate()
        .map(|(i, _)| {
            r.iter()
                .zip(&row_eq)
                .filter(|(_, &e)| e == i)
                .fold(0.0, |a: f64, (v, _)| a.max(v.abs()))
        })
        .collect();
    let (rank, right) = null_space(&j, o.rank_tol, o.backend);
    let (_, left) = null_space(&j.transpose(), o.rank_tol, o.backend);
    let mut dependencies: Vec<Dependency> = Vec::new();
    for k in 0..left.ncols() {
        let mut equations: Vec<usize> = support(left.column(k))
            .into_iter()
            .map(|i| row_eq[i])
            .collect();
        equations.sort_unstable();
        equations.dedup();
        let residual = equations.iter().fold(0.0, |a: f64, &e| a.max(residuals[e]));
        let d = Dependency {
            equations,
            residual,
        };
        if !dependencies.contains(&d) {
            dependencies.push(d);
        }
    }
    let (flags, placement_count, internal) = placement(layout, &x, &right, bodies, o.rank_tol);
    let internal = canonical_basis(&internal, o.rank_tol);
    let motion = motions(layout, &x, &right, &internal, bodies, &flags, o.rank_tol);
    let bodies = flags;
    // The whole is free to move only if every body is.
    let placement = Placement {
        x: !bodies.is_empty() && bodies.iter().all(|b| b.x),
        y: !bodies.is_empty() && bodies.iter().all(|b| b.y),
        rotation: !bodies.is_empty() && bodies.iter().all(|b| b.rotation),
    };
    let freedoms = (0..internal.ncols())
        .map(|k| Freedom {
            values: support(internal.column(k))
                .into_iter()
                .map(|i| layout.names[i])
                .collect(),
        })
        .collect();
    let report = Report {
        parameters: layout.n,
        equations: m,
        rank,
        free: layout.n - rank,
        redundant: m - rank,
        placement,
        bodies,
        internal_free: layout.n - rank - placement_count,
        residual_rms: if m == 0 {
            0.0
        } else {
            (r.norm_squared() / m as f64).sqrt()
        },
        residual_max: r.iter().fold(0.0, |a: f64, v| a.max(v.abs())),
        evaluations,
        termination,
        residuals,
        dependencies,
        freedoms,
        motion,
    };
    Ok(Solution { geometry, report })
}

#[cfg(test)]
mod tests {
    use super::super::equation::*;
    use super::*;
    use crate::geometry;
    use crate::value::geometry::Model;
    use approx::assert_abs_diff_eq;
    use proptest::prelude::*;
    use std::f64::consts::FRAC_PI_2;

    /// One of everything, in general position (nothing axis-aligned).
    fn zoo() -> Geometry<Model> {
        geometry!(Model {
            p: point (1.3, -0.7);
            l: line (0.2, 0.5) -> (4.1, 2.9);
            m: line (-1.0, 3.0) -> (2.5, -2.2);
            a: arc (3.0, 1.0), r = 1.7, from = 0.4, sweep = 1.9;
            b: arc (-1.0, 2.0), r = 1.1, from = -0.8, sweep = -2.3;
            c: circle (-2.0, -1.5), r = 0.9;
        })
    }

    /// One of every equation kind, over the zoo.
    fn every_equation() -> Vec<Equation> {
        let (p, l, m, a, b, c) = (
            point("p"),
            line("l"),
            line("m"),
            arc("a"),
            arc("b"),
            circle("c"),
        );
        vec![
            coincide(p, a.mid()),
            on_line(a.end(), l),
            on_circle(m.mid(), a),
            same_x(p, l.end()),
            same_y(a.start(), m.start()),
            same(p, Axis::X, b.mid(), Axis::Y),
            parallel(l, m),
            perpendicular(l, m),
            line_tangent_circle(m, c),
            circles_tangent(a, c),
            line_tangent_arc(l, a, ArcEnd::End),
            arcs_tangent(a, ArcEnd::Start, b, ArcEnd::End),
            equal_length(l, m),
            equal_radius(a, c),
            distance(p, a.centre(), 2.0),
            dx(p, l.start(), 0.3),
            dy(l.start(), m.end(), -0.4),
            line_distance(a.centre(), l, 1.0),
            radius(a, 1.5),
            angle(l, m, 0.3),
            mirror(p, c.centre(), l),
            midpoint_of(p, m.end(), b.centre()),
            equidistant(a.centre(), l, m, true),
            on_bisector(p, l.end(), m.start(), c.centre()),
            pinned(a.start(), 1.0, 2.0),
        ]
    }

    #[test]
    fn jacobian_matches_finite_differences() {
        let g = zoo();
        let eqs = every_equation();
        for e in &eqs {
            e.check(&g).unwrap();
        }
        let layout = Layout::of(&g);
        let x = layout.flatten(&g);
        let (r, j, _) = linearise(&layout, &eqs, &x, Jacobian::Dense);
        // The local evaluation gives the same matrix exactly.
        let (r2, j2, _) = linearise(&layout, &eqs, &x, Jacobian::Local);
        assert_eq!(r, r2);
        assert_eq!(j, j2);
        let m: usize = eqs.iter().map(Equation::count).sum();
        assert_eq!(r.len(), m);
        assert_eq!(j.shape(), (m, layout.n));
        let h = 1e-6;
        for col in 0..layout.n {
            let mut xp = x.clone();
            xp[col] += h;
            let mut xm = x.clone();
            xm[col] -= h;
            let fd = (evaluate(&layout, &eqs, &xp).0 - evaluate(&layout, &eqs, &xm).0) / (2.0 * h);
            for row in 0..m {
                assert!(
                    (j[(row, col)] - fd[row]).abs() < 1e-6,
                    "d r[{row}] / d x[{col}]: dual {} vs fd {}",
                    j[(row, col)],
                    fd[row]
                );
            }
        }
    }

    #[test]
    fn freedoms_and_redundancy() {
        let g = geometry!(Model {
            l: line (0.0, 0.0) -> (10.0, 1.0);
            m: line (0.0, 5.0) -> (10.0, 4.0);
        });
        let s = solve(&g, &[], Options::default()).unwrap();
        assert_eq!((s.report.free, s.report.redundant), (8, 0));
        // Horizontal, horizontal, parallel: one of the three is redundant.
        let (l, m) = (line("l"), line("m"));
        let eqs = [
            same_y(l.start(), l.end()),
            same_y(m.start(), m.end()),
            parallel(l, m),
        ];
        let s = solve(&g, &eqs, Options::default()).unwrap();
        assert_eq!(
            (s.report.rank, s.report.free, s.report.redundant),
            (2, 6, 1)
        );
        assert!(s.report.residual_max < 1e-9);
        if let Entity::Line { start, end } = s.geometry.get("l").unwrap() {
            assert_abs_diff_eq!(start.y, end.y);
        }
    }

    #[test]
    fn clip_from_dimensions() {
        // A line, a bend and a leg, built loosely, then driven to a 4 mm
        // bend radius with tangent joins and a 10 mm top.
        let g = geometry!(Model {
            top: line (0.1, 0.2) -> (9.0, -0.3);
            bend: arc (9.5, 5.0), r = 5.0, from = -FRAC_PI_2 - 0.1, sweep = FRAC_PI_2 + 0.2;
            leg: line (15.0, 5.5) -> (14.0, 20.0);
        });
        let (top, bend, leg) = (line("top"), arc("bend"), line("leg"));
        let eqs = [
            pinned(top.start(), 0.0, 0.0),
            same_y(top.start(), top.end()),
            dx(top.start(), top.end(), 10.0),
            coincide(top.end(), bend.start()),
            line_tangent_arc(top, bend, ArcEnd::Start),
            radius(bend, 4.0),
            coincide(bend.end(), leg.start()),
            line_tangent_arc(leg, bend, ArcEnd::End),
        ];
        let s = solve(&g, &eqs, Options::default()).unwrap();
        assert!(s.report.residual_max < 1e-9, "{:?}", s.report);
        // 13 parameters, 11 independent equations: the sweep and the leg's
        // length stay free.
        assert_eq!(
            (s.report.rank, s.report.free, s.report.redundant),
            (11, 2, 0)
        );
        match s.geometry.get("bend").unwrap() {
            Entity::Arc {
                centre,
                radius,
                start_angle,
                ..
            } => {
                assert_abs_diff_eq!(*radius, 4.0, epsilon = 1e-8);
                assert_abs_diff_eq!(centre.x, 10.0, epsilon = 1e-8);
                assert_abs_diff_eq!(centre.y, 4.0, epsilon = 1e-8);
                assert!((start_angle + FRAC_PI_2).abs() < 1e-8);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn tangent_branch_follows_the_state() {
        let mut g = geometry!(Model {
            c1: circle (0.0, 0.0), r = 2.0;
            c2: circle (5.0, 0.0), r = 1.0;
        });
        let (c1, c2) = (circle("c1"), circle("c2"));
        let eqs = [
            pinned(c1.centre(), 0.0, 0.0),
            pinned(c2.centre(), 5.0, 0.0),
            radius(c1, 2.0),
            circles_tangent(c1, c2),
        ];
        // Outer branch: the circles are apart, so c2 grows to meet c1.
        let s = solve(&g, &eqs, Options::default()).unwrap();
        let r2 = |s: &Solution<Model>| match s.geometry.get("c2").unwrap() {
            Entity::Circle { radius, .. } => *radius,
            _ => unreachable!(),
        };
        assert!((r2(&s) - 3.0).abs() < 1e-9);
        // Inner branch: start with c2 inside c1 and large enough to be nearer
        // internal tangency.
        g.set(
            "c2",
            Entity::Circle {
                centre: P2::new(5.0, 0.0),
                radius: 6.5,
            },
        )
        .unwrap();
        let s = solve(&g, &eqs, Options::default()).unwrap();
        assert!((r2(&s) - 7.0).abs() < 1e-9, "{}", r2(&s));
    }
    #[test]
    fn a_point_is_held_midway() {
        let g = geometry!(Model {
            p: point (3.0, 4.0);
            q: point (0.0, 0.0);
            r: point (8.0, 2.0);
            l: line (0.0, 0.0) -> (10.0, 0.0);
            m: line (10.0, 6.0) -> (0.0, 6.0);
            s: point (7.0, 1.0);
            t: point (5.0, 0.5);
        });
        let (p, q, r, l, m, s, t) = (
            point("p"),
            point("q"),
            point("r"),
            line("l"),
            line("m"),
            point("s"),
            point("t"),
        );
        let eqs = [
            pinned(q, 0.0, 0.0),
            pinned(r, 8.0, 2.0),
            midpoint_of(p, q, r),
            // The lines run opposite ways: the point still lands between.
            equidistant(s, l, m, false),
            same_x(s, q),
            // On the bisector of q and r, level with q: one row, linear
            // in the positions.
            on_bisector(t, t, q, r),
            same_y(t, q),
        ];
        let sol = solve(&g, &eqs, Options::default()).unwrap();
        let at = |id: &str| match sol.geometry.get(id).unwrap() {
            Entity::Point { at } => (at.x, at.y),
            _ => unreachable!(),
        };
        let (px, py) = at("p");
        assert_abs_diff_eq!(px, 4.0, epsilon = 1e-9);
        assert_abs_diff_eq!(py, 1.0, epsilon = 1e-9);
        let (sx, sy) = at("s");
        assert_abs_diff_eq!(sx, 0.0, epsilon = 1e-9);
        assert_abs_diff_eq!(sy, 3.0, epsilon = 1e-9);
        let (tx, ty) = at("t");
        assert_abs_diff_eq!(tx, 4.25, epsilon = 1e-9);
        assert_abs_diff_eq!(ty, 0.0, epsilon = 1e-9);
    }

    #[test]
    fn null_space_is_null() {
        // Rank 2 in 4 columns: two null vectors, each annihilated by `a`.
        let a = DMatrix::from_row_slice(
            3,
            4,
            &[1.0, 2.0, 3.0, 4.0, 2.0, 4.0, 6.0, 8.0, 0.0, 1.0, 1.0, 0.0],
        );
        let (rank, basis) = null_space_nalgebra(&a, 1e-9);
        assert_eq!(rank, 2);
        assert_eq!(basis.shape(), (4, 2));
        assert!((&a * &basis).iter().all(|v| v.abs() < 1e-12));
        // Full rank: no null space. Zero rows: everything is free.
        let id = DMatrix::<f64>::identity(3, 3);
        assert_eq!(null_space_nalgebra(&id, 1e-9), (3, DMatrix::zeros(3, 0)));
        let none = DMatrix::<f64>::zeros(0, 3);
        assert_eq!(
            null_space_nalgebra(&none, 1e-9),
            (0, DMatrix::identity(3, 3))
        );
    }

    #[test]
    fn redundancy_is_named() {
        let g = geometry!(Model {
            l: line (0.0, 0.0) -> (10.0, 1.0);
            m: line (0.0, 5.0) -> (10.0, 4.0);
        });
        let (l, m) = (line("l"), line("m"));
        let eqs = [
            same_y(l.start(), l.end()),
            same_y(m.start(), m.end()),
            parallel(l, m),
        ];
        let s = solve(&g, &eqs, Options::default()).unwrap();
        // One group of all three, with nothing left over: redundant.
        assert_eq!(s.report.dependencies.len(), 1);
        let d = &s.report.dependencies[0];
        assert_eq!(d.equations, vec![0, 1, 2]);
        assert!(d.residual < 1e-9);
        assert!(s.report.residuals.iter().all(|&r| r < 1e-9));
    }

    #[test]
    fn conflict_is_named() {
        // Both ends pinned 10 apart, and a length of 12: cannot all hold.
        let g = geometry!(Model { l: line (0.0, 0.0) -> (10.0, 0.0) });
        let l = line("l");
        let eqs = [
            pinned(l.start(), 0.0, 0.0),
            pinned(l.end(), 10.0, 0.0),
            length(l, 12.0),
        ];
        let s = solve(&g, &eqs, Options::default()).unwrap();
        assert_eq!(
            (s.report.rank, s.report.free, s.report.redundant),
            (4, 0, 1)
        );
        assert_eq!(s.report.dependencies.len(), 1);
        let d = &s.report.dependencies[0];
        assert_eq!(d.equations, vec![0, 1, 2]);
        // Least squares shares the 2 mm shortfall: 2/3 on the length and
        // 2/3 on each pin.
        assert!((d.residual - 2.0 / 3.0).abs() < 1e-6, "{}", d.residual);
        assert!(s
            .report
            .residuals
            .iter()
            .all(|&r| (r - 2.0 / 3.0).abs() < 1e-6));
        // A consistent length is not a conflict.
        let ok = [eqs[0], eqs[1], length(l, 10.0)];
        let s = solve(&g, &ok, Options::default()).unwrap();
        assert_eq!(s.report.dependencies.len(), 1);
        assert!(s.report.dependencies[0].residual < 1e-9);
    }

    #[test]
    fn freedoms_are_named() {
        let g = geometry!(Model {
            top: line (0.1, 0.2) -> (9.0, -0.3);
            bend: arc (9.5, 5.0), r = 5.0, from = -FRAC_PI_2 - 0.1, sweep = FRAC_PI_2 + 0.2;
            leg: line (15.0, 5.5) -> (14.0, 20.0);
        });
        let (top, bend, leg) = (line("top"), arc("bend"), line("leg"));
        let eqs = [
            pinned(top.start(), 0.0, 0.0),
            same_y(top.start(), top.end()),
            dx(top.start(), top.end(), 10.0),
            coincide(top.end(), bend.start()),
            line_tangent_arc(top, bend, ArcEnd::Start),
            radius(bend, 4.0),
            coincide(bend.end(), leg.start()),
            line_tangent_arc(leg, bend, ArcEnd::End),
        ];
        let s = solve(&g, &eqs, Options::default()).unwrap();
        assert_eq!(s.report.freedoms.len(), 2);
        assert!(s.report.dependencies.is_empty());
        // The two freedoms are the sweep (which drags the leg with it) and
        // the leg's length (its end alone). The top is fully fixed.
        let touches = |f: &Freedom, id: &str| f.values.iter().any(|(e, _)| *e == id.into());
        assert!(s.report.freedoms.iter().all(|f| !touches(f, "top")));
        assert!(s.report.freedoms.iter().any(|f| touches(f, "bend")));
        let leg_only: Vec<_> = s
            .report
            .freedoms
            .iter()
            .filter(|f| f.values.iter().all(|(e, _)| *e == "leg".into()))
            .collect();
        assert_eq!(leg_only.len(), 1);
        assert!(leg_only[0]
            .values
            .iter()
            .all(|(_, v)| matches!(v, Value::EndX | Value::EndY)));
        // With nothing to solve, the figure can slide and turn (placement)
        // and every other value is its own freedom.
        let s = solve(&g, &[], Options::default()).unwrap();
        assert_eq!(s.report.free, 13);
        assert_eq!(
            s.report.placement,
            Placement {
                x: true,
                y: true,
                rotation: true
            }
        );
        assert_eq!(s.report.internal_free, 10);
        assert_eq!(s.report.freedoms.len(), 10);
    }
    #[test]
    fn placement_is_recognised() {
        // A rectangle with no fiducial: rigid, but free to sit anywhere.
        let g = geometry!(Model {
            bottom: line (0.0, 0.0) -> (20.0, 0.0);
            right: line (20.0, 0.0) -> (20.0, 10.0);
            top: line (20.0, 10.0) -> (0.0, 10.0);
            left: line (0.0, 10.0) -> (0.0, 0.0);
        });
        let (b, r, t, l) = (line("bottom"), line("right"), line("top"), line("left"));
        let eqs = [
            coincide(b.end(), r.start()),
            coincide(r.end(), t.start()),
            coincide(t.end(), l.start()),
            coincide(l.end(), b.start()),
            same_y(b.start(), b.end()),
            same_x(r.start(), r.end()),
            same_y(t.start(), t.end()),
            same_x(l.start(), l.end()),
            dx(b.start(), b.end(), 20.0),
            dy(r.start(), r.end(), 10.0),
        ];
        let s = solve(&g, &eqs, Options::default()).unwrap();
        assert_eq!(s.report.free, 2);
        assert_eq!(
            s.report.placement,
            Placement {
                x: true,
                y: true,
                rotation: false
            }
        );
        assert_eq!(s.report.internal_free, 0);
        assert!(s.report.freedoms.is_empty());
        // Pin one corner: nothing is free. Analysis without solving agrees.
        let mut pinned_eqs = eqs.to_vec();
        pinned_eqs.push(pinned(b.start(), 0.0, 0.0));
        let a = analyse(&s.geometry, &pinned_eqs, Options::default()).unwrap();
        assert_eq!((a.report.free, a.report.placement.count()), (0, 0));
        assert!(a.report.residual_max < 1e-9);
        // Drop the horizontals and the rectangle can also turn.
        let turning: Vec<Equation> = eqs[..4]
            .iter()
            .copied()
            .chain(eqs[8..].iter().copied())
            .collect();
        let s = solve(&g, &turning, Options::default()).unwrap();
        assert!(s.report.placement.rotation);
        // Signed residuals: the rectangle is 20 wide, a claim of 21 is 1 short.
        let r = residuals(&s.geometry, &[dx(b.start(), b.end(), 21.0)]).unwrap();
        assert_eq!(r.len(), 1);
        assert!((r[0][0] + 1.0).abs() < 1e-9);
    }
    #[test]
    fn local_and_dense_solves_agree() {
        let g = geometry!(Model {
            top: line (0.1, 0.2) -> (9.0, -0.3);
            bend: arc (9.5, 5.0), r = 5.0, from = -FRAC_PI_2 - 0.1, sweep = FRAC_PI_2 + 0.2;
            leg: line (15.0, 5.5) -> (14.0, 20.0);
        });
        let (top, bend, leg) = (line("top"), arc("bend"), line("leg"));
        let eqs = [
            pinned(top.start(), 0.0, 0.0),
            same_y(top.start(), top.end()),
            dx(top.start(), top.end(), 10.0),
            coincide(top.end(), bend.start()),
            line_tangent_arc(top, bend, ArcEnd::Start),
            radius(bend, 4.0),
            coincide(bend.end(), leg.start()),
            line_tangent_arc(leg, bend, ArcEnd::End),
        ];
        let dense = solve(&g, &eqs, Options::default().with_jacobian(Jacobian::Dense)).unwrap();
        let local = solve(&g, &eqs, Options::default().with_jacobian(Jacobian::Local)).unwrap();
        assert_eq!(dense.geometry, local.geometry);
        assert_eq!(dense.report, local.report);
        assert_eq!(Options::default().jacobian, Jacobian::Local);
    }
    /// Where the time goes, part by part, on Section A-A. Run with
    /// `cargo test --release -p lmsheets-document profile_parts -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore]
    fn profile_parts() {
        use std::time::Instant;
        let s = crate::examples::section_aa();
        let low = s.lower(&crate::value::params::Params::NONE).unwrap();
        let g = s.geometry();
        let layout = Layout::of(g);
        let x = layout.flatten(g);
        let n = 50;
        let time = |name: &str, f: &mut dyn FnMut()| {
            let t = Instant::now();
            for _ in 0..n {
                f();
            }
            eprintln!("{name:>28}: {:?}", t.elapsed() / n);
        };
        time("residuals (f64)", &mut || {
            evaluate(&layout, &low.equations, &x);
        });
        time("jacobian local", &mut || {
            linearise_local(&layout, &low.equations, &x);
        });
        time("jacobian dense", &mut || {
            linearise_dense(&layout, &low.equations, &x);
        });
        let (_, j, _) = linearise_local(&layout, &low.equations, &x);
        time("null_space nalgebra", &mut || {
            null_space_nalgebra(&j, 1e-9);
        });
        time("null_space faer", &mut || {
            super::super::faer_backend::null_space(&j, 1e-9);
        });
        let (_, basis) = null_space_nalgebra(&j, 1e-9);
        time("placement", &mut || {
            let all: Vec<EntityId> = g.iter().map(|(id, _)| *id).collect();
            placement(&layout, &x, &basis, &[all], 1e-9);
        });
        time("lm faer (ours)", &mut || {
            let problem = Problem {
                layout: &layout,
                eqs: &low.equations,
                x: x.clone(),
                jacobian: Jacobian::Local,
            };
            let settings = super::super::lm::Settings {
                patience: 200,
                ftol: 1e-14,
                xtol: 1e-14,
                gtol: 0.0,
            };
            super::super::lm::minimize(&problem, x.clone(), &settings);
        });
        time("lm crate (5 evals)", &mut || {
            let problem = Problem {
                layout: &layout,
                eqs: &low.equations,
                x: x.clone(),
                jacobian: Jacobian::Local,
            };
            LevenbergMarquardt::new()
                .with_patience(200)
                .with_tol(1e-14)
                .minimize(problem);
        });
        time("sketch.lower()", &mut || {
            s.lower(&crate::value::params::Params::NONE).unwrap();
        });
        time("solver::analyse", &mut || {
            let _ = analyse(g, &low.equations, Options::default()).unwrap();
        });
        time("sketch.analyse()", &mut || {
            let _ = s
                .analyse(Options::default(), &crate::value::params::Params::NONE)
                .unwrap();
        });
    }
    #[test]
    fn backends_agree() {
        // A fully determined figure: both backends reach the same figure.
        let g = geometry!(Model {
            bottom: line (0.3, -0.2) -> (19.0, 0.4);
            right: line (19.0, 0.4) -> (21.0, 18.0);
            top: line (21.0, 18.0) -> (0.5, 20.5);
            left: line (0.5, 20.5) -> (0.3, -0.2);
        });
        let (b, r, t, l) = (line("bottom"), line("right"), line("top"), line("left"));
        let eqs = [
            pinned(b.start(), 0.0, 0.0),
            coincide(b.end(), r.start()),
            coincide(r.end(), t.start()),
            coincide(t.end(), l.start()),
            coincide(l.end(), b.start()),
            same_y(b.start(), b.end()),
            same_x(r.start(), r.end()),
            same_y(t.start(), t.end()),
            same_x(l.start(), l.end()),
            dx(b.start(), b.end(), 20.0),
            dy(r.start(), r.end(), 10.0),
        ];
        let faer = solve(&g, &eqs, Options::default().with_backend(Backend::Faer)).unwrap();
        let krate = solve(&g, &eqs, Options::default().with_backend(Backend::Crate)).unwrap();
        assert!(faer.report.residual_max < 1e-9, "{:?}", faer.report);
        assert!(krate.report.residual_max < 1e-9, "{:?}", krate.report);
        assert_abs_diff_eq!(
            faer.geometry,
            krate.geometry,
            epsilon = crate::solver::tolerance::Tolerance::new(1e-7, 1e-7)
        );
        assert_eq!((faer.report.rank, faer.report.free), (16, 0));
        assert_eq!((krate.report.rank, krate.report.free), (16, 0));
        assert_eq!(Options::default().backend, Backend::Faer);

        // An under-determined figure (the clip: sweep and leg length free):
        // both solve it, and ours moves no further from the start than the
        // crate does, which is the property the branch choice needs.
        let g = geometry!(Model {
            top: line (0.1, 0.2) -> (9.0, -0.3);
            bend: arc (9.5, 5.0), r = 5.0, from = -FRAC_PI_2 - 0.1, sweep = FRAC_PI_2 + 0.2;
            leg: line (15.0, 5.5) -> (14.0, 20.0);
        });
        let (top, bend, leg) = (line("top"), arc("bend"), line("leg"));
        let eqs = [
            pinned(top.start(), 0.0, 0.0),
            same_y(top.start(), top.end()),
            dx(top.start(), top.end(), 10.0),
            coincide(top.end(), bend.start()),
            line_tangent_arc(top, bend, ArcEnd::Start),
            radius(bend, 4.0),
            coincide(bend.end(), leg.start()),
            line_tangent_arc(leg, bend, ArcEnd::End),
        ];
        let layout = Layout::of(&g);
        let x0 = layout.flatten(&g);
        let moved = |s: &Solution<Model>| (layout.flatten(&s.geometry) - &x0).norm();
        let faer = solve(&g, &eqs, Options::default().with_backend(Backend::Faer)).unwrap();
        let krate = solve(&g, &eqs, Options::default().with_backend(Backend::Crate)).unwrap();
        assert!(faer.report.residual_max < 1e-9, "{:?}", faer.report);
        assert!(krate.report.residual_max < 1e-9);
        assert!(
            moved(&faer) <= moved(&krate) * 1.01 + 1e-9,
            "{} vs {}",
            moved(&faer),
            moved(&krate)
        );
        for s in [&faer, &krate] {
            assert_eq!(
                (s.report.rank, s.report.free, s.report.redundant),
                (11, 2, 0)
            );
            assert_eq!(s.report.freedoms.len(), 2);
        }
        assert_eq!(faer.report.freedoms, krate.report.freedoms);

        // A redundant group is found by both.
        let g = geometry!(Model {
            l: line (0.0, 0.0) -> (10.0, 1.0);
            m: line (0.0, 5.0) -> (10.0, 4.0);
        });
        let (l, m) = (line("l"), line("m"));
        let eqs = [
            same_y(l.start(), l.end()),
            same_y(m.start(), m.end()),
            parallel(l, m),
        ];
        for b in [Backend::Faer, Backend::Crate] {
            let s = solve(&g, &eqs, Options::default().with_backend(b)).unwrap();
            assert_eq!((s.report.rank, s.report.redundant), (2, 1), "{b:?}");
            assert_eq!(s.report.dependencies.len(), 1);
            assert_eq!(s.report.dependencies[0].equations, vec![0, 1, 2]);
            assert!(s.report.placement.x && s.report.placement.y, "{b:?}");
        }
    }

    #[test]
    fn freedom_basis_is_canonical() {
        // Two bases of one plane in R^3 reduce to the same vectors.
        let a = DMatrix::from_column_slice(3, 2, &[1.0, 1.0, 0.0, 0.0, 1.0, 1.0]);
        let b = DMatrix::from_column_slice(3, 2, &[2.0, 3.0, 1.0, -1.0, 0.0, 1.0]);
        let ca = canonical_basis(&a, 1e-9);
        let cb = canonical_basis(&b, 1e-9);
        assert_eq!(ca.shape(), (3, 2));
        assert!((ca - cb).iter().all(|v| v.abs() < 1e-12));
        assert_eq!(canonical_basis(&DMatrix::zeros(3, 0), 1e-9).shape(), (3, 0));
    }
    /// Run `f` on its own thread; `None` if it has not returned within
    /// two seconds (the thread is left behind), so a hang fails the test
    /// instead of stalling the suite.
    fn within_2s<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            tx.send(f()).ok();
        });
        rx.recv_timeout(std::time::Duration::from_secs(2)).ok()
    }

    /// A long plate with a square pointed tip, drawn exactly at its
    /// solution, as the generator writes it: the two tip edges are
    /// oblique and exactly perpendicular, and the tip is dimensioned by
    /// its angle. Found as synthsheets sheet seed 12052.
    #[test]
    fn square_pointed_tip_by_its_angle() {
        let g = geometry!(Model {
            bottom: line (0.0, 0.0) -> (10.0, 0.0);
            tip1: line (10.0, 0.0) -> (11.0, 1.0);
            tip2: line (11.0, 1.0) -> (10.0, 2.0);
            top: line (10.0, 2.0) -> (0.0, 2.0);
            left: line (0.0, 2.0) -> (0.0, 0.0);
        });
        let (b, t1, t2) = (line("bottom"), line("tip1"), line("tip2"));
        let (t, lf) = (line("top"), line("left"));
        let eqs = vec![
            pinned(b.start(), 0.0, 0.0),
            coincide(b.end(), t1.start()),
            coincide(t1.end(), t2.start()),
            coincide(t2.end(), t.start()),
            coincide(t.end(), lf.start()),
            coincide(lf.end(), b.start()),
            same_y(b.start(), b.end()),
            same_y(t.start(), t.end()),
            same_x(lf.start(), lf.end()),
            dx(b.start(), b.end(), 10.0),
            dy(lf.end(), lf.start(), 2.0),
            dx(t1.start(), t1.end(), 1.0),
            dy(t1.start(), t1.end(), 1.0),
            angle(t1, t2, FRAC_PI_2),
        ];
        let solved = within_2s(move || solve(&g, &eqs, Options::default()))
            .expect("the solver returns within 2 s")
            .unwrap();
        assert!(solved.report.residual_max < 1e-9);
        // 20 parameters, 20 independent equations.
        assert_eq!((solved.report.free, solved.report.redundant), (0, 0));
    }
    /// Angle equations at and within 1e-9 rad of 0°, 90°, 180° and 270°,
    /// between axis-aligned and oblique lines, some drawn so that the dot
    /// or cross product of their directions is exactly zero: derivatives
    /// finite and equal to finite differences, and the solve returns with
    /// the angle met, both from the drawn state and from a rough one.
    #[test]
    fn angles_near_the_quarter_turns() {
        // Integer directions, so the products vanish exactly.
        let dirs = [(1.0, 0.0), (0.0, 1.0), (3.0, 4.0), (1.0, 1.0), (-5.0, 2.0)];
        // Quarter turns of (a, b), exactly, in the y-down frame.
        let turn = |(a, b): (f64, f64), k: usize| match k % 4 {
            0 => (a, b),
            1 => (-b, a),
            2 => (-a, -b),
            _ => (b, -a),
        };
        for &d in &dirs {
            for k in 0..4 {
                for nudge in [0.0, 1e-9, -1e-9, 1e-12] {
                    let rad = wrap_angle(k as f64 * FRAC_PI_2 + nudge);
                    let (ex, ey) = turn(d, k);
                    let (c, s) = (nudge.cos(), nudge.sin());
                    let e = (ex * c - ey * s, ex * s + ey * c);
                    let g = geometry!(Model {
                        l: line (1.0, 2.0) -> (1.0 + d.0, 2.0 + d.1);
                        m: line (1.0, 2.0) -> (1.0 + e.0, 2.0 + e.1);
                    });
                    let (l, m) = (line("l"), line("m"));
                    let eqs = vec![
                        pinned(l.start(), 1.0, 2.0),
                        coincide(l.start(), m.start()),
                        pinned(l.end(), 1.0 + d.0, 2.0 + d.1),
                        length(m, (e.0 * e.0 + e.1 * e.1).sqrt()),
                        angle(l, m, rad),
                    ];
                    let case = format!("{d:?} turned {k}/4 + {nudge:e}");
                    // Derivatives at the drawn state.
                    let layout = Layout::of(&g);
                    let x = layout.flatten(&g);
                    let (r, jac, _) = linearise(&layout, &eqs, &x, Jacobian::Local);
                    assert!(r.amax() < 1e-9, "{case}: residual {r}");
                    let h = 1e-6;
                    for col in 0..layout.n {
                        let (mut xp, mut xm) = (x.clone(), x.clone());
                        xp[col] += h;
                        xm[col] -= h;
                        let fd = (evaluate(&layout, &eqs, &xp).0 - evaluate(&layout, &eqs, &xm).0)
                            / (2.0 * h);
                        for row in 0..r.len() {
                            assert!(
                                (jac[(row, col)] - fd[row]).abs() < 1e-6,
                                "{case}: d r[{row}] / d x[{col}]: {} vs {}",
                                jac[(row, col)],
                                fd[row]
                            );
                        }
                    }
                    // Solves, from the drawn state and from a rough one.
                    let rough = geometry!(Model {
                        l: line (1.0, 2.0) -> (1.0 + d.0, 2.0 + d.1);
                        m: line (1.1, 2.0) -> (1.3 + e.0, 1.8 + e.1);
                    });
                    for start in [g.clone(), rough] {
                        let e2 = eqs.clone();
                        let solved = within_2s(move || solve(&start, &e2, Options::default()))
                            .unwrap_or_else(|| panic!("{case}: no return in 2 s"))
                            .unwrap();
                        let rep = &solved.report;
                        assert!(rep.residual_max < 1e-9, "{case}: {rep:?}");
                        assert!(!matches!(rep.termination, Termination::Failed(_)));
                        assert_eq!((rep.free, rep.redundant), (0, 0), "{case}");
                    }
                }
            }
        }
    }

    /// An equation where it has no derivative (a distance between two
    /// points that coincide: the norm is a cone there) gives a solve and
    /// an analysis that name it, not a hang, a panic or a bare failure;
    /// the other equations are still analysed.
    #[test]
    fn a_degenerate_equation_is_named() {
        let g = geometry!(Model {
            p: point (1.0, 1.0);
            q: point (1.0, 1.0);
            r: point (4.0, 0.0);
            l: line (0.0, 0.0) -> (5.0, 1.0);
            m: line (5.0, 1.0) -> (6.0, 3.0);
        });
        let (p, q, r, l, m) = (point("p"), point("q"), point("r"), line("l"), line("m"));
        let eqs = vec![
            pinned(p, 1.0, 1.0),
            distance(p, q, 2.0),
            on_line(r, l),
            coincide(l.end(), m.start()),
            angle(l, m, 1.0),
            length(l, 5.0),
        ];
        let named = Termination::Degenerate {
            equation: 1,
            what: Degeneracy::Coincident(p, q),
        };
        for backend in [Backend::Faer, Backend::Crate] {
            let o = Options::default().with_backend(backend);
            let (g2, e2) = (g.clone(), eqs.clone());
            let solved = within_2s(move || solve(&g2, &e2, o))
                .expect("the solver returns within 2 s")
                .unwrap();
            assert_eq!(solved.report.termination, named);
            assert_eq!(solved.geometry, g);
            let (g2, e2) = (g.clone(), eqs.clone());
            let analysed = within_2s(move || analyse(&g2, &e2, o))
                .expect("the analysis returns within 2 s")
                .unwrap();
            assert_eq!(analysed.report.termination, named);
            // The distance's row is left out: 7 independent residuals.
            assert_eq!(analysed.report.rank, 7);
        }
        // Its value is still defined: the points are 2 mm short.
        assert_eq!(residuals(&g, &eqs[1..2]).unwrap(), vec![vec![-2.0]]);
    }

    /// One relation at one state: where the relation is differentiable its
    /// residuals and derivatives are finite and the derivatives agree with
    /// finite differences; where not, the degeneracy named is real and
    /// the solver stops on it by that name. Local and dense evaluation
    /// agree exactly either way.
    fn differentiable_where_defined(e: Equation, x: &DVector<f64>) -> Result<(), TestCaseError> {
        let layout = super::super::testing::zoo_layout();
        let eqs = [e];
        let (r, j, singular) = linearise(&layout, &eqs, x, Jacobian::Local);
        let (rd, jd, sd) = linearise(&layout, &eqs, x, Jacobian::Dense);
        prop_assert_eq!(singular, sd);
        let same = |a: f64, b: f64| a == b || (a.is_nan() && b.is_nan());
        prop_assert!(r.iter().zip(&rd).all(|(a, b)| same(*a, *b)));
        prop_assert_eq!(&j, &jd);
        let problem = Problem {
            layout: &layout,
            eqs: &eqs,
            x: x.clone(),
            jacobian: Jacobian::Local,
        };
        if let Some(s) = singular {
            match s.1 {
                Degeneracy::Coincident(a, b) => {
                    prop_assert_eq!(layout.position(x, &a), layout.position(x, &b))
                }
                Degeneracy::ZeroRadius(c) => prop_assert_eq!(x[layout.radius_slot(&c)], 0.0),
            }
            prop_assert!(j.iter().all(|v| *v == 0.0));
            prop_assert!(problem.residuals_in_domain(x).is_none());
            let settings = super::super::lm::Settings {
                patience: 10,
                ftol: 1e-14,
                xtol: 1e-14,
                gtol: 0.0,
            };
            let (_, _, t) = super::super::lm::minimize(&problem, x.clone(), &settings);
            prop_assert_eq!(t, Termination::degenerate(s));
            return Ok(());
        }
        prop_assert!(r.iter().chain(j.iter()).all(|v| v.is_finite()), "{r} {j}");
        // In the domain where the entities are too (a tie may leave a
        // line of no length, a radius of zero).
        prop_assert_eq!(problem.residuals_in_domain(x).is_some(), layout.admits(x));
        // Each derivative against one-sided differences either way, with
        // Richardson extrapolation from steps h and h/2; the difference
        // between the two steps measures the curvature, and bounds the
        // error. Steps from 1e-3 down by quarters to 1e-9, for curvature
        // that needs a short step; the first that agrees will do. At a
        // branch's edge (a tangency's side, the angle's wrap) the
        // derivative is the branch's, which one side always matches.
        //
        // Where two positions read are under 1e-6 mm apart and not equal
        // (they differ by rounding, say) no step resolves the function in
        // f64, and only finiteness (above) is checked: the derivative is
        // computed there by the same code, with no branch on size.
        if super::super::testing::nearly_coincident(&eqs, &layout, x) {
            return Ok(());
        }
        let f = |col: usize, t: f64| {
            let mut y = x.clone();
            y[col] += t;
            evaluate(&layout, &eqs, &y).0
        };
        for col in 0..layout.n {
            for row in 0..r.len() {
                let d = j[(row, col)];
                let agrees = |t: f64| {
                    let d1 = (f(col, t)[row] - r[row]) / t;
                    let d2 = (f(col, t / 2.0)[row] - r[row]) / (t / 2.0);
                    let rich = 2.0 * d2 - d1;
                    let tol = 1e-6 * (1.0 + d.abs()) + 2.0 * (d2 - d1).abs();
                    rich.is_finite() && (d - rich).abs() <= tol
                };
                let ok = (0..11).any(|k| {
                    let h = 1e-3 / 4f64.powi(k);
                    agrees(h) || agrees(-h)
                });
                prop_assert!(ok, "d r[{}] / d {:?}: {}", row, layout.names[col], d);
            }
        }
        Ok(())
    }

    proptest! {
        // 100 cases per relation kind; about a second in a debug build.
        #![proptest_config(ProptestConfig::with_cases(100 * KINDS as u32))]

        /// Every relation, at random and at special states (square,
        /// parallel, axis-aligned, coincident, zero-length, zero radius).
        #[test]
        fn every_relation_is_differentiable_where_defined(
            (e, x) in super::super::testing::case()
        ) {
            differentiable_where_defined(e, &x)?;
        }
    }

    /// nalgebra's unbounded SVD never returns on a NaN; ours refuses it.
    #[test]
    fn the_analysis_svd_refuses_nan() {
        let mut a = DMatrix::from_element(6, 3, 1.0);
        a[(2, 1)] = f64::NAN;
        let (b, v) = (a.clone(), DVector::from_element(6, 1.0));
        let (q, inside) = within_2s(move || (orthonormal(&b, 1e-10), in_span(&b, &v, 1e-10)))
            .expect("returns within 2 s");
        assert_eq!(q.ncols(), 0);
        assert!(!inside);
        assert!(bounded_svd(&a, true).is_none());
    }
}

#[cfg(test)]
mod doc_examples {
    use crate::geometry;

    #[test]
    fn a_clip_driven_to_size() {
        use crate::solver::equation::*;
        use crate::solver::solve::{solve, Options};
        use crate::value::geometry::{Entity, Model};

        let g = geometry!(Model {
            top: line (0.1, 0.2) -> (9.0, -0.3);
            bend: arc (9.5, 5.0), r = 5.0, from = -1.6, sweep = 1.7;
            leg: line (15.0, 5.5) -> (14.0, 20.0);
        });

        let (top, bend, leg) = (line("top"), arc("bend"), line("leg"));
        let eqs = [
            pinned(top.start(), 0.0, 0.0),
            same_y(top.start(), top.end()),
            dx(top.start(), top.end(), 10.0),
            coincide(top.end(), bend.start()),
            line_tangent_arc(top, bend, ArcEnd::Start),
            radius(bend, 4.0),
            coincide(bend.end(), leg.start()),
            line_tangent_arc(leg, bend, ArcEnd::End),
        ];

        let solved = solve(&g, &eqs, Options::default()).unwrap();
        assert!(solved.report.residual_max < 1e-9);
        // 13 parameters, 11 independent equations: the bend's sweep and the
        // leg's length are still free. Nothing is redundant.
        assert_eq!((solved.report.free, solved.report.redundant), (2, 0));
        match solved.geometry.get("bend") {
            Some(Entity::Arc { centre, radius, .. }) => {
                assert!((radius - 4.0).abs() < 1e-9);
                assert!((centre.x - 10.0).abs() < 1e-9);
            }
            _ => unreachable!(),
        }
        // `g` is untouched; the result is a new geometry.
    }
}

#[cfg(test)]
mod extreme_tests {
    use super::*;
    use crate::geometry;
    use crate::solver::equation::{arc, circle, dx, dy, pinned, radius};
    use crate::solver::solve::{solve, Options};

    fn g() -> Geometry<Model> {
        geometry!(Model {
            end: arc (3.0, 1.0), r = 2.0, from = 2.0, sweep = 2.5;
            ball: circle (10.0, 2.0), r = 1.0;
        })
    }

    /// Both Jacobians agree with finite differences, and the extreme is
    /// linear in its centre and radius.
    #[test]
    fn an_extreme_is_linear_in_centre_and_radius() {
        let g = g();
        let eqs = [
            dx(
                PointRef::arc_extreme("end", Extreme::MinusX),
                PointRef::circle_extreme("ball", Extreme::PlusX),
                0.0,
            ),
            dy(
                PointRef::arc_extreme("end", Extreme::PlusY),
                PointRef::circle_extreme("ball", Extreme::MinusY),
                0.0,
            ),
        ];
        let layout = Layout::of(&g);
        let x = layout.flatten(&g);
        let (r, j, _) = linearise(&layout, &eqs, &x, Jacobian::Dense);
        let (r2, j2, _) = linearise(&layout, &eqs, &x, Jacobian::Local);
        assert_eq!((&r, &j), (&r2, &j2));
        // ball: x y r (0..3); end: x y r a0 sweep (3..8). The left
        // extreme is centre_x - r, the right one centre_x + r.
        let row = |i: usize| j.row(i).iter().copied().collect::<Vec<_>>();
        assert_eq!(row(0), [1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 0.0, 0.0]);
        assert_eq!(row(1), [0.0, 1.0, -1.0, 0.0, -1.0, -1.0, 0.0, 0.0]);
    }

    /// A distance between two rounded ends' extremes solves like any
    /// other, in both axes.
    #[test]
    fn a_dimension_over_two_extremes_solves() {
        let g = g();
        let left = PointRef::arc_extreme("end", Extreme::MinusX);
        let right = PointRef::circle_extreme("ball", Extreme::PlusX);
        let eqs = [
            pinned(PointRef::arc("end", ArcPos::Centre), 0.0, 0.0),
            radius(arc("end"), 2.0),
            radius(circle("ball"), 1.0),
            dx(left, right, 20.0),
            dy(
                PointRef::arc_extreme("end", Extreme::PlusY),
                PointRef::circle_extreme("ball", Extreme::MinusY),
                -1.0,
            ),
        ];
        let solved = solve(&g, &eqs, Options::default()).unwrap();
        assert!(solved.report.residual_max < 1e-9);
        let p = |r: &PointRef| solved.geometry.position(r).unwrap();
        assert!((p(&right).x - p(&left).x - 20.0).abs() < 1e-9);
        assert!((p(&left).x + 2.0).abs() < 1e-9);
    }
}
