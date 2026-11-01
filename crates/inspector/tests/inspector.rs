//! Chunk 19's tests: the chain round trip, the locking of the panes, and
//! the replay of recorded message logs. Fixtures only: a small synthetic
//! chain built here, and 17619's bundle where it exists (`--ignored`).

use lmsheets_document::value::frame::{Crop, Registration, Rotation, Similarity, Step, Warp};
use lmsheets_document::value::provenance::{Agent, Citation, Date, Method, Provenance};
use lmsheets_inspector::model::FIT_FILL;
use lmsheets_inspector::{update, Chain, Grid, Layer, Mode, Model, Msg, NodeId, Pane, Theme};
use std::path::PathBuf;

const OFFSET: [f64; 2] = [100.0, 50.0];
const EXTENT: [f64; 2] = [1000.0, 800.0];

fn prov() -> Provenance {
    Provenance::new(
        Method::Measured { sigma_mm: 0.1 },
        Agent::Script {
            name: "test".into(),
        },
        Date::new(2026, 10, 31).unwrap(),
    )
    .cite(Citation::new("scan", "px (1, 2)"))
}

/// A crop of a frame, a cubic warp of a px or so, a small turn, 10 px per mm.
fn registration() -> Registration {
    let warp = Warp {
        centre: [500.0, 400.0],
        half: 640.0,
        shear: 0.001,
        dx: vec![0.9, -0.6, 0.3, 1.2, -0.9, 0.6, -0.3],
        dy: vec![-0.6, 0.9, 0.45, -1.2, 0.6, 0.3, 0.9],
        extent: [0.0, 0.0, 1000.0, 800.0],
    };
    Registration {
        crop: Step::new(
            Crop {
                box_px: [100, 50, 1100, 850],
            },
            prov(),
        ),
        warp: Step::new(warp, prov()),
        rotation: Step::new(
            Rotation {
                centre: [500.0, 400.0],
                angle: 0.002,
            },
            prov(),
        ),
        paper: Step::new(Similarity::new([40.0, 30.0], 10.0, 0.0).unwrap(), prov()),
    }
}

/// Paper mm to crop px, as the bundle writer samples it, on a quarter-mm grid (the real bundle's is 2 mm over a smoother warp).
fn grid(reg: &Registration) -> Grid {
    use lmsheets_document::value::frame::Space;
    use lmsheets_document::value::geometry::P2;
    Grid::from_fn([-10.0, -10.0], 0.25, [520, 440], |p| {
        let f = reg.from_paper(P2::new(p[0], p[1]), Space::Frame);
        [f[0] - OFFSET[0], f[1] - OFFSET[1]]
    })
}

fn chain() -> Chain {
    let reg = registration();
    let g = grid(&reg);
    Chain::new(reg, OFFSET, EXTENT, g)
}

fn bits(m: &Model) -> [u64; 3] {
    [
        m.centre().0.to_bits(),
        m.centre().1.to_bits(),
        m.scale().to_bits(),
    ]
}

/// The worst paper to scan to paper error over a grid of the sheet, as crop px.
fn round_trip_error(c: &Chain, n: usize) -> f64 {
    let (lo, hi) = c.paper_bounds();
    let mut worst = 0.0f64;
    for j in 0..=n {
        for i in 0..=n {
            let p = (
                lo.0 + (hi.0 - lo.0) * i as f64 / n as f64,
                lo.1 + (hi.1 - lo.1) * j as f64 / n as f64,
            );
            let back = c.scan_to_paper(c.paper_to_scan(p));
            let err = (back.0 - p.0).hypot(back.1 - p.1) * c.scan_per_mm(p);
            worst = worst.max(err);
        }
    }
    worst
}

#[test]
fn the_synthetic_chain_round_trips_within_a_thousandth_of_a_pixel() {
    let c = chain();
    assert!(round_trip_error(&c, 40) < 1e-3);
    // The warp is real: crop px per mm is not 10 everywhere.
    let (a, b) = (c.scan_per_mm((10.0, 10.0)), c.scan_per_mm((90.0, 70.0)));
    assert!((a - b).abs() > 1e-3 && (a - 10.0).abs() < 0.5, "{a} {b}");
}

#[test]
fn the_loader_reads_a_bundles_files() {
    let reg = registration();
    let g = grid(&reg);
    let bytes: Vec<u8> = g
        .cells
        .iter()
        .flat_map(|c| c.iter().flat_map(|v| v.to_le_bytes()))
        .collect();
    let manifest = serde_json::json!({
        "crop": {"offset": OFFSET, "size": EXTENT},
        "registration": {"grid": {
            "origin_mm": g.origin, "step_mm": g.step, "nx": g.n[0], "ny": g.n[1]}},
    });
    let document = serde_json::json!({"frames": {"scan": {"registration": reg}}});
    let loaded = Chain::from_bundle(&manifest.to_string(), &document.to_string(), &bytes).unwrap();
    assert_eq!(loaded, chain());
    assert!(Chain::from_bundle(&manifest.to_string(), &document.to_string(), &bytes[8..]).is_err());
    assert!(Chain::from_bundle("{}", &document.to_string(), &bytes).is_err());
}

/// The scan camera's centre, from its origin and size.
fn scan_centre_of(m: &Model) -> (f64, f64) {
    let cam = m.scan_camera().unwrap();
    (
        cam.origin.0 + cam.size.0 as f64 / cam.zoom / 2.0,
        cam.origin.1 + cam.size.1 as f64 / cam.zoom / 2.0,
    )
}

