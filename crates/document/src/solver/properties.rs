//! Properties of the equations and the solver over the zoo
//! (`solver::testing`): what moving the drawing does to residuals and to
//! the analysis, what a solve reports about where it stopped, and that a
//! solved state stays put.

use super::equation::{Axis, Degeneracy, Equation};
use super::solve::{analyse, evaluate, residuals, solve, Layout, Options, Report, Termination};
use super::testing::{
    case, critical, nearly_coincident, positions, rigid, system, zoo, zoo_layout, Rigid,
};
use super::tolerance::Tolerance;
use crate::value::geometry::{Entity, Geometry, Model};
use approx::AbsDiffEq;
use nalgebra::DVector;
use proptest::prelude::*;
use proptest::sample::Index;

/// Relation kinds stated along the frame's axes (`same`, `dx`, `dy`):
/// unchanged by a shift, not by a turn.
const AXIS_ALIGNED: [usize; 3] = [3, 14, 15];
/// `coincide`: its residual is the vector between the points.
const COINCIDE: usize = 0;
/// `circles_tangent`: its residual is the nearer branch's, outer or
/// inner, and where both are as near (the smaller centre on the larger
/// circle) their residuals are of opposite signs.
const CIRCLES_TANGENT: usize = 7;
/// `pinned`: tied to the frame's origin as well.
const PINNED: usize = 20;
/// `midpoint_of`: like a coincidence, its residual is a vector.
const MIDPOINT_OF: usize = 23;

/// The motion `m` keeps only its shift if any of `eqs` is tied to the
/// frame's axes.
fn for_frame(m: Rigid, eqs: &[Equation]) -> Rigid {
    let tied = eqs
        .iter()
        .any(|e| AXIS_ALIGNED.contains(&e.kind()) || e.kind() == PINNED);
    Rigid {
        turn: if tied { 0.0 } else { m.turn },
        ..m
    }
}

/// The zoo at state `x`, if the values make one (no zero radius or
/// zero-length line).
fn at(layout: &Layout, x: &DVector<f64>) -> Option<Geometry<Model>> {
    layout.unflatten(&zoo(), x).ok()
}

/// What the analysis says that does not depend on where the drawing is.
fn counts(r: &Report) -> [usize; 5] {
    [
        r.rank,
        r.free,
        r.redundant,
        r.internal_free,
        r.freedoms.len(),
    ]
}

/// The report's own consistency: one freedom named per internal freedom,
/// and dependencies named exactly when something is redundant.
fn consistent(r: &Report) -> Result<(), TestCaseError> {
    prop_assert_eq!(r.freedoms.len(), r.internal_free, "{:?}", r);
    prop_assert_eq!(r.redundant == 0, r.dependencies.is_empty(), "{:?}", r);
    Ok(())
}

proptest! {
    // 2000 cases, about 0.2 s in a debug build.
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// A relation between positions does not see where the drawing is:
    /// its residuals are the same after any rigid motion, but for a
    /// coincidence's, a vector, which turns with it. Those along the
    /// axes are the same after a shift, unless across two axes; a pin's
    /// change by the shift.
    #[test]
    fn residuals_move_with_the_drawing((e, x) in case(), m in rigid()) {
        let layout = zoo_layout();
        let m = for_frame(m, &[e]);
        let (r0, s0) = evaluate(&layout, &[e], &x);
        let (r1, s1) = evaluate(&layout, &[e], &m.state(&layout, &x));
        // Coincidences of computed positions need not survive rounding,
        // and the direction of a line 1e-15 long is rounding itself.
        if !s0.is_empty() || !s1.is_empty() || nearly_coincident(&[e], &layout, &x) {
            return Ok(());
        }
        let want = match e.kind() {
            COINCIDE | MIDPOINT_OF => {
                let turned = Rigid { shift: [0.0; 2], ..m }.point([r0[0], r0[1]]);
                DVector::from_column_slice(&turned)
            }
            PINNED => r0.clone() + DVector::from_column_slice(&m.shift),
            // Along one axis or across (a link between views): by the
            // shift's difference between the two.
            _ if e.same_axes().is_some() => {
                let (ax, bx) = e.same_axes().expect("a same");
                let along = |a| match a {
                    Axis::X => m.shift[0],
                    Axis::Y => m.shift[1],
                };
                r0.add_scalar(along(bx) - along(ax))
            }
            _ => r0.clone(),
        };
        // Which branch rounding picks at the tie is arbitrary: the size
        // of the residual is what is the same.
        let (r1, want) = if e.kind() == CIRCLES_TANGENT {
            (r1.abs(), want.abs())
        } else {
            (r1, want)
        };
        let ok = r1.iter().zip(&want).all(|(a, b)| (a - b).abs() <= 1e-9);
        prop_assert!(ok, "{:?} by {:?}: {} became {}", e, m, r0, r1);
    }
}

