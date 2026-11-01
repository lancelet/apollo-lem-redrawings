//! Properties of the api over documents reached by ops
//! (`testing::document`): an op is all or nothing and undoes to the
//! identical document, the journal replays and survives its wire form,
//! statuses only move forward, and the file form is an identity that a
//! damaged file cannot panic.

use crate::api::model::{Model, Msg};
use crate::api::msg::{Op, SketchOp};
use crate::examples;
use crate::solver::solve::Options;
use crate::state::document::Document;
use crate::state::views::{aligned, in_view, SystemAnalysis};
use crate::testing::document::{
    document, example, msg, op, ops, point_ref, start, with_views, VIEWS,
};
use crate::value::measure::Measure;
use crate::value::provenance::{Provenance, Status};
use crate::value::relation::distance;
use crate::value::sketch::Sketch;
use crate::value::view::ModelAxis;
use proptest::prelude::*;
use proptest::sample::{select, Index};
use serde_json::Value;

/// How far a status has come: each allowed change moves it up.
fn rank(s: &Status) -> (u8, usize) {
    match s {
        Status::Provisional => (0, 0),
        Status::Accepted => (1, 0),
        Status::Verified { checks } => (2, checks.len()),
        Status::Disputed { .. } => (3, 0),
        Status::Superseded { .. } => (4, 0),
    }
}

/// Every claim of a sketch, by name: its entities' and constraints'.
fn claims<F: crate::value::geometry::Frame>(s: &Sketch<F>) -> Vec<(String, &Provenance)> {
    let entities = s
        .entities()
        .map(|(id, _, m)| (format!("e:{}", id.as_str()), &m.provenance));
    let constraints = s
        .constraints()
        .map(|(id, c)| (format!("c:{}", id.as_str()), c.provenance()));
    entities.chain(constraints).collect()
}

/// Every claim of the document, by sketch and name.
fn all_claims(d: &Document) -> Vec<(String, &Provenance)> {
    let mut out: Vec<_> = claims(d.sheet())
        .into_iter()
        .map(|(n, p)| (format!("sheet/{n}"), p))
        .collect();
    for (v, view) in d.views().views() {
        let named = claims(&view.sketch).into_iter();
        out.extend(named.map(|(n, p)| (format!("{}/{n}", v.as_str()), p)));
    }
    out
}

/// A result as text, so that errors carrying a NaN compare.
fn said<T: std::fmt::Debug, E: std::fmt::Debug>(r: &Result<T, E>) -> String {
    format!("{r:?}")
}

/// A change to one node of a file's JSON.
#[derive(Clone, Debug)]
enum Damage {
    Number(f64),
    Text(&'static str),
    Flip,
    Remove,
}

fn damage() -> impl Strategy<Value = (Index, Damage)> {
    let what = prop_oneof![
        select(vec![0.0, -1.0, 1e300, -0.0, 0.5, 7.0]).prop_map(Damage::Number),
        select(vec!["", " a", "nope", "visible", "x"]).prop_map(Damage::Text),
        Just(Damage::Flip),
        Just(Damage::Remove),
    ];
    (any::<Index>(), what)
}

/// The JSON pointers of every node below `v`.
fn nodes(v: &Value, at: String, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, c) in m {
                let p = format!("{at}/{}", k.replace('~', "~0").replace('/', "~1"));
                nodes(c, p.clone(), out);
                out.push(p);
            }
        }
        Value::Array(a) => {
            for (i, c) in a.iter().enumerate() {
                let p = format!("{at}/{i}");
                nodes(c, p.clone(), out);
                out.push(p);
            }
        }
        _ => {}
    }
}

/// `v` with the node at `index` (of [`nodes`]) damaged.
fn apply_damage(mut v: Value, index: Index, d: &Damage) -> Value {
    let mut all = Vec::new();
    nodes(&v, String::new(), &mut all);
    if all.is_empty() {
        return v;
    }
    let at = &all[index.index(all.len())];
    if let Damage::Remove = d {
        let (parent, key) = at.rsplit_once('/').expect("a child");
        match v.pointer_mut(parent).expect("exists") {
            Value::Object(m) => {
                m.remove(&key.replace("~1", "/").replace("~0", "~"));
            }
            Value::Array(a) => {
                a.remove(key.parse::<usize>().expect("an index"));
            }
            _ => unreachable!("a parent"),
        }
        return v;
    }
    let node = v.pointer_mut(at).expect("exists");
    *node = match d {
        Damage::Number(x) => serde_json::json!(x),
        Damage::Text(s) => Value::String(s.to_string()),
        Damage::Flip => match node {
            Value::Bool(b) => Value::Bool(!*b),
            _ => Value::Null,
        },
        Damage::Remove => unreachable!("handled above"),
    };
    v
}

