//! Numbers by what they measure, measures, printed numbers, dates and
//! provenance. Every union lists the grid (or the plain case) first, so
//! failures shrink towards it.

use crate::value::measure::{Measure, Quantity, Unit};
use crate::value::provenance::{Agent, Citation, Date, Method, Provenance};
use proptest::prelude::*;
use proptest::sample::select;
use std::f64::consts::{FRAC_PI_2, PI};

/// An integer in `lo..=hi`, as a float: the grid.
pub fn grid(lo: i8, hi: i8) -> impl Strategy<Value = f64> + Clone {
    (lo..=hi).prop_map(f64::from)
}

/// A coordinate, mm: on the grid or anywhere in ±20.
pub fn coord() -> BoxedStrategy<f64> {
    prop_oneof![grid(-3, 3), -20.0..20.0].boxed()
}

/// A radius, mm.
pub fn radius_mm() -> BoxedStrategy<f64> {
    prop_oneof![grid(1, 3), 0.2..20.0].boxed()
}

/// An angle, rad: a whole number of quarter turns, or any.
pub fn angle_rad() -> BoxedStrategy<f64> {
    prop_oneof![grid(-4, 4).prop_map(|k| k * FRAC_PI_2), -PI..PI].boxed()
}

/// An angle, rad, as stored or as a file may give it: anything above,
/// or within a few units in the last place of a half turn either way
/// (where keeping it in `[−π, π)` has to round), or many turns out.
pub fn any_angle_rad() -> BoxedStrategy<f64> {
    let near_half_turn = (select(vec![-PI, PI]), -4i64..=4).prop_map(|(a, k)| {
        let bits = a.to_bits() as i64 + k;
        f64::from_bits(bits as u64)
    });
    prop_oneof![angle_rad(), near_half_turn, -100.0..100.0].boxed()
}

/// An arc's sweep, rad, either way round.
pub fn sweep() -> BoxedStrategy<f64> {
    prop_oneof![
        grid(-3, 3).prop_map(|k| if k == 0.0 { PI } else { k * FRAC_PI_2 }),
        0.05..6.2,
        -6.2..-0.05,
    ]
    .boxed()
}

/// A positive length, mm.
pub fn length_mm() -> impl Strategy<Value = f64> + Clone {
    prop_oneof![grid(1, 4), 0.1..20.0]
}

/// A signed length, mm.
pub fn signed_mm() -> impl Strategy<Value = f64> + Clone {
    prop_oneof![grid(-4, 4), -20.0..20.0]
}

/// A number that may not be one: finite mostly, NaN or infinite now
/// and then, for testing what is refused.
pub fn maybe_finite(finite: impl Strategy<Value = f64> + 'static) -> BoxedStrategy<f64> {
    prop_oneof![
        8 => finite,
        1 => select(vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY]),
    ]
    .boxed()
}

/// A unit of quantity `q`.
pub fn unit(q: Quantity) -> impl Strategy<Value = Unit> + Clone {
    match q {
        Quantity::Length => select(vec![Unit::Mm, Unit::In]),
        Quantity::Angle => select(vec![Unit::Deg]),
    }
}

/// Printed text that parses, with the number it means: a decimal as
/// the sheets print them (`3.200`, `.188`, `-0.5`), a fraction (`5/16`)
/// or a mixed number (`2 5/16`).
pub fn printed() -> impl Strategy<Value = (String, f64)> + Clone {
    let sign = select(vec!["", "-", "+"]);
    let decimal =
        (sign.clone(), 0u32..1000, 0u32..4, any::<bool>()).prop_map(|(s, digits, places, lead)| {
            let mut t = format!("{:0>w$}", digits, w = places as usize + 1);
            if places > 0 {
                t.insert(t.len() - places as usize, '.');
            }
            if !lead && t.starts_with("0.") {
                t.remove(0);
            }
            let text = format!("{s}{t}");
            let v: f64 = text.parse().expect("a decimal");
            (text, v)
        });
    let fraction = (sign, 0u32..20, 0u32..64, 1u32..64).prop_map(|(s, w, n, d)| {
        let text = if w == 0 {
            format!("{s}{n}/{d}")
        } else {
            format!("{s}{w} {n}/{d}")
        };
        let v = f64::from(w) + f64::from(n) / f64::from(d);
        (text, if s == "-" { -v } else { v })
    });
    prop_oneof![decimal, fraction]
}

