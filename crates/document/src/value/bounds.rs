//! Bounding boxes: the axis-aligned [`Rect`] around anything that has
//! extent in a frame.
//!
//! [`Bounded`] is the one way to ask for a box, in whatever frame the
//! thing lives in: an [`Entity`] in its view's model frame or on the
//! paper, and, in the render crate, every path, primitive, text and
//! scene on the paper. The boxes are exact for the curves the document
//! draws: an arc's box takes in each quadrant point its sweep passes,
//! a cubic's each turning point of its two coordinates, an elliptical
//! arc's each turning point of the rotated ellipse. Nothing is sampled.
//!
//! A box is of the centre line: a pen's width is the caller's to add
//! ([`Rect::grow`]), because what draws a curve, and how heavily, is
//! not the curve's business.
//!
//! Something with no extent (an empty scene) or with a coordinate that
//! is not finite has no box: `None`.

use crate::value::geometry::{Entity, Frame, Rect, P2};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Something with an axis-aligned box in frame `F`.
pub trait Bounded<F: Frame> {
    /// The box around it, or `None` if it has no extent or a
    /// coordinate is not finite.
    fn bounds(&self) -> Option<Rect<F>>;
}

impl<F: Frame, T: Bounded<F>> Bounded<F> for [T] {
    /// The box around every item that has one; `None` if none has.
    fn bounds(&self) -> Option<Rect<F>> {
        Rect::union_all(self.iter().filter_map(Bounded::bounds))
    }
}

impl<F: Frame, T: Bounded<F>> Bounded<F> for Vec<T> {
    fn bounds(&self) -> Option<Rect<F>> {
        self.as_slice().bounds()
    }
}

impl<F: Frame, T: Bounded<F> + ?Sized> Bounded<F> for &T {
    fn bounds(&self) -> Option<Rect<F>> {
        (**self).bounds()
    }
}

/// Whether angle `a` lies on the sweep from `start` through `sweep`
/// (signed), ends included.
fn on_sweep(a: f64, start: f64, sweep: f64) -> bool {
    if sweep.abs() >= TAU {
        return true;
    }
    let d = if sweep >= 0.0 { a - start } else { start - a };
    let d = d.rem_euclid(TAU);
    // An angle a hair past the start by rounding counts as the start.
    let d = if TAU - d < 1e-12 { 0.0 } else { d };
    d <= sweep.abs() + 1e-12
}

/// The angles in `[start, start + sweep]` (signed) of the form
/// `base + k·step`, for the integers `k` that put them on the sweep.
fn on_sweep_at(base: f64, step: f64, start: f64, sweep: f64) -> Vec<f64> {
    let (lo, hi) = if sweep >= 0.0 {
        (start, start + sweep)
    } else {
        (start + sweep, start)
    };
    let k0 = ((lo - base) / step).floor() as i64 - 1;
    let k1 = ((hi - base) / step).ceil() as i64 + 1;
    (k0..=k1)
        .map(|k| base + k as f64 * step)
        .filter(|a| on_sweep(*a, start, sweep))
        .collect()
}

/// The box of a circular arc about `centre` of `radius`, from angle
/// `start` through `sweep` radians (signed), angles from +x towards +y
/// in the frame's own axes: its two ends and every axis point it
/// passes. A sweep of a full turn or more is the whole circle.
pub fn arc_bounds<F: Frame>(centre: P2<F>, radius: f64, start: f64, sweep: f64) -> Option<Rect<F>> {
    if !(radius.is_finite() && start.is_finite() && sweep.is_finite()) {
        return None;
    }
    let at = |a: f64| P2::new(centre.x + radius * a.cos(), centre.y + radius * a.sin());
    let mut points = vec![at(start), at(start + sweep)];
    points.extend(
        on_sweep_at(0.0, FRAC_PI_2, start, sweep)
            .into_iter()
            .map(at),
    );
    Rect::of_points(points)
}

