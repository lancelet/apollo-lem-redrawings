//! The nearest design that holds the equations: a weighted, constrained
//! least squares, solved by sequential quadratic programming.
//!
//! [`solve`](super::solve::solve) drives the equations to zero from a
//! start and, where they leave freedom, stays nearest the start in the
//! plain norm of the parameters. That mixes millimetres with radians and
//! trusts every value alike. [`solve_near`] takes the start as a *prior*
//! with a weight per entity (the inverse covariance of an ink fit: what
//! is seen well moves little) and finds
//!
//! ```text
//!     minimise  ½ (x − p)ᵀ W (x − p)    subject to    c(x) = 0
//! ```
//!
//! for the equations `c` taken as exact. The problem is the textbook
//! equality-constrained one, and so is the method: from `x`, linearise
//! the constraints, `c + J d = 0`, and take the step `d` that minimises
//! the quadratic objective on that affine space (a KKT step). It is
//! computed by the null-space method rather than by factorising the
//! KKT matrix, so that neither redundant constraints (rank-deficient
//! `J`) nor unweighted values (singular `W`) make it ill-posed, and
//! mixed scales in `W` cannot spoil the constraint part: one SVD of `J`
//! gives the minimum-norm particular step that meets the constraints and
//! a basis `Z` of the directions that leave them alone; `ZᵀWZ` is then
//! solved for the weighted-best movement along `Z` (pseudo-inverse, so
//! directions that nothing weighs stay put). The Hessian of the
//! Lagrangian is taken as `W`, the Gauss–Newton choice: the curvature of
//! the constraints is left out, which costs speed near a far prior and
//! nothing in the answer. A step is accepted along an ℓ₁ merit function
//! (objective plus `ρ‖c‖₁`, `ρ` from the multipliers), halved until it
//! decreases, so that a nonlinear constraint cannot throw the iterate
//! onto another branch; and it starts at the prior, so the branch is the
//! one nearest the ink.
//!
//! Constraints that conflict have no solution; the iteration then comes
//! to rest on a compromise, reported as [`Termination::Stalled`] with
//! the dependency analysis of the solver's report naming the group.

use super::equation::{Equation, Error};
use super::solve::Options;
use super::solve::{bounded_svd, evaluate, linearise, report_at, Layout, Solution, Termination};
use crate::value::geometry::{EntityId, Frame, Geometry};
use nalgebra::{DMatrix, DVector};
use std::collections::BTreeMap;

/// The ink's design to stay near, and how firmly, per entity.
#[derive(Clone, Debug)]
pub struct Prior<F: Frame> {
    /// The prior values. The solve is over exactly these entities, and
    /// starts from them.
    pub geometry: Geometry<F>,
    /// The weight of each entity's departure: a symmetric positive
    /// semi-definite matrix as large as the entity has values (a line 4,
    /// an arc 5, ...), in the order of `Value`, normally the inverse
    /// covariance of the fit. An entity without one is not weighed. Its
    /// values are moved only as the constraints and the other entities'
    /// weights require, each step by the plain minimum-norm amount; where
    /// they end up is then a property of the path, not a minimiser of
    /// anything, and [`Nearest::unheld`] counts such directions. Give
    /// every value a weight (however small) where the answer must be
    /// well defined.
    pub weights: BTreeMap<EntityId, DMatrix<f64>>,
}

/// What [`solve_near`] found.
#[derive(Clone, Debug)]
#[must_use = "the solved geometry is returned, not applied"]
pub struct Nearest<F: Frame> {
    /// The solved geometry and the solver's report on it (rank, freedoms,
    /// dependencies, residuals). `termination` says why it stopped.
    pub solution: Solution<F>,
    /// `½ (x − p)ᵀ W (x − p)` at the solution: the weighted departure.
    pub cost: f64,
    /// Whether every constraint holds (to `FEASIBLE`) at a point where
    /// the iteration came to rest.
    pub converged: bool,
    /// The Lagrange multiplier of each equation, in the order given: the
    /// largest magnitude over its rows, in cost per unit of residual. How
    /// hard the equation pulls the design away from the prior; zero for
    /// one the prior already satisfies.
    pub multipliers: Vec<f64>,
    /// How many directions are free in the constraints and weighed by
    /// nothing, so that the prior does not hold them either. (`Report::free`
    /// counts these together with the directions the prior does hold.)
    pub unheld: usize,
}

/// The largest residual that still counts as holding.
const FEASIBLE: f64 = 1e-9;
/// A step below this fraction of `1 + ‖x‖` is no step.
const STEP: f64 = 1e-12;
/// On feasible ground, the optimum is where the reduced gradient would
/// gain less than this fraction of the objective.
const GAIN: f64 = 1e-13;
/// The sufficient-decrease constant of the line search.
const ARMIJO: f64 = 1e-4;
/// Halvings of a step before giving up on it.
const HALVINGS: usize = 40;
/// Iterations after which the step stops leaving the constraints'
/// curvature out of the Hessian (see [`curvature`]): the Gauss–Newton
/// choice converges linearly, at a rate that worsens as the multipliers
/// grow against the weights, so that a solve still going by now is on
/// such a path.
const SLOW: usize = 30;

/// The block-diagonal weight matrix of the prior over `layout`.
fn weight_matrix<F: Frame>(layout: &Layout, prior: &Prior<F>) -> Result<DMatrix<f64>, Error> {
    let mut w = DMatrix::zeros(layout.n, layout.n);
    for (id, m) in &prior.weights {
        let bad = || Error::BadWeight(*id);
        let &(o, _, k) = layout.slots.get(id).ok_or_else(bad)?;
        let scale = m.iter().fold(0.0, |a: f64, v| a.max(v.abs()));
        let symmetric = (m - m.transpose()).iter().all(|v| v.abs() <= 1e-9 * scale);
        if m.shape() != (k, k) || !m.iter().all(|v| v.is_finite()) || !symmetric {
            return Err(bad());
        }
        if m.clone().symmetric_eigen().eigenvalues.min() < -1e-9 * scale {
            return Err(bad());
        }
        // Symmetrised exactly: the eigen-decompositions assume it.
        w.view_mut((o, o), (k, k))
            .copy_from(&((m + m.transpose()) * 0.5));
    }
    Ok(w)
}