/// A deterministic mixed log of pans and zooms in both panes.
fn log() -> Vec<Msg> {
    let mut v = vec![Msg::Resize {
        width: 1200,
        height: 700,
    }];
    let mut s = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    for k in 0..60 {
        let pane = if k % 3 == 0 { Pane::Scan } else { Pane::Redraw };
        if k % 2 == 0 {
            v.push(Msg::Pan {
                pane,
                by: (next() * 200.0 - 100.0, next() * 200.0 - 100.0),
            });
        } else {
            v.push(Msg::Zoom {
                pane,
                factor: if k % 4 == 1 {
                    1.0 + next()
                } else {
                    1.0 / (1.0 + next())
                },
                about: (next() * 600.0, next() * 700.0),
            });
        }
    }
    v
}

#[test]
fn the_scan_pane_stays_centred_on_the_image_of_the_paper_centre() {
    let mut m = Model::new(chain(), (1000, 600));
    for msg in log() {
        update(&mut m, msg.clone());
        // Exactly the grid's image of the paper centre ...
        let c = scan_centre_of(&m);
        let want = m.chain().paper_to_scan(m.centre());
        assert!(
            (c.0 - want.0).abs() < 1e-9 && (c.1 - want.1).abs() < 1e-9,
            "{msg:?}"
        );
        // ... and the closed form maps it back to the paper centre.
        let back = m.chain().scan_to_paper(c);
        let err = (back.0 - m.centre().0).hypot(back.1 - m.centre().1)
            * m.chain().scan_per_mm(m.centre());
        // (Beyond the sheet the grid extrapolates linearly and the warp's
        // cubic does not; the round trip is only promised over the sheet.)
        let (lo, hi) = m.chain().paper_bounds();
        let (p, on) = (m.centre(), |a: f64, l: f64, h: f64| a >= l && a <= h);
        if on(p.0, lo.0, hi.0) && on(p.1, lo.1, hi.1) {
            assert!(err < 1e-3, "{msg:?}: {err}");
        }
        // The zoom is the local scale times the paper scale.
        let z = m.scan_camera().unwrap().zoom;
        assert!((z - m.scale() / m.chain().scan_per_mm(m.centre())).abs() < 1e-12);
    }
}

