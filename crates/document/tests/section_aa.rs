//! Stage 7 acceptance: Section A-A of Grumman LDW280-23631 sheet 3, whole,
//! from its traced start, through the public API only.
//!
//! Model millimetres: model Y along x, model X up the page. The pin's
//! stations fix the placement; the sheet's dimensions, the relations the
//! drawing shows and the sizes measured off the ink constrain the rest.

use lmsheets_document::examples::section_aa;
use lmsheets_document::state::analysis::{Backend, Jacobian, Options};
use lmsheets_document::value::geometry::Model;
use lmsheets_document::value::params::Params;
use lmsheets_document::value::relation::line;
use lmsheets_document::value::sketch::{EntityStatus, Sketch};
use std::time::Instant;

#[test]
fn section_aa_solves_fully_constrained_from_its_trace() {
    solves_with(Options::default());
}

/// Every backend and Jacobian on the whole section. Slow in a debug
/// build; run with `cargo test --release -p lmsheets-document --test
/// section_aa -- --ignored`.
#[test]
#[ignore]
fn section_aa_under_every_backend() {
    for backend in [Backend::Faer, Backend::Crate] {
        for how in [Jacobian::Local, Jacobian::Dense] {
            solves_with(Options::default().with_backend(backend).with_jacobian(how));
        }
    }
}

fn solves_with(options: Options) {
    let t0 = Instant::now();
    let mut s = section_aa();
    let built = t0.elapsed();
    let n_entities = s.geometry().len();
    let n_constraints = s.constraints().count();

    let t1 = Instant::now();
    let a = s.solve(options, &Params::NONE).unwrap();
    let solved = t1.elapsed();

    let not_fixed: Vec<_> = a
        .status
        .iter()
        .filter(|(_, st)| **st != EntityStatus::Fixed)
        .collect();
    eprintln!(
        "A-A ({options:?}): {n_entities} entities, {n_constraints} constraints, {} parameters, {} equations; \
         built in {built:?}, solved in {solved:?} ({} evaluations); rank {}, internal free {}, \
         placement {:?}, groups {}, residual max {:.2e}",
        a.report.parameters,
        a.report.equations,
        a.report.evaluations,
        a.report.rank,
        a.internal_free,
        a.placement,
        a.groups.len(),
        a.report.residual_max
    );
    assert!(
        a.internal_free == 0 && a.placement.count() == 0,
        "free {} placement {:?}\nnot fixed: {not_fixed:?}\nfreedoms: {:?}",
        a.internal_free,
        a.placement,
        a.report.freedoms
    );
    assert!(a.groups.is_empty(), "{:?}", a.groups);
    assert!(a.report.residual_max < 1e-6, "{:?}", a.report.residuals);
    for check in &a.reference {
        assert!(
            check.off_by.iter().all(|v| v.abs() < 0.2),
            "REF {} off by {:?} mm",
            check.constraint,
            check.off_by
        );
    }
    // The printed 6.400 between the flange inner faces holds exactly.
    let upper = s.geometry().position(&line("u_top_i").end()).unwrap();
    let lower = s.geometry().position(&line("l_fl_i").start()).unwrap();
    assert!((upper.y - lower.y - 162.56).abs() < 1e-9);

    // Costs that the Elm-style model will pay: a copy, and a re-analysis.
    let t2 = Instant::now();
    let copy = s.clone();
    let cloned = t2.elapsed();
    let t3 = Instant::now();
    let b = copy.analyse(options, &Params::NONE).unwrap();
    let analysed = t3.elapsed();
    eprintln!("A-A: clone {cloned:?}, analyse {analysed:?}");
    assert_eq!(b.status, a.status);

    // The file round trip holds the whole section to the same rules.
    let json = serde_json::to_string(&s).unwrap();
    let back: Sketch<Model> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
    eprintln!("A-A: file {} bytes", json.len());
}

/// The file must give back exactly the numbers written. serde_json's
/// default float parsing is fast but not exact; the `float_roundtrip`
/// feature (on in Cargo.toml) makes it so. A solved sketch has values like
/// these, one ulp from a round number, and this is what first showed it.
#[test]
fn file_numbers_round_trip_exactly() {
    for v in [1.5707963267950493f64, 1.5707963267949099f64, 4662.602f64] {
        let s = serde_json::to_string(&v).unwrap();
        let back: f64 = serde_json::from_str(&s).unwrap();
        assert_eq!(v.to_bits(), back.to_bits(), "{v:?} via {s}");
    }
}