/// The pseudo-inverse's action on `b`: `H⁺ b` for symmetric `H`, with
/// eigenvalues below rounding (relative to the largest) taken as zero,
/// and the rank that leaves.
fn pinv_symmetric(h: &DMatrix<f64>, b: &DVector<f64>) -> (DVector<f64>, usize) {
    let e = h.clone().symmetric_eigen();
    let top = e.eigenvalues.iter().fold(0.0, |a: f64, v| a.max(v.abs()));
    let floor = top * h.nrows() as f64 * f64::EPSILON;
    let coords = e.eigenvectors.transpose() * b;
    let scaled = DVector::from_iterator(
        coords.len(),
        coords
            .iter()
            .zip(e.eigenvalues.iter())
            .map(|(c, l)| if *l > floor { c / l } else { 0.0 }),
    );
    let rank = e.eigenvalues.iter().filter(|&&l| l > floor).count();
    (e.eigenvectors * scaled, rank)
}

/// The curvature of the constraints along the multipliers, `Σ λᵢ ∇²cᵢ`
/// at `x`: the part of the Lagrangian's Hessian that the weights leave
/// out. Central differences of `Jᵀλ`, symmetrised; `None` where a
/// linearisation fails.
fn curvature(
    layout: &Layout,
    eqs: &[Equation],
    x: &DVector<f64>,
    lambda: &DVector<f64>,
    o: Options,
) -> Option<DMatrix<f64>> {
    let n = x.len();
    let m: usize = eqs.iter().map(Equation::count).sum();
    let pull = |x: &DVector<f64>| -> Option<DVector<f64>> {
        let (_, j, singular) = linearise(layout, eqs, x, o.jacobian);
        singular
            .is_none()
            .then(|| j.transpose() * lambda.rows(0, m))
    };
    let mut c = DMatrix::zeros(n, n);
    for k in 0..n {
        let h = 1e-6 * (1.0 + x[k].abs());
        let (mut up, mut down) = (x.clone(), x.clone());
        up[k] += h;
        down[k] -= h;
        let column = (pull(&up)? - pull(&down)?) / (2.0 * h);
        if !column.iter().all(|v| v.is_finite()) {
            return None;
        }
        c.set_column(k, &column);
    }
    Some((&c + c.transpose()) * 0.5)
}

/// One KKT step's result.
struct Step {
    /// The step.
    d: DVector<f64>,
    /// The multipliers, one per row of the Jacobian (and zeros after).
    lambda: DVector<f64>,
    /// Directions free in the constraints that no weight holds.
    unheld: usize,
    /// What the weighted-best movement along the constraints gains in the
    /// objective, `½ g_zᵀ H⁺ g_z` (zero with no null space): the
    /// reduced-gradient measure of stationarity.
    reduced: f64,
}

/// One KKT step from `x`: the step `d` onto the linearised constraints
/// and along them to the weighted nearest, and the multipliers `λ` there
/// (`W (x + d − p) + Jᵀ λ = 0`). `None` if the SVD fails.
fn kkt_step(
    j: &DMatrix<f64>,
    c: &DVector<f64>,
    gradient: impl Fn(&DVector<f64>) -> DVector<f64>,
    w: &DMatrix<f64>,
    curved: Option<&DMatrix<f64>>,
    x: &DVector<f64>,
    rank_tol: f64,
) -> Option<Step> {
    let n = x.len();
    // Pad with zero rows so that the thin SVD's V is square: the columns
    // past the rank are then a basis of the null space.
    let rows = j.nrows().max(n);
    let mut padded = DMatrix::zeros(rows, n);
    padded.view_mut((0, 0), j.shape()).copy_from(j);
    let mut b = DVector::zeros(rows);
    b.rows_mut(0, c.len()).copy_from(c);
    let svd = bounded_svd(&padded, true)?;
    let (u, v) = (svd.u?, svd.v_t?.transpose());
    let s = &svd.singular_values;
    let top = s.iter().fold(0.0, |a: f64, v| a.max(*v));
    // The singular values come in no promised order: the rank's columns
    // are those above the threshold, the rest the null space.
    let kept: Vec<usize> = (0..n)
        .filter(|&i| s[i] > rank_tol * top && s[i] > 0.0)
        .collect();
    let null: Vec<usize> = (0..n).filter(|i| !kept.contains(i)).collect();
    // Minimum-norm step with J d = −c (least squares if it has none).
    let mut d = DVector::zeros(n);
    for &i in &kept {
        d -= v.column(i) * (u.column(i).dot(&b) / s[i]);
    }
    // Weighted-best movement along the null space.
    let (mut unheld, mut reduced) = (0, 0.0);
    if !null.is_empty() {
        let z = v.select_columns(null.iter());
        let wz = w * &z;
        let mut h = z.transpose() * &wz;
        // With the constraints' curvature in the Hessian, where that
        // keeps every direction the weights hold held, and positive: else
        // the curvature is not trusted and the weights alone are used.
        if let Some(c) = curved {
            let all = |h: &DMatrix<f64>| {
                let e = h.clone().symmetric_eigen().eigenvalues;
                let top = e.iter().fold(0.0, |a: f64, v| a.max(v.abs()));
                e.iter().all(|l| *l > top * n as f64 * f64::EPSILON.sqrt())
            };
            let hc = &h + z.transpose() * c * &z;
            if all(&h) && all(&hc) {
                h = hc;
            }
        }
        let g = z.transpose() * gradient(&(x + &d));
        let (step, held) = pinv_symmetric(&h, &g);
        unheld = null.len() - held;
        reduced = 0.5 * g.dot(&step);
        d -= &z * step;
    }
    // Multipliers: Jᵀ λ = −W (x + d − p), least squares.
    let g = gradient(&(x + &d));
    let mut lambda = DVector::zeros(rows);
    for &i in &kept {
        lambda -= u.column(i) * (v.column(i).dot(&g) / s[i]);
    }
    Some(Step {
        d,
        lambda,
        unheld,
        reduced,
    })
}

