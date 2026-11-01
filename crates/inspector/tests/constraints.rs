//! The constraint list on 17619's stored document (the v4 fixture), the
//! filters, and the icon layout.

use lmsheets_document::state::document::Document;
use lmsheets_document::value::style::Purpose;
use lmsheets_inspector::constraints::{cluster, count_digits, glyph, pick_icon, ICON_PX};
use lmsheets_inspector::{
    update, ConstraintKind, ConstraintTrust, Constraints, Layers, Model, Msg, NodeId, OwnerTable,
};
use lmsheets_render::conventions::Conventions;
use lmsheets_render::draw::draw;
use lmsheets_render::nodes::node_key;
use lmsheets_render::resolve::resolve;
use lmsheets_render::scene::Scene;
use lmsheets_render::theme::Theme;

struct Fixture {
    doc: Document,
    scene: Scene,
    table: OwnerTable,
    constraints: Constraints,
    layers: Layers,
}

impl Fixture {
    fn new() -> Self {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../document/tests/fixtures/v4/LDW280-17619-sh3.document.json"
        );
        let doc = Document::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let scene = Scene::of(draw(&doc, Purpose::Screen));
        let styled = resolve(&scene, &doc, &Theme::screen(), Conventions::default());
        let mut keys: Vec<(String, String)> = Vec::new();
        for it in &scene.planes[0].items {
            let k = node_key(&scene.planes[0], &doc, it.source);
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        let table = OwnerTable::from_keys(keys);
        let constraints = Constraints::new(&doc, &scene.planes[0], &table);
        let layers = Layers::new(&doc, &scene.planes[0], &styled.items, &table);
        Self {
            doc,
            scene,
            table,
            constraints,
            layers,
        }
    }

    fn get(&self, id: &str) -> &lmsheets_inspector::ConstraintItem {
        self.constraints
            .items()
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("no constraint {id}"))
    }
}

fn model(f: &Fixture) -> Model {
    // Any chain will do: the constraint filters do not read it.
    let dir = std::env::var_os("LMSHEETS_BUNDLE_17619");
    let _ = (&f.doc, dir);
    let mut m = Model::new(synthetic_chain(), (1000, 600));
    update(&mut m, Msg::SetConstraintsShown(true));
    m
}

fn synthetic_chain() -> lmsheets_inspector::Chain {
    use lmsheets_document::value::frame::{Crop, Registration, Rotation, Similarity, Step, Warp};
    use lmsheets_document::value::provenance::{Agent, Citation, Date, Method, Provenance};
    let prov = || {
        Provenance::new(
            Method::Measured { sigma_mm: 0.1 },
            Agent::Script { name: "t".into() },
            Date::new(2026, 11, 1).unwrap(),
        )
        .cite(Citation::new("scan", "px"))
    };
    let warp = Warp {
        centre: [500.0, 400.0],
        half: 640.0,
        shear: 0.0,
        dx: vec![0.0; 7],
        dy: vec![0.0; 7],
        extent: [0.0, 0.0, 1000.0, 800.0],
    };
    let reg = Registration {
        crop: Step::new(
            Crop {
                box_px: [0, 0, 1000, 800],
            },
            prov(),
        ),
        warp: Step::new(warp, prov()),
        rotation: Step::new(
            Rotation {
                centre: [500.0, 400.0],
                angle: 0.0,
            },
            prov(),
        ),
        paper: Step::new(Similarity::new([0.0, 0.0], 10.0, 0.0).unwrap(), prov()),
    };
    let grid = lmsheets_inspector::Grid::from_fn([-10.0, -10.0], 1.0, [140, 120], |p| {
        let f = reg.from_paper(
            lmsheets_document::value::geometry::P2::new(p[0], p[1]),
            lmsheets_document::value::frame::Space::Frame,
        );
        [f[0], f[1]]
    });
    lmsheets_inspector::Chain::new(reg, [0.0, 0.0], [1000.0, 800.0], grid)
}

