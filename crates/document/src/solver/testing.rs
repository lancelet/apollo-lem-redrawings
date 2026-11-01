//! Proptest strategies for the solver's tests, in three layers that can be
//! used apart (for solver invariants, op/undo or serialisation round trips
//! as well as derivatives):
//!
//! 1. **Values.** [`state`] gives a value for every parameter of a layout,
//!    by what the parameter is: coordinates on a small integer grid or in
//!    general position, radii, start angles (quarter turns or general),
//!    sweeps. On the grid, axis-aligned, diagonal, parallel and square
//!    directions and coincidences occur exactly and often, and failures
//!    shrink towards it (the grid is each union's first branch).
//! 2. **Special positions.** A [`Tie`] makes one relationship hold
//!    exactly: a position moved onto another (coincident points, a line of
//!    no length, a point at a centre, concentric circles) or onto one of
//!    its coordinates (axis-aligned), a vector set to quarter turns of
//!    another (exactly parallel or square), a radius zeroed. [`ties`] picks
//!    a few among given positions; [`Tie::apply`] makes them hold.
//! 3. **Equations.** [`zoo`] is one geometry with entities of every kind;
//!    [`equation`] gives an equation of each relation kind over it (one
//!    strategy per kind, `Equation::kind` order), and [`case`] puts the
//!    three together: an equation, and values with ties among the
//!    positions it reads; [`system`] the same for a few equations.
//! 4. **Motions.** [`rigid`] gives a turn and a shift of the whole
//!    drawing, [`Rigid::state`] applies one to a state.
//!
//! The numbers themselves are `crate::testing::values`. Two predicates
//! say where only what holds at any scale can be asked:
//! [`nearly_coincident`] (positions apart by rounding) and [`critical`]
//! (a residual at an extreme, its derivative rounding).

use super::equation::*;
use super::solve::{Layout, Value};
use crate::geometry;
use crate::testing::values::{angle_rad, coord, length_mm, radius_mm, signed_mm, sweep};
use crate::value::geometry::{Kind, LinePos, Model, PointAt, PointRef, SplinePos};
use nalgebra::DVector;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::strategy::Union;
use std::f64::consts::FRAC_PI_2;

/// One geometry with entities of every kind, enough for any relation:
/// points `p`, `q`; lines `l`, `m`, `n`; arcs `a`, `b`; circles `c`, `d`;
/// a one-segment spline `s`. Its values are placeholders: [`state`]
/// supplies the values a test uses.
pub(super) fn zoo() -> crate::value::geometry::Geometry<Model> {
    geometry!(Model {
        p: point (0.0, 0.0);
        q: point (1.0, 0.0);
        l: line (0.0, 0.0) -> (1.0, 0.0);
        m: line (0.0, 1.0) -> (1.0, 1.0);
        n: line (0.0, 2.0) -> (1.0, 2.0);
        a: arc (0.0, 0.0), r = 1.0, from = 0.0, sweep = 1.0;
        b: arc (3.0, 0.0), r = 1.0, from = 0.0, sweep = 1.0;
        c: circle (0.0, 5.0), r = 1.0;
        d: circle (3.0, 5.0), r = 1.0;
        s: spline [(0.0, 9.0), (1.0, 9.0), (2.0, 9.0), (3.0, 9.0)];
    })
}

/// The zoo's layout and the name of each parameter.
pub(super) fn zoo_layout() -> Layout {
    Layout::of(&zoo())
}

// ---- 1. values

/// A value for each parameter of `layout`, by what it is.
pub(super) fn state(layout: &Layout) -> impl Strategy<Value = DVector<f64>> {
    let each: Vec<BoxedStrategy<f64>> = layout
        .names
        .iter()
        .map(|(_, v)| match v {
            Value::Radius => radius_mm(),
            Value::StartAngle => angle_rad(),
            Value::Sweep => sweep(),
            _ => coord(),
        })
        .collect();
    each.prop_map(DVector::from_vec)
}