/// Find the geometry nearest `prior` (in its weights) on which every
/// equation holds. Where the equations admit several, the one nearest
/// the prior's own values is found. With no equations it is the prior.
pub fn solve_near<F: Frame>(
    prior: &Prior<F>,
    eqs: &[Equation],
    o: Options,
) -> Result<Nearest<F>, Error> {
    for e in eqs {
        e.check(&prior.geometry)?;
    }
    let layout = Layout::of(&prior.geometry);
    let w = weight_matrix(&layout, prior)?;
    let p = layout.flatten(&prior.geometry);
    let gradient = |x: &DVector<f64>| &w * (x - &p);
    let cost = |x: &DVector<f64>| 0.5 * (x - &p).dot(&gradient(x));
    let m: usize = eqs.iter().map(Equation::count).sum();

    let mut x = p.clone();
    let mut evaluations = 0;
    // The penalty of the merit function only grows (Nocedal & Wright
    // §18.3), so that the merit cannot cycle.
    let mut rho = 0.0f64;
    let termination = 'run: {
        if m == 0 || layout.n == 0 {
            break 'run Termination::NothingToSolve;
        }
        for iteration in 0..o.patience {
            evaluations += 1;
            let (c, j, singular) = linearise(&layout, eqs, &x, o.jacobian);
            if let Some(s) = singular {
                break 'run Termination::degenerate(s);
            }
            if !c.iter().chain(j.iter()).all(|v| v.is_finite()) {
                break 'run Termination::Failed(super::lm::NOT_FINITE);
            }
            let Some(mut step) = kkt_step(&j, &c, gradient, &w, None, &x, o.rank_tol) else {
                break 'run Termination::Failed("the singular value decomposition failed");
            };
            // Linear progress is not coming fast: take the constraints'
            // curvature into the Hessian, from the multipliers just found.
            if iteration >= SLOW {
                if let Some(cv) = curvature(&layout, eqs, &x, &step.lambda, o) {
                    if let Some(better) = kkt_step(&j, &c, gradient, &w, Some(&cv), &x, o.rank_tol)
                    {
                        step = better;
                    }
                }
            }
            let Step {
                d, lambda, reduced, ..
            } = step;
            let infeasible = c.iter().fold(0.0, |a: f64, v| a.max(v.abs()));
            if d.norm() <= STEP * (1.0 + x.norm()) {
                break 'run if infeasible <= FEASIBLE {
                    Termination::Converged
                } else {
                    Termination::Stalled
                };
            }
            // ℓ₁ merit, and its directional derivative along d (the
            // linearised constraints are met at the end of d).
            if lambda.amax() + 1.0 > rho {
                rho = lambda.amax() + 2.0;
            }
            let merit = |x: &DVector<f64>| -> Option<f64> {
                if !layout.admits(x) {
                    return None;
                }
                let (r, singular) = evaluate(&layout, eqs, x);
                (singular.is_empty() && r.iter().all(|v| v.is_finite()))
                    .then(|| cost(x) + rho * r.lp_norm(1))
            };
            let here = cost(&x) + rho * c.lp_norm(1);
            let slope = gradient(&x).dot(&d) - rho * c.lp_norm(1);
            // A step that cannot change the objective measurably, onto
            // constraints that hold, is the optimum: the Gauss–Newton
            // iteration is linear, and below rounding it only stalls.
            // On constraints that hold, the optimum is where the objective
            // gains nothing more along them: the reduced gradient, not the
            // merit's slope, whose penalty on residuals at rounding is a
            // decrease no step can make.
            if infeasible <= FEASIBLE && reduced <= GAIN * cost(&x) {
                break 'run Termination::Converged;
            }
            let mut alpha = 1.0;
            let mut moved = false;
            for _ in 0..HALVINGS {
                let trial = &x + alpha * &d;
                if let Some(f) = merit(&trial) {
                    if f <= here + ARMIJO * alpha * slope.min(0.0) && f < here {
                        x = trial;
                        moved = true;
                        break;
                    }
                }
                alpha *= 0.5;
            }
            if !moved {
                break 'run Termination::Stalled;
            }
        }
        Termination::OutOfPatience
    };

    // The multipliers and the unheld directions, at the solution.
    let (c, j, singular) = linearise(&layout, eqs, &x, o.jacobian);
    let last =
        (layout.n > 0 && singular.is_none() && c.iter().chain(j.iter()).all(|v| v.is_finite()))
            .then(|| kkt_step(&j, &c, gradient, &w, None, &x, o.rank_tol))
            .flatten();
    let (multipliers, unheld) = match last {
        Some(Step { lambda, unheld, .. }) => {
            let mut at = 0;
            let per_equation = eqs
                .iter()
                .map(|e| {
                    at += e.count();
                    lambda.rows(at - e.count(), e.count()).amax()
                })
                .collect();
            (per_equation, unheld)
        }
        None => (vec![f64::NAN; eqs.len()], 0),
    };
    let all: Vec<EntityId> = prior.geometry.iter().map(|(id, _)| *id).collect();
    let cost = cost(&x);
    let solution = report_at(
        &prior.geometry,
        &layout,
        eqs,
        &[all],
        x,
        o,
        evaluations,
        termination,
    )?;
    let r = &solution.report;
    let converged = matches!(
        r.termination,
        Termination::Converged | Termination::NothingToSolve
    ) && r.residual_max <= FEASIBLE;
    Ok(Nearest {
        solution,
        cost,
        converged,
        multipliers,
        unheld,
    })
}

