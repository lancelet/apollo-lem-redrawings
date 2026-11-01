//! Dense linear algebra on `faer` (blocked, SIMD, pure Rust, builds for
//! wasm): the factorisations the solver and its analysis need. nalgebra
//! types are the exchange format; the conversion is a copy, which is small
//! next to a factorisation.
//!
//! `LeastSquares` is MINPACK's view of a least-squares problem after one
//! pivoted QR: the upper triangle `R`, `Qᵀb`, the permutation and the
//! column norms, with the O(n²) routines `lmder` needs on them (`qrsolv`'s
//! Givens elimination of a diagonal, the triangular solves, `‖Ax‖`, the
//! scaled gradient). Only the QR itself is `faer`'s; the rest is ported
//! from the `levenberg-marquardt` crate so the solver's outcomes are the
//! same.

use faer::linalg::solvers::ColPivQr;
use faer::prelude::*;
use nalgebra::{DMatrix, DVector};

fn to_faer(a: &DMatrix<f64>) -> Mat<f64> {
    Mat::from_fn(a.nrows(), a.ncols(), |i, j| a[(i, j)])
}

fn to_faer_col(v: &DVector<f64>) -> Mat<f64> {
    Mat::from_fn(v.len(), 1, |i, _| v[i])
}

/// Rank of `a` and a basis of its null space (vectors `v` with `a v = 0`),
/// as the columns of an `n × (n − rank)` matrix, from a column-pivoting
/// QR. Diagonal entries of R below `tol` times the largest count as zero.
/// Same contract as the nalgebra version in `solver`.
pub(crate) fn null_space(a: &DMatrix<f64>, tol: f64) -> (usize, DMatrix<f64>) {
    let n = a.ncols();
    if a.nrows() == 0 || n == 0 {
        return (0, DMatrix::identity(n, n));
    }
    let qr = ColPivQr::new(to_faer(a).as_ref());
    let r = qr.R();
    let lead = r[(0, 0)].abs();
    let rank = if lead <= 0.0 {
        0
    } else {
        (0..r.nrows().min(n))
            .take_while(|&i| r[(i, i)].abs() > tol * lead)
            .count()
    };
    // A P = Q R with R = [R11 R12; 0 0]. In the permuted order each
    // dependent column j gives the null vector [−R11⁻¹ R12[:, j]; e_j];
    // `perm` maps permuted positions back to original columns.
    let (perm, _) = qr.P().arrays();
    let mut basis = DMatrix::zeros(n, n - rank);
    for (k, j) in (rank..n).enumerate() {
        let mut w = vec![0.0; n];
        w[j] = 1.0;
        for i in (0..rank).rev() {
            let mut acc = -r[(i, j)];
            for l in i + 1..rank {
                acc -= r[(i, l)] * w[l];
            }
            w[i] = acc / r[(i, i)];
        }
        for (pos, &val) in w.iter().enumerate() {
            basis[(perm[pos], k)] = val;
        }
    }
    debug_assert!((a * &basis).iter().all(|v| v.abs() < 1e-6 * lead.max(1.0)));
    (rank, basis)
}

/// The least-squares problem `min ‖A x − b‖` after a pivoted QR of `A`
/// (m × n), padded to n rows when m < n, as MINPACK keeps it.
pub(crate) struct LeastSquares {
    /// Upper triangle of R, n × n; rows beyond m are zero.
    r: DMatrix<f64>,
    /// The first n entries of Qᵀb.
    qtb: DVector<f64>,
    /// `perm[j]` is the original column at permuted position j.
    perm: Vec<usize>,
    pub(crate) column_norms: DVector<f64>,
    m: usize,
}

/// The factor after `qrsolv`: `L Lᵀ = Pᵀ(AᵀA + DD)P`, or `R` itself when
/// the diagonal was zero.
pub(crate) struct Factor<'a> {
    l: DMatrix<f64>,
    l_diag: DVector<f64>,
    lower: bool,
    ls: &'a LeastSquares,
}

