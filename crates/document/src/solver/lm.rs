//! Levenberg–Marquardt as MINPACK's `lmder` (Moré, 1978), on `faer`'s
//! pivoted QR. The same algorithm, in the same arithmetic, as the
//! `levenberg-marquardt` crate, but for one thing: a column whose norm at
//! the start is rounding is scaled as a zero column is (MINPACK takes
//! rounding at its word, and lets the step move that parameter without
//! bound). Otherwise only the factorisation differs, so the outcomes are
//! the same and the solve is faster.
//!
//! The step solves `min ‖J p − r‖` subject to `‖D p‖ ≤ Δ`, with `D` the
//! column norms of `J` (never decreasing) and `Δ` a trust radius, adjusted
//! by the ratio of actual to predicted decrease. When the Gauss–Newton step
//! fits the radius it is taken as is, from the pivoted QR with the
//! components of dependent columns set to zero, so the parameters an
//! under-determined system leaves free stay where they are: the solution
//! nearest the start, which the sketch relies on for its branch choice.
//! Otherwise λ > 0 is found by Newton's method on `‖D p(λ)‖ = Δ`, each
//! trial an O(n²) elimination of `√λ D` from `[R; √λ D]` by Givens
//! rotations (`qrsolv`), never a new factorisation.

use super::faer_backend::LeastSquares;
use super::solve::{Singular, Termination};
use nalgebra::{DMatrix, DVector};

/// What one iteration needs from the problem.
pub(crate) trait Residuals {
    /// Residuals at `x`, or `None` if `x` is outside the problem's domain
    /// (there the objective counts as +∞, so a step there is rejected).
    fn residuals(&self, x: &DVector<f64>) -> Option<DVector<f64>>;
    /// Residuals and Jacobian at `x`, or the equation with no derivative.
    fn residuals_and_jacobian(
        &self,
        x: &DVector<f64>,
    ) -> Result<(DVector<f64>, DMatrix<f64>), Singular>;
}

/// Tolerances, in the crate's sense: `ftol` on the relative decrease of
/// the objective, `xtol` on the relative step, `gtol` on the scaled
/// gradient; `patience` bounds residual evaluations.
pub(crate) struct Settings {
    pub patience: usize,
    pub ftol: f64,
    pub xtol: f64,
    pub gtol: f64,
}

/// The initial trust radius as a multiple of `‖D x‖` (MINPACK's `factor`).
const STEP_BOUND: f64 = 100.0;
const P1: f64 = 0.1;
const P0001: f64 = 1.0e-4;
const EPS: f64 = f64::EPSILON;
/// A residual norm at or below this is zero for the drawing, mm: the
/// stop is a solution whatever the gradient says of rounding.
const RESIDUAL_ZERO: f64 = 1e-9;
/// The scaled gradient (the largest cosine between a column of the
/// Jacobian and the residual) at or below this is a stationary point.
const STATIONARY: f64 = 1e-6;
/// Column norms at or below this fraction of the scale are rounding,
/// for the scaling: √ε.
const NOISE: f64 = 1.4901161193847656e-8;

/// Why the solve failed when a residual or derivative is NaN or infinite.
pub(crate) const NOT_FINITE: &str = "a residual or derivative is not finite";