#[cfg(test)]
mod tests {
    use super::super::equation::*;
    use super::super::solve::{residuals, solve, Jacobian};
    use super::*;
    use crate::geometry;
    use crate::value::geometry::{Entity, Model, P2};
    use approx::assert_abs_diff_eq;
    use proptest::prelude::*;

    /// The weight `w` times the identity, on each of the entities.
    fn uniform(g: &Geometry<Model>, w: f64) -> BTreeMap<EntityId, DMatrix<f64>> {
        g.iter()
            .map(|(id, e)| (*id, DMatrix::identity(e.values(), e.values()) * w))
            .collect()
    }

    fn near(
        g: &Geometry<Model>,
        weights: BTreeMap<EntityId, DMatrix<f64>>,
        eqs: &[Equation],
    ) -> Nearest<Model> {
        let prior = Prior {
            geometry: g.clone(),
            weights,
        };
        solve_near(&prior, eqs, Options::default()).unwrap()
    }

    fn at(g: &Geometry<Model>, id: &str) -> P2<Model> {
        match g.get(id).unwrap() {
            Entity::Point { at } => *at,
            e => panic!("not a point: {e:?}"),
        }
    }

    /// A rectangle drawn as four corners, with a width and a height read.
    fn rectangle_equations(w: f64, h: f64) -> Vec<Equation> {
        let (p0, p1, p2, p3) = (point("p0"), point("p1"), point("p2"), point("p3"));
        vec![
            same_y(p0, p1),
            same_x(p1, p2),
            same_y(p2, p3),
            same_x(p3, p0),
            dx(p0, p1, w),
            dy(p1, p2, h),
        ]
    }

    fn corners(c: [(f64, f64); 4]) -> Geometry<Model> {
        geometry!(Model {
            p0: point (c[0].0, c[0].1);
            p1: point (c[1].0, c[1].1);
            p2: point (c[2].0, c[2].1);
            p3: point (c[3].0, c[3].1);
        })
    }

    fn corners_distance(a: &Geometry<Model>, b: &Geometry<Model>) -> f64 {
        ["p0", "p1", "p2", "p3"]
            .iter()
            .map(|id| {
                let (p, q) = (at(a, id), at(b, id));
                (p.x - q.x).powi(2) + (p.y - q.y).powi(2)
            })
            .sum()
    }

    /// Equal weights, linear constraints: the answer is the orthogonal
    /// projection of the prior onto the constraints. For the rectangle
    /// that is the corners' mean on each shared coordinate, shifted to the
    /// read width and height.
    #[test]
    fn a_noisy_rectangle_is_projected_onto_the_read_size() {
        let prior = corners([(0.02, -0.01), (4.1, 0.03), (3.95, 2.04), (-0.05, 1.96)]);
        let (w, h) = (4.0, 2.0);
        let n = near(&prior, uniform(&prior, 1.0), &rectangle_equations(w, h));
        assert!(n.converged, "{:?}", n.solution.report);
        // a = x of the left side, c = y of the bottom side.
        let a = (0.02 + 4.1 + 3.95 - 0.05 - 2.0 * w) / 4.0;
        let c = (-0.01 + 0.03 + 2.04 + 1.96 - 2.0 * h) / 4.0;
        let g = &n.solution.geometry;
        for (id, x, y) in [
            ("p0", a, c),
            ("p1", a + w, c),
            ("p2", a + w, c + h),
            ("p3", a, c + h),
        ] {
            let p = at(g, id);
            assert_abs_diff_eq!(p.x, x, epsilon = 1e-9);
            assert_abs_diff_eq!(p.y, y, epsilon = 1e-9);
        }
        // The rectangle's position is left free, and the report says so.
        let r = &n.solution.report;
        assert_eq!((r.parameters, r.rank, r.redundant), (8, 6, 0));
        assert_eq!(r.free, 2);
        // The cost is the squared distance moved, halved.
        assert_abs_diff_eq!(n.cost, 0.5 * corners_distance(&prior, g), epsilon = 1e-9);
    }

    /// A point on a circle of fixed centre and radius: the nearest point
    /// of the circle to the prior, along the ray from the centre.
    #[test]
    fn a_point_is_projected_onto_a_circle() {
        let prior = geometry!(Model {
            p: point (3.0, 4.0);
            c: circle (0.0, 0.0), r = 2.0;
        });
        let c = circle("c");
        let eqs = [
            pinned(c.centre(), 0.0, 0.0),
            radius(c, 2.0),
            on_circle(point("p"), c),
        ];
        let n = near(&prior, uniform(&prior, 1.0), &eqs);
        assert!(n.converged, "{:?}", n.solution.report);
        let p = at(&n.solution.geometry, "p");
        assert_abs_diff_eq!(p.x, 1.2, epsilon = 1e-9);
        assert_abs_diff_eq!(p.y, 1.6, epsilon = 1e-9);
        assert_abs_diff_eq!(n.cost, 0.5 * 9.0, epsilon = 1e-9);
    }