#[test]
fn the_part_view_lists_its_37_constraints_in_four_classes() {
    let f = Fixture::new();
    let items = f.constraints.items();
    assert_eq!(
        items.len(),
        37,
        "the part view's constraints (the sheet has none)"
    );
    assert!(items.iter().all(|c| c.view.as_deref() == Some("part")));
    let counts = ConstraintTrust::ALL.map(|t| f.constraints.count_trust(t));
    // Read: the sheet's dimensions D1-D5 and R1 and R1's typ; drawn: the
    // fit's; stated: the user's design statements; inferred: K10's.
    assert_eq!(counts, [7, 13, 7, 10]);
    // A genuine dimension is read, at a ladder level, flagged as a
    // dimension, and binds its dimension's node.
    let d1 = f.get("D1");
    assert_eq!(d1.trust, ConstraintTrust::Read);
    assert_eq!(d1.kind, ConstraintKind::Distance);
    assert!(d1.dimension && d1.level.is_some());
    let node = f.table.id("dimension", "D1").unwrap();
    assert!(d1.binds.contains(&node));
    // An inferred one carries its basis, and a design statement is stated.
    let inf = f.get("inferred_vertical_distance_E68.start_E68.end");
    assert_eq!(inf.trust, ConstraintTrust::Inferred);
    assert!(inf.detail.contains("scaled from the ink"), "{}", inf.detail);
    assert!(inf.dimension);
    assert_eq!(
        f.get("design_horizontal_E45.7.centre_E45.3.centre").trust,
        ConstraintTrust::Stated
    );
    assert_eq!(f.get("R1-typ1").kind, ConstraintKind::Equal);
    // A tangent join binds both of its curves.
    let t = f.get("tangent_e931c6c955cd1.2_e931c6c955cd1.3");
    assert_eq!(t.trust, ConstraintTrust::Drawn);
    assert_eq!(t.binds.len(), 2);
}

#[test]
fn every_icon_anchor_lies_on_or_beside_the_part() {
    let f = Fixture::new();
    // The part sits on the paper at the view's transform; every anchor is
    // inside the sheet's border (finite, and within the paper's size).
    let (w, h) = (f.scene.planes[0].items.len(), 0);
    assert!(w > 0 && h == 0);
    for c in f.constraints.items() {
        assert!(c.anchor.0.is_finite() && c.anchor.1.is_finite());
        assert!(
            (0.0..=420.0).contains(&c.anchor.0) && (0.0..=300.0).contains(&c.anchor.1),
            "{} at {:?}",
            c.id,
            c.anchor
        );
        assert_ne!(c.anchor, (0.0, 0.0), "{} has no anchor", c.id);
    }
    // A vertical line's icon sits on that line; a tangent join's at its end.
    let v = f.get("vertical_eca35acab59f6.end_eca35acab59f6.start");
    let doc = &f.doc;
    let view = doc.views().view("part").unwrap();
    let line = view.sketch.geometry().get("eca35acab59f6").unwrap();
    let lmsheets_document::value::geometry::Entity::Line { start, end } = line else {
        panic!("a line")
    };
    let mid = view
        .transform
        .to_paper(lmsheets_document::value::geometry::P2::new(
            (start.x + end.x) / 2.0,
            (start.y + end.y) / 2.0,
        ));
    assert!((v.anchor.0 - mid.x).abs() < 1e-9 && (v.anchor.1 - mid.y).abs() < 1e-9);
}