proptest! {
    // 256 cases each, about 1 s each in a debug build.
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// An op either gives the changed document or an error and leaves
    /// the document as it was, by value or in place; taken through the
    /// model it undoes to the identical document and redoes to the
    /// changed one.
    #[test]
    fn an_op_is_all_or_nothing_and_undoes(doc in document(8), op in op()) {
        let applied = doc.apply(&op);
        let mut in_place = doc.clone();
        let result = in_place.apply_in_place(&op);
        let mut model = Model::new(doc.clone());
        let through = model.update(Msg::Op { op });
        match applied {
            Err(e) => {
                // An error may carry the NaN that caused it: as text.
                prop_assert_eq!(said(&result), said(&Err::<(), _>(e.clone())));
                prop_assert_eq!(&in_place, &doc);
                prop_assert_eq!(said(&through), said(&Err::<(), _>(e)));
                prop_assert_eq!(model, Model::new(doc));
            }
            Ok(next) => {
                prop_assert_eq!(result, Ok(()));
                prop_assert_eq!(&in_place, &next);
                prop_assert_eq!(through, Ok(()));
                prop_assert_eq!(model.doc(), &next);
                model.update(Msg::Undo).unwrap();
                prop_assert_eq!(model.doc(), &doc);
                model.update(Msg::Redo).unwrap();
                prop_assert_eq!(model.doc(), &next);
            }
        }
    }

    /// A claim's status only moves forward through an op (accepted,
    /// verified with more checks, disputed, superseded); going back is
    /// undo's, by value.
    #[test]
    fn statuses_move_forward_only(doc in document(8), op in op()) {
        let Ok(next) = doc.apply(&op) else {
            return Ok(());
        };
        let after = all_claims(&next);
        for (name, before) in all_claims(&doc) {
            if let Some((_, now)) = after.iter().find(|(n, _)| *n == name) {
                let (b, a) = (rank(before.status()), rank(now.status()));
                prop_assert!(a >= b, "{} went from {:?} to {:?}", name, before, now);
            }
        }
    }

    /// Ops in a batch are applied in turn as one step: the result of
    /// them all, or the first error and nothing changed.
    #[test]
    fn a_batch_is_all_or_nothing(doc in document(6), ops in ops(4)) {
        let folded = ops.iter().try_fold(doc.clone(), |d, op| d.apply(op));
        let mut model = Model::new(doc.clone());
        let result = model.update(Msg::Batch { ops });
        match folded {
            Ok(d) => {
                prop_assert_eq!(result, Ok(()));
                prop_assert_eq!(model.doc(), &d);
            }
            Err(e) => {
                prop_assert_eq!(said(&result), said(&Err::<(), _>(e)));
                prop_assert_eq!(model, Model::new(doc));
            }
        }
    }

    /// Any messages: a refused one changes nothing (not even the
    /// journal); the journal replays from the start to the same model,
    /// and does so again after its round trip through JSON.
    #[test]
    fn the_journal_replays(start in start(), msgs in proptest::collection::vec(msg(), 0..10)) {
        let mut model = Model::new(start.clone());
        for m in msgs {
            let before = model.clone();
            if model.update(m).is_err() {
                prop_assert_eq!(&model, &before);
            }
        }
        let again = Model::replay(start.clone(), model.journal());
        prop_assert_eq!(again.as_ref(), Ok(&model));
        let wire = serde_json::to_string(model.journal()).unwrap();
        let read: Vec<Msg> = serde_json::from_str(&wire).unwrap();
        prop_assert_eq!(read.as_slice(), model.journal());
        prop_assert_eq!(Model::replay(start, &read), Ok(model));
    }

    /// The file form is an identity on every document there can be, and
    /// writing what was read gives the same text.
    #[test]
    fn documents_round_trip_through_json(doc in prop_oneof![document(10), example()]) {
        let text = doc.to_json().unwrap();
        let read = Document::parse(&text);
        prop_assert!(read.is_ok(), "{:?}\n{}", read, text);
        let read = read.unwrap();
        prop_assert_eq!(&read, &doc);
        prop_assert_eq!(read.to_json().unwrap(), text);
    }

    /// A link says the same whichever way round its points are given:
    /// the document takes both or neither, and analyses the system the
    /// same.
    #[test]
    fn links_are_symmetric(
        doc in with_views(6),
        a in (select(VIEWS), point_ref()),
        b in (select(VIEWS), point_ref()),
        axis in select(vec![ModelAxis::X, ModelAxis::Y, ModelAxis::Z]),
    ) {
        let (a, b) = (in_view(a.0, a.1), in_view(b.0, b.1));
        let link = |l| doc.apply(&Op::add_link("l", l, examples::ours()));
        let (ab, ba) = (link(aligned(a, b, axis)), link(aligned(b, a, axis)));
        prop_assert_eq!(ab.is_ok(), ba.is_ok(), "{:?} {:?}", ab, ba);
        if let (Ok(ab), Ok(ba)) = (ab, ba) {
            let o = Options::default();
            let (s, t) = (ab.analyse(o).unwrap().1, ba.analyse(o).unwrap().1);
            let counts = |s: &SystemAnalysis| (s.rank, s.free, s.redundant, s.internal_free);
            prop_assert_eq!(counts(&s), counts(&t));
            prop_assert_eq!(s.views, t.views);
        }
    }
}

