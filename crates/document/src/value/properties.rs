//! Properties of the plain values: printed numbers and units, dates,
//! entities' canonical form, and the similarities that place a view on
//! the paper and the paper on the scan.

use crate::testing::geometry::{entity, p2, transform};
use crate::testing::values::{any_angle_rad, coord, grid, printed, unit};
use crate::value::drafting::Units;
use crate::value::frame::{Map, Similarity};
use crate::value::geometry::{Entity, EntityId, Geometry, Model, Paper, P2};
use crate::value::measure::{Measure, Quantity, Unit};
use crate::value::provenance::Date;
use proptest::prelude::*;
use std::f64::consts::PI;

/// `a` and `b` within `tol` of each other, relative to their size.
fn near(a: P2<impl crate::value::geometry::Frame>, b: [f64; 2], tol: f64) -> bool {
    let scale = 1.0 + a.x.abs().max(a.y.abs());
    (a.x - b[0]).abs() <= tol * scale && (a.y - b[1]).abs() <= tol * scale
}

proptest! {
    // 512 cases each; all of them together well under a second.
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Printed text (a decimal, a fraction, a mixed number) means the
    /// number it says, in its unit; converted to the base unit and back
    /// it is the same; it survives the file form unchanged.
    #[test]
    fn printed_numbers_mean_what_they_say((text, v) in printed(), u in unit(Quantity::Length)) {
        let m = Measure::printed(&text, u);
        prop_assert!(m.is_ok(), "{:?}", text);
        let m = m.unwrap();
        prop_assert_eq!(m.number(), Some(v));
        let back = m.base().unwrap() / u.to_base();
        prop_assert!((back - v).abs() <= 4.0 * f64::EPSILON * v.abs(), "{} {}", back, v);
        let json = serde_json::to_string(&m).unwrap();
        prop_assert_eq!(serde_json::from_str::<Measure>(&json).unwrap(), m);
    }

    /// A value of ours prints by the units policy as text the sheet
    /// could have printed: it reads back as a printed measure, within
    /// half a unit in its last place, and zero is never `-0`.
    #[test]
    fn values_print_as_text_that_reads_back(
        v in prop_oneof![grid(-3, 3), -1e4..1e4, -1.0..1.0],
        decimals in 0u8..6,
        leading_zero in any::<bool>(),
        unit in unit(Quantity::Length),
        angle in any::<bool>(),
    ) {
        let units = Units { unit, decimals, leading_zero };
        let (q, shown) = if angle {
            (Quantity::Angle, Unit::Deg)
        } else {
            (Quantity::Length, unit)
        };
        let text = units.print(v * shown.to_base(), q);
        let bare = text.trim_end_matches('°');
        let m = Measure::printed(bare, shown);
        prop_assert!(m.is_ok(), "{:?} from {}", text, v);
        let read = m.unwrap().number().unwrap();
        let half = 0.5 * 10f64.powi(-i32::from(decimals));
        prop_assert!((read - v).abs() <= half * (1.0 + 1e-9) + 1e-9 * v.abs(), "{} {}", text, v);
        if read == 0.0 {
            prop_assert!(!text.starts_with('-'), "{}", text);
        }
    }

    /// A date the type takes writes as `YYYY-MM-DD` and reads back; text
    /// it reads is exactly what it writes.
    #[test]
    fn dates_write_as_they_read(
        y in prop_oneof![1900u16..2100, any::<u16>()],
        m in 0u8..14,
        d in 0u8..33,
        text in "[0-9+]{4}-[0-9+ ]{2}-[0-9+]{2}",
    ) {
        if let Ok(date) = Date::new(y, m, d) {
            prop_assert_eq!(Date::parse(&date.to_string()), Ok(date));
            let json = serde_json::to_string(&date).unwrap();
            prop_assert_eq!(serde_json::from_str::<Date>(&json).unwrap(), date);
        }
        if let Ok(date) = Date::parse(&text) {
            prop_assert_eq!(date.to_string(), text);
        }
    }

    /// An entity is stored in canonical form, and storing that again
    /// changes nothing: an arc's start angle lands in `[−π, π)` and
    /// stays there. The file form is the identity on a geometry.
    #[test]
    fn entities_are_stored_canonically(e in entity::<Model>()) {
        let id = EntityId::new("e");
        let mut g = Geometry::default();
        if g.add(id, e).is_err() {
            return Ok(());
        }
        let stored = g.get(id).unwrap().clone();
        if let Entity::Arc { start_angle, .. } = stored {
            prop_assert!((-PI..PI).contains(&start_angle), "{}", start_angle);
        }
        g.set(id, stored.clone()).unwrap();
        prop_assert_eq!(g.get(id), Some(&stored));
        let json = serde_json::to_string(&g).unwrap();
        prop_assert_eq!(serde_json::from_str::<Geometry<Model>>(&json).unwrap(), g);
    }

    /// The start angle alone, anywhere: canonical once is canonical.
    #[test]
    fn start_angles_settle(a in any_angle_rad()) {
        let arc = |start_angle| Entity::<Model>::Arc {
            centre: P2::new(0.0, 0.0),
            radius: 1.0,
            start_angle,
            sweep: 1.0,
        };
        let once = arc(a).canonical().unwrap();
        let Entity::Arc { start_angle, .. } = once else { unreachable!() };
        prop_assert!((-PI..PI).contains(&start_angle), "{} from {}", start_angle, a);
        prop_assert_eq!(arc(start_angle).canonical().unwrap(), once);
    }

    /// A view's placement and its inverse undo each other, both ways.
    #[test]
    fn views_place_and_back(t in transform(), p in p2::<Model>(), q in p2::<Paper>()) {
        let m = t.to_model(t.to_paper(p));
        prop_assert!(near(p, [m.x, m.y], 1e-9), "{:?} {:?}", p, m);
        let back = t.to_paper(t.to_model(q));
        prop_assert!(near(q, [back.x, back.y], 1e-9), "{:?} {:?}", q, back);
    }

    /// The paper similarity and its inverse likewise.
    #[test]
    fn papers_register_and_back(
        origin in (coord(), coord()),
        px_per_mm in prop_oneof![Just(8.0), 1.0..40.0],
        rotation in any_angle_rad(),
        p in p2::<Paper>(),
    ) {
        let r = Similarity::new([origin.0, origin.1], px_per_mm, rotation).unwrap();
        let back = r.forward(r.inverse([p.x, p.y]));
        prop_assert!(near(p, back, 1e-9), "{:?} {:?}", p, back);
    }
}