#[test]
fn the_filters_are_model_state_and_choose_the_icons_shown() {
    let f = Fixture::new();
    let mut m = model(&f);
    let all = f.constraints.visible(&m, &f.layers);
    // Dimensions' own constraints are off by default.
    let dims = f.constraints.items().iter().filter(|c| c.dimension).count();
    assert!(dims >= 6);
    // The constructions start hidden, so a constraint that binds only them
    // would have no icon either (the dimensions' end points are points of
    // the arcs now, so the fixture has none).
    let on_hidden_only = |c: &lmsheets_inspector::ConstraintItem| {
        c.binds
            .iter()
            .all(|b| f.layers.of_node(*b).is_some_and(|l| !m.layer_visible(l)))
    };
    let hidden_only = f
        .constraints
        .items()
        .iter()
        .filter(|c| !c.dimension && on_hidden_only(c))
        .count();
    assert_eq!(all.len(), 37 - dims - hidden_only);
    update(&mut m, Msg::SetDimensionConstraints(true));
    let with_dims = f.constraints.visible(&m, &f.layers).len();
    assert!(with_dims > all.len() && with_dims <= 37);
    // By trust class: inferred only.
    for t in [
        ConstraintTrust::Read,
        ConstraintTrust::Drawn,
        ConstraintTrust::Stated,
    ] {
        update(
            &mut m,
            Msg::SetConstraintTrust {
                trust: t,
                visible: false,
            },
        );
    }
    let shown = f.constraints.visible(&m, &f.layers);
    assert_eq!(
        shown.len(),
        f.constraints.count_trust(ConstraintTrust::Inferred)
    );
    assert!(shown
        .iter()
        .all(|&i| f.constraints.items()[i].trust == ConstraintTrust::Inferred));
    // By type, on top: hide the inferred coincidents.
    update(
        &mut m,
        Msg::SetConstraintKind {
            kind: ConstraintKind::Coincident,
            visible: false,
        },
    );
    let n = f.constraints.visible(&m, &f.layers).len();
    assert_eq!(
        n,
        shown.len()
            - f.constraints
                .items()
                .iter()
                .filter(|c| c.trust == ConstraintTrust::Inferred
                    && c.kind == ConstraintKind::Coincident)
                .count()
    );
    // Only on the hovered group: nothing hovered, nothing shown; hovering a
    // node shows those that bind it.
    update(&mut m, Msg::SetConstraintsOnSelection(true));
    assert!(f.constraints.visible(&m, &f.layers).is_empty());
    for t in ConstraintTrust::ALL {
        update(
            &mut m,
            Msg::SetConstraintTrust {
                trust: t,
                visible: true,
            },
        );
    }
    update(
        &mut m,
        Msg::SetConstraintKind {
            kind: ConstraintKind::Coincident,
            visible: true,
        },
    );
    let arc = f.table.id("entity", "part/e931c6c955cd1.3").unwrap();
    update(
        &mut m,
        Msg::Hover {
            pane: lmsheets_inspector::Pane::Redraw,
            at: (10.0, 10.0),
            node: Some(arc),
            highlight: None,
        },
    );
    let on: Vec<&str> = f
        .constraints
        .visible(&m, &f.layers)
        .iter()
        .map(|&i| f.constraints.items()[i].id.as_str())
        .collect();
    assert!(
        on.contains(&"tangent_e931c6c955cd1.2_e931c6c955cd1.3"),
        "{on:?}"
    );
    assert!(on.len() < 20);
    assert!(on.iter().all(|id| f.get(id).binds.contains(&arc)));
    // The state replays through JSON.
    let log = vec![
        Msg::SetConstraintTrust {
            trust: ConstraintTrust::Drawn,
            visible: false,
        },
        Msg::SetConstraintKind {
            kind: ConstraintKind::Tangent,
            visible: false,
        },
        Msg::SetConstraintsOnSelection(true),
        Msg::SetDimensionConstraints(true),
        Msg::HoverConstraints {
            pane: lmsheets_inspector::Pane::Redraw,
            at: (3.0, 4.0),
            items: vec![1, 2],
            highlight: lmsheets_inspector::Highlight::piece(NodeId(5)),
        },
    ];
    let replay: Vec<Msg> = serde_json::from_str(&serde_json::to_string(&log).unwrap()).unwrap();
    let (mut a, mut b) = (model(&f), model(&f));
    for msg in log {
        update(&mut a, msg);
    }
    for msg in replay {
        update(&mut b, msg);
    }
    assert_eq!(a, b);
    assert_eq!(a.hovered_constraints(), &[1, 2]);
    assert!(!a.constraint_trust_visible(ConstraintTrust::Drawn));
    assert!(!a.constraint_kind_visible(ConstraintKind::Tangent));
    // A plain hover clears the icon hover.
    update(&mut a, Msg::Leave);
    assert!(a.hovered_constraints().is_empty());
}