impl Factor<'_> {
    /// Solve `L x = Pᵀ rhs` (lower) or `Rᵀ x = Pᵀ rhs` (upper).
    pub(crate) fn solve(&self, rhs: &DVector<f64>) -> DVector<f64> {
        let n = self.ls.r.ncols();
        let mut w = DVector::from_fn(n, |i, _| rhs[self.ls.perm[i]]);
        if self.lower {
            for j in 0..n {
                w[j] /= self.l_diag[j];
                let x = w[j];
                for i in j + 1..n {
                    w[i] -= x * self.l[(i, j)];
                }
            }
        } else {
            for j in 0..n {
                let sum: f64 = (0..j).map(|i| w[i] * self.l[(i, j)]).sum();
                w[j] = (w[j] - sum) / self.l[(j, j)];
            }
        }
        w
    }

    /// `L Qᵀb` (lower) or `Rᵀ Qᵀb` (upper): the gradient in permuted order.
    pub(crate) fn mul_qt_b(&self) -> DVector<f64> {
        let n = self.ls.r.ncols();
        let mut out = DVector::zeros(n);
        if self.lower {
            for i in 0..n {
                for k in i + 1..n {
                    out[k] += self.ls.qtb[i] * self.l[(k, i)];
                }
                out[i] += self.ls.qtb[i] * self.l_diag[i];
            }
        } else {
            for i in 0..n {
                out[i] = (0..=i).map(|k| self.ls.qtb[k] * self.l[(k, i)]).sum();
            }
        }
        out
    }
}

impl LeastSquares {
    /// Factor `a` and apply `Qᵀ` to `b`.
    pub(crate) fn new(a: &DMatrix<f64>, b: &DVector<f64>) -> Self {
        let (m, n) = a.shape();
        let k = m.min(n);
        let qr = ColPivQr::new(to_faer(a).as_ref());
        let rk = qr.R();
        let lead = rk[(0, 0)].abs();
        let mut r = DMatrix::zeros(n, n);
        for i in 0..k {
            for j in i..n {
                r[(i, j)] = rk[(i, j)];
            }
            // Dependent columns: MINPACK's logic tests the diagonal for
            // exact zero, so make numerically zero ones exactly zero.
            if r[(i, i)].abs() <= 1e-14 * lead {
                r[(i, i)] = 0.0;
            }
        }
        let q = qr.compute_thin_Q();
        let qtb_k: Mat<f64> = q.transpose() * to_faer_col(b);
        let qtb = DVector::from_fn(n, |i, _| if i < k { qtb_k[(i, 0)] } else { 0.0 });
        let (perm, _) = qr.P().arrays();
        let column_norms = DVector::from_fn(n, |c, _| a.column(c).norm());
        LeastSquares {
            r,
            qtb,
            perm: perm.to_vec(),
            column_norms,
            m,
        }
    }

    /// The original column at permuted position `j`.
    pub(crate) fn perm(&self, j: usize) -> usize {
        self.perm[j]
    }

    /// `max_j |(Aᵀb)_j| / (‖A e_j‖ ‖b‖)`, columns of zero norm skipped.
    pub(crate) fn max_a_t_b_scaled(&self, b_norm: f64) -> f64 {
        let n = self.r.ncols();
        let mut max: f64 = 0.0;
        for j in 0..n {
            let scale = self.column_norms[self.perm[j]];
            if scale == 0.0 {
                continue;
            }
            let sum: f64 = (0..=j).map(|i| self.r[(i, j)] * self.qtb[i] / b_norm).sum();
            max = max.max((sum / scale).abs());
        }
        max
    }

    /// `‖A x‖`.
    pub(crate) fn a_x_norm(&self, x: &DVector<f64>) -> f64 {
        let n = self.r.ncols();
        let mut work: DVector<f64> = DVector::zeros(n);
        for j in 0..n {
            let xj = x[self.perm[j]];
            for i in 0..=j {
                work[i] += xj * self.r[(i, j)];
            }
        }
        work.norm()
    }

    /// Whether `A` has full column rank.
    pub(crate) fn is_non_singular(&self) -> bool {
        let n = self.r.ncols();
        self.m.min(n) == n && (0..n).all(|j| self.r[(j, j)] != 0.0)
    }

    fn r_rank(&self) -> usize {
        let n = self.r.ncols();
        let max_rank = self.m.min(n);
        (0..max_rank)
            .position(|i| self.r[(i, i)] == 0.0)
            .unwrap_or(max_rank)
    }

