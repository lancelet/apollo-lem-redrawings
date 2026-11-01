//! Every stored file of every version loads, forever
//! (docs/redraw-plan.md §2.1): `tests/fixtures/vN/` holds files as
//! version N wrote them, and are never changed.

use lmsheets_document::state::document::Document;
use lmsheets_document::state::version::{FORMAT, VERSION};
use lmsheets_document::value::frame::{Frame, Space};
use lmsheets_document::value::geometry::{Paper, P2};
use serde_json::Value;
use std::path::PathBuf;

/// Points on an `n` × `n` grid over a frame, its corners among them.
fn grid(f: &Frame, n: usize) -> Vec<[f64; 2]> {
    let [w, h] = f.size_px.map(f64::from);
    let at = |k: usize, l: f64| l * k as f64 / (n - 1) as f64;
    (0..n)
        .flat_map(|i| (0..n).map(move |j| [at(i, w), at(j, h)]))
        .collect()
}

/// Each fixture's path and text, every version's directory.
fn fixtures() -> Vec<(PathBuf, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|dir| dir.is_dir())
        .flat_map(|dir| std::fs::read_dir(dir).unwrap())
        .map(|f| f.unwrap().path())
        .filter(|f| f.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(files.len() >= 3, "{files:?}");
    files
        .into_iter()
        .map(|f| {
            let text = std::fs::read_to_string(&f).unwrap();
            (f, text)
        })
        .collect()
}

#[test]
fn every_fixture_loads_and_round_trips() {
    for (path, text) in fixtures() {
        let d = Document::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d, "{}", path.display());
        let header: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(header["format"], FORMAT);
        assert_eq!(header["version"], VERSION);
    }
}

#[test]
fn the_v1_fixtures_have_no_header() {
    for (path, text) in fixtures() {
        if path.parent().unwrap().ends_with("v1") {
            let v: Value = serde_json::from_str(&text).unwrap();
            assert!(v.get("format").is_none() && v.get("version").is_none());
        }
    }
}

#[test]
fn a_newer_file_is_refused() {
    let (_, text) = &fixtures()[0];
    let mut v: Value = serde_json::from_str(text).unwrap();
    v["format"] = FORMAT.into();
    v["version"] = (VERSION + 1).into();
    let e = Document::parse(&v.to_string()).unwrap_err().to_string();
    let want = format!(
        "the file is version {}, newer than this reader (version {VERSION})",
        VERSION + 1
    );
    assert!(e.contains(&want), "{e}");
    v["format"] = "lmsheets-sketch".into();
    v["version"] = 1.into();
    let e = Document::parse(&v.to_string()).unwrap_err().to_string();
    assert!(e.contains("not an lmsheets-document file"), "{e}");
}

#[test]
fn a_type_error_names_its_path() {
    let (_, text) = &fixtures()[0];
    let mut v: Value = serde_json::from_str(text).unwrap();
    v["title"]["sheet"] = 3.into();
    let e = Document::parse(&v.to_string()).unwrap_err().to_string();
    assert!(e.starts_with("title.sheet: invalid type"), "{e}");
}