    /// A sweep of priors (the radius and the point's weight varying over
    /// forty-one steps): every solve ends at a first-order optimum with
    /// the constraints holding. (A regression guard only: the stall this
    /// round fixed was seen on a real sheet, not reproduced here.)
    #[test]
    fn a_sweep_of_priors_always_converges() {
        for k in 0..41 {
            let f = 1.0 + 0.0125 * k as f64;
            let prior = geometry!(Model {
                p: point (3.0 * f, 4.0 * f);
                q: point (1.0, 7.0 * f);
                c: circle (0.0, 0.0), r = 2.0 * f;
            });
            let c = circle("c");
            let eqs = [
                pinned(c.centre(), 0.0, 0.0),
                radius(c, 2.0),
                on_circle(point("p"), c),
                dy(point("p"), point("q"), 1.5),
            ];
            let mut w = uniform(&prior, 1.0);
            w.insert(EntityId::new("p"), DMatrix::identity(2, 2) * (5e4 * f * f));
            w.insert(EntityId::new("c"), DMatrix::identity(3, 3) * 1e-2);
            let n = near(&prior, w, &eqs);
            assert!(n.converged, "k = {k}: {:?}", n.solution.report);
        }
    }

    /// The reduced gradient is the stationarity measure: on a feasible
    /// point that is not the optimum it gains something along the
    /// constraints, at the optimum nothing.
    #[test]
    fn the_reduced_gradient_measures_stationarity() {
        // x0 + x1 = 0, prior (1, 0), weight I: the optimum is (½, −½).
        let j = DMatrix::from_row_slice(1, 2, &[1.0, 1.0]);
        let (c, p) = (DVector::zeros(1), DVector::from_vec(vec![1.0, 0.0]));
        let w = DMatrix::identity(2, 2);
        let at = |x: [f64; 2]| {
            let x = DVector::from_vec(x.to_vec());
            kkt_step(&j, &c, |y| &w * (y - &p), &w, None, &x, 1e-9)
                .unwrap()
                .reduced
        };
        assert_abs_diff_eq!(at([0.0, 0.0]), 0.25, epsilon = 1e-12);
        assert_abs_diff_eq!(at([0.5, -0.5]), 0.0, epsilon = 1e-12);
    }

    #[test]
    fn with_no_constraints_the_prior_is_returned() {
        let prior = corners([(0.02, -0.01), (4.1, 0.03), (3.95, 2.04), (-0.05, 1.96)]);
        let n = near(&prior, uniform(&prior, 3.0), &[]);
        assert!(n.converged);
        assert_eq!(n.solution.geometry, prior);
        assert_eq!(n.cost, 0.0);
        assert_eq!(n.solution.report.termination, Termination::NothingToSolve);
    }

    /// A prior that already satisfies the constraints does not move, with
    /// any weights.
    #[test]
    fn a_satisfied_prior_stays() {
        let prior = corners([(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)]);
        let n = near(&prior, uniform(&prior, 7.0), &rectangle_equations(4.0, 2.0));
        assert!(n.converged);
        assert_abs_diff_eq!(n.cost, 0.0, epsilon = 1e-18);
        assert_abs_diff_eq!(
            corners_distance(&prior, &n.solution.geometry),
            0.0,
            epsilon = 1e-18
        );
    }

    /// A square system has one solution; the weights do not matter and it
    /// is the one `solve` finds from the same start.
    #[test]
    fn a_square_system_is_what_solve_finds() {
        let prior = geometry!(Model {
            c: circle (0.1, -0.1), r = 1.8;
            p: point (0.9, 1.8);
            q: point (1.2, 5.3);
        });
        let (c, p, q) = (circle("c"), point("p"), point("q"));
        let eqs = [
            pinned(c.centre(), 0.0, 0.0),
            radius(c, 2.0),
            on_circle(p, c),
            same_x(p, q),
            pinned(q, 1.0, 5.0),
        ];
        let exact = solve(&prior, &eqs, Options::default()).unwrap();
        assert_eq!(exact.report.free, 0);
        let mut weights = uniform(&prior, 1.0);
        weights.insert("p".into(), DMatrix::identity(2, 2) * 50.0);
        weights.remove(&EntityId::from("q"));
        let n = near(&prior, weights, &eqs);
        assert!(n.converged, "{:?}", n.solution.report);
        let tol = crate::solver::tolerance::Tolerance::new(1e-8, 1e-8);
        assert_abs_diff_eq!(n.solution.geometry, exact.geometry, epsilon = tol);
        let p = at(&n.solution.geometry, "p");
        assert_abs_diff_eq!(p.y, 3.0f64.sqrt(), epsilon = 1e-8);
    }

    /// Two points a read distance apart, each weighed: the shortfall is
    /// shared in inverse proportion to the weights.
    #[test]
    fn the_heavier_weight_moves_less() {
        let prior = geometry!(Model { a: point (0.0, 0.0); b: point (6.0, 0.0); });
        let eqs = [
            dx(point("a"), point("b"), 5.0),
            same_y(point("a"), point("b")),
        ];
        let weigh = |wa: f64, wb: f64| {
            let mut m = uniform(&prior, 1.0);
            m.insert("a".into(), DMatrix::identity(2, 2) * wa);
            m.insert("b".into(), DMatrix::identity(2, 2) * wb);
            near(&prior, m, &eqs).solution.geometry
        };
        // Excess 1 mm: a moves wb/(wa+wb), b moves wa/(wa+wb).
        let g = weigh(1.0, 3.0);
        assert_abs_diff_eq!(at(&g, "a").x, 0.75, epsilon = 1e-9);
        assert_abs_diff_eq!(at(&g, "b").x, 6.0 - 0.25, epsilon = 1e-9);
        let g = weigh(1000.0, 1.0);
        assert!(at(&g, "a").x.abs() < 2e-3);
    }