proptest! {
    // 96 cases each, about 4 s each in a debug build: every case runs
    // the full analysis (or a solve) of a 40-parameter system several
    // times.
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// The analysis does not see where the drawing is either: rank,
    /// freedoms and redundancy are the same after a rigid motion
    /// (placement after a shift: a turn changes which axes it is along).
    /// Duplicating an equation adds exactly its rows to the redundancy
    /// and names it and its copy in dependencies; dropping one never
    /// fixes anything.
    #[test]
    fn the_analysis_is_of_the_figure(
        (eqs, x) in system(),
        m in rigid(),
        k in any::<Index>(),
    ) {
        let layout = zoo_layout();
        let m = for_frame(m, &eqs);
        let (Some(g), Some(h)) = (at(&layout, &x), at(&layout, &m.state(&layout, &x))) else {
            return Ok(());
        };
        // Near a coincidence one row is huge and the rank tolerance,
        // relative to the largest, is rounding for the rest; at an
        // extreme a row is rounding itself.
        if nearly_coincident(&eqs, &layout, &x) || critical(&eqs, &layout, &x) {
            return Ok(());
        }
        let o = Options::default();
        let a = analyse(&g, &eqs, o).unwrap().report;
        let b = analyse(&h, &eqs, o).unwrap().report;
        consistent(&a)?;
        if a.termination != Termination::NothingToSolve
            || b.termination != Termination::NothingToSolve
        {
            // Degenerate: rows left out where the operands coincide, a
            // coincidence of computed positions that rounding may undo.
            return Ok(());
        }
        prop_assert_eq!(counts(&a), counts(&b), "{:?}", m);
        if m.turn == 0.0 {
            prop_assert_eq!(a.placement, b.placement);
        }
        let k = k.index(eqs.len());
        let mut more = eqs.clone();
        more.push(eqs[k]);
        let c = analyse(&g, &more, o).unwrap().report;
        consistent(&c)?;
        prop_assert_eq!((c.rank, c.free), (a.rank, a.free));
        prop_assert_eq!(c.redundant, a.redundant + eqs[k].count());
        // Both are named (the groups are a basis of the dependencies, so
        // not necessarily together).
        for i in [k, eqs.len()] {
            let named = c.dependencies.iter().any(|d| d.equations.contains(&i));
            prop_assert!(named, "{} in {:?}", i, c.dependencies);
        }
        let mut fewer = eqs.clone();
        fewer.remove(k);
        let d = analyse(&g, &fewer, o).unwrap().report;
        prop_assert!(d.free >= a.free, "{} < {}", d.free, a.free);
    }

    /// A solve reports the state it reached: each equation's residual
    /// as evaluated there; a degeneracy only where the operands named
    /// really coincide; at a converged stop (a stationary point of the
    /// least squares) every equation left unsatisfied is in a dependency
    /// with others. Solved, it stays solved: solving again moves nothing
    /// and reports the same.
    #[test]
    fn a_solve_reports_where_it_stopped((eqs, x) in system()) {
        let layout = zoo_layout();
        let Some(g) = at(&layout, &x) else {
            return Ok(());
        };
        let o = Options::default();
        let s = solve(&g, &eqs, o).unwrap();
        let r = &s.report;
        consistent(r)?;
        if let Termination::Degenerate { equation, what } = r.termination {
            let reads = positions(&eqs[equation]);
            match what {
                Degeneracy::Coincident(a, b) => {
                    prop_assert!(reads.contains(&a) && reads.contains(&b));
                    let pos = |p| s.geometry.position(&p).unwrap();
                    prop_assert_eq!(pos(a), pos(b));
                }
                Degeneracy::ZeroRadius(c) => {
                    let radius = match s.geometry.get(c.id()) {
                        Some(Entity::Arc { radius, .. } | Entity::Circle { radius, .. }) => *radius,
                        _ => f64::NAN,
                    };
                    prop_assert_eq!(radius, 0.0);
                }
            }
            return Ok(());
        }
        // Near a coincidence a direction is rounding, and two ways of
        // evaluating it (dual numbers, plain) round differently; only
        // what holds at any scale is asked there.
        let stopped = layout.flatten(&s.geometry);
        if nearly_coincident(&eqs, &layout, &stopped) {
            return Ok(());
        }
        let there = residuals(&s.geometry, &eqs).unwrap();
        for (i, rs) in there.iter().enumerate() {
            let max = rs.iter().fold(0.0f64, |a, v| a.max(v.abs()));
            prop_assert!((max - r.residuals[i]).abs() <= 1e-9, "{} {:?}", i, r);
        }
        // A stop by the step's size is a stationary point only where it
        // did not start with a derivative that is rounding (an extreme
        // of a residual): the trust region's scale is rounding too.
        if matches!(r.termination, Termination::Converged | Termination::ResidualsZero)
            && !critical(&eqs, &layout, &x)
        {
            for (i, &ri) in r.residuals.iter().enumerate() {
                let named = r.dependencies.iter().any(|d| d.equations.contains(&i));
                prop_assert!(ri <= 1e-6 || named, "equation {} left at {}: {:?}", i, ri, r);
            }
        }
        if r.residual_max <= 1e-9 {
            let again = solve(&s.geometry, &eqs, o).unwrap();
            let tol = Tolerance::new(1e-9, 1e-9);
            prop_assert!(again.geometry.abs_diff_eq(&s.geometry, tol));
            prop_assert!(again.report.residual_max <= 1e-9);
            prop_assert_eq!(counts(&again.report), counts(r));
        }
    }
}