/// Version 1's `scan` is version 2's one frame, its chain the old
/// similarity exactly: the same paper point for every pixel, to the bit,
/// and the same pixel back.
#[test]
fn version_1_upgrades_to_its_old_similarity() {
    let mut upgraded = 0;
    for (path, text) in fixtures() {
        if !path.parent().unwrap().ends_with("v1") {
            continue;
        }
        let old: Value = serde_json::from_str(&text).unwrap();
        let d = Document::parse(&text).unwrap();
        let Some(scan) = old.get("scan") else {
            assert_eq!(d.frames().count(), 0, "{}", path.display());
            continue;
        };
        let f = d.frame("scan").expect("the frame");
        assert_eq!(d.frames().count(), 1);
        assert_eq!(f.file, scan["file"].as_str().unwrap());
        assert_eq!(serde_json::to_value(f.size_px).unwrap(), scan["size_px"]);
        let g = &scan["registration"];
        let at = |k: &str| g[k].as_f64().unwrap();
        let o = [
            g["origin_px"][0].as_f64().unwrap(),
            g["origin_px"][1].as_f64().unwrap(),
        ];
        let (s, c) = at("rotation").sin_cos();
        let k = at("px_per_mm");
        for p in grid(f, 9) {
            // Version 1's `to_paper` and `to_px`, as they were.
            let (dx, dy) = ((p[0] - o[0]) / k, (p[1] - o[1]) / k);
            let mm = P2::<Paper>::new(c * dx + s * dy, -s * dx + c * dy);
            let px = [
                o[0] + k * (c * mm.x - s * mm.y),
                o[1] + k * (s * mm.x + c * mm.y),
            ];
            let r = &f.registration;
            assert_eq!(r.to_paper(p, Space::Frame), mm, "{}", path.display());
            assert_eq!(r.from_paper(mm, Space::Frame), px, "{}", path.display());
        }
        upgraded += 1;
    }
    assert!(upgraded >= 2, "{upgraded}");
}

/// 17619's frame as exported, warp and all: every pixel to the paper and
/// back within a millionth of a pixel, over the whole frame.
#[test]
fn the_17619_chain_round_trips_over_its_frame() {
    let (_, text) = fixtures()
        .into_iter()
        .find(|(p, _)| p.ends_with("v2/LDW280-17619-sh3.document.json"))
        .expect("the v2 fixture");
    let d = Document::parse(&text).unwrap();
    let f = d.frame("scan").expect("the frame");
    let r = &f.registration;
    assert!(!r.warp.map.is_none() && r.rotation.map.angle != 0.0);
    let worst = grid(f, 65)
        .into_iter()
        .map(|p| {
            let q = r.from_paper(r.to_paper(p, Space::Frame), Space::Frame);
            (q[0] - p[0]).hypot(q[1] - p[1])
        })
        .fold(0.0, f64::max);
    assert!(worst < 1e-6, "{worst} px");
}

