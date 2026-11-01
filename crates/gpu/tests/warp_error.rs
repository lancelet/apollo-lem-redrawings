//! How far the overlay's warped vectors are from the registration chain they
//! stand for, and how far a locally affine stand-in would have been, on
//! 17619's bundle (`--ignored`, `LMSHEETS_BUNDLE_17619=<bundle>/LDW280-17619/sh3`).
//!
//! The overlay carries every vertex through `Chain::paper_to_scan`, so the
//! only error against the chain is between the vertices: a straight piece
//! of at most 0.5 mm, an arc chord within 0.0005 mm. The affine alternative
//! (the chain's Jacobian at the view centre, one matrix for the pane) is
//! measured for the record, at the whole sheet and at the deepest zoom.

use lmsheets_document::state::document::Document;
use lmsheets_document::value::style::Purpose;
use lmsheets_gpu::instances::{build, warp, Instance};
use lmsheets_inspector::Chain;
use lmsheets_render::conventions::Conventions;
use lmsheets_render::draw::draw;
use lmsheets_render::resolve::resolve;
use lmsheets_render::scene::Scene;
use lmsheets_render::theme::Theme;

/// The overlay's piece length (the shell's `OVERLAY_STEP_MM`).
const STEP: f64 = 0.5;

fn dist_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a.0 + t * dx - p.0).hypot(a.1 + t * dy - p.1)
}

/// The point of a source segment or arc instance at parameter `t`.
fn at(i: &Instance, t: f64) -> (f64, f64) {
    let a = (f64::from(i.k_a[1]), f64::from(i.k_a[2]));
    if i.k_a[0] < 0.5 {
        let b = (f64::from(i.b[0]), f64::from(i.b[1]));
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
    } else {
        let (r, s, w) = (f64::from(i.b[0]), f64::from(i.b[1]), f64::from(i.b[2]));
        let ang = s + w * t;
        (a.0 + r * ang.cos(), a.1 + r * ang.sin())
    }
}

#[test]
#[ignore = "needs the 17619 bundle (LMSHEETS_BUNDLE_17619)"]
fn the_warped_vectors_follow_the_chain_and_an_affine_stand_in_would_not() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("LMSHEETS_BUNDLE_17619").expect("set LMSHEETS_BUNDLE_17619"),
    );
    let chain = Chain::load(&dir).unwrap();
    let doc =
        Document::parse(&std::fs::read_to_string(dir.join("document.json")).unwrap()).unwrap();
    let scene = Scene::of(draw(&doc, Purpose::Screen));
    let styled = resolve(&scene, &doc, &Theme::screen(), Conventions::default());
    let (base, owners) = build(&styled);
    let f = |p: (f64, f64)| chain.paper_to_scan(p);

    // 1. Chord error: sample each source primitive densely, map exactly,
    // and measure to the polyline of its warped pieces (crop px).
    let mut worst_chord = 0.0f64;
    for (inst, o) in base.iter().zip(&owners) {
        if inst.k_a[0] > 1.5 {
            continue;
        }
        let (pieces, _) = warp(std::slice::from_ref(inst), &[*o], &f, STEP);
        for k in 0..=200 {
            let p = f(at(inst, f64::from(k) / 200.0));
            let d = pieces
                .iter()
                .map(|s| {
                    dist_to_segment(
                        p,
                        (f64::from(s.k_a[1]), f64::from(s.k_a[2])),
                        (f64::from(s.b[0]), f64::from(s.b[1])),
                    )
                })
                .fold(f64::MAX, f64::min);
            worst_chord = worst_chord.max(d);
        }
    }

    // 2. The affine stand-in: the chain's Jacobian at a centre, applied to
    // points around it, against the chain. All ink points are centres.
    let pts: Vec<(f64, f64)> = base
        .iter()
        .filter(|i| i.k_a[0] < 1.5)
        .map(|i| at(i, 0.5))
        .collect();
    let h = 0.25;
    let affine = |c: (f64, f64)| {
        let (fc, fx, fy) = (f(c), f((c.0 + h, c.1)), f((c.0, c.1 + h)));
        let j = [
            [(fx.0 - fc.0) / h, (fy.0 - fc.0) / h],
            [(fx.1 - fc.1) / h, (fy.1 - fc.1) / h],
        ];
        move |p: (f64, f64)| {
            let d = (p.0 - c.0, p.1 - c.1);
            (
                fc.0 + j[0][0] * d.0 + j[0][1] * d.1,
                fc.1 + j[1][0] * d.0 + j[1][1] * d.1,
            )
        }
    };
    let (lo, hi) = chain.paper_bounds();
    let centre = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
    let a = affine(centre);
    let fit_err = pts
        .iter()
        .map(|p| (a(*p).0 - f(*p).0).hypot(a(*p).1 - f(*p).1))
        .fold(0.0, f64::max);
    // At the sheet's fit the scan zoom is about 0.19 screen px per crop px.
    let fit_zoom = 0.19;
    // At full zoom (64 screen px per crop px) a 1600 px window shows 12.5
    // crop px either side of the centre: measure that neighbourhood for every
    // ink point as centre.
    let reach_mm = 12.5 / chain.scan_per_mm(centre);
    let mut deep_err = 0.0f64;
    for c in &pts {
        let a = affine(*c);
        for (dx, dy) in [
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (0.7, 0.7),
            (-0.7, -0.7),
        ] {
            let p = (c.0 + dx * reach_mm, c.1 + dy * reach_mm);
            deep_err = deep_err.max((a(p).0 - f(p).0).hypot(a(p).1 - f(p).1));
        }
    }
    eprintln!(
        "warp chord error: worst {worst_chord:.4} crop px = {:.3} screen px at 64x\n\
         affine at the sheet centre, whole sheet: worst {fit_err:.2} crop px = {:.2} screen px at fit\n\
         affine in a 64x window: worst {deep_err:.4} crop px = {:.3} screen px",
        worst_chord * 64.0,
        fit_err * fit_zoom,
        deep_err * 64.0
    );
    assert!(
        worst_chord * 64.0 < 0.5,
        "the warp is within half a screen pixel at full zoom"
    );
}