    proptest! {
        /// For any weights on a line's two ends, the shortfall is shared
        /// in inverse proportion, so the heavier end moves no more.
        #[test]
        fn heavier_weights_move_a_line_less(
            wa in 1e-3..1e3f64, wb in 1e-3..1e3f64, read in 1.0..9.0f64,
        ) {
            let prior = geometry!(Model { l: line (0.0, 0.0) -> (6.0, 0.0); });
            let l = line("l");
            let mut m = BTreeMap::new();
            m.insert(
                EntityId::from("l"),
                DMatrix::from_diagonal(&DVector::from_vec(vec![wa, wa, wb, wb])),
            );
            let n = near(&prior, m, &[length(l, read), same_y(l.start(), l.end())]);
            prop_assert!(n.converged);
            let Entity::Line { start, end } = n.solution.geometry.get("l").unwrap().clone() else {
                unreachable!()
            };
            prop_assert!(((end.x - start.x) - read).abs() < 1e-8);
            let (ma, mb) = (start.x.abs(), (end.x - 6.0).abs());
            prop_assert!((ma * wa - mb * wb).abs() < 1e-6 * (ma * wa + mb * wb + 1e-9));
            if wa > wb {
                prop_assert!(ma <= mb + 1e-9);
            }
        }

        /// Linear constraints and any symmetric positive definite weight
        /// (correlated, as a covariance gives): the answer is the
        /// closed-form weighted projection `p − W⁻¹Aᵀ(A W⁻¹ Aᵀ)⁻¹(Ap − b)`.
        #[test]
        fn linear_constraints_give_the_weighted_projection(
            p in proptest::collection::vec(-5.0..5.0f64, 4),
            lt in proptest::collection::vec(-1.0..1.0f64, 16),
            (a, b) in (0.5..4.0f64, -3.0..3.0f64),
        ) {
            let prior = geometry!(Model {
                s: point (p[0], p[1]);
                t: point (p[2], p[3]);
            });
            // A weight per point, W = LᵀL + I on each.
            let l = DMatrix::from_row_slice(4, 4, &lt);
            let w = l.transpose() * &l + DMatrix::identity(4, 4);
            let blocks = [w.view((0, 0), (2, 2)).into_owned(), w.view((2, 2), (2, 2)).into_owned()];
            let m = BTreeMap::from([
                (EntityId::from("s"), blocks[0].clone()),
                (EntityId::from("t"), blocks[1].clone()),
            ]);
            let eqs = [dx(point("s"), point("t"), a), dy(point("s"), point("t"), b)];
            let n = near(&prior, m, &eqs);
            prop_assert!(n.converged);
            // The closed form, over the block-diagonal W.
            let mut wf = DMatrix::zeros(4, 4);
            wf.view_mut((0, 0), (2, 2)).copy_from(&blocks[0]);
            wf.view_mut((2, 2), (2, 2)).copy_from(&blocks[1]);
            let am = DMatrix::from_row_slice(2, 4, &[-1.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 1.0]);
            let bv = DVector::from_vec(vec![a, b]);
            let pv = DVector::from_vec(p.clone());
            let winv = wf.try_inverse().unwrap();
            let k = &am * &winv * am.transpose();
            let x = &pv - &winv * am.transpose() * k.try_inverse().unwrap() * (&am * &pv - bv);
            let (s, t) = (at(&n.solution.geometry, "s"), at(&n.solution.geometry, "t"));
            for (got, want) in [s.x, s.y, t.x, t.y].iter().zip(x.iter()) {
                prop_assert!((got - want).abs() < 1e-7, "{got} {want}");
            }
        }

        /// The answer is a constrained optimum, by an independent
        /// derivative: with the constraints' Jacobian taken by finite
        /// differences, the gradient of the departure is in its row space
        /// (KKT stationarity) and the residuals vanish.
        #[test]
        fn the_solution_is_stationary_by_finite_differences(
            dp in proptest::collection::vec(-0.3..0.3f64, 7),
            wts in proptest::collection::vec(0.1..10.0f64, 3),
        ) {
            let prior = geometry!(Model {
                c: circle (0.0 + dp[0], 0.0 + dp[1]), r = 2.0 + dp[2];
                p: point (1.0 + dp[3], 1.7 + dp[4]);
                q: point (1.5 + dp[5], 4.8 + dp[6]);
            });
            let (c, p, q) = (circle("c"), point("p"), point("q"));
            let eqs = [on_circle(p, c), same_x(p, q), distance(q, c.centre(), 5.0)];
            let m = BTreeMap::from([
                (EntityId::from("c"), DMatrix::identity(3, 3) * wts[0]),
                (EntityId::from("p"), DMatrix::identity(2, 2) * wts[1]),
                (EntityId::from("q"), DMatrix::identity(2, 2) * wts[2]),
            ]);
            let n = near(&prior, m.clone(), &eqs);
            prop_assert!(n.converged, "{:?}", n.solution.report);
            let g = &n.solution.geometry;
            let layout = Layout::of(g);
            let x = layout.flatten(g);
            let pr = layout.flatten(&prior);
            let w = weight_matrix(&layout, &Prior { geometry: prior.clone(), weights: m }).unwrap();
            let grad = &w * (&x - &pr);
            let at_x = |x: &DVector<f64>| -> DVector<f64> {
                let g = layout.unflatten(&prior, x).unwrap();
                DVector::from_vec(residuals(&g, &eqs).unwrap().concat())
            };
            let h = 1e-6;
            let cols: Vec<DVector<f64>> = (0..x.len())
                .map(|i| {
                    let (mut u, mut d) = (x.clone(), x.clone());
                    u[i] += h;
                    d[i] -= h;
                    (at_x(&u) - at_x(&d)) / (2.0 * h)
                })
                .collect();
            let jt = DMatrix::from_columns(&cols).transpose();
            // grad = −Jᵀ λ for some λ: least squares leaves no remainder.
            let lambda = jt.clone().svd(true, true).solve(&(-&grad), 1e-9).unwrap();
            prop_assert!((&jt * lambda + &grad).norm() < 1e-5 * (1.0 + grad.norm()));
            prop_assert!(at_x(&x).amax() < 1e-9);
        }
    }