/// The box of a circle.
pub fn circle_bounds<F: Frame>(centre: P2<F>, radius: f64) -> Option<Rect<F>> {
    if !radius.is_finite() {
        return None;
    }
    let r = radius.abs();
    Rect::of_points([
        P2::new(centre.x - r, centre.y - r),
        P2::new(centre.x + r, centre.y + r),
    ])
}

/// The parameters in `(0, 1)` where one coordinate of a cubic Bézier
/// with these four values turns: the roots of its derivative.
fn cubic_turns(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    // B'(t)/3 = a t² + b t + c.
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let scale = a.abs().max(b.abs()).max(c.abs());
    if scale == 0.0 {
        return Vec::new();
    }
    let mut roots = Vec::new();
    if a.abs() <= 1e-12 * scale {
        if b.abs() > 1e-12 * scale {
            roots.push(-c / b);
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            // The numerically stable pair.
            let q = -0.5 * (b + b.signum() * disc.sqrt());
            roots.push(q / a);
            if q != 0.0 {
                roots.push(c / q);
            }
        }
    }
    roots.into_iter().filter(|t| *t > 0.0 && *t < 1.0).collect()
}

/// The point at `t` of a cubic Bézier.
fn cubic_at<F: Frame>(p: [P2<F>; 4], t: f64) -> P2<F> {
    let u = 1.0 - t;
    let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    P2::new(
        w0 * p[0].x + w1 * p[1].x + w2 * p[2].x + w3 * p[3].x,
        w0 * p[0].y + w1 * p[1].y + w2 * p[2].y + w3 * p[3].y,
    )
}

/// The box of a cubic Bézier: its two ends and every turning point of
/// either coordinate (tighter than the box of its control points).
pub fn cubic_bounds<F: Frame>(p: [P2<F>; 4]) -> Option<Rect<F>> {
    let mut points = vec![p[0], p[3]];
    let ts = cubic_turns(p[0].x, p[1].x, p[2].x, p[3].x)
        .into_iter()
        .chain(cubic_turns(p[0].y, p[1].y, p[2].y, p[3].y));
    points.extend(ts.map(|t| cubic_at(p, t)));
    Rect::of_points(points)
}

/// The box of an arc of an ellipse: the point at parameter `t` is
/// `centre + R(rotation)·(rx cos t, ry sin t)`, from `start` through
/// `sweep` (signed); its two ends and every turning point of either
/// coordinate on the sweep.
pub fn elliptical_arc_bounds<F: Frame>(
    centre: P2<F>,
    rx: f64,
    ry: f64,
    rotation: f64,
    start: f64,
    sweep: f64,
) -> Option<Rect<F>> {
    if ![rx, ry, rotation, start, sweep]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    let (s, c) = rotation.sin_cos();
    let at = |t: f64| {
        let (ex, ey) = (rx * t.cos(), ry * t.sin());
        P2::new(centre.x + c * ex - s * ey, centre.y + s * ex + c * ey)
    };
    // x' = −rx sin t cos φ − ry cos t sin φ = 0 at t = atan2(−ry sin φ,
    // rx cos φ) + kπ; y' = −rx sin t sin φ + ry cos t cos φ = 0 at
    // t = atan2(ry cos φ, rx sin φ) + kπ.
    let tx = (-ry * s).atan2(rx * c);
    let ty = (ry * c).atan2(rx * s);
    let mut points = vec![at(start), at(start + sweep)];
    for base in [tx, ty] {
        points.extend(
            on_sweep_at(base, std::f64::consts::PI, start, sweep)
                .into_iter()
                .map(at),
        );
    }
    Rect::of_points(points)
}

impl<F: Frame> Bounded<F> for P2<F> {
    fn bounds(&self) -> Option<Rect<F>> {
        Rect::around(*self)
    }
}

impl<F: Frame> Bounded<F> for Rect<F> {
    fn bounds(&self) -> Option<Rect<F>> {
        Some(*self)
    }
}

