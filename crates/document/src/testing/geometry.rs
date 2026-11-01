//! Points, entities, boxes and view transforms, in either frame.

use super::values::{angle_rad, any_angle_rad, coord, radius_mm, sweep};
use crate::value::geometry::{Entity, Frame, Kind, Rect, P2};
use crate::value::view::Transform;
use proptest::prelude::*;
use proptest::sample::select;

/// A point, mm.
pub fn p2<F: Frame>() -> impl Strategy<Value = P2<F>> + Clone {
    (coord(), coord()).prop_map(|(x, y)| P2::new(x, y))
}

/// An entity of any kind. On the grid a line's ends coincide now and
/// then, which the geometry refuses; an arc's start angle may be
/// anywhere (the geometry keeps it in `[−π, π)`).
pub fn entity<F: Frame + 'static>() -> BoxedStrategy<Entity<F>> {
    use Kind::*;
    let kinds = [Point, Line, Arc, Circle, Spline].map(entity_of);
    proptest::strategy::Union::new(kinds).boxed()
}

/// An entity of kind `kind`, as [`entity`].
pub fn entity_of<F: Frame + 'static>(kind: Kind) -> BoxedStrategy<Entity<F>> {
    match kind {
        Kind::Point => p2().prop_map(|at| Entity::Point { at }).boxed(),
        Kind::Line => (p2(), p2())
            .prop_map(|(start, end)| Entity::Line { start, end })
            .boxed(),
        Kind::Arc => (p2(), radius_mm(), any_angle_rad(), sweep())
            .prop_map(|(centre, radius, start_angle, sweep)| Entity::Arc {
                centre,
                radius,
                start_angle,
                sweep,
            })
            .boxed(),
        Kind::Circle => (p2(), radius_mm())
            .prop_map(|(centre, radius)| Entity::Circle { centre, radius })
            .boxed(),
        Kind::Spline => prop_oneof![
            proptest::collection::vec(p2(), 4),
            proptest::collection::vec(p2(), 7)
        ]
        .prop_map(|points| Entity::Spline { points })
        .boxed(),
    }
}

/// A box, its corners in order.
pub fn rect<F: Frame>() -> impl Strategy<Value = Rect<F>> + Clone {
    (p2(), p2()).prop_map(|(a, b): (P2<F>, P2<F>)| {
        let min = P2::new(a.x.min(b.x), a.y.min(b.y));
        let max = P2::new(a.x.max(b.x), a.y.max(b.y));
        Rect::new(min, max).expect("ordered and finite")
    })
}

/// A valid view transform: anywhere on an A0 sheet, at a usual drawing
/// scale or any in 0.05..20, turned, perhaps mirrored.
pub fn transform() -> impl Strategy<Value = Transform> + Clone {
    let at = prop_oneof![(0i16..=1189).prop_map(f64::from), 0.0..1189.0];
    let scale = prop_oneof![select(vec![1.0, 0.5, 2.0, 0.25, 4.0]), 0.05..20.0];
    (at.clone(), at, scale, angle_rad(), any::<bool>())
        .prop_map(|(x, y, s, r, m)| Transform::new(P2::new(x, y), s, r, m).expect("valid"))
}

/// A transform as [`transform`] gives, or now and then one a view
/// refuses (no scale, a NaN): built from the public fields.
pub fn transform_or_bad() -> impl Strategy<Value = Transform> + Clone {
    let bad = select(vec![0.0, -1.0, f64::NAN]).prop_map(|v| Transform {
        origin: P2::new(v, 0.0),
        scale: v,
        rotation: 0.0,
        mirror: false,
    });
    prop_oneof![8 => transform(), 1 => bad]
}