/// The trust-region subproblem (MINPACK's `lmpar`): the step `p`, the λ
/// that gave it, and `‖D p‖`.
fn lmpar(
    ls: &LeastSquares,
    diag: &DVector<f64>,
    delta: f64,
    initial_lambda: f64,
) -> (DVector<f64>, f64, f64) {
    let n = diag.len();
    let is_non_singular = ls.is_non_singular();
    let (mut p, factor) = ls.solve_with_zero_diagonal();
    let mut diag_p = p.component_mul(diag);
    let mut diag_p_norm = diag_p.norm();
    let mut fp = diag_p_norm - delta;
    if fp <= delta * P1 {
        return (p, 0.0, diag_p_norm);
    }
    let mut lambda_lower = if is_non_singular {
        let q = (&diag_p / diag_p_norm).component_mul(diag);
        let norm = factor.solve(&q).norm();
        ((fp / delta) / norm) / norm
    } else {
        0.0
    };
    let gnorm;
    let mut lambda_upper = {
        let mut g = factor.mul_qt_b();
        for j in 0..n {
            g[j] /= diag[ls_perm(ls, j)];
        }
        gnorm = g.norm();
        let upper = gnorm / delta;
        if upper == 0.0 {
            f64::MIN_POSITIVE / delta.min(P1)
        } else {
            upper
        }
    };
    let mut lambda = initial_lambda.max(lambda_lower).min(lambda_upper);
    if lambda == 0.0 {
        lambda = gnorm / diag_p_norm;
    }
    for iteration in 1.. {
        if lambda == 0.0 {
            lambda = f64::MIN_POSITIVE.max(lambda_upper * 0.001);
        }
        let scaled = diag * lambda.sqrt();
        let (p_new, factor) = ls.solve_with_diagonal(&scaled);
        p = p_new;
        diag_p = p.component_mul(diag);
        diag_p_norm = diag_p.norm();
        if iteration == 10 {
            break;
        }
        let fp_old = fp;
        fp = diag_p_norm - delta;
        if fp.abs() <= delta * P1 || (lambda_lower == 0.0 && fp <= fp_old && fp_old < 0.0) {
            break;
        }
        let newton_correction = {
            let q = (&diag_p / diag_p_norm).component_mul(diag);
            let norm = factor.solve(&q).norm();
            ((fp / delta) / norm) / norm
        };
        if fp > 0.0 {
            lambda_lower = lambda_lower.max(lambda);
        } else {
            lambda_upper = lambda_upper.min(lambda);
        }
        lambda = lambda_lower.max(lambda + newton_correction);
    }
    (p, lambda, diag_p_norm)
}

fn ls_perm(ls: &LeastSquares, j: usize) -> usize {
    ls.perm(j)
}