#[test]
fn the_point_under_the_cursor_stays_under_it_in_the_pane_zoomed() {
    let mut m = Model::new(chain(), (1000, 600));
    for (k, &about) in [(10.0, 20.0), (250.0, 300.0), (480.0, 590.0), (100.0, 500.0)]
        .iter()
        .enumerate()
    {
        for factor in [2.0, 1.3, 0.5, 3.0] {
            let pane = if k % 2 == 0 { Pane::Redraw } else { Pane::Scan };
            update(&mut m, Msg::Fit);
            update(
                &mut m,
                Msg::Pan {
                    pane,
                    by: (13.0, -7.0),
                },
            );
            let (paper, crop) = (m.paper_at(about), m.crop_at(about));
            let (scale, zoom) = (m.scale(), m.scan_camera().unwrap().zoom);
            update(
                &mut m,
                Msg::Zoom {
                    pane,
                    factor,
                    about,
                },
            );
            match pane {
                Pane::Redraw => {
                    let (a, b) = (paper.unwrap(), m.paper_at(about).unwrap());
                    assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
                    assert!((m.scale() - scale * factor).abs() < 1e-9 * scale);
                }
                Pane::Scan => {
                    // The scan zoom is exact; the point to the grid's error.
                    let z = m.scan_camera().unwrap().zoom;
                    assert!(
                        (z - zoom * factor).abs() < 1e-9 * zoom,
                        "{z} {zoom} {factor}"
                    );
                    let (a, b) = (crop.unwrap(), m.crop_at(about).unwrap());
                    assert!(
                        (a.0 - b.0).abs() < 2e-3 && (a.1 - b.1).abs() < 2e-3,
                        "{a:?} {b:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_pan_moves_the_content_with_the_pointer_in_both_panes() {
    let mut m = Model::new(chain(), (1000, 600));
    let by = (40.0, -25.0);
    let p0 = m.paper_at((100.0, 100.0)).unwrap();
    update(
        &mut m,
        Msg::Pan {
            pane: Pane::Redraw,
            by,
        },
    );
    let p1 = m.paper_at((100.0 + by.0, 100.0 + by.1)).unwrap();
    assert!((p0.0 - p1.0).abs() < 1e-9 && (p0.1 - p1.1).abs() < 1e-9);
    let c0 = m.crop_at((100.0, 100.0)).unwrap();
    update(
        &mut m,
        Msg::Pan {
            pane: Pane::Scan,
            by,
        },
    );
    let c1 = m.crop_at((100.0 + by.0, 100.0 + by.1)).unwrap();
    assert!((c0.0 - c1.0).abs() < 2e-3 && (c0.1 - c1.1).abs() < 2e-3);
}

#[test]
fn a_recorded_log_replays_bitwise_identically() {
    let log = log();
    let run = |log: &[Msg]| {
        let mut m = Model::new(chain(), (1000, 600));
        for msg in log {
            update(&mut m, msg.clone());
        }
        m
    };
    let (a, b) = (run(&log), run(&log));
    assert_eq!(a, b);
    assert_eq!(bits(&a), bits(&b));
    // Through the JSON a harness would store it in.
    let json = serde_json::to_string(&log).unwrap();
    let back: Vec<Msg> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, log);
    assert_eq!(bits(&run(&back)), bits(&a));
    assert_ne!(bits(&a), bits(&Model::new(chain(), (1000, 600))));
}

#[test]
fn modes_size_the_panes_and_hidden_panes_ignore_messages() {
    let mut m = Model::new(chain(), (1001, 600));
    assert_eq!(m.pane_size(Pane::Redraw), Some((500, 600)));
    assert_eq!(m.pane_size(Pane::Scan), Some((501, 600)));
    assert_eq!(m.pane_origin(Pane::Scan), Some((500, 0)));
    update(&mut m, Msg::SetMode(Mode::RedrawOnly));
    assert_eq!(m.pane_size(Pane::Scan), None);
    assert!(m.scan_camera().is_none());
    let before = m.clone();
    update(
        &mut m,
        Msg::Pan {
            pane: Pane::Scan,
            by: (5.0, 5.0),
        },
    );
    update(
        &mut m,
        Msg::Zoom {
            pane: Pane::Scan,
            factor: 2.0,
            about: (1.0, 1.0),
        },
    );
    update(
        &mut m,
        Msg::Zoom {
            pane: Pane::Redraw,
            factor: f64::NAN,
            about: (1.0, 1.0),
        },
    );
    assert_eq!(m, before);
    // Fit in scan-only mode fits the crop in the window.
    update(&mut m, Msg::SetMode(Mode::ScanOnly));
    update(&mut m, Msg::Fit);
    let cam = m.scan_camera().unwrap();
    assert!((cam.zoom - FIT_FILL * (1001.0f64 / 1000.0).min(600.0 / 800.0)).abs() < 1e-9);
    let c = scan_centre_of(&m);
    assert!((c.0 - 500.0).abs() < 2e-3 && (c.1 - 400.0).abs() < 2e-3);
}

fn bundle_dir() -> PathBuf {
    std::env::var_os("LMSHEETS_BUNDLE_17619")
        .map(PathBuf::from)
        .expect("set LMSHEETS_BUNDLE_17619 to <bundle>/LDW280-17619/sh3")
}

/// Needs the scratch bundle: `blobsketch bundle ... --out DIR`, then
/// `LMSHEETS_BUNDLE_17619=DIR/LDW280-17619/sh3 cargo test -p lmsheets-inspector -- --ignored`.
#[test]
#[ignore = "needs the 17619 bundle (LMSHEETS_BUNDLE_17619)"]
fn the_17619_bundle_round_trips_and_locks() {
    let c = Chain::load(&bundle_dir()).unwrap();
    // The grid's own recorded interpolation error is 0.00076 px; with the
    // f32 cells the round trip stays under a thousandth.
    let e = round_trip_error(&c, 120);
    assert!(e < 1e-3, "round trip {e} px");
    let mut m = Model::new(c, (1600, 1000));
    for msg in log() {
        update(&mut m, msg);
        let c = scan_centre_of(&m);
        let want = m.chain().paper_to_scan(m.centre());
        assert!((c.0 - want.0).abs() < 1e-6 && (c.1 - want.1).abs() < 1e-6);
    }
}

/// Hover and leave are plain messages: the picked node rides in, the pane
/// must be shown, and a log with them replays to the identical model.
#[test]
fn hover_and_leave_replay_and_respect_the_panes() {
    let log = vec![
        Msg::Resize {
            width: 1600,
            height: 1000,
        },
        Msg::Hover {
            pane: Pane::Redraw,
            at: (100.0, 200.0),
            node: Some(NodeId(4)),
            highlight: None,
        },
        Msg::Pan {
            pane: Pane::Scan,
            by: (30.0, -10.0),
        },
        Msg::Hover {
            pane: Pane::Scan,
            at: (50.0, 60.0),
            node: Some(NodeId(9)),
            highlight: None,
        },
        Msg::Hover {
            pane: Pane::Scan,
            at: (51.0, 60.0),
            node: None,
            highlight: None,
        },
        Msg::Leave,
        Msg::Hover {
            pane: Pane::Redraw,
            at: (5.0, 5.0),
            node: Some(NodeId(2)),
            highlight: None,
        },
    ];
    let json = serde_json::to_string(&log).unwrap();
    let back: Vec<Msg> = serde_json::from_str(&json).unwrap();
    let run = |log: &[Msg]| {
        let mut m = Model::new(chain(), (1000, 800));
        let mut seen = Vec::new();
        for msg in log {
            update(&mut m, msg.clone());
            seen.push((m.hovered(), m.pointer()));
        }
        (m, seen)
    };
    let (a, sa) = run(&log);
    let (b, sb) = run(&back);
    assert_eq!(a, b);
    assert_eq!(sa, sb);
    assert_eq!(sa[1].0, Some(NodeId(4)));
    assert_eq!(sa[3].0, Some(NodeId(9)));
    assert_eq!(sa[3].1, Some((Pane::Scan, (50.0, 60.0))));
    assert_eq!(sa[4].0, None);
    assert_eq!(sa[5], (None, None));
    // A non-finite position changes nothing.
    let mut n = a.clone();
    update(
        &mut n,
        Msg::Hover {
            pane: Pane::Redraw,
            at: (f64::NAN, 0.0),
            node: Some(NodeId(1)),
            highlight: None,
        },
    );
    assert_eq!(n, a);
    assert_eq!(a.hovered(), Some(NodeId(2)));
    // A hidden pane takes no hover; changing the mode drops the hover.
    let mut m = Model::new(chain(), (1000, 800));
    update(&mut m, Msg::SetMode(Mode::RedrawOnly));
    update(
        &mut m,
        Msg::Hover {
            pane: Pane::Scan,
            at: (1.0, 1.0),
            node: Some(NodeId(3)),
            highlight: None,
        },
    );
    assert_eq!(m.hovered(), None);
    update(
        &mut m,
        Msg::Hover {
            pane: Pane::Redraw,
            at: (1.0, 1.0),
            node: Some(NodeId(3)),
            highlight: None,
        },
    );
    update(&mut m, Msg::SetMode(Mode::ScanOnly));
    assert_eq!((m.hovered(), m.pointer()), (None, None));
}

/// With no owner tiles the scan pane falls back to the geometry through the
/// chain, so both panes name the same node for the same paper point; once
/// the pointer's owner tile is held (and zoomed in) the map decides.
#[test]
fn the_two_panes_pick_the_same_node_and_the_owner_map_decides_when_held() {
    use lmsheets_document::value::geometry::P2;
    use lmsheets_document::value::style::Dash;
    use lmsheets_inspector::pick::{HitItem, OWNER_ZOOM};
    use lmsheets_inspector::{OwnerTable, OwnerTile, Picker};
    use lmsheets_render::scene::{Path, Primitive};
    let item = HitItem {
        primitive: Primitive::Stroke {
            path: Path::Line {
                a: P2::new(60.0, 50.0),
                b: P2::new(80.0, 50.0),
            },
            weight: 0.3,
            dash: Dash::hidden(),
            anchors: vec![],
        },
        node: Some(NodeId(1)),
    };
    let mut picker = Picker::from_items(OwnerTable::default(), 1024, vec![item]);
    let mut m = Model::new(chain(), (1600, 800));
    let paper = (70.0, 50.0);
    // Put the line's midpoint under the redraw pane's centre.
    let by = (
        (m.centre().0 - paper.0) * m.scale(),
        (m.centre().1 - paper.1) * m.scale(),
    );
    update(
        &mut m,
        Msg::Pan {
            pane: Pane::Redraw,
            by,
        },
    );
    let size = m.pane_size(Pane::Redraw).unwrap();
    let mid = (size.0 as f64 / 2.0, size.1 as f64 / 2.0);
    assert_eq!(picker.pick(&m, Pane::Redraw, mid), Some(NodeId(1)));
    assert_eq!(picker.pick(&m, Pane::Redraw, (mid.0, mid.1 + 40.0)), None);
    // The scan pane's pixel for the same paper point.
    let scan_px = |m: &Model| {
        let cam = m.scan_camera().unwrap();
        let c = m.chain().paper_to_scan(paper);
        (
            (c.0 - cam.origin.0) * cam.zoom,
            (c.1 - cam.origin.1) * cam.zoom,
        )
    };
    let px = scan_px(&m);
    assert_eq!(picker.pick(&m, Pane::Scan, px), Some(NodeId(1)));
    assert_eq!(picker.pick(&m, Pane::Scan, (px.0, px.1 + 60.0)), None);
    // Zoom the scan in past the threshold; with its tile held the map
    // rules: ink of node 7 (value 8) under the pointer is picked, and
    // blank paper there picks nothing even though geometry is near.
    update(
        &mut m,
        Msg::Zoom {
            pane: Pane::Scan,
            factor: 8.0,
            about: mid,
        },
    );
    assert!(m.scan_camera().unwrap().zoom >= OWNER_ZOOM);
    let px = scan_px(&m);
    let crop = m.crop_at(px).unwrap();
    let mut values = vec![0u16; 1000 * 800];
    let (cx, cy) = (crop.0.floor() as usize, crop.1.floor() as usize);
    values[cy * 1000 + cx] = 8;
    picker.owners.insert(
        0,
        0,
        OwnerTile {
            width: 1000,
            height: 800,
            values,
        },
    );
    assert_eq!(picker.pick(&m, Pane::Scan, px), Some(NodeId(7)));
    assert_eq!(picker.pick(&m, Pane::Scan, (px.0, px.1 + 200.0)), None);
}

/// The hover is visible at every zoom, down to the whole sheet (the chunk 20
/// bug: below the owner threshold the scan pane tinted nothing, and a
/// highlight of a hairline was a pixel wide). At each zoom the pick finds
/// the line under the pointer in both panes, and what is lit is not empty
/// and reaches the scan pane's screen: the group's geometry carried through
/// the chain, at least `LIT_MIN_PX` wide.
#[test]
fn the_hover_is_visible_at_every_zoom_down_to_the_whole_sheet() {
    use lmsheets_document::value::geometry::P2;
    use lmsheets_document::value::style::Dash;
    use lmsheets_inspector::pick::{HitItem, LIT_MIN_PX, OWNER_ZOOM, SERVING_MIN_PX};
    use lmsheets_inspector::{Highlight, OwnerTable, Picker};
    // A lit line is never thinner than a visible stroke.
    const _: () = assert!(LIT_MIN_PX >= SERVING_MIN_PX && SERVING_MIN_PX >= 2.0);
    use lmsheets_render::scene::{Path, Primitive};
    let hair = |a: (f64, f64), b: (f64, f64), node: u32| HitItem {
        primitive: Primitive::Stroke {
            path: Path::Line {
                a: P2::new(a.0, a.1),
                b: P2::new(b.0, b.1),
            },
            weight: 0.1,
            dash: Dash::hidden(),
            anchors: vec![],
        },
        node: Some(NodeId(node)),
    };
    // A dimension-like group of two lines (nodes 1 and 2) and a bystander.
    let picker = Picker::from_items(
        OwnerTable::default(),
        1024,
        vec![
            hair((60.0, 50.0), (90.0, 50.0), 1),
            hair((60.0, 40.0), (60.0, 52.0), 2),
            hair((10.0, 10.0), (20.0, 10.0), 3),
        ],
    );
    let on = (75.0, 50.0);
    let mut zooms: Vec<f64> = Vec::new();
    for pane in [Pane::Redraw, Pane::Scan] {
        let mut m = Model::new(chain(), (1600, 800));
        // The whole sheet (fitted), then in by steps to well past the owner
        // threshold, then out again.
        let steps: Vec<f64> = std::iter::once(1.0)
            .chain(std::iter::repeat_n(1.6, 12))
            .chain(std::iter::repeat_n(1.0 / 1.6, 12))
            .collect();
        for step in steps {
            // Put the line's point under the pane's centre, zoom about it.
            let size = m.pane_size(pane).unwrap();
            let mid = (size.0 as f64 / 2.0, size.1 as f64 / 2.0);
            let at = match pane {
                Pane::Redraw => {
                    let cam = m.redraw_camera().unwrap();
                    (
                        (on.0 - cam.origin.0) * cam.zoom,
                        (on.1 - cam.origin.1) * cam.zoom,
                    )
                }
                Pane::Scan => {
                    let cam = m.scan_camera().unwrap();
                    let c = m.chain().paper_to_scan(on);
                    (
                        (c.0 - cam.origin.0) * cam.zoom,
                        (c.1 - cam.origin.1) * cam.zoom,
                    )
                }
            };
            update(
                &mut m,
                Msg::Pan {
                    pane,
                    by: (mid.0 - at.0, mid.1 - at.1),
                },
            );
            update(
                &mut m,
                Msg::Zoom {
                    pane,
                    factor: step,
                    about: mid,
                },
            );
            let zoom = m.scan_camera().unwrap().zoom;
            zooms.push(zoom);
            // The pick finds the line in either pane, at any zoom.
            let node = picker.pick(&m, pane, mid);
            assert_eq!(
                node,
                Some(NodeId(1)),
                "{pane:?} pick at scan zoom {zoom:.3}"
            );
            let lit = Highlight {
                members: vec![NodeId(1), NodeId(2)],
                serving: vec![],
            };
            update(
                &mut m,
                Msg::Hover {
                    pane,
                    at: mid,
                    node,
                    highlight: Some(lit.clone()),
                },
            );
            assert_eq!(m.highlight(), &lit);
            // The redraw side lights both lines (drawn LIT_MIN_PX wide).
            assert_eq!(picker.items_in(&lit.members), [0, 1]);
            // The scan side: the group's geometry through the chain passes
            // the scan pane's pointer, however far out.
            let outline = picker.outline(&lit.members, m.chain());
            assert!(!outline.is_empty());
            let cam = m.scan_camera().unwrap();
            let centre = {
                let c = m.chain().paper_to_scan(on);
                (
                    (c.0 - cam.origin.0) * cam.zoom,
                    (c.1 - cam.origin.1) * cam.zoom,
                )
            };
            let near = outline.iter().any(|[a, b]| {
                let s = |p: (f64, f64)| {
                    (
                        (p.0 - cam.origin.0) * cam.zoom,
                        (p.1 - cam.origin.1) * cam.zoom,
                    )
                };
                let (a, b) = (s(*a), s(*b));
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((centre.0 - a.0) * dx + (centre.1 - a.1) * dy)
                    / (dx * dx + dy * dy).max(1e-12))
                .clamp(0.0, 1.0);
                (a.0 + t * dx - centre.0).hypot(a.1 + t * dy - centre.1) < 1.0
            });
            assert!(
                near,
                "{pane:?}: the outline passes the pointer at scan zoom {zoom:.3}"
            );
        }
    }
    // The walk spans the whole-sheet fit to well past the owner threshold.
    zooms.sort_by(f64::total_cmp);
    assert!(
        zooms[0] < OWNER_ZOOM / 2.0 && *zooms.last().unwrap() > OWNER_ZOOM * 4.0,
        "{zooms:?}"
    );
}

#[test]
fn the_overlay_is_one_pane_the_scan_and_fits_like_the_scan_alone() {
    let mut m = Model::new(chain(), (1001, 600));
    update(&mut m, Msg::SetMode(Mode::Overlay));
    assert_eq!(m.pane_size(Pane::Scan), Some((1001, 600)));
    assert_eq!(m.pane_size(Pane::Redraw), None);
    assert_eq!(m.pane_origin(Pane::Scan), Some((0, 0)));
    assert!(m.redraw_camera().is_none());
    update(&mut m, Msg::Fit);
    let cam = m.scan_camera().unwrap();
    assert!((cam.zoom - FIT_FILL * (1001.0f64 / 1000.0).min(600.0 / 800.0)).abs() < 1e-9);
    // Panning and zooming the scan pane work as in the scan alone, and the
    // redraw's pane is still hidden.
    let before = m.clone();
    update(
        &mut m,
        Msg::Pan {
            pane: Pane::Redraw,
            by: (9.0, 9.0),
        },
    );
    assert_eq!(m, before);
    update(
        &mut m,
        Msg::Zoom {
            pane: Pane::Scan,
            factor: 2.0,
            about: (300.0, 200.0),
        },
    );
    assert!((m.scan_camera().unwrap().zoom - 2.0 * cam.zoom).abs() < 1e-9);
}

#[test]
fn the_opacity_is_clamped_and_replays() {
    let mut m = Model::new(chain(), (800, 600));
    assert!(m.opacity() > 0.0 && m.opacity() <= 1.0);
    update(&mut m, Msg::SetOpacity(0.3));
    assert_eq!(m.opacity(), 0.3);
    update(&mut m, Msg::SetOpacity(7.0));
    assert_eq!(m.opacity(), 1.0);
    update(&mut m, Msg::SetOpacity(-1.0));
    assert_eq!(m.opacity(), 0.0);
    update(&mut m, Msg::SetOpacity(f64::NAN));
    assert_eq!(m.opacity(), 0.0);
    let json = serde_json::to_string(&Msg::SetOpacity(0.5)).unwrap();
    assert_eq!(
        serde_json::from_str::<Msg>(&json).unwrap(),
        Msg::SetOpacity(0.5)
    );
}

#[test]
fn the_navigator_maps_the_sheet_and_centring_moves_the_view_in_every_mode() {
    let mut m = Model::new(chain(), (1000, 600));
    let nav = m.nav_map((200.0, 150.0));
    let (lo, hi) = m.chain().paper_bounds();
    // The sheet fits the thumbnail and is centred in it.
    let (a, b) = (nav.to_px(lo), nav.to_px(hi));
    assert!(a.0 >= -1e-9 && a.1 >= -1e-9 && b.0 <= 200.0 + 1e-9 && b.1 <= 150.0 + 1e-9);
    assert!((a.0 + b.0 - 200.0).abs() < 1e-6 && (a.1 + b.1 - 150.0).abs() < 1e-6);
    assert!(((b.0 - a.0) - 200.0).abs() < 1e-6 || ((b.1 - a.1) - 150.0).abs() < 1e-6);
    let p = nav.to_paper((50.0, 60.0));
    let back = nav.to_px(p);
    assert!((back.0 - 50.0).abs() < 1e-9 && (back.1 - 60.0).abs() < 1e-9);
    // The view box is the pane's paper extent about the centre.
    let zoom_in = |m: &mut Model| {
        update(
            m,
            Msg::Zoom {
                pane: Pane::Redraw,
                factor: 4.0,
                about: (250.0, 300.0),
            },
        )
    };
    zoom_in(&mut m);
    let (v0, v1) = m.view_box();
    let size = m.pane_size(Pane::Redraw).unwrap();
    assert!(((v1.0 - v0.0) * m.scale() - size.0 as f64).abs() < 1e-6);
    assert!((((v0.1 + v1.1) / 2.0) - m.centre().1).abs() < 1e-9);
    for mode in [
        Mode::SideBySide,
        Mode::RedrawOnly,
        Mode::ScanOnly,
        Mode::Overlay,
    ] {
        update(&mut m, Msg::SetMode(mode));
        let target = nav.to_paper((70.0, 40.0));
        update(&mut m, Msg::CentreOn { at: target });
        assert_eq!(m.centre(), target, "{mode:?}");
        let (v0, v1) = m.view_box();
        assert!((((v0.0 + v1.0) / 2.0) - target.0).abs() < 1e-9, "{mode:?}");
        // The scan camera follows, through the chain.
        if let Some(cam) = m.scan_camera() {
            let c = m.chain().paper_to_scan(target);
            let mid = (
                cam.origin.0 + cam.size.0 as f64 / cam.zoom / 2.0,
                cam.origin.1 + cam.size.1 as f64 / cam.zoom / 2.0,
            );
            assert!(
                (mid.0 - c.0).abs() < 1e-6 && (mid.1 - c.1).abs() < 1e-6,
                "{mode:?}"
            );
        }
    }
    // Off the sheet clamps to its bounds; nothing finite-less moves it.
    update(&mut m, Msg::CentreOn { at: (1e9, -1e9) });
    assert_eq!(m.centre(), (hi.0, lo.1));
    let here = m.centre();
    update(
        &mut m,
        Msg::CentreOn {
            at: (f64::NAN, 0.0),
        },
    );
    assert_eq!(m.centre(), here);
}

#[test]
fn layers_and_the_theme_are_model_state_and_replay() {
    let mut m = Model::new(chain(), (800, 600));
    // The original sheet's layers show; ours (inferred, constructions) start hidden.
    for l in Layer::ALL {
        let ours = matches!(l, Layer::Inferred | Layer::Constructions);
        assert_eq!(m.layer_visible(l), !ours, "{l:?}");
    }
    assert!(!m.constraints_shown());
    assert_eq!(m.theme(), Theme::WhiteLab);
    let log = vec![
        Msg::SetLayer {
            layer: Layer::Inferred,
            visible: false,
        },
        Msg::SetLayer {
            layer: Layer::Centre,
            visible: false,
        },
        Msg::SetLayer {
            layer: Layer::Centre,
            visible: true,
        },
        Msg::SetTheme(Theme::Hud),
        Msg::SetMode(Mode::Overlay),
        Msg::SetOpacity(0.4),
    ];
    let json = serde_json::to_string(&log).unwrap();
    let replay: Vec<Msg> = serde_json::from_str(&json).unwrap();
    let mut a = Model::new(chain(), (800, 600));
    let mut b = Model::new(chain(), (800, 600));
    for msg in log {
        update(&mut a, msg);
    }
    for msg in replay {
        update(&mut b, msg);
    }
    assert_eq!(a, b);
    assert!(!a.layer_visible(Layer::Inferred));
    assert!(a.layer_visible(Layer::Centre));
    assert_eq!(a.theme(), Theme::Hud);
    // Inferred and the constructions start hidden; the log hid the former
    // again and showed and hid centre lines.
    assert_eq!(a.hidden_layers().len(), 2);
    update(&mut m, Msg::SetTheme(Theme::Hud));
    update(&mut m, Msg::SetTheme(Theme::WhiteLab));
    assert_eq!(m.theme(), Theme::WhiteLab);
}

#[test]
fn hidden_layers_are_not_picked_in_either_pane() {
    use lmsheets_document::value::geometry::P2;
    use lmsheets_document::value::style::Dash;
    use lmsheets_inspector::pick::{HitItem, OWNER_ZOOM};
    use lmsheets_inspector::{Layers, OwnerTable, OwnerTile, Picker};
    use lmsheets_render::scene::{Path, Primitive};
    // Two crossing lines of different layers; the one on top (drawn last)
    // is hidden, so the pick falls through to the one beneath.
    let line = |a: (f64, f64), b: (f64, f64), node: u32| HitItem {
        primitive: Primitive::Stroke {
            path: Path::Line {
                a: P2::new(a.0, a.1),
                b: P2::new(b.0, b.1),
            },
            weight: 0.3,
            dash: Dash::hidden(),
            anchors: vec![],
        },
        node: Some(NodeId(node)),
    };
    let items = vec![
        line((60.0, 50.0), (80.0, 50.0), 0),
        line((70.0, 45.0), (70.0, 55.0), 1),
    ];
    let mut picker = Picker::from_items(OwnerTable::default(), 1024, items);
    picker.set_layers(Layers::of_items(
        vec![Layer::Part, Layer::Centre],
        vec![Some(Layer::Part), Some(Layer::Centre)],
    ));
    let mut m = Model::new(chain(), (1600, 800));
    let paper = (70.0, 50.0);
    let by = (
        (m.centre().0 - paper.0) * m.scale(),
        (m.centre().1 - paper.1) * m.scale(),
    );
    update(
        &mut m,
        Msg::Pan {
            pane: Pane::Redraw,
            by,
        },
    );
    let size = m.pane_size(Pane::Redraw).unwrap();
    let mid = (size.0 as f64 / 2.0, size.1 as f64 / 2.0);
    let hide = |m: &mut Model, layer, visible| update(m, Msg::SetLayer { layer, visible });
    // The redraw pane.
    assert_eq!(picker.pick(&m, Pane::Redraw, mid), Some(NodeId(1)));
    hide(&mut m, Layer::Centre, false);
    assert_eq!(picker.pick(&m, Pane::Redraw, mid), Some(NodeId(0)));
    assert!(!picker.node_visible(&m, NodeId(1)));
    hide(&mut m, Layer::Part, false);
    assert_eq!(picker.pick(&m, Pane::Redraw, mid), None);
    hide(&mut m, Layer::Part, true);
    hide(&mut m, Layer::Centre, true);
    // The scan pane zoomed in: node 7's owner pixel (value 8) is the pointer's,
    // in the Part layer here, and falls out of reach when hidden.
    let px_of = |m: &Model| {
        let cam = m.scan_camera().unwrap();
        let c = m.chain().paper_to_scan(paper);
        (
            (c.0 - cam.origin.0) * cam.zoom,
            (c.1 - cam.origin.1) * cam.zoom,
        )
    };
    update(
        &mut m,
        Msg::Zoom {
            pane: Pane::Scan,
            factor: 8.0,
            about: mid,
        },
    );
    assert!(m.scan_camera().unwrap().zoom >= OWNER_ZOOM);
    let px = px_of(&m);
    let crop = m.crop_at(px).unwrap();
    let mut values = vec![0u16; 1000 * 800];
    values[crop.1.floor() as usize * 1000 + crop.0.floor() as usize] = 2;
    picker.owners.insert(
        0,
        0,
        OwnerTile {
            width: 1000,
            height: 800,
            values,
        },
    );
    assert_eq!(picker.pick(&m, Pane::Scan, px), Some(NodeId(1)));
    hide(&mut m, Layer::Centre, false);
    assert_eq!(picker.pick(&m, Pane::Scan, px), None);
}

/// Every mode's fit holds the whole sheet inside the pane the mode shows,
/// with a margin, after a resize too.
#[test]
fn the_fit_holds_the_whole_sheet_inside_the_pane_in_every_mode() {
    let mut m = Model::new(chain(), (1600, 800));
    for (w, h) in [(1600, 800), (700, 900), (1000, 300)] {
        update(
            &mut m,
            Msg::Resize {
                width: w,
                height: h,
            },
        );
        for mode in [
            Mode::SideBySide,
            Mode::RedrawOnly,
            Mode::ScanOnly,
            Mode::Overlay,
        ] {
            update(&mut m, Msg::SetMode(mode));
            update(&mut m, Msg::Fit);
            let (lo, hi) = m.chain().paper_bounds();
            if let Some(cam) = m.redraw_camera() {
                let a = (
                    (lo.0 - cam.origin.0) * cam.zoom,
                    (lo.1 - cam.origin.1) * cam.zoom,
                );
                let b = (
                    (hi.0 - cam.origin.0) * cam.zoom,
                    (hi.1 - cam.origin.1) * cam.zoom,
                );
                let (pw, ph) = (cam.size.0 as f64, cam.size.1 as f64);
                assert!(
                    a.0 > 0.0 && a.1 > 0.0 && b.0 < pw && b.1 < ph,
                    "{mode:?} {w}x{h} redraw {a:?} {b:?}"
                );
            }
            if let Some(cam) = m.scan_camera() {
                let e = m.chain().extent();
                let (pw, ph) = (cam.size.0 as f64, cam.size.1 as f64);
                // The crop's corners, through the warp, are inside the pane
                // (the panes lock by centre, so in side by side the scan keeps
                // the redraw's scale).
                for c in [(0.0, 0.0), (e.0, 0.0), (0.0, e.1), (e.0, e.1)] {
                    let p = (
                        (c.0 - cam.origin.0) * cam.zoom,
                        (c.1 - cam.origin.1) * cam.zoom,
                    );
                    assert!(
                        p.0 > 0.0 && p.1 > 0.0 && p.0 < pw && p.1 < ph,
                        "{mode:?} {w}x{h} scan {p:?}"
                    );
                }
            }
        }
    }
}

// ---- chunk 24a: the mobile layout, the wipe, the sheet, the menu ----

#[test]
fn the_mobile_layout_has_one_pane_and_no_navigator() {
    use lmsheets_inspector::Layout;
    let mut m = Model::new(chain(), (390, 844));
    assert_eq!(m.layout(), Layout::Desktop);
    assert!(m.navigator());
    assert_eq!(m.mode(), Mode::SideBySide);
    update(&mut m, Msg::SetLayout(Layout::Mobile));
    assert_eq!(m.layout(), Layout::Mobile);
    // Side by side becomes the redraw alone; the navigator is off.
    assert_eq!(m.mode(), Mode::RedrawOnly);
    assert!(!m.navigator());
    assert_eq!(m.pane_size(Pane::Redraw), Some((390, 844)));
    assert_eq!(m.pane_size(Pane::Scan), None);
    // It cannot be asked for back, but the navigator can be.
    update(&mut m, Msg::SetMode(Mode::SideBySide));
    assert_eq!(m.mode(), Mode::RedrawOnly);
    update(&mut m, Msg::SetNavigator(true));
    assert!(m.navigator());
    update(&mut m, Msg::SetMode(Mode::Overlay));
    assert_eq!(m.mode(), Mode::Overlay);
    // Back on the desktop, the wipe is not a mode and the navigator is back.
    update(&mut m, Msg::SetMode(Mode::Wipe));
    update(&mut m, Msg::SetLayout(Layout::Desktop));
    assert_eq!(m.mode(), Mode::SideBySide);
    assert!(m.navigator());
    update(&mut m, Msg::SetMode(Mode::Wipe));
    assert_eq!(m.mode(), Mode::SideBySide);
}

#[test]
fn the_wipe_shows_both_panes_full_size_locked_through_the_chain() {
    use lmsheets_inspector::Layout;
    let mut m = Model::new(chain(), (390, 844));
    update(&mut m, Msg::SetLayout(Layout::Mobile));
    update(&mut m, Msg::SetMode(Mode::Wipe));
    for pane in [Pane::Redraw, Pane::Scan] {
        assert_eq!(m.pane_size(pane), Some((390, 844)));
        assert_eq!(m.pane_origin(pane), Some((0, 0)));
    }
    // The divider is clamped, ignores non-finite values and picks the pane.
    assert_eq!(m.wipe(), 0.5);
    update(&mut m, Msg::SetWipe(0.25));
    assert_eq!(m.pane_at((97.0, 10.0)), Some(Pane::Redraw));
    assert_eq!(m.pane_at((98.0, 10.0)), Some(Pane::Scan));
    update(&mut m, Msg::SetWipe(7.0));
    assert_eq!(m.wipe(), 0.98);
    update(&mut m, Msg::SetWipe(-1.0));
    assert_eq!(m.wipe(), 0.02);
    update(&mut m, Msg::SetWipe(f64::NAN));
    assert_eq!(m.wipe(), 0.02);
    // Panning either pane moves both: one view in paper, the scan derived.
    update(&mut m, Msg::Fit);
    let (c0, s0) = (m.centre(), m.scan_centre());
    update(
        &mut m,
        Msg::Pan {
            pane: Pane::Scan,
            by: (30.0, -20.0),
        },
    );
    assert_ne!(m.centre(), c0);
    assert_ne!(m.scan_centre(), s0);
    let cam = m.scan_camera().unwrap();
    let mid = (
        cam.origin.0 + cam.size.0 as f64 / cam.zoom / 2.0,
        cam.origin.1 + cam.size.1 as f64 / cam.zoom / 2.0,
    );
    let want = m.chain().paper_to_scan(m.centre());
    assert!((mid.0 - want.0).abs() < 1e-6 && (mid.1 - want.1).abs() < 1e-6);
    // Fit holds the whole sheet inside the redraw pane.
    update(&mut m, Msg::Fit);
    let (lo, hi) = m.chain().paper_bounds();
    let cam = m.redraw_camera().unwrap();
    assert!((lo.0 - cam.origin.0) * cam.zoom > 0.0);
    assert!((hi.0 - cam.origin.0) * cam.zoom < 390.0);
}

#[test]
fn the_sheet_follows_the_pin_and_the_details() {
    use lmsheets_inspector::{Highlight, PinHead, Sheet};
    let mut m = Model::new(chain(), (390, 844));
    assert_eq!(m.sheet(), Sheet::Closed);
    // Nothing pinned: the sheet cannot be raised.
    update(&mut m, Msg::SetSheet(Sheet::Full));
    assert_eq!(m.sheet(), Sheet::Closed);
    update(
        &mut m,
        Msg::Pin {
            head: PinHead::Node(NodeId(3)),
            highlight: Highlight::default(),
        },
    );
    assert_eq!(m.sheet(), Sheet::Peek);
    update(&mut m, Msg::SetSheet(Sheet::Half));
    assert_eq!(m.sheet(), Sheet::Half);
    assert!(m.details_open());
    update(&mut m, Msg::SetSheet(Sheet::Full));
    assert_eq!(m.sheet(), Sheet::Full);
    // Closing the details (the desktop message too) drops it to the summary.
    update(&mut m, Msg::SetDetails(false));
    assert_eq!(m.sheet(), Sheet::Peek);
    update(&mut m, Msg::SetDetails(true));
    assert_eq!(m.sheet(), Sheet::Full, "the height it was raised to");
    update(&mut m, Msg::SetSheet(Sheet::Peek));
    assert!(!m.details_open());
    update(&mut m, Msg::SetSheet(Sheet::Half));
    update(&mut m, Msg::Unpin);
    assert_eq!(m.sheet(), Sheet::Closed);
    assert!(!m.details_open());
}

#[test]
fn the_menu_and_the_about_panel_exclude_each_other() {
    let mut m = Model::new(chain(), (390, 844));
    assert!(!m.menu_open() && !m.about_open());
    update(&mut m, Msg::SetMenu(true));
    assert!(m.menu_open());
    update(&mut m, Msg::SetAbout(true));
    assert!(m.about_open() && !m.menu_open());
    update(&mut m, Msg::SetMenu(true));
    assert!(m.menu_open() && !m.about_open());
    update(&mut m, Msg::SetMenu(false));
    update(&mut m, Msg::SetAbout(false));
    assert!(!m.menu_open() && !m.about_open());
}

#[test]
fn the_mobile_messages_replay_and_round_trip() {
    use lmsheets_inspector::{Layout, Sheet};
    let log = vec![
        Msg::SetLayout(Layout::Mobile),
        Msg::SetMode(Mode::Wipe),
        Msg::SetWipe(0.3),
        Msg::SetSheet(Sheet::Half),
        Msg::SetMenu(true),
        Msg::SetAbout(true),
        Msg::SetNavigator(true),
    ];
    let json = serde_json::to_string(&log).unwrap();
    let back: Vec<Msg> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, log);
    let run = |l: &[Msg]| {
        let mut m = Model::new(chain(), (390, 844));
        for x in l {
            update(&mut m, x.clone());
        }
        m
    };
    assert_eq!(run(&log), run(&back));
    let m = run(&log);
    assert_eq!((m.mode(), m.wipe(), m.navigator()), (Mode::Wipe, 0.3, true));
}