    /// The least-squares solution with the components of dependent
    /// columns set to zero, and the factor `R` for further solves.
    pub(crate) fn solve_with_zero_diagonal(&self) -> (DVector<f64>, Factor<'_>) {
        let n = self.r.ncols();
        let rank = self.r_rank();
        let mut w = self.qtb.clone();
        for i in rank..n {
            w[i] = 0.0;
        }
        for i in (0..rank).rev() {
            let sum: f64 = (i + 1..rank).map(|l| self.r[(i, l)] * w[l]).sum();
            w[i] = (w[i] - sum) / self.r[(i, i)];
        }
        let mut x = DVector::zeros(n);
        for j in 0..n {
            x[self.perm[j]] = w[j];
        }
        let factor = Factor {
            l: self.r.clone(),
            l_diag: DVector::from_fn(n, |i, _| self.r[(i, i)]),
            lower: false,
            ls: self,
        };
        (x, factor)
    }

    /// Solve `(AᵀA + DD) x = Aᵀb` for a diagonal `D` by eliminating it
    /// from `[R; D]` with Givens rotations (MINPACK's `qrsolv`), and
    /// return the factor `L` with `L Lᵀ = Pᵀ(AᵀA + DD)P`.
    pub(crate) fn solve_with_diagonal(&self, diag: &DVector<f64>) -> (DVector<f64>, Factor<'_>) {
        let n = self.r.ncols();
        // Lower triangle holds Rᵀ, overwritten with L; the diagonal of R
        // is kept aside and restored, as the crate does.
        let mut l = self.r.clone();
        for i in 0..n {
            for j in 0..i {
                l[(i, j)] = l[(j, i)];
            }
        }
        let saved_diag = DVector::from_fn(n, |i, _| l[(i, i)]);
        let mut l_diag = DVector::zeros(n);
        let mut rhs = self.qtb.clone();
        for j in 0..n {
            let diag_entry = diag[self.perm[j]];
            if diag_entry != 0.0 {
                l_diag[j] = diag_entry;
                for i in j + 1..n {
                    l_diag[i] = 0.0;
                }
                let mut qtbpj = 0.0;
                for k in j..n {
                    if l_diag[k] == 0.0 {
                        continue;
                    }
                    let r_kk = l[(k, k)];
                    let (sin, cos) = if r_kk.abs() < l_diag[k].abs() {
                        let cot = r_kk / l_diag[k];
                        let sin = 0.5 / (0.25 + 0.25 * cot * cot).sqrt();
                        (sin, sin * cot)
                    } else {
                        let tan = l_diag[k] / r_kk;
                        let cos = 0.5 / (0.25 + 0.25 * tan * tan).sqrt();
                        (cos * tan, cos)
                    };
                    l[(k, k)] = cos * r_kk + sin * l_diag[k];
                    let temp = cos * rhs[k] + sin * qtbpj;
                    qtbpj = -sin * rhs[k] + cos * qtbpj;
                    rhs[k] = temp;
                    for i in k + 1..n {
                        let r_ik = l[(i, k)];
                        let temp = cos * r_ik + sin * l_diag[i];
                        l_diag[i] = -sin * r_ik + cos * l_diag[i];
                        l[(i, k)] = temp;
                    }
                }
            }
            l_diag[j] = l[(j, j)];
            l[(j, j)] = saved_diag[j];
        }
        // Solve Lᵀ z = rhs over the nonsingular leading block, then
        // un-permute.
        let rank = (0..n).position(|i| l_diag[i] == 0.0).unwrap_or(n);
        for i in rank..n {
            rhs[i] = 0.0;
        }
        for j in (0..rank).rev() {
            let dot: f64 = (j + 1..rank).map(|i| l[(i, j)] * rhs[i]).sum();
            rhs[j] = (rhs[j] - dot) / l_diag[j];
        }
        let mut x = DVector::zeros(n);
        for j in 0..n {
            x[self.perm[j]] = rhs[j];
        }
        let factor = Factor {
            l,
            l_diag,
            lower: true,
            ls: self,
        };
        (x, factor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_space_matches_the_nalgebra_version() {
        let a = DMatrix::from_row_slice(
            3,
            4,
            &[1.0, 2.0, 3.0, 4.0, 2.0, 4.0, 6.0, 8.0, 0.0, 1.0, 1.0, 0.0],
        );
        let (rank, basis) = null_space(&a, 1e-9);
        assert_eq!(rank, 2);
        assert_eq!(basis.shape(), (4, 2));
        assert!((&a * &basis).iter().all(|v| v.abs() < 1e-12));
        let (rank2, basis2) = crate::solver::solve::null_space_nalgebra(&a, 1e-9);
        assert_eq!(rank, rank2);
        assert_eq!(basis2.shape(), basis.shape());
        let id = DMatrix::<f64>::identity(3, 3);
        assert_eq!(null_space(&id, 1e-9), (3, DMatrix::zeros(3, 0)));
        let none = DMatrix::<f64>::zeros(0, 3);
        assert_eq!(null_space(&none, 1e-9), (0, DMatrix::identity(3, 3)));
    }

    fn tall() -> (DMatrix<f64>, DVector<f64>) {
        let a = DMatrix::from_row_slice(
            4,
            3,
            &[2.0, 1.0, 4.0, 0.0, 10.0, -1.0, 0.0, 4.0, 0.5, 1.0, 0.0, 0.0],
        );
        let b = DVector::from_vec(vec![1.0, 2.0, 3.0, 4.0]);
        (a, b)
    }

    #[test]
    fn zero_diagonal_is_least_squares() {
        let (a, b) = tall();
        let ls = LeastSquares::new(&a, &b);
        assert!(ls.is_non_singular());
        let (x, _) = ls.solve_with_zero_diagonal();
        let ata = a.transpose() * &a;
        let atb = a.transpose() * &b;
        assert!((&ata * &x - &atb).iter().all(|v| v.abs() < 1e-10));
        assert!((ls.a_x_norm(&x) - (&a * &x).norm()).abs() < 1e-12);
        let expect = (0..3)
            .map(|j| (atb[j] / (a.column(j).norm() * b.norm())).abs())
            .fold(0.0, f64::max);
        assert!((ls.max_a_t_b_scaled(b.norm()) - expect).abs() < 1e-12);
    }

    #[test]
    fn diagonal_elimination_solves_the_damped_normal_equations() {
        let (a, b) = tall();
        let ls = LeastSquares::new(&a, &b);
        let d = DVector::from_vec(vec![0.5, 2.0, 1.0]);
        let (x, factor) = ls.solve_with_diagonal(&d);
        let mut m = a.transpose() * &a;
        for i in 0..3 {
            m[(i, i)] += d[i] * d[i];
        }
        let atb = a.transpose() * &b;
        assert!((&m * &x - &atb).iter().all(|v| v.abs() < 1e-10));
        // The factor: L Lᵀ = Pᵀ M P, and solve() inverts L on Pᵀ rhs.
        let n = 3;
        let mut l = factor.l.clone();
        for i in 0..n {
            l[(i, i)] = factor.l_diag[i];
            for j in i + 1..n {
                l[(i, j)] = 0.0;
            }
        }
        let llt = &l * l.transpose();
        for i in 0..n {
            for j in 0..n {
                assert!((llt[(i, j)] - m[(ls.perm[i], ls.perm[j])]).abs() < 1e-9);
            }
        }
        let rhs = DVector::from_vec(vec![1.0, -2.0, 0.5]);
        let w = factor.solve(&rhs);
        let permuted = DVector::from_fn(n, |i, _| rhs[ls.perm[i]]);
        assert!((&l * &w - permuted).iter().all(|v| v.abs() < 1e-10));
    }

    #[test]
    fn wide_systems_leave_dependent_columns_at_zero() {
        let a = DMatrix::from_row_slice(1, 2, &[2.0, 1.0]);
        let b = DVector::from_vec(vec![4.0]);
        let ls = LeastSquares::new(&a, &b);
        assert!(!ls.is_non_singular());
        let (x, _) = ls.solve_with_zero_diagonal();
        assert!((a * &x - b).iter().all(|v| v.abs() < 1e-12));
        assert_eq!(x.iter().filter(|v| **v == 0.0).count(), 1);
    }
}