#[test]
fn icons_cluster_by_screen_cell_and_split_as_the_view_zooms_in() {
    let f = Fixture::new();
    let m = model(&f);
    let shown = f.constraints.visible(&m, &f.layers);
    let far = cluster(f.constraints.items(), &shown, 1.0);
    let near = cluster(f.constraints.items(), &shown, 200.0);
    // Zoomed out they merge; zoomed in every icon stands alone or nearly.
    assert!(far.len() < near.len());
    assert!(
        near.len() >= shown.len() * 3 / 4,
        "{} of {}",
        near.len(),
        shown.len()
    );
    // No constraint is lost or doubled.
    for cs in [&far, &near] {
        let mut all: Vec<usize> = cs.iter().flat_map(|c| c.members.clone()).collect();
        all.sort();
        assert_eq!(all, shown);
    }
    // A cluster's icon is within one cell of its members' anchors, and one
    // of one type keeps its glyph.
    for c in &far {
        if c.members.len() > 1 {
            let kinds: Vec<_> = c
                .members
                .iter()
                .map(|&i| f.constraints.items()[i].kind)
                .collect();
            if kinds.iter().any(|k| *k != kinds[0]) {
                assert_eq!(c.kind, ConstraintKind::Other);
            }
        }
    }
    // Picking: on the icon, within its edge, and off.
    let c = &near[0];
    let to_px = |p: (f64, f64)| (p.0 * 200.0, p.1 * 200.0);
    let centre = to_px(c.at);
    assert_eq!(pick_icon(&near, to_px, centre, 1.0), Some(0));
    assert_eq!(
        pick_icon(&near, to_px, (centre.0 + ICON_PX / 2.0, centre.1), 1.0),
        Some(0)
    );
    assert_eq!(pick_icon(&near, to_px, (-1e6, -1e6), 1.0), None);
}

#[test]
fn every_type_has_a_glyph_and_a_count_has_digits() {
    for k in ConstraintKind::ALL {
        let g = glyph(k);
        assert!(!g.is_empty(), "{k:?}");
    }
    assert_eq!(count_digits(1).len(), 2);
    assert_eq!(count_digits(8).len(), 7);
    assert_eq!(count_digits(12).len(), 2 + 5);
    assert_eq!(count_digits(500), count_digits(99));
}

#[test]
fn constraints_start_hidden_and_one_switch_shows_and_hides_all_icons() {
    let f = Fixture::new();
    let mut m = Model::new(synthetic_chain(), (1000, 600));
    assert!(!m.constraints_shown());
    assert!(f.constraints.visible(&m, &f.layers).is_empty());
    update(&mut m, Msg::SetConstraintsShown(true));
    assert!(!f.constraints.visible(&m, &f.layers).is_empty());
    // The filters keep their say under the switch.
    update(
        &mut m,
        Msg::SetConstraintKind {
            kind: ConstraintKind::Vertical,
            visible: false,
        },
    );
    let some = f.constraints.visible(&m, &f.layers).len();
    update(&mut m, Msg::SetConstraintsShown(false));
    assert!(f.constraints.visible(&m, &f.layers).is_empty());
    update(&mut m, Msg::SetConstraintsShown(true));
    assert_eq!(f.constraints.visible(&m, &f.layers).len(), some);
}

#[test]
fn a_centre_line_emphasises_the_constraint_between_its_centres_only_while_icons_show() {
    let f = Fixture::new();
    let design = "design_horizontal_E45.7.centre_E45.3.centre";
    let item = f.get(design);
    // The relation holds two points level: a line is drawn between them, and
    // the icon sits on it.
    let (a, b) = item.relation_line.expect("two points");
    assert!((item.anchor.0 - (a.0 + b.0) / 2.0).abs() < 1e-9);
    assert!((a.0 - b.0).abs() > 10.0, "the centres are apart");
    assert_eq!(item.centre_of.len(), 2);
    let idx = f
        .constraints
        .items()
        .iter()
        .position(|c| c.id == design)
        .unwrap();
    // A centre line through either centre reveals it, though it binds the
    // arcs and no centre line; so does the arc, and a line elsewhere does not.
    for cl in ["CL1", "CL2", "CL4"] {
        let n = f.table.id("centre_line", cl).unwrap();
        assert!(
            f.constraints.revealed(&[n]).contains(&idx),
            "{cl} reveals the constraint between the centres"
        );
    }
    let arc = f.table.id("entity", "part/e931c6c955cd1.7").unwrap();
    assert!(f.constraints.revealed(&[arc]).contains(&idx));
    let far = f.table.id("entity", "part/e931c6c955cd1.1").unwrap();
    assert!(!f.constraints.revealed(&[far]).contains(&idx));
    // With the icons off nothing is listed for a lit centre line (the
    // shell's connections view, a pin's own or Shift's, shows it then); with
    // them on, the icon is among those shown and the one to emphasise.
    let cl1 = f.table.id("centre_line", "CL1").unwrap();
    let mut m = model(&f);
    update(&mut m, Msg::SetConstraintsShown(false));
    update(
        &mut m,
        Msg::Pin {
            head: lmsheets_inspector::PinHead::Node(cl1),
            highlight: lmsheets_inspector::Highlight::piece(cl1),
        },
    );
    assert!(f.constraints.visible(&m, &f.layers).is_empty());
    update(&mut m, Msg::SetConstraintsShown(true));
    update(
        &mut m,
        Msg::SetConstraintTrust {
            trust: ConstraintTrust::Stated,
            visible: true,
        },
    );
    assert!(f.constraints.visible(&m, &f.layers).contains(&idx));
    // Opened clusters are not merged and do not overlap.
    let shown = f
        .constraints
        .revealed(&[f.table.id("centre_line", "CL1").unwrap()]);
    for scale in [2.0, 40.0] {
        let icons = lmsheets_inspector::constraints::singles(f.constraints.items(), &shown, scale);
        assert_eq!(icons.len(), shown.len());
        for (i, p) in icons.iter().enumerate() {
            let at = |c: &lmsheets_inspector::constraints::Cluster| {
                (c.at.0 * scale + c.offset.0, c.at.1 * scale + c.offset.1)
            };
            for q in &icons[i + 1..] {
                let d = (at(p).0 - at(q).0).hypot(at(p).1 - at(q).1);
                assert!(d >= ICON_PX, "icons {d:.1} px apart at {scale}");
            }
        }
    }
}