    /// Constraints that cannot all hold are reported, not solved, and not
    /// a panic: the group is named and the solve did not converge.
    #[test]
    fn conflicting_constraints_are_reported() {
        let prior = geometry!(Model { l: line (0.0, 0.0) -> (10.0, 0.0); });
        let l = line("l");
        let eqs = [
            pinned(l.start(), 0.0, 0.0),
            pinned(l.end(), 10.0, 0.0),
            length(l, 12.0),
        ];
        let n = near(&prior, uniform(&prior, 1.0), &eqs);
        assert!(!n.converged);
        let r = &n.solution.report;
        assert_eq!(r.dependencies.len(), 1);
        assert_eq!(r.dependencies[0].equations, vec![0, 1, 2]);
        assert!(r.dependencies[0].residual > 0.1);
        assert!(r.residual_max > 0.1);
    }

    /// Redundant but consistent constraints are solved, and named.
    #[test]
    fn redundant_constraints_are_solved_and_named() {
        let prior = geometry!(Model { l: line (0.1, 0.0) -> (9.8, 0.1); });
        let l = line("l");
        let eqs = [
            pinned(l.start(), 0.0, 0.0),
            pinned(l.end(), 10.0, 0.0),
            length(l, 10.0),
        ];
        let n = near(&prior, uniform(&prior, 1.0), &eqs);
        assert!(n.converged, "{:?}", n.solution.report);
        assert_eq!(n.solution.report.redundant, 1);
        assert_eq!(n.solution.report.dependencies.len(), 1);
        assert!(n.solution.report.dependencies[0].residual < 1e-9);
    }

    /// A prior at a point where an equation has no derivative is
    /// reported as degenerate.
    #[test]
    fn a_degenerate_prior_is_reported() {
        let prior = geometry!(Model { a: point (1.0, 1.0); b: point (1.0, 1.0); });
        let eqs = [distance(point("a"), point("b"), 2.0)];
        let n = near(&prior, uniform(&prior, 1.0), &eqs);
        assert!(!n.converged);
        assert!(matches!(
            n.solution.report.termination,
            Termination::Degenerate { equation: 0, .. }
        ));
    }

    #[test]
    fn bad_weights_are_refused() {
        let prior = geometry!(Model { a: point (1.0, 1.0); });
        let id = EntityId::from("a");
        let weigh = |id: EntityId, m: DMatrix<f64>| {
            let weights = BTreeMap::from([(id, m)]);
            let prior = Prior {
                geometry: prior.clone(),
                weights,
            };
            solve_near(&prior, &[], Options::default()).err()
        };
        let bad = Some(Error::BadWeight(id));
        assert_eq!(weigh(id, DMatrix::identity(3, 3)), bad);
        assert_eq!(
            weigh(id, DMatrix::from_row_slice(2, 2, &[1.0, 2.0, 0.0, 1.0])),
            bad
        );
        assert_eq!(
            weigh(id, DMatrix::from_row_slice(2, 2, &[1.0, 2.0, 2.0, 1.0])),
            bad
        );
        assert_eq!(weigh(id, DMatrix::from_element(2, 2, f64::NAN)), bad);
        let stray = EntityId::from("zz");
        assert_eq!(
            weigh(stray, DMatrix::identity(2, 2)),
            Some(Error::BadWeight(stray))
        );
    }

    /// The dense and the local Jacobian give the same answer.
    #[test]
    fn both_jacobians_agree() {
        let g = corners([(0.02, -0.01), (4.1, 0.03), (3.95, 2.04), (-0.05, 1.96)]);
        let prior = Prior {
            weights: uniform(&g, 2.0),
            geometry: g,
        };
        let eqs = rectangle_equations(4.0, 2.0);
        let a = solve_near(&prior, &eqs, Options::default()).unwrap();
        let dense = Options::default().with_jacobian(Jacobian::Dense);
        let b = solve_near(&prior, &eqs, dense).unwrap();
        assert_abs_diff_eq!(a.cost, b.cost, epsilon = 1e-12);
    }

    /// A circle, a point on it and a point level with that one, the three
    /// departing from the system by `dp` (seven values).
    fn circle_system(dp: &[f64]) -> (Geometry<Model>, Vec<Equation>) {
        let prior = geometry!(Model {
            c: circle (0.0 + dp[0], 0.0 + dp[1]), r = 2.0 + dp[2];
            p: point (1.0 + dp[3], 1.7 + dp[4]);
            q: point (1.5 + dp[5], 4.8 + dp[6]);
        });
        let (c, p, q) = (circle("c"), point("p"), point("q"));
        let eqs = vec![on_circle(p, c), same_x(p, q), distance(q, c.centre(), 5.0)];
        (prior, eqs)
    }

    /// Correlated weights, a block per entity: `LᵀL + I`.
    fn correlated(g: &Geometry<Model>, l: &[f64]) -> BTreeMap<EntityId, DMatrix<f64>> {
        let mut at = 0;
        g.iter()
            .map(|(id, e)| {
                let k = e.values();
                let m = DMatrix::from_row_slice(k, k, &l[at..at + k * k]);
                at += k * k;
                (*id, m.transpose() * &m + DMatrix::identity(k, k))
            })
            .collect()
    }

    fn departure(
        prior: &Geometry<Model>,
        weights: &BTreeMap<EntityId, DMatrix<f64>>,
        g: &Geometry<Model>,
    ) -> f64 {
        let layout = Layout::of(prior);
        let w = weight_matrix(
            &layout,
            &Prior {
                geometry: prior.clone(),
                weights: weights.clone(),
            },
        )
        .unwrap();
        let d = layout.flatten(g) - layout.flatten(prior);
        0.5 * d.dot(&(w * &d))
    }

