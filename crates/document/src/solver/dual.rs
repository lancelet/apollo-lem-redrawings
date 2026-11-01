//! Arithmetic on dual numbers that num-dual does not provide, or provides
//! with a derivative that is wrong for us: one helper per operation, each
//! exact (value and every derivative) wherever the operation is
//! differentiable, and documented with where that is.
//!
//! The 2-vector helpers are polynomials (`sub`, `dot`, `cross`) and so
//! smooth everywhere; `norm` and `unit` are smooth except at the zero
//! vector, where `norm` has no derivative (a cone) and `unit` no value.
//! Callers that may meet the zero vector check for it first and say so
//! (see `equation::Degeneracy`); these helpers do not guess a direction.

use num_dual::DualNum;

/// `b − a`.
pub(crate) fn sub<T: DualNum<f64>>(a: &[T; 2], b: &[T; 2]) -> [T; 2] {
    [a[0].clone() - &b[0], a[1].clone() - &b[1]]
}

/// `a · b`.
pub(crate) fn dot<T: DualNum<f64>>(a: &[T; 2], b: &[T; 2]) -> T {
    a[0].clone() * &b[0] + a[1].clone() * &b[1]
}

/// `a × b`, the z component.
pub(crate) fn cross<T: DualNum<f64>>(a: &[T; 2], b: &[T; 2]) -> T {
    a[0].clone() * &b[1] - a[1].clone() * &b[0]
}

/// `a` turned a quarter turn: `(−a.y, a.x)`.
pub(crate) fn perp<T: DualNum<f64>>(a: &[T; 2]) -> [T; 2] {
    [-a[1].clone(), a[0].clone()]
}

/// `a` divided by its largest component (a constant, so derivatives stay
/// exact): a vector with norm in [1, √2], or `None` for the zero vector.
/// Norms and directions are taken of this, as `hypot` does, so that no
/// square or quotient of a vector that is not zero overflows or
/// underflows (num-dual divides by the square of the divisor).
fn scaled<T: DualNum<f64>>(a: &[T; 2]) -> Option<([T; 2], f64)> {
    let s = a[0].re().abs().max(a[1].re().abs());
    (s > 0.0).then(|| ([a[0].clone() / s, a[1].clone() / s], s))
}

/// `|a|`. Differentiable except at `a = 0`, where the value is 0 and the
/// derivative NaN. Finite with a finite derivative for every other `a`.
pub(crate) fn norm<T: DualNum<f64>>(a: &[T; 2]) -> T {
    match scaled(a) {
        Some((w, s)) => dot(&w, &w).sqrt() * s,
        None => dot(a, a).sqrt(),
    }
}

/// `a / |a|`. Undefined (NaN) at `a = 0`; finite with a finite derivative
/// for every other `a`.
pub(crate) fn unit<T: DualNum<f64>>(a: &[T; 2]) -> [T; 2] {
    let w = scaled(a).map_or_else(|| a.clone(), |(w, _)| w);
    let n = dot(&w, &w).sqrt();
    [w[0].clone() / &n, w[1].clone() / &n]
}

/// `atan2(y, x)`, the angle of `(x, y)` in (−π, π], with the derivative
/// `(x y′ − y x′) / (x² + y²)` (and every higher one) at every
/// `(x, y) ≠ (0, 0)`. At the origin the angle is undefined: NaN.
///
/// Not num-dual's `atan2`: num-dual 0.11 differentiates `atan(y / x)`,
/// whose derivative is NaN wherever `x = 0` exactly (`y / x` is infinite
/// in value and derivative, and `atan′` of that is ∞/∞), so a right
/// angle drawn exactly gets a NaN Jacobian. Here the derivative goes
/// through `atan(y / x)` or `−atan(x / y)`, whichever ratio is at most
/// one in size: each differs from `atan2` by a constant (a multiple of
/// π/2) near the point, so carries its derivatives exactly, and the
/// ratio's divisor is the larger of `|x|`, `|y|`, never zero.
pub(crate) fn atan2<T: DualNum<f64>>(y: T, x: T) -> T {
    let (yr, xr) = (y.re(), x.re());
    let t = if xr.abs() >= yr.abs() {
        (y / x).atan()
    } else {
        -(x / y).atan()
    };
    // `t − t.re()` has a real part of exactly zero.
    let re = t.re();
    t - re + yr.atan2(xr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_dual::Dual64;
    use proptest::prelude::*;

    /// The case num-dual gets wrong: d/dx atan2(1, x) at x = 0 is −1
    /// (num-dual 0.11's `Dual64::from_re(1.0).atan2(Dual64::new(0.0,
    /// 1.0))` gives NaN).
    #[test]
    fn atan2_on_the_axes() {
        let a = atan2(Dual64::from_re(1.0), Dual64::new(0.0, 1.0));
        assert_eq!((a.re, a.eps), (std::f64::consts::FRAC_PI_2, -1.0));
        for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
            let a = atan2(Dual64::new(y, 1.0), Dual64::new(x, 1.0));
            assert_eq!(a.re, f64::atan2(y, x));
            assert_eq!(a.eps, x - y);
        }
        assert!(atan2(Dual64::from_re(0.0), Dual64::from_re(0.0))
            .re
            .is_nan());
    }

    proptest! {
        // 256 cases each: a few milliseconds.

        /// Value is f64's atan2; derivative (x y′ − y x′) / (x² + y²),
        /// on a grid of small integers (axes and diagonals exactly) and
        /// in general position.
        #[test]
        fn atan2_derivative_is_exact(
            x in prop_oneof![(-3i8..=3).prop_map(f64::from), -9.0..9.0f64],
            y in prop_oneof![(-3i8..=3).prop_map(f64::from), -9.0..9.0f64],
            dx in -2.0..2.0f64,
            dy in -2.0..2.0f64,
        ) {
            prop_assume!(x != 0.0 || y != 0.0);
            let a = atan2(Dual64::new(y, dy), Dual64::new(x, dx));
            prop_assert_eq!(a.re, y.atan2(x));
            let want = (x * dy - y * dx) / (x * x + y * y);
            prop_assert!((a.eps - want).abs() <= 1e-14 * (1.0 + want.abs()));
        }

        /// `norm` and `unit` are finite with exact derivatives for every
        /// vector that is not zero, however small or large.
        #[test]
        fn norm_is_finite_off_zero(
            m in -300i32..300,
            ax in -1.0..1.0f64,
            ay in -1.0..1.0f64,
            dx in -1.0..1.0f64,
            dy in -1.0..1.0f64,
        ) {
            // Components either side of the largest by up to 1e3, so no
            // value is subnormal.
            prop_assume!(ax.abs().max(ay.abs()) >= 1e-3);
            let s = 10f64.powi(m);
            let a = [Dual64::new(ax * s, dx * s), Dual64::new(ay * s, dy * s)];
            let n = norm(&a);
            let want = ax.hypot(ay) * s;
            prop_assert!(n.re > 0.0 && n.re.is_finite());
            prop_assert!((n.re - want).abs() <= 1e-15 * want);
            // d|a| = â · da.
            let deriv = (ax * dx + ay * dy) / ax.hypot(ay) * s;
            prop_assert!((n.eps - deriv).abs() <= 1e-14 * s);
            let u = unit(&a);
            prop_assert!(u.iter().all(|c| c.re.is_finite() && c.eps.is_finite()));
        }
    }
}