/// A claim keeps the node it names: on 17619's export, removing a node of
/// each kind a claim can name (that would go without the claim) is
/// refused while a claim names it, and the document is as it was.
#[test]
fn a_claimed_node_stays() {
    use lmsheets_document::api::msg::{Op, SketchOp};
    use lmsheets_document::state::document::Error;
    use lmsheets_document::value::claims::{self, Claim, Evidence, Kind, Node, Output, Part};
    let (_, text) = fixtures()
        .into_iter()
        .find(|(p, _)| p.ends_with("v2/LDW280-17619-sh3.document.json"))
        .expect("the v2 fixture");
    let mut d = Document::parse(&text).unwrap();
    let mut frame = d.frame("scan").unwrap().clone();
    let output = |model: &str| Output {
        model: model.into(),
        hash: "fnv1a64:0".into(),
    };
    frame.evidence = Some(Evidence {
        pixelnet: output("r3b@110000"),
        strokenet: Some(output("r3b@48000")),
        ink_px: 10,
        segments: Vec::new(),
    });
    d.apply_in_place(&Op::SetFrame {
        id: "scan".into(),
        frame: Some(Box::new(frame)),
    })
    .unwrap();
    let id = |s: &dyn ToString| s.to_string();
    let mut candidates: Vec<(Kind, String, Op)> = Vec::new();
    for (e, _, _) in d.sheet().entities() {
        candidates.push((Kind::Entity, id(e), SketchOp::remove_entity(*e).on_sheet()));
    }
    for (v, view) in d.views().views() {
        for (e, _, _) in view.sketch.entities() {
            let op = SketchOp::remove_entity(*e).in_view(*v);
            candidates.push((Kind::Entity, format!("{}/{e}", v.as_str()), op));
        }
    }
    for (t, _) in d.texts() {
        candidates.push((Kind::Text, id(t), Op::RemoveText { id: *t }));
    }
    for (l, _) in d.leaders() {
        candidates.push((Kind::Leader, id(l), Op::RemoveLeader { id: *l }));
    }
    for (m, _) in d.dimensions() {
        candidates.push((Kind::Dimension, id(m), Op::RemoveDimension { id: *m }));
        candidates.push((Kind::Witness, id(m), Op::RemoveDimension { id: *m }));
    }
    for (l, _) in d.centre_lines() {
        candidates.push((Kind::CentreLine, id(l), Op::RemoveCentreLine { id: *l }));
    }
    for (u, _) in d.unmodelled_marks() {
        candidates.push((Kind::Unmodelled, id(u), Op::RemoveUnmodelled { id: *u }));
    }
    let mut tried = std::collections::BTreeSet::new();
    for (kind, name, op) in candidates {
        if tried.contains(&kind) || d.apply(&op).is_err() {
            continue;
        }
        let claim = Claim {
            node: Node { kind, name },
            part: Part::Line,
            by: None,
            evidence: "scan".into(),
            segments: Vec::new(),
            runs: claims::runs([1, 2]),
        };
        let claimed = d
            .apply(&Op::SetClaims {
                claims: vec![claim.clone()],
            })
            .unwrap();
        assert_eq!(
            claimed.apply(&op),
            Err(Error::BadClaim {
                claim: 0,
                error: claims::Error::NoNode(claim.node.clone())
            }),
            "{kind:?}"
        );
        tried.insert(kind);
    }
    use Kind::*;
    let all = [
        Entity, Text, Leader, Dimension, Witness, CentreLine, Unmodelled,
    ];
    assert_eq!(tried, all.into_iter().collect(), "kinds tried");
}

/// Version 3's observed features (docs/redraw-plan.md §8.3): 17619's four
/// corner roundings in one shared group (one radius, a centre per corner,
/// the basis stated: symmetry), on the `observed` layer, each a
/// measurement off the scan with its fit, what ignoring it loses and its
/// own fit alone, so the disagreement with the shared radius stays on the
/// record; the design is not touched by them (they are not in the part
/// view's sketch).
#[test]
fn version_3_has_17619s_observed_roundings() {
    use lmsheets_document::value::observed::{Basis, Kind};
    use lmsheets_document::value::provenance::Method;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v3/LDW280-17619-sh3.document.json");
    let d = Document::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(d.observed().len(), 4);
    let mut group: Vec<(f64, [f64; 2])> = Vec::new();
    let mut owns = Vec::new();
    for o in d.observed().values() {
        assert_eq!(o.layer, "observed".into());
        assert!(o.drawn && o.graduated_to.is_none());
        assert!(matches!(o.provenance.method(), Method::Measured { .. }));
        assert_eq!(o.shared.as_deref(), Some("G"));
        assert_eq!(o.basis, Some(Basis::Symmetry));
        let Kind::Rounding {
            junction,
            radius,
            centre,
            fit,
        } = &o.kind
        else {
            panic!("a rounding")
        };
        assert!(fit.loss_mm > 0.0 && fit.ink_radius_mm > fit.pen_mm && fit.sigma_mm > 0.0);
        // Its own fit is kept, and is never worse than the shared one.
        let own = fit.own.as_ref().expect("the own fit is kept");
        assert!(own.rms_mm <= fit.rms_best_mm + 1e-9 && own.rms_mm <= fit.rms_sharp_mm + 1e-9);
        owns.push((own.ink_radius_mm, own.disagrees(fit)));
        let view = d.views().view(o.view).unwrap();
        for line in junction {
            assert!(view.sketch.geometry().get(*line).is_some());
        }
        group.push((*radius, [centre.x, centre.y]));
    }
    assert_eq!(group.len(), 4);
    assert!(group.iter().all(|g| g.0 == group[0].0), "one radius");
    for (k, g) in group.iter().enumerate() {
        for h in &group[k + 1..] {
            assert_ne!(g.1, h.1, "a centre per corner");
        }
    }
    // The corners alone disagree: the bottom ones are the rounder, and
    // every one is flagged against the one radius.
    owns.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(owns[3].0 > 2.0 * owns[1].0, "{owns:?}");
    assert!(owns.iter().all(|o| o.1), "{owns:?}");
    // The ink at the corners is still the lines' (the sketch's word: an
    // entity claims the segments it was fitted from); the roundings are
    // the document's account of what that ink shows, not a claim on it.
    assert!(d
        .claims()
        .iter()
        .all(|c| c.node.kind != lmsheets_document::value::claims::Kind::Observed));
}