    proptest! {
        /// At small departures, with correlated weights, the answer is
        /// no further from the prior (in the weights) than the solution
        /// `solve` finds from the same start: a minimum, not just a
        /// stationary point.
        #[test]
        fn the_answer_is_no_further_than_solve_finds(
            dp in proptest::collection::vec(-0.2..0.2f64, 7),
            l in proptest::collection::vec(-1.0..1.0f64, 9 + 4 + 4),
        ) {
            let (prior, eqs) = circle_system(&dp);
            let weights = correlated(&prior, &l);
            let n = near(&prior, weights.clone(), &eqs);
            prop_assert!(n.converged, "{:?}", n.solution.report);
            let plain = solve(&prior, &eqs, Options::default()).unwrap();
            if plain.report.residual_max <= 1e-9 {
                let theirs = departure(&prior, &weights, &plain.geometry);
                prop_assert!(n.cost <= theirs + 1e-9, "{} > {}", n.cost, theirs);
            }
        }
    }

    /// Weights known only up to a factor give the same answer.
    #[test]
    fn the_weights_scale_does_not_matter() {
        let (prior, eqs) = circle_system(&[0.05, -0.04, 0.03, 0.1, -0.08, 0.06, -0.1]);
        let weights = correlated(&prior, &[0.3; 17]);
        let big: BTreeMap<_, _> = weights.iter().map(|(k, w)| (*k, w * 1e6)).collect();
        let small: BTreeMap<_, _> = weights.iter().map(|(k, w)| (*k, w * 1e-6)).collect();
        let (a, b, c) = (
            near(&prior, weights, &eqs),
            near(&prior, big, &eqs),
            near(&prior, small, &eqs),
        );
        let tol = crate::solver::tolerance::Tolerance::new(1e-7, 1e-7);
        for n in [&b, &c] {
            assert!(n.converged);
            assert_abs_diff_eq!(n.solution.geometry, a.solution.geometry, epsilon = tol);
        }
    }

    /// A weight that is singular (x only) is well-posed: the point keeps
    /// its x, the unweighed point takes up the constraint, and the two
    /// directions nothing holds are counted.
    #[test]
    fn a_semi_definite_weight_is_solved() {
        let prior = geometry!(Model { p: point (0.0, 0.0); q: point (2.0, 0.0); });
        let weights = BTreeMap::from([(
            EntityId::from("p"),
            DMatrix::from_diagonal(&DVector::from_vec(vec![1.0, 0.0])),
        )]);
        let n = near(&prior, weights, &[distance(point("p"), point("q"), 3.0)]);
        assert!(n.converged, "{:?}", n.solution.report);
        assert_abs_diff_eq!(at(&n.solution.geometry, "p").x, 0.0, epsilon = 1e-9);
        assert_abs_diff_eq!(n.cost, 0.0, epsilon = 1e-9);
        // 4 values, 1 constraint, 1 weighed direction.
        assert_eq!(n.unheld, 2);
        // The multiplier of a constraint the prior pays nothing for.
        assert_abs_diff_eq!(n.multipliers[0], 0.0, epsilon = 1e-9);
    }

    /// The multiplier is the pull of the equation on the design: for two
    /// points 1 mm too far apart it is the shared cost per mm.
    #[test]
    fn multipliers_measure_the_pull() {
        let prior = geometry!(Model { a: point (0.0, 0.0); b: point (6.0, 0.0); });
        let eqs = [
            dx(point("a"), point("b"), 5.0),
            same_y(point("a"), point("b")),
        ];
        let weights = BTreeMap::from([
            (EntityId::from("a"), DMatrix::identity(2, 2) * 1.0),
            (EntityId::from("b"), DMatrix::identity(2, 2) * 3.0),
        ]);
        let n = near(&prior, weights, &eqs);
        // Moves 0.75 and 0.25: the cost ½·1·0.75² + ½·3·0.25² = 3/8, and
        // its rate per mm of the constraint is wa·wb/(wa+wb) · excess.
        assert_abs_diff_eq!(n.cost, 3.0 / 8.0, epsilon = 1e-9);
        assert_abs_diff_eq!(n.multipliers[0].abs(), 0.75, epsilon = 1e-9);
        assert_abs_diff_eq!(n.multipliers[1], 0.0, epsilon = 1e-9);
        assert_eq!(n.unheld, 0);
    }

    /// Far departures: a point near the centre of a circle goes to the
    /// nearest point of it, along its own ray, from every side. (Every
    /// step of these is accepted whole; the line search is a safeguard
    /// that no case I could build needs.)
    #[test]
    fn a_far_point_takes_the_nearest_branch() {
        for (x, y) in [
            (0.1, 0.1),
            (-0.3, 0.05),
            (0.02, -0.4),
            (-0.1, -0.1),
            (1.9, 0.0),
        ] {
            let prior = geometry!(Model {
                p: point (x, y);
                c: circle (0.0, 0.0), r = 2.0;
            });
            let c = circle("c");
            let eqs = [
                pinned(c.centre(), 0.0, 0.0),
                radius(c, 2.0),
                on_circle(point("p"), c),
            ];
            let n = near(&prior, uniform(&prior, 1.0), &eqs);
            assert!(n.converged, "{:?}", n.solution.report);
            let k = 2.0 / f64::hypot(x, y);
            let p = at(&n.solution.geometry, "p");
            assert_abs_diff_eq!(p.x, x * k, epsilon = 1e-8);
            assert_abs_diff_eq!(p.y, y * k, epsilon = 1e-8);
        }
    }
}