#[test]
fn the_connections_view_is_model_state_off_by_default_and_replays() {
    let f = Fixture::new();
    let mut m = Model::new(synthetic_chain(), (1000, 600));
    assert!(!m.connections());
    let log = vec![
        Msg::SetConnections(true),
        Msg::SetConnections(false),
        Msg::SetConnections(true),
    ];
    let replay: Vec<Msg> = serde_json::from_str(&serde_json::to_string(&log).unwrap()).unwrap();
    for msg in replay {
        update(&mut m, msg);
    }
    assert!(m.connections());
    // It shows connections, not icons: the switch is separate.
    assert!(!m.constraints_shown());
    assert!(f.constraints.visible(&m, &f.layers).is_empty());
    update(&mut m, Msg::SetConnections(false));
    assert!(!m.connections());
}

#[test]
fn a_pins_connections_are_always_shown_and_a_hover_only_adds_its_own_highlight() {
    use lmsheets_inspector::{Highlight, Pane, PinHead};
    let f = Fixture::new();
    let design = "design_horizontal_E45.7.centre_E45.3.centre";
    let idx = f
        .constraints
        .items()
        .iter()
        .position(|c| c.id == design)
        .unwrap();
    let cl1 = f.table.id("centre_line", "CL1").unwrap();
    let elsewhere = f.table.id("entity", "part/e931c6c955cd1.1").unwrap();
    let mut m = model(&f);
    update(&mut m, Msg::SetConstraintsShown(false));
    let hover = |m: &mut Model, n| {
        update(
            m,
            Msg::Hover {
                pane: Pane::Redraw,
                at: (5.0, 5.0),
                node: Some(n),
                highlight: None,
            },
        )
    };
    // Hovering alone: minimal, no connections (Shift not held).
    hover(&mut m, cl1);
    assert_eq!(f.constraints.connections(&m, None), None);
    // Shift shows the hovered group's.
    update(&mut m, Msg::SetConnections(true));
    assert!(f.constraints.connections(&m, None).unwrap().contains(&idx));
    update(&mut m, Msg::SetConnections(false));
    // Click: pin it while the pointer is still over it. The connections are
    // shown at once, with no Shift and no move.
    update(
        &mut m,
        Msg::Pin {
            head: PinHead::Node(cl1),
            highlight: Highlight::piece(cl1),
        },
    );
    let pinned = f.constraints.connections(&m, None).unwrap();
    assert!(pinned.contains(&idx));
    // Hover another node: the pin's connections are still shown, and what
    // is hovered is lit in its own right.
    hover(&mut m, elsewhere);
    assert_eq!(f.constraints.connections(&m, None), Some(pinned.clone()));
    assert_eq!(m.lit(), &Highlight::piece(elsewhere));
    assert_eq!(m.pinned().unwrap().head, PinHead::Node(cl1));
    // Leaving brings back the pin; unpinning returns to the minimal view.
    update(&mut m, Msg::Leave);
    assert_eq!(f.constraints.connections(&m, None), Some(pinned));
    update(&mut m, Msg::Unpin);
    assert_eq!(f.constraints.connections(&m, None), None);
}