impl<F: Frame> Bounded<F> for Entity<F> {
    /// The exact box of the entity's curve (a point's is the point).
    fn bounds(&self) -> Option<Rect<F>> {
        match self {
            Entity::Point { at } => Rect::around(*at),
            Entity::Line { start, end } => Rect::of_points([*start, *end]),
            Entity::Arc {
                centre,
                radius,
                start_angle,
                sweep,
            } => arc_bounds(*centre, *radius, *start_angle, *sweep),
            Entity::Circle { centre, radius } => circle_bounds(*centre, *radius),
            Entity::Spline { points } => Rect::union_all(
                points
                    .windows(4)
                    .step_by(3)
                    .map(|w| cubic_bounds([w[0], w[1], w[2], w[3]]))
                    .collect::<Option<Vec<_>>>()?,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::geometry::{Model, Paper};
    use std::f64::consts::PI;

    fn p(x: f64, y: f64) -> P2<Model> {
        P2::new(x, y)
    }

    fn close(r: Rect<Model>, min: (f64, f64), max: (f64, f64)) {
        let ok = (r.min().x - min.0).abs() < 1e-9
            && (r.min().y - min.1).abs() < 1e-9
            && (r.max().x - max.0).abs() < 1e-9
            && (r.max().y - max.1).abs() < 1e-9;
        assert!(ok, "{r:?} is not {min:?}..{max:?}");
    }

    /// The box of many points along a curve: a check from outside, and
    /// always inside the exact box (to rounding).
    fn sampled(at: impl Fn(f64) -> P2<Model>) -> Rect<Model> {
        Rect::of_points((0..=20_000).map(|i| at(i as f64 / 20_000.0))).unwrap()
    }

    fn agrees(exact: Rect<Model>, sample: Rect<Model>, tol: f64) {
        assert!(
            exact.grow(1e-9).contains_rect(&sample),
            "{exact:?} misses {sample:?}"
        );
        assert!(
            sample.grow(tol).contains_rect(&exact),
            "{exact:?} too loose for {sample:?}"
        );
    }

    #[test]
    fn rect_operations() {
        let a = Rect::of_points([p(0.0, 0.0), p(2.0, 1.0)]).unwrap();
        let b = Rect::of_points([p(1.0, -1.0), p(3.0, 0.5)]).unwrap();
        close(a.union(b), (0.0, -1.0), (3.0, 1.0));
        assert!(a.intersects(&b) && b.intersects(&a));
        let far = Rect::around(p(10.0, 10.0)).unwrap();
        assert!(!a.intersects(&far));
        // Touching edges intersect.
        let edge = Rect::of_points([p(2.0, 0.0), p(4.0, 1.0)]).unwrap();
        assert!(a.intersects(&edge));
        assert_eq!((a.width(), a.height()), (2.0, 1.0));
        assert_eq!(a.centre(), p(1.0, 0.5));
        close(a.grow(0.5), (-0.5, -0.5), (2.5, 1.5));
        // Shrinking stops at the centre line, never inverts.
        close(a.grow(-0.75), (0.75, 0.5), (1.25, 0.5));
        close(a.grow(-5.0), (1.0, 0.5), (1.0, 0.5));
        assert_eq!(a.grow(f64::NAN), a);
        close(a.translated(1.0, -1.0), (1.0, -1.0), (3.0, 0.0));
        assert!(a.grow(1.0).contains_rect(&a) && !a.contains_rect(&a.grow(1.0)));
        assert_eq!(Rect::<Model>::union_all([]), None);
        assert_eq!(Rect::<Model>::of_points([]), None);
        assert_eq!(Rect::of_points([p(0.0, 0.0), p(f64::NAN, 1.0)]), None);
        assert_eq!(Rect::around(p(f64::INFINITY, 0.0)), None);
    }

    #[test]
    fn arcs_take_in_the_axis_points_they_pass() {
        // A quarter from +x to +y: no axis point inside, just the ends.
        let q = arc_bounds(p(0.0, 0.0), 1.0, 0.0, FRAC_PI_2).unwrap();
        close(q, (0.0, 0.0), (1.0, 1.0));
        // From 45° through 90°: passes +y.
        let a = arc_bounds(p(0.0, 0.0), 2.0, PI / 4.0, FRAC_PI_2).unwrap();
        let r = 2.0 * (PI / 4.0).cos();
        close(a, (-r, r), (r, 2.0));
        // Clockwise (decreasing angle) from +y through +x to −y: passes
        // +x, not −x.
        let cw = arc_bounds(p(1.0, 1.0), 1.0, FRAC_PI_2, -PI).unwrap();
        close(cw, (1.0, 0.0), (2.0, 2.0));
        // A sweep over ±π wraps: from 170° to −170° the short way
        // passes −x; the long way passes the other three.
        let short = arc_bounds(p(0.0, 0.0), 1.0, 170f64.to_radians(), 20f64.to_radians()).unwrap();
        assert!((short.min().x + 1.0).abs() < 1e-12);
        let long = arc_bounds(p(0.0, 0.0), 1.0, 170f64.to_radians(), -340f64.to_radians()).unwrap();
        close(long, (-(170f64.to_radians().cos().abs()), -1.0), (1.0, 1.0));
        // A full turn is the circle.
        close(
            arc_bounds(p(0.0, 0.0), 1.0, 0.3, TAU).unwrap(),
            (-1.0, -1.0),
            (1.0, 1.0),
        );
        assert_eq!(arc_bounds(p(0.0, 0.0), f64::NAN, 0.0, 1.0), None);
    }

    #[test]
    fn arcs_agree_with_dense_sampling() {
        for (start, sweep) in [
            (0.1, 0.2),
            (-3.0, 5.5),
            (2.9, -4.0),
            (-PI, PI),
            (1.0, -6.2),
            (FRAC_PI_2, FRAC_PI_2),
            (0.0, -FRAC_PI_2),
        ] {
            let (c, r) = (p(3.0, -2.0), 1.7);
            let exact = arc_bounds(c, r, start, sweep).unwrap();
            let s = sampled(|t| {
                let a = start + sweep * t;
                p(c.x + r * a.cos(), c.y + r * a.sin())
            });
            agrees(exact, s, 1e-6);
        }
    }

    #[test]
    fn cubics_take_in_their_turning_points() {
        // A symmetric hump: ends on y = 0, controls at y = 1; the peak is
        // 3/4 at t = 1/2, well inside the control box.
        let c = [p(0.0, 0.0), p(0.0, 1.0), p(1.0, 1.0), p(1.0, 0.0)];
        close(cubic_bounds(c).unwrap(), (0.0, 0.0), (1.0, 0.75));
        // A straight cubic is its segment.
        let line = [p(0.0, 0.0), p(1.0, 1.0), p(2.0, 2.0), p(3.0, 3.0)];
        close(cubic_bounds(line).unwrap(), (0.0, 0.0), (3.0, 3.0));
        // A degenerate one is its point.
        let dot = [p(1.0, 2.0); 4];
        close(cubic_bounds(dot).unwrap(), (1.0, 2.0), (1.0, 2.0));
        // An S overshoots in x on both sides.
        // x(t) = 12u²t − 9ut² + t³: 1.28 at t = 1/4, −0.28 at t = 3/4.
        let s = [p(0.0, 0.0), p(4.0, 0.0), p(-3.0, 1.0), p(1.0, 1.0)];
        let b = cubic_bounds(s).unwrap();
        assert!(b.min().x < 0.0 && b.max().x > 1.0);
    }

    #[test]
    fn cubics_agree_with_dense_sampling() {
        let cases = [
            [p(0.0, 0.0), p(5.0, 3.0), p(-2.0, 4.0), p(1.0, -1.0)],
            [p(0.0, 0.0), p(0.0, 0.0), p(4.0, 0.0), p(4.0, 4.0)],
            [p(-1.0, 2.0), p(3.0, -5.0), p(3.0, 9.0), p(-1.0, 2.0)],
            [p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0), p(3.0, 0.0)],
            [p(0.0, 0.0), p(1e-9, 1.0), p(1.0, 1.0 + 1e-9), p(1.0, 0.0)],
        ];
        for c in cases {
            let exact = cubic_bounds(c).unwrap();
            agrees(exact, sampled(|t| cubic_at(c, t)), 1e-6);
        }
    }

    #[test]
    fn elliptical_arcs_agree_with_dense_sampling() {
        for (rx, ry, rot, start, sweep) in [
            (2.0, 1.0, 0.0, 0.0, TAU),
            (2.0, 1.0, 0.7, 0.0, TAU),
            (3.0, 0.5, -1.2, 0.4, 2.5),
            (1.0, 4.0, 2.0, -1.0, -3.9),
            (1.5, 1.5, 0.3, 2.0, 1.0),
        ] {
            let c = p(-1.0, 2.0);
            let exact = elliptical_arc_bounds(c, rx, ry, rot, start, sweep).unwrap();
            let (s, co) = f64::sin_cos(rot);
            let s = sampled(|t| {
                let a = start + sweep * t;
                let (ex, ey) = (rx * a.cos(), ry * a.sin());
                p(c.x + co * ex - s * ey, c.y + s * ex + co * ey)
            });
            agrees(exact, s, 1e-6);
        }
        // An unrotated full ellipse is ±rx, ±ry.
        let e = elliptical_arc_bounds(p(0.0, 0.0), 2.0, 1.0, 0.0, 0.0, TAU).unwrap();
        close(e, (-2.0, -1.0), (2.0, 1.0));
    }

    #[test]
    fn entities_have_their_curves_boxes() {
        let line = Entity::Line {
            start: p(3.0, 1.0),
            end: p(-1.0, 4.0),
        };
        close(line.bounds().unwrap(), (-1.0, 1.0), (3.0, 4.0));
        let pt = Entity::Point { at: p(2.0, 2.0) };
        close(pt.bounds().unwrap(), (2.0, 2.0), (2.0, 2.0));
        let circle = Entity::Circle {
            centre: p(1.0, 1.0),
            radius: 2.0,
        };
        close(circle.bounds().unwrap(), (-1.0, -1.0), (3.0, 3.0));
        let arc = Entity::Arc {
            centre: p(0.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            sweep: PI,
        };
        close(arc.bounds().unwrap(), (-1.0, 0.0), (1.0, 1.0));
        // A two-segment spline is the union of its segments' boxes.
        let spline = Entity::Spline {
            points: vec![
                p(0.0, 0.0),
                p(0.0, 1.0),
                p(1.0, 1.0),
                p(1.0, 0.0),
                p(1.0, -1.0),
                p(2.0, -1.0),
                p(2.0, 0.0),
            ],
        };
        close(spline.bounds().unwrap(), (0.0, -0.75), (2.0, 0.75));
        let bad = Entity::Line {
            start: p(f64::NAN, 0.0),
            end: p(1.0, 1.0),
        };
        assert_eq!(bad.bounds(), None);
        // Slices and vectors union what they hold, skipping what has no
        // box; an empty one has none.
        let all = vec![line.clone(), circle.clone(), bad];
        close(all.bounds().unwrap(), (-1.0, -1.0), (3.0, 4.0));
        assert_eq!(Vec::<Entity<Model>>::new().bounds(), None);
        // The frame is carried: a paper entity gives a paper box.
        let on_paper: Entity<Paper> = Entity::Line {
            start: P2::new(0.0, 0.0),
            end: P2::new(1.0, 1.0),
        };
        let r: Rect<Paper> = on_paper.bounds().unwrap();
        assert_eq!(r.max(), P2::new(1.0, 1.0));
    }
}