// ---- 2. special positions

/// A relationship made to hold exactly.
#[derive(Clone, Debug)]
pub(super) enum Tie {
    /// `to` moves onto `from`: both coordinates, or only `axis`.
    Onto {
        to: PointRef,
        from: PointRef,
        axis: Option<Axis>,
    },
    /// `to` moves to `from` plus the vector `a → b` turned `quarter`
    /// quarter turns and scaled by a power of two: `from → to` is then
    /// parallel (even turns) or square (odd) to `a → b`, exactly where the
    /// coordinates are on the grid.
    Turn {
        to: PointRef,
        from: PointRef,
        a: PointRef,
        b: PointRef,
        quarter: u8,
        scale: f64,
    },
    /// The radius is zero.
    ZeroRadius(CircRef),
}

impl Tie {
    /// Make the tie hold in `x` (`to` must be a settable position).
    pub(super) fn apply(&self, layout: &Layout, x: &mut DVector<f64>) {
        let set = |x: &mut DVector<f64>, to: &PointRef, v: [f64; 2], axis: Option<Axis>| {
            let i = layout.slot(to).expect("ties move settable positions");
            match axis {
                None => (x[i], x[i + 1]) = (v[0], v[1]),
                Some(Axis::X) => x[i] = v[0],
                Some(Axis::Y) => x[i + 1] = v[1],
            }
        };
        match self {
            Tie::Onto { to, from, axis } => {
                let v = layout.position(x, from);
                set(x, to, v, *axis);
            }
            Tie::Turn {
                to,
                from,
                a,
                b,
                quarter,
                scale,
            } => {
                let (pa, pb) = (layout.position(x, a), layout.position(x, b));
                let mut d = [pb[0] - pa[0], pb[1] - pa[1]];
                for _ in 0..*quarter {
                    d = [-d[1], d[0]];
                }
                let f = layout.position(x, from);
                set(x, to, [f[0] + scale * d[0], f[1] + scale * d[1]], None);
            }
            Tie::ZeroRadius(c) => x[layout.radius_slot(c)] = 0.0,
        }
    }
}

