//! Equality up to a tolerance.
//!
//! The document's quantities are physical: lengths in millimetres and angles
//! in radians. Two values are "near" when they differ by at most an absolute
//! tolerance in their own unit. Relative and ULP comparisons are not offered,
//! since neither means anything for a position on a drawing. Angles are
//! compared on the circle, so −π and π are near.
//!
//! Nearness is a predicate, not an equivalence: it is not transitive, so it
//! never backs `Eq`, `Hash` or a map key. Exact `PartialEq` stays exact. The
//! types implement `approx::AbsDiffEq` with `Tolerance` as the epsilon, so
//! `abs_diff_eq!(a, b)` and `assert_abs_diff_eq!(a, b, epsilon = tol)` work.

use std::f64::consts::TAU;

/// An absolute tolerance, one value per unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    /// Millimetres.
    pub length: f64,
    /// Radians.
    pub angle: f64,
}

impl Tolerance {
    /// Absorbs floating-point noise and nothing else: 1 nm and 1 nrad. A scan
    /// pixel is about 0.127 mm, so no real difference on a sheet is this
    /// small. This is the default epsilon.
    pub const NOISE: Tolerance = Tolerance {
        length: 1e-6,
        angle: 1e-9,
    };

    /// A tolerance from its parts.
    pub const fn new(length: f64, angle: f64) -> Self {
        Self { length, angle }
    }

    /// Whether two lengths (mm) are within the length tolerance.
    pub fn length_near(&self, a: f64, b: f64) -> bool {
        (a - b).abs() <= self.length
    }

    /// Angles are compared on the circle: the difference is reduced to
    /// (−π, π] first.
    pub fn angle_near(&self, a: f64, b: f64) -> bool {
        let d = (a - b).rem_euclid(TAU);
        d.min(TAU - d) <= self.angle
    }

    /// For plain differences of angle (a sweep), which are not periodic.
    pub fn sweep_near(&self, a: f64, b: f64) -> bool {
        (a - b).abs() <= self.angle
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self::NOISE
    }
}

/// Convenience for a `Tolerance` with a given length and the noise angle.
pub const fn length(mm: f64) -> Tolerance {
    Tolerance {
        length: mm,
        angle: Tolerance::NOISE.angle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn angles_are_circular() {
        let t = Tolerance::NOISE;
        assert!(t.angle_near(-PI, PI));
        assert!(t.angle_near(0.0, TAU));
        assert!(t.angle_near(PI - 1e-12, -PI + 1e-12));
        assert!(!t.angle_near(0.0, 1e-6));
        assert!(!t.sweep_near(0.0, TAU));
    }

    #[test]
    fn lengths() {
        assert!(Tolerance::NOISE.length_near(1.0, 1.0 + 1e-7));
        assert!(!Tolerance::NOISE.length_near(1.0, 1.0 + 1e-5));
        assert!(length(0.2).length_near(1.0, 1.15));
    }
}