/// A measure of quantity `q`, built through the constructors: a value
/// of ours, printed text, or a parameter from `params`.
pub fn measure(q: Quantity, params: &'static [&'static str]) -> BoxedStrategy<Measure> {
    let number = match q {
        Quantity::Length => length_mm().boxed(),
        Quantity::Angle => {
            prop_oneof![select(vec![30.0, 45.0, 60.0, 90.0, 135.0]), -180.0..180.0].boxed()
        }
    };
    let value = (number, unit(q)).prop_map(|(v, u)| Measure::value(v, u).expect("finite"));
    let printed =
        (printed(), unit(q)).prop_map(|((t, _), u)| Measure::printed(&t, u).expect("parses"));
    let mut kinds = vec![(4, value.boxed()), (2, printed.boxed())];
    if !params.is_empty() {
        let param = select(params).prop_map(|n| Measure::param(n.into()).expect("a name"));
        kinds.push((1, param.boxed()));
    }
    proptest::strategy::Union::new_weighted(kinds).boxed()
}

/// A measure as [`measure`] gives, or now and then one built from its
/// public variants with a value no constructor allows (NaN, infinite):
/// what the document must refuse at its boundary.
pub fn measure_or_raw(q: Quantity, params: &'static [&'static str]) -> BoxedStrategy<Measure> {
    let bad = select(vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY]);
    let raw = (bad, unit(q)).prop_map(|(value, unit)| Measure::Value { value, unit });
    prop_oneof![12 => measure(q, params), 1 => raw].boxed()
}

/// A calendar date, mostly this decade's; any day the type allows now
/// and then (a few in the second branch are not days, and are skipped).
pub fn date() -> impl Strategy<Value = Date> + Clone {
    prop_oneof![
        (2020u16..2030, 1u8..=12, 1u8..=28),
        (1900u16..=9999, 1u8..=12, 1u8..=31),
    ]
    .prop_filter_map("a calendar day", |(y, m, d)| Date::new(y, m, d).ok())
}

/// Who.
pub fn agent() -> impl Strategy<Value = Agent> + Clone {
    select(vec![
        Agent::User,
        Agent::Claude,
        Agent::Script { name: "fit".into() },
    ])
}

/// A provenance record, provisional: ours (needing no evidence) or read
/// or measured citing one of `sources` (which a document must have
/// registered) mostly; now and then one the record refuses (read with no
/// evidence, a negative sigma).
pub fn provenance(sources: &'static [&'static str]) -> BoxedStrategy<Provenance> {
    let cite = move |m: Method| select(sources).prop_map(move |doc| (m.clone(), Some(doc)));
    let how = prop_oneof![
        4 => Just((Method::Authored, None)),
        1 => Just((Method::Assumed, None)),
        3 => cite(Method::Read),
        1 => select(vec![0.1, 0.5]).prop_flat_map(move |sigma_mm| {
            cite(Method::Measured { sigma_mm })
        }),
        1 => prop_oneof![
            Just((Method::Read, None)),
            cite(Method::Measured { sigma_mm: -1.0 }),
        ],
    ];
    (how, agent(), date())
        .prop_map(|((method, cite), by, when)| {
            let p = Provenance::new(method, by, when);
            match cite {
                Some(doc) => p.cite(Citation::new(doc, "zone C4")),
                None => p,
            }
        })
        .boxed()
}