/// Every position an equation reads: its point operands, and the ends,
/// middles and centres of its line, arc and circle operands.
pub(super) fn positions(e: &Equation) -> Vec<PointRef> {
    use crate::value::geometry::ArcPos;
    let mut out = e.point_refs();
    for (id, kind) in e.operands() {
        match kind {
            Kind::Point => out.push(PointRef::point(id)),
            Kind::Line => out.extend(
                [LinePos::Start, LinePos::Mid, LinePos::End].map(|at| PointRef::line(id, at)),
            ),
            Kind::Arc => out.extend(
                [ArcPos::Centre, ArcPos::Start, ArcPos::Mid, ArcPos::End]
                    .map(|at| PointRef::arc(id, at)),
            ),
            Kind::Circle => out.push(PointRef::circle(id)),
            Kind::Spline => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Whether two of the positions `eqs` read at `x` are under 1e-6 mm
/// apart but not equal (they differ by rounding, say): no step resolves
/// the residuals there in f64, and their derivatives are huge, so only
/// what holds at any scale can be asked of them.
pub(super) fn nearly_coincident(eqs: &[Equation], layout: &Layout, x: &DVector<f64>) -> bool {
    let at: Vec<[f64; 2]> = eqs
        .iter()
        .flat_map(positions)
        .map(|p| layout.position(x, &p))
        .collect();
    at.iter().enumerate().any(|(i, a)| {
        at[i + 1..].iter().any(|b| {
            let d = (a[0] - b[0]).hypot(a[1] - b[1]);
            d > 0.0 && d < 1e-6
        })
    })
}

/// Whether some equation of `eqs` is at an extreme of its residual at
/// `x` (perpendicular lines for `parallel`, say): its derivative is
/// zero but for rounding, so whether its row counts in a rank is
/// rounding too.
pub(super) fn critical(eqs: &[Equation], layout: &Layout, x: &DVector<f64>) -> bool {
    use super::solve::{linearise, Jacobian};
    let (_, j, _) = linearise(layout, eqs, x, Jacobian::Local);
    j.row_iter().any(|r| r.norm() < 1e-8)
}

/// Up to three ties among the positions `e` reads: each of the kinds
/// above, so every special configuration of its operands comes up often.
pub(super) fn ties(e: &Equation, layout: &Layout) -> impl Strategy<Value = Vec<Tie>> {
    let all = positions(e);
    let settable: Vec<PointRef> = all
        .iter()
        .copied()
        .filter(|p| layout.slot(p).is_some())
        .collect();
    let circles: Vec<CircRef> = e
        .operands()
        .into_iter()
        .filter_map(|(id, kind)| match kind {
            Kind::Arc => Some(arc(id).into()),
            Kind::Circle => Some(circle(id).into()),
            _ => None,
        })
        .collect();
    // Weighted towards moving one position onto another: a coincidence is
    // what most relations' degeneracies are.
    let mut kinds: Vec<(u32, BoxedStrategy<Tie>)> = Vec::new();
    if !settable.is_empty() {
        let axis = prop_oneof![Just(None), Just(Some(Axis::X)), Just(Some(Axis::Y))];
        kinds.push((
            3,
            (select(settable.clone()), select(all.clone()), axis)
                .prop_map(|(to, from, axis)| Tie::Onto { to, from, axis })
                .boxed(),
        ));
        // The positions an equation names (its points, line ends,
        // centres) onto each other: the degeneracies, often.
        let named: Vec<PointRef> = settable
            .iter()
            .copied()
            .filter(|p| !matches!(p.0, PointAt::Spline { .. }) || e.point_refs().contains(p))
            .collect();
        kinds.push((
            3,
            (select(named.clone()), select(named))
                .prop_map(|(to, from)| Tie::Onto {
                    to,
                    from,
                    axis: None,
                })
                .boxed(),
        ));
        let scale = select(vec![1.0, -1.0, 2.0, 0.5]);
        let ends = (select(all.clone()), select(all.clone()));
        kinds.push((
            2,
            (select(settable), select(all.clone()), ends, 0..4u8, scale)
                .prop_map(|(to, from, (a, b), quarter, scale)| Tie::Turn {
                    to,
                    from,
                    a,
                    b,
                    quarter,
                    scale,
                })
                .boxed(),
        ));
    }
    if !circles.is_empty() {
        kinds.push((1, select(circles).prop_map(Tie::ZeroRadius).boxed()));
    }
    if kinds.is_empty() {
        return Just(Vec::new()).boxed();
    }
    proptest::collection::vec(Union::new_weighted(kinds), 0..=3).boxed()
}

// ---- 3. equations

/// Any position in the zoo.
fn any_point() -> impl Strategy<Value = PointRef> + Clone {
    use crate::value::geometry::ArcPos;
    let mut all = vec![point("p"), point("q")];
    for l in ["l", "m", "n"].map(line) {
        all.extend([l.start(), l.mid(), l.end()]);
    }
    for a in ["a", "b"].map(arc) {
        all.extend(
            [ArcPos::Centre, ArcPos::Start, ArcPos::Mid, ArcPos::End]
                .map(|at| PointRef::arc(a.id(), at)),
        );
    }
    all.extend(["c", "d"].map(|c| circle(c).centre()));
    all.extend(
        [
            SplinePos::Start,
            SplinePos::StartHandle,
            SplinePos::EndHandle,
            SplinePos::End,
        ]
        .map(|at| PointRef::spline("s", at)),
    );
    select(all)
}

fn any_line() -> impl Strategy<Value = LineRef> + Clone {
    select(vec![line("l"), line("m"), line("n")])
}

fn any_arc() -> impl Strategy<Value = ArcRef> + Clone {
    select(vec![arc("a"), arc("b")])
}

fn any_circ() -> impl Strategy<Value = CircRef> + Clone {
    select(vec![
        arc("a").into(),
        arc("b").into(),
        circle("c").into(),
        circle("d").into(),
    ])
}

fn any_end() -> impl Strategy<Value = ArcEnd> + Clone {
    select(vec![ArcEnd::Start, ArcEnd::End])
}

fn any_axis() -> impl Strategy<Value = Axis> + Clone {
    select(vec![Axis::X, Axis::Y])
}

/// An equation of relation kind `kind` (`Equation::kind`) over the zoo,
/// in canonical form. Panics for a kind with no strategy: a relation added
/// to `Equation::kind` must be given one here.
pub(super) fn equation(kind: usize) -> BoxedStrategy<Equation> {
    let (p, l, c) = (any_point, any_line, any_circ);
    let s = match kind {
        0 => (p(), p()).prop_map(|(a, b)| coincide(a, b)).boxed(),
        1 => (p(), l()).prop_map(|(a, l)| on_line(a, l)).boxed(),
        2 => (p(), c()).prop_map(|(a, c)| on_circle(a, c)).boxed(),
        3 => (p(), any_axis(), p(), any_axis())
            .prop_map(|(a, ax, b, bx)| same(a, ax, b, bx))
            .boxed(),
        4 => (l(), l()).prop_map(|(a, b)| parallel(a, b)).boxed(),
        5 => (l(), l()).prop_map(|(a, b)| perpendicular(a, b)).boxed(),
        6 => (l(), c())
            .prop_map(|(l, c)| line_tangent_circle(l, c))
            .boxed(),
        7 => (c(), c()).prop_map(|(a, b)| circles_tangent(a, b)).boxed(),
        8 => (l(), any_arc(), any_end())
            .prop_map(|(l, a, at)| line_tangent_arc(l, a, at))
            .boxed(),
        9 => (l(), select(vec![LinePos::Start, LinePos::End]), c())
            .prop_map(|(l, at, c)| line_tangent_circle_at(l, at, c))
            .boxed(),
        10 => (any_arc(), any_end(), any_arc(), any_end())
            .prop_map(|(a, at_a, b, at_b)| arcs_tangent(a, at_a, b, at_b))
            .boxed(),
        11 => (l(), l()).prop_map(|(a, b)| equal_length(a, b)).boxed(),
        12 => (c(), c()).prop_map(|(a, b)| equal_radius(a, b)).boxed(),
        13 => (p(), p(), length_mm())
            .prop_map(|(a, b, mm)| distance(a, b, mm))
            .boxed(),
        14 => (p(), p(), signed_mm())
            .prop_map(|(a, b, mm)| dx(a, b, mm))
            .boxed(),
        15 => (p(), p(), signed_mm())
            .prop_map(|(a, b, mm)| dy(a, b, mm))
            .boxed(),
        16 => (p(), l(), signed_mm())
            .prop_map(|(a, l, mm)| line_distance(a, l, mm))
            .boxed(),
        17 => (c(), length_mm()).prop_map(|(c, mm)| radius(c, mm)).boxed(),
        18 => (l(), l(), angle_rad())
            .prop_map(|(a, b, rad)| angle(a, b, rad))
            .boxed(),
        19 => (p(), p(), l())
            .prop_map(|(a, b, l)| mirror(a, b, l))
            .boxed(),
        20 => (p(), coord(), coord())
            .prop_map(|(a, x, y)| pinned(a, x, y))
            .boxed(),
        21 => (p(), p(), p(), p())
            .prop_map(|(a, b, c, d)| vectors_parallel(a, b, c, d))
            .boxed(),
        22 => (p(), p(), p(), p())
            .prop_map(|(a, b, c, d)| vectors_perpendicular(a, b, c, d))
            .boxed(),
        23 => (p(), p(), p())
            .prop_map(|(a, b, c)| midpoint_of(a, b, c))
            .boxed(),
        24 => (p(), l(), l(), any::<bool>())
            .prop_map(|(a, l, m, same)| equidistant(a, l, m, same))
            .boxed(),
        25 => (p(), p(), p(), p())
            .prop_map(|(a, b, c, d)| on_bisector(a, b, c, d))
            .boxed(),
        _ => panic!("relation kind {kind} has no strategy"),
    };
    s.prop_filter_map("relates a thing to itself", |e| e.canonical().ok())
        .boxed()
}

/// An equation of any kind over the zoo, and values for the zoo with up
/// to three ties among the positions the equation reads.
pub(super) fn case() -> impl Strategy<Value = (Equation, DVector<f64>)> {
    (0..KINDS)
        .prop_flat_map(equation)
        .prop_flat_map(|e| {
            let layout = zoo_layout();
            (Just(e), state(&layout), ties(&e, &layout))
        })
        .prop_map(|(e, mut x, ties)| {
            let layout = zoo_layout();
            for t in &ties {
                t.apply(&layout, &mut x);
            }
            (e, x)
        })
}

/// Up to four equations of any kinds over the zoo, and values for the
/// zoo with ties among the positions the equations read: a small system
/// at a state that is often special.
pub(super) fn system() -> impl Strategy<Value = (Vec<Equation>, DVector<f64>)> {
    proptest::collection::vec((0..KINDS).prop_flat_map(equation), 1..=4)
        .prop_flat_map(|eqs| {
            let layout = zoo_layout();
            let ties: Vec<_> = eqs.iter().map(|e| ties(e, &layout)).collect();
            (Just(eqs), state(&layout), ties)
        })
        .prop_map(|(eqs, mut x, ties)| {
            let layout = zoo_layout();
            for t in ties.iter().flatten() {
                t.apply(&layout, &mut x);
            }
            (eqs, x)
        })
}

// ---- 4. motions

/// A rigid motion of the plane: a turn about the origin, then a shift.
#[derive(Clone, Copy, Debug)]
pub(super) struct Rigid {
    pub(super) turn: f64,
    pub(super) shift: [f64; 2],
}

impl Rigid {
    /// The turn's sine and cosine, exact for whole quarter turns.
    fn sin_cos(&self) -> (f64, f64) {
        let k = (self.turn / FRAC_PI_2).round();
        if k * FRAC_PI_2 != self.turn {
            return self.turn.sin_cos();
        }
        [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)][k.rem_euclid(4.0) as usize]
    }

    pub(super) fn point(&self, p: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.sin_cos();
        [
            c * p[0] - s * p[1] + self.shift[0],
            s * p[0] + c * p[1] + self.shift[1],
        ]
    }

    /// The state with the drawing moved: every position turned and
    /// shifted, every arc's start angle turned; radii and sweeps as they
    /// were.
    pub(super) fn state(&self, layout: &Layout, x: &DVector<f64>) -> DVector<f64> {
        let mut y = x.clone();
        for (i, (_, v)) in layout.names.iter().enumerate() {
            match v {
                Value::X | Value::StartX | Value::EndX | Value::CentreX | Value::SplineX(_) => {
                    [y[i], y[i + 1]] = self.point([x[i], x[i + 1]]);
                }
                Value::StartAngle => y[i] += self.turn,
                _ => {}
            }
        }
        y
    }
}

/// A rigid motion: quarter turns and whole shifts (exact on the grid)
/// first, then any.
pub(super) fn rigid() -> impl Strategy<Value = Rigid> {
    (angle_rad(), coord(), coord()).prop_map(|(turn, x, y)| Rigid {
        turn,
        shift: [x, y],
    })
}

#[test]
fn every_kind_has_a_strategy_of_its_own_kind() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    for kind in 0..KINDS {
        let e = equation(kind).new_tree(&mut runner).unwrap().current();
        assert_eq!(e.kind(), kind, "{e:?}");
        e.check(&zoo()).unwrap();
    }
}
