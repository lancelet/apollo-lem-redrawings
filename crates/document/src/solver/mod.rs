//! A low-level geometric constraint solver.
//!
//! Crate-private: clients reach it through the sketch and the document,
//! and see its results through [`crate::state::analysis`].
//!
//! [`equation`] is a closed set of elementary relations between entities of
//! a `Geometry`: points coinciding, a point on a line, lines parallel, a
//! line tangent to an arc at a shared end, a distance, a radius, an angle,
//! and so on. Each is one equation with a well-defined residual. [`solve`]
//! takes a geometry and a list of equations, moves the geometry's values
//! until the residuals are zero, and reports what is left: how many
//! freedoms remain and how many equations were redundant.
//!
//! The relations here are elementary on purpose. Anything a drawing states
//! can be written as a few of them: a corner is a coincidence, a tangent
//! join is a coincidence plus a tangency, concentric arcs are coincident
//! centres, a dimension is a distance. Higher-level constraints can be
//! expressed in these and built on top; nothing here depends on them.
//!
//! # Example
//!
//! A 2 × 2 square with its top edge replaced by a semicircular arc tangent
//! to both sides. The geometry starts out approximate, as if traced by hand:
//! corners not quite meeting, sides not quite straight, the arc the wrong
//! size. Every relation that closes the figure is written out: the anchor
//! fixed, the corners coincident, the sides axis-aligned, the two
//! dimensions, and at each top corner the coincidence and the tangency that
//! make a tangent join. The solver moves the geometry onto the exact figure.
//!
//! (`tests::square_with_an_arc`, below, is this figure.)
//!
//! # When it cannot be solved
//!
//! Equations may be more than the figure can satisfy. The report then says
//! which ones are involved: a `Dependency` is a group of equations that are
//! not independent of each other, and its `residual` tells whether they
//! merely repeat one another (zero: redundant, drop any one) or contradict
//! one another (not zero: a conflict). A line pinned at both ends 10 mm
//! apart cannot also be 12 mm long:
//!
//! (`tests::a_line_pinned_too_long`, below.)
//!
//! The same group with a consistent length (10 mm) is reported too, with a
//! zero residual: it is then redundant rather than wrong. Freedoms are
//! reported the other way round, as `Freedom`s naming the values that can
//! still move; a fully constrained figure has none.

mod dual;
pub mod equation;
mod faer_backend;
mod lm;
pub mod near;
#[cfg(test)]
mod properties;
pub mod solve;
#[cfg(test)]
mod testing;
pub mod tolerance;

#[cfg(test)]
mod tests {
    use crate::geometry;

    #[test]
    fn square_with_an_arc() {
        use crate::solver::equation::*;
        use crate::solver::solve::{solve, Options};
        use crate::solver::tolerance::Tolerance;
        use crate::value::geometry::Model;
        use approx::assert_abs_diff_eq;
        use std::f64::consts::PI;

        // A square with one edge replaced by an arc, drawn approximately.
        let rough = geometry!(Model {
            anchor: point (1.1, -0.9); // fixed point at bottom right
            bottom: line (-0.9, -1.1) -> (1.2, -0.8);
            left:   line (-1.1, -0.9) -> (-0.8, 1.2);
            right:  line (0.9, -1.2) -> (1.1, 0.8);
            top:    arc (0.1, 0.8), r = 1.2, from = 0.2, sweep = 2.9;
        });

        // The complete constraint set: the anchor fixed and the square tied to
        // it, the figure closed, the sides axis-aligned, width 2, height 2, and
        // the arc joined tangentially to both sides. 19 equations for the 19
        // parameters (anchor 2, three lines 4 each, arc 5).
        let (anchor, bottom, left, right, top) = (
            point("anchor"),
            line("bottom"),
            line("left"),
            line("right"),
            arc("top"),
        );
        let eqs = [
            // Fixed anchor point, and the square's corner on it.
            pinned(anchor, 1.0, -1.0),
            coincide(anchor, bottom.end()),
            coincide(anchor, right.start()),
            // The other corners and the arc's ends.
            coincide(bottom.start(), left.start()),
            coincide(right.end(), top.start()),
            coincide(left.end(), top.end()),
            // Axis-aligned sides.
            same_y(bottom.start(), bottom.end()),
            same_x(left.start(), left.end()),
            same_x(right.start(), right.end()),
            // Width 2, height 2.
            dx(bottom.start(), bottom.end(), 2.0),
            dy(right.start(), right.end(), 2.0),
            // The arc meets each side tangentially at the shared end.
            line_tangent_arc(right, top, ArcEnd::Start),
            line_tangent_arc(left, top, ArcEnd::End),
        ];

        let solved = solve(&rough, &eqs, Options::default()).unwrap();
        let r = &solved.report;
        assert!(r.residual_max < 1e-9);
        assert_eq!((r.parameters, r.rank, r.free, r.redundant), (19, 19, 0, 0));

        // The exact figure: the arc is the top half of the unit circle centred
        // at (0, 1), from the right side's end round through (0, 2) to the left
        // side's end.
        let exact = geometry!(Model {
            anchor: point (1.0, -1.0);
            bottom: line (-1.0, -1.0) -> (1.0, -1.0);
            left:   line (-1.0, -1.0) -> (-1.0, 1.0);
            right:  line (1.0, -1.0) -> (1.0, 1.0);
            top:    arc (0.0, 1.0), r = 1.0, from = 0.0, sweep = PI;
        });
        let tol = Tolerance::new(1e-8, 1e-8);
        assert_abs_diff_eq!(solved.geometry, exact, epsilon = tol);
    }

    #[test]
    fn a_line_pinned_too_long() {
        use crate::solver::equation::*;
        use crate::solver::solve::{solve, Options};
        use crate::value::geometry::Model;

        let g = geometry!(Model { l: line (0.0, 0.0) -> (10.0, 0.0) });
        let l = line("l");
        let eqs = [
            pinned(l.start(), 0.0, 0.0),
            pinned(l.end(), 10.0, 0.0),
            length(l, 12.0),
        ];

        let solved = solve(&g, &eqs, Options::default()).unwrap();
        let r = &solved.report;
        // Five residuals for four values: one more equation than the figure has
        // freedom to meet, and it does not hold at the solution.
        assert_eq!((r.equations, r.parameters, r.redundant), (5, 4, 1));
        assert!(r.residual_max > 0.1);
        // The report names the group: the two pins and the length, with the
        // shortfall shared between them. Dropping any one would resolve it.
        assert_eq!(r.dependencies.len(), 1);
        assert_eq!(r.dependencies[0].equations, vec![0, 1, 2]);
        assert!(r.dependencies[0].residual > 0.1);
        // Per-equation residuals show how the least-squares solution spread
        // the 2 mm disagreement: about 2/3 mm on each.
        assert!(r.residuals.iter().all(|&x| (x - 2.0 / 3.0).abs() < 1e-6));
    }
}