proptest! {
    // 64 cases each, about 3 s each in a debug build: a solve and an
    // analysis of a document, or the examples built (once) and their
    // files read and written.
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// What a solve reports is the state it leaves: its checks of the
    /// reference (`REF`) dimensions and what it says of each entity are
    /// what an analysis of the solved document says, in the sheet and
    /// in the views.
    #[test]
    fn a_solve_reports_the_state_it_leaves(
        doc in with_views(12),
        (view, a, b) in (select(VIEWS), point_ref(), point_ref()),
    ) {
        // A reference dimension in a view, to be checked at the solution.
        let check = distance(a, b, Measure::mm(5.0).unwrap()).driven().unwrap();
        let add = SketchOp::add_constraint("ref", check, examples::ours());
        let doc = doc.apply(&add.in_view(view)).unwrap_or(doc);
        let o = Options::default();
        let mut solved = doc.clone();
        let Ok((sheet, system)) = solved.solve(o) else {
            return Ok(());
        };
        let (sheet_now, system_now) = solved.analyse(o).unwrap();
        let near = |a: &[f64], b: &[f64]| {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-9)
        };
        let (r, r_now) = (&sheet.reference, &sheet_now.reference);
        prop_assert_eq!(r.len(), r_now.len());
        for (a, b) in r.iter().zip(r_now) {
            prop_assert!(a.constraint == b.constraint && near(&a.off_by, &b.off_by), "{:?} {:?}", a, b);
        }
        prop_assert_eq!(&sheet.status, &sheet_now.status);
        let (r, r_now) = (&system.reference, &system_now.reference);
        prop_assert_eq!(r.len(), r_now.len());
        for (a, b) in r.iter().zip(r_now) {
            let same = a.view == b.view && a.constraint == b.constraint;
            prop_assert!(same && near(&a.off_by, &b.off_by), "{:?} {:?}", a, b);
        }
        prop_assert_eq!(&system.views, &system_now.views);
    }

    /// A file damaged in one place (a number, a string, a flag changed,
    /// a field or an element removed) never panics the reader; whatever
    /// it accepts is a document like any other, which round-trips.
    #[test]
    fn a_damaged_file_never_panics(
        doc in prop_oneof![document(6), example()],
        (at, d) in damage(),
    ) {
        let v = serde_json::to_value(&doc).unwrap();
        let damaged = apply_damage(v, at, &d).to_string();
        if let Ok(read) = Document::parse(&damaged) {
            let text = read.to_json().unwrap();
            prop_assert_eq!(Document::parse(&text).ok(), Some(read));
        }
    }
}