/// Version 4's marking region (docs/redraw-plan.md K5): 17619's boxed X is
/// one node owning its rectangle's four sides and two diagonals, which stay
/// plain visible-layer lines of the solved part view; named by the form's
/// printed note line and the box-X printed beside it; and the ink of those
/// six lines is the region's claim, no longer six entities'.
#[test]
fn version_4_has_17619s_marking_region() {
    use lmsheets_document::value::claims::Kind;
    use lmsheets_document::value::marking::Meaning;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v4/LDW280-17619-sh3.document.json");
    let d = Document::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(d.marking_regions().len(), 1);
    let (id, region) = d.marking_regions().iter().next().unwrap();
    assert_eq!(region.meaning, Meaning::PartNumberAndMarkings);
    // Six distinct lines, still geometry of the view, on the drawing's layer.
    let view = d.views().view(region.view).unwrap();
    let lines: Vec<_> = region.lines().collect();
    assert_eq!(lines.len(), 6);
    for line in &lines {
        assert!(view.sketch.geometry().get(*line).is_some());
        assert_eq!(view.sketch.meta(*line).unwrap().layer, "visible".into());
        assert_eq!(
            d.marking_region_of(region.view.as_str(), line.as_str())
                .map(|r| r.0),
            Some(id)
        );
    }
    // The form's line and its box-X name it, and are named back.
    let link = region.named_by.as_ref().expect("linked to the form");
    assert_eq!(link.box_x.len(), 6);
    let template = d.form().unwrap().template().unwrap();
    let note = lmsheets_document::value::form::FormItem::parse(&link.note).unwrap();
    assert_eq!(template.lettered(note).unwrap().text, "IN REGION MARKED");
    for item in std::iter::once(&link.note).chain(&link.box_x) {
        assert_eq!(d.regions_named_by(item).count(), 1, "{item}");
    }
    // The box-X it is named by is the note's inline ⊠ (symbol S1): its ink is
    // claimed by those rules, and no unmodelled mark of the sheet is left.
    for item in &link.box_x {
        assert!(
            d.claims()
                .iter()
                .any(|c| c.node.kind == Kind::Form && &c.node.name == item),
            "{item} claims no ink"
        );
    }
    assert_eq!(d.unmodelled_marks().count(), 0);
    // Its claims replace the six entities'.
    let claimed: Vec<_> = d
        .claims()
        .iter()
        .filter(|c| c.node.kind == Kind::MarkingRegion)
        .collect();
    assert!(!claimed.is_empty() && claimed.iter().all(|c| c.node.name == id.as_str()));
    for line in &lines {
        let node = format!("{}/{}", region.view.as_str(), line.as_str());
        assert!(
            d.claims()
                .iter()
                .all(|c| !(c.node.kind == Kind::Entity && c.node.name == node)),
            "{node} is claimed by the region, not as an entity"
        );
    }
    // The gap it closes names it.
    let gap = &d.gaps()[&"marking-region".into()];
    assert!(gap
        .nodes
        .iter()
        .any(|n| n.kind == Kind::MarkingRegion && n.name == id.as_str()));
}