/// Minimise ‖r(x)‖² from `x0`. Returns the final `x`, the number of
/// residual evaluations, and why it stopped.
pub(crate) fn minimize<P: Residuals>(
    p: &P,
    x0: DVector<f64>,
    s: &Settings,
) -> (DVector<f64>, usize, Termination) {
    let mut x = x0;
    let mut evaluations = 1;
    let (mut r, mut j) = match p.residuals_and_jacobian(&x) {
        Ok(rj) => rj,
        Err(s) => return (x, evaluations, Termination::degenerate(s)),
    };
    let mut rnorm = r.norm();
    if rnorm <= f64::MIN_POSITIVE {
        return (x, evaluations, Termination::ResidualsZero);
    }
    let n = j.ncols();
    let mut diag = DVector::zeros(n);
    let mut xnorm: f64 = 0.0;
    let mut delta: f64 = 0.0;
    let mut lambda: f64 = 0.0;
    let mut first_update = true;
    let mut first_trust_region = true;
    loop {
        // A NaN or infinite derivative makes every step NaN: no step is
        // ever accepted and the patience runs out. Say so at once.
        if !r.iter().chain(j.iter()).all(|v| v.is_finite()) {
            return (x, evaluations, Termination::Failed(NOT_FINITE));
        }
        let ls = LeastSquares::new(&j, &r);
        let gnorm = ls.max_a_t_b_scaled(rnorm);
        if gnorm <= s.gtol {
            return (x, evaluations, Termination::Converged);
        }
        if first_update {
            // A column whose norm is rounding carries no scale: the
            // parameter has no first-order effect, and a norm of the
            // order of ε would let the trust region move it without
            // bound. It is scaled as a zero column is, by 1. Rounding is
            // judged against the largest column, and against the
            // residual over the drawing's extent (at an extreme of every
            // residual all the columns are rounding).
            let largest = ls.column_norms.iter().fold(0.0f64, |a, &c| a.max(c));
            let extent = x.amax().max(1.0);
            let noise = NOISE * largest.max(rnorm / extent);
            for k in 0..n {
                diag[k] = if ls.column_norms[k] <= noise {
                    1.0
                } else {
                    ls.column_norms[k]
                };
            }
            xnorm = x.component_mul(&diag).norm();
            delta = if xnorm == 0.0 {
                STEP_BOUND
            } else {
                STEP_BOUND * xnorm
            };
            first_update = false;
        } else {
            for k in 0..n {
                diag[k] = diag[k].max(ls.column_norms[k]);
            }
        }
        // Inner loop: shrink the radius until a step is accepted.
        loop {
            let (step, new_lambda, pnorm) = lmpar(&ls, &diag, delta, lambda);
            lambda = new_lambda;
            let temp1 = (ls.a_x_norm(&step) / rnorm).powi(2);
            let temp2 = ((lambda.sqrt() * pnorm) / rnorm).powi(2);
            let predicted = temp1 + temp2 / 0.5;
            let dir_der = -(temp1 + temp2);
            if first_trust_region && pnorm < delta {
                delta = pnorm;
            }
            first_trust_region = false;
            let x_new = &x - &step;
            let rnorm_new = p.residuals(&x_new).map_or(f64::INFINITY, |r| r.norm());
            evaluations += 1;
            let actual = if rnorm_new * P1 < rnorm {
                1.0 - (rnorm_new / rnorm).powi(2)
            } else {
                -1.0
            };
            let ratio = if predicted == 0.0 {
                0.0
            } else {
                actual / predicted
            };
            if ratio <= 0.25 {
                let mut temp = if actual >= 0.0 {
                    0.5
                } else {
                    0.5 * dir_der / (dir_der + 0.5 * actual)
                };
                if rnorm_new * P1 >= rnorm || temp < P1 {
                    temp = P1;
                }
                delta = temp * delta.min(pnorm * 10.0);
                lambda /= temp;
            } else if lambda == 0.0 || ratio >= 0.75 {
                delta = pnorm / 0.5;
                lambda *= 0.5;
            }
            let good = ratio >= P0001;
            if good {
                x = x_new;
                xnorm = x.component_mul(&diag).norm();
                rnorm = rnorm_new;
            }
            if rnorm <= f64::MIN_POSITIVE {
                return (x, evaluations, Termination::ResidualsZero);
            }
            let ftol_check = actual.abs() <= s.ftol && predicted <= s.ftol && ratio * 0.5 <= 1.0;
            let xtol_check = delta <= s.xtol * xnorm;
            if ftol_check || xtol_check {
                // A small step or a small decrease says the iteration has
                // stopped, not that it has arrived: the scaled norm of
                // the step is dominated by any column that was once huge
                // (`diag` never decreases), so the trust region can look
                // negligible while a good step is still on offer. It has
                // arrived where the residuals vanish, or where the
                // gradient does (a least-squares minimum of equations
                // that conflict); anywhere else it only stopped.
                let arrived = rnorm <= RESIDUAL_ZERO
                    || match p.residuals_and_jacobian(&x) {
                        Ok((rx, jx)) => {
                            let at = LeastSquares::new(&jx, &rx);
                            at.max_a_t_b_scaled(rx.norm()) <= STATIONARY
                        }
                        Err(_) => false,
                    };
                let how = if arrived {
                    Termination::Converged
                } else {
                    Termination::Stalled
                };
                return (x, evaluations, how);
            }
            if evaluations >= s.patience {
                return (x, evaluations, Termination::OutOfPatience);
            }
            if actual.abs() <= EPS && predicted <= EPS && ratio * 0.5 <= 1.0 {
                return (x, evaluations, Termination::Stalled);
            }
            if delta <= EPS * xnorm || gnorm <= EPS {
                return (x, evaluations, Termination::Stalled);
            }
            if good {
                // An accepted step is in the domain, so this succeeds.
                match p.residuals_and_jacobian(&x) {
                    Ok((rj, jj)) => (r, j) = (rj, jj),
                    Err(s) => return (x, evaluations, Termination::degenerate(s)),
                }
                break;
            }
        }
    }
}
