//! The detail panel's data on 17619's stored document (the v4 fixture), and
//! its agreement with the paper-trail walker the acceptance check A8 uses.

use lmsheets_document::state::document::Document;
use lmsheets_document::value::style::Purpose;
use lmsheets_inspector::detail::{claim_node, departure, detail, Detail, Source, NOT_RECORDED};
use lmsheets_inspector::{
    update, Constraints, Groups, Highlight, Layers, Model, Msg, NodeId, OwnerTable, PinHead,
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
    groups: Groups,
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
        let plane = &scene.planes[0];
        let constraints = Constraints::new(&doc, plane, &table);
        let layers = Layers::new(&doc, plane, &styled.items, &table);
        let groups = Groups::new(&doc, plane, &table);
        Self {
            doc,
            scene,
            table,
            constraints,
            layers,
            groups,
        }
    }

    fn detail(&self, kind: &str, name: &str) -> Detail {
        let id = self
            .table
            .id(kind, name)
            .unwrap_or_else(|| panic!("no node {kind} {name}"));
        self.detail_of(PinHead::Node(id))
    }

    fn detail_of(&self, head: PinHead) -> Detail {
        let scale = |_: (f64, f64)| 7.87;
        detail(
            &Source {
                doc: &self.doc,
                plane: &self.scene.planes[0],
                table: &self.table,
                constraints: &self.constraints,
                layers: &self.layers,
                groups: &self.groups,
                scan_per_mm: &scale,
                residuals: None,
            },
            head,
        )
    }
}

fn values<'a>(d: &'a Detail, section: &str, key: &str) -> Vec<&'a str> {
    d.section(section)
        .map(|s| {
            s.rows
                .iter()
                .filter(|r| r.key == key)
                .map(|r| r.value.as_str())
                .collect()
        })
        .unwrap_or_default()
}

/// The panel's trail and A8's walk are one walk: on every node a claim
/// names, the panel lists as many claims as the document's trail walker
/// finds, and the archive URLs it shows are the walker's.
#[test]
fn the_panel_and_the_a8_walker_agree_on_every_claimed_node() {
    let f = Fixture::new();
    // A8's own rule: every claim resolves to an archive.org original.
    for t in f.doc.claim_trails() {
        let url = t
            .archive_url()
            .unwrap_or_else(|| panic!("claim {}", t.index));
        assert!(url.starts_with("https://archive.org/"));
        assert!(t.evidence.is_some());
    }
    let mut sampled = 0;
    for i in 0..f.table.len() {
        let id = NodeId(i as u32);
        let (kind, name) = f.table.get(id).unwrap();
        let Some(node) = claim_node(kind, name) else {
            continue;
        };
        let walked: Vec<_> = f.doc.trail_of(&node).collect();
        if walked.is_empty() {
            continue;
        }
        sampled += 1;
        let d = f.detail_of(PinHead::Node(id));
        let trail = d
            .section("Paper trail")
            .unwrap_or_else(|| panic!("{kind} {name}"));
        let urls: Vec<&str> = trail
            .rows
            .iter()
            .filter(|r| r.key == "archive")
            .map(|r| r.value.as_str())
            .collect();
        let want: Vec<&str> = walked.iter().filter_map(|t| t.archive_url()).collect();
        assert_eq!(urls, want, "{kind} {name}");
        assert!(trail
            .rows
            .iter()
            .filter(|r| r.key == "archive")
            .all(|r| r.href.as_deref() == Some(r.value.as_str())));
        let claims = d
            .section("Evidence")
            .unwrap()
            .rows
            .iter()
            .filter(|r| r.key == "claim")
            .count();
        assert_eq!(claims, walked.len(), "{kind} {name}");
    }
    assert!(sampled >= 20, "{sampled} claimed nodes sampled");
}

#[test]
fn a_dimension_shows_what_it_read_what_was_solved_and_what_is_not_recorded() {
    let f = Fixture::new();
    let d = f.detail("dimension", "D3");
    assert_eq!(d.title, "dimension D3");
    assert_eq!(values(&d, "Geometry", "read value"), ["1 3/4 in"]);
    // 1 3/4 in is 44.45 mm, held by the solve.
    let solved = values(&d, "Geometry", "solved value");
    assert!(solved[0].starts_with("44.450 mm"), "{solved:?}");
    assert_eq!(values(&d, "Geometry", "residual"), [NOT_RECORDED]);
    // Its constraints include its own, linked; its group is linked.
    let own = d.section("Constraints").unwrap();
    assert!(own
        .rows
        .iter()
        .any(|r| r.value.starts_with("D3 [") && r.pin.is_some()));
    let what = d.section("What it is").unwrap();
    assert!(what.rows.iter().any(|r| r.key == "with" && r.pin.is_some()));
    // The trail ends at the archive and has the frame's hops.
    let trail = d.section("Paper trail").unwrap();
    assert!(trail.rows.iter().any(|r| r.key == "hop: upload"));
    assert!(trail.rows.iter().any(|r| r.key == "frame hash"));
}

#[test]
fn a_centre_line_shows_its_said_departure_in_its_reason() {
    let f = Fixture::new();
    let d = f.detail("centre_line", "CL4");
    let reason = values(&d, "Evidence", "reason");
    assert_eq!(reason.len(), 1);
    assert!(reason[0].contains("23.7 px from it"), "{reason:?}");
    // Open gaps naming it come with the panel.
    let gaps = d.section("Open gaps").expect("the CL4 gap names it");
    assert!(gaps.rows.iter().any(|r| r.key == "centre-line-off-centre"));
}

#[test]
fn an_inferred_dimension_and_a_constraint_pin_through_links() {
    let f = Fixture::new();
    let d = f.detail("dimension", "inferred_vertical_distance_E68.start_E68.end");
    let cons = d.section("Constraints").unwrap();
    let own = cons
        .rows
        .iter()
        .find(|r| r.value.contains("Inferred") && r.pin.is_some())
        .expect("its own, inferred");
    // Following the link gives the constraint's panel, which links back to
    // the dimension and to what it binds.
    let c = f.detail_of(own.pin.unwrap());
    assert!(c.title.starts_with("constraint inferred_vertical"));
    assert!(c.section("Dimension").unwrap().rows[0].pin.is_some());
    assert!(c
        .section("Binds")
        .unwrap()
        .rows
        .iter()
        .all(|r| r.pin.is_some()));
    assert_eq!(values(&c, "Constraint", "residual"), [NOT_RECORDED]);
    assert!(values(&c, "Constraint", "basis / evidence")[0].contains("scaled from the ink"));
}

#[test]
fn a_view_entity_shows_model_and_paper_values_and_its_style() {
    let f = Fixture::new();
    let d = f.detail("entity", "part/e931c6c955cd1.7");
    let geometry = d.section("Geometry").unwrap();
    assert!(geometry
        .rows
        .iter()
        .any(|r| r.key == "radius" && r.value.contains(" in)")));
    assert!(geometry
        .rows
        .iter()
        .any(|r| r.key == "centre" && r.value.contains("paper")));
    assert!(d.section("Style").is_some());
    assert!(d.section("Scale sample").is_some());
    // The departure from the ink fit is read from the note's sentence.
    assert_eq!(
        departure("x; departs from its ink fit, by 5.1 px rms, 6.4 px at most, moved"),
        Some((5.1, 6.4))
    );
    assert_eq!(departure("no numbers here"), None);
}

#[test]
fn pinning_is_model_state_that_lights_while_nothing_is_hovered_and_replays() {
    let f = Fixture::new();
    let id = f.table.id("dimension", "D3").unwrap();
    let hl = f.groups.highlight(id, false);
    let log = vec![
        Msg::Pin {
            head: PinHead::Node(id),
            highlight: hl.clone(),
        },
        Msg::Pin {
            head: PinHead::Constraint(3),
            highlight: Highlight::piece(id),
        },
        Msg::Unpin,
        Msg::Pin {
            head: PinHead::Node(id),
            highlight: hl.clone(),
        },
    ];
    let json = serde_json::to_string(&log).unwrap();
    let replay: Vec<Msg> = serde_json::from_str(&json).unwrap();
    let mut a = pin_model();
    let mut b = pin_model();
    for m in log {
        update(&mut a, m);
    }
    for m in replay {
        update(&mut b, m);
    }
    assert_eq!(a, b);
    assert_eq!(a.pinned().unwrap().head, PinHead::Node(id));
    // Nothing hovered: the pin is what is lit; a hover takes over, and
    // leaving gives the pin back.
    assert_eq!(a.lit(), &hl);
    update(
        &mut a,
        Msg::Hover {
            pane: lmsheets_inspector::Pane::Redraw,
            at: (5.0, 5.0),
            node: Some(NodeId(1)),
            highlight: None,
        },
    );
    assert_eq!(a.lit(), &Highlight::piece(NodeId(1)));
    update(&mut a, Msg::Leave);
    assert_eq!(a.lit(), &hl);
    update(&mut a, Msg::Unpin);
    assert!(a.lit().is_empty() && a.pinned().is_none());
}

fn pin_model() -> Model {
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
    Model::new(
        lmsheets_inspector::Chain::new(reg, [0.0, 0.0], [1000.0, 800.0], grid),
        (1000, 600),
    )
}

#[test]
fn the_panel_opens_only_on_request_and_closes_with_the_pin() {
    let mut m = pin_model();
    // Nothing pinned: there is nothing to open.
    update(&mut m, Msg::SetDetails(true));
    assert!(!m.details_open());
    // A pin lights and offers the panel; it does not open it.
    update(
        &mut m,
        Msg::Pin {
            head: PinHead::Node(NodeId(2)),
            highlight: Highlight::piece(NodeId(2)),
        },
    );
    assert!(!m.details_open() && m.lit_is_pin());
    update(&mut m, Msg::SetDetails(true));
    assert!(m.details_open());
    // Pinning another item with the drawer open keeps it open.
    update(
        &mut m,
        Msg::Pin {
            head: PinHead::Node(NodeId(3)),
            highlight: Highlight::piece(NodeId(3)),
        },
    );
    assert!(m.details_open());
    // Closing it leaves the pin; letting go of the pin closes it.
    update(&mut m, Msg::SetDetails(false));
    assert!(!m.details_open() && m.pinned().is_some());
    update(&mut m, Msg::SetDetails(true));
    update(&mut m, Msg::Unpin);
    assert!(!m.details_open());
    update(
        &mut m,
        Msg::Pin {
            head: PinHead::Node(NodeId(2)),
            highlight: Highlight::piece(NodeId(2)),
        },
    );
    assert!(!m.details_open(), "a new pin starts closed");
}

/// A stub of what `Document::residuals()` will give.
struct Stub;

impl lmsheets_inspector::detail::Residuals for Stub {
    fn dimension(&self, name: &str) -> Option<lmsheets_inspector::detail::DimensionResidual> {
        (name == "D3").then(|| lmsheets_inspector::detail::DimensionResidual {
            read: "1 3/4 in".into(),
            ink: "1.7418 in".into(),
            delta_in: 0.0082,
            delta_px: 6.1,
            scale_sample: 2.0182,
            scale_without: Some(2.0136),
            pull: 0.31,
        })
    }
    fn constraint(
        &self,
        _view: Option<&str>,
        id: &str,
    ) -> Option<lmsheets_inspector::detail::ConstraintResidual> {
        (id == "D3").then_some(lmsheets_inspector::detail::ConstraintResidual {
            ink_px: 6.1,
            pull: 0.31,
        })
    }
    fn entity(&self, name: &str) -> Option<lmsheets_inspector::detail::EntityDeparture> {
        (name == "part/e931c6c955cd1.7").then(|| lmsheets_inspector::detail::EntityDeparture {
            rms_px: 2.5,
            max_px: 4.0,
            moved_by: Some("D3".into()),
        })
    }
}

#[test]
fn recorded_residuals_slot_into_the_rows_and_absent_ones_say_not_recorded() {
    let f = Fixture::new();
    let id = f.table.id("dimension", "D3").unwrap();
    let scale = |_: (f64, f64)| 7.87;
    let with = |head| {
        detail(
            &Source {
                doc: &f.doc,
                plane: &f.scene.planes[0],
                table: &f.table,
                constraints: &f.constraints,
                layers: &f.layers,
                groups: &f.groups,
                scan_per_mm: &scale,
                residuals: Some(&Stub),
            },
            head,
        )
    };
    let d = with(PinHead::Node(id));
    assert_eq!(
        values(&d, "Geometry", "difference"),
        ["+0.0082 in = +6.1 px"]
    );
    assert_eq!(values(&d, "Geometry", "pull"), ["0.3100"]);
    assert!(values(&d, "Geometry", "residual").is_empty());
    assert!(d.section("Constraints").unwrap().rows[0]
        .value
        .contains("residual: ink residual 6.10 px"));
    // An entity gets its departure numbers and what moved it.
    let e = with(PinHead::Node(
        f.table.id("entity", "part/e931c6c955cd1.7").unwrap(),
    ));
    assert_eq!(values(&e, "Evidence", "departure rms"), ["2.50 px"]);
    assert_eq!(values(&e, "Evidence", "moved most by"), ["D3"]);
    // Without them the same rows say so.
    let none = f.detail("dimension", "D3");
    assert_eq!(values(&none, "Geometry", "residual"), [NOT_RECORDED]);
}

#[test]
fn the_documents_stored_residuals_fill_the_rows_and_unlisted_ids_say_not_recorded() {
    let f = Fixture::new();
    let stored = f.doc.residuals().expect("the fixture carries residuals");
    let adapter = lmsheets_inspector::detail::DocResiduals(stored);
    let scale = |_: (f64, f64)| 7.87;
    let with = |kind: &str, name: &str| {
        detail(
            &Source {
                doc: &f.doc,
                plane: &f.scene.planes[0],
                table: &f.table,
                constraints: &f.constraints,
                layers: &f.layers,
                groups: &f.groups,
                scan_per_mm: &scale,
                residuals: Some(&adapter),
            },
            PinHead::Node(f.table.id(kind, name).unwrap()),
        )
    };
    let d = with("dimension", "D3");
    let diff = values(&d, "Geometry", "difference");
    assert_eq!(diff.len(), 1, "{diff:?}");
    assert!(diff[0].contains(" in = ") && diff[0].ends_with(" px"));
    assert!(!values(&d, "Geometry", "pull").is_empty());
    assert!(values(&d, "Geometry", "scale sample")[0].contains(":1"));
    // An entity the solve measured gets numbers, not the note's reading.
    let listed = "e931c6c955cd1.7";
    assert!(stored.entities.keys().any(|k| k.as_str() == listed));
    let e = with("entity", &format!("part/{listed}"));
    assert!(!values(&e, "Evidence", "departure rms")[0].contains("note"));
}

#[test]
fn an_observed_rounding_shows_its_record_and_the_two_entities_it_rounds() {
    use lmsheets_inspector::{Layer, Role};
    let f = Fixture::new();
    let name = "rounding_E45.1_E45.2";
    let node = f.table.id("observed", name).unwrap();
    // Its own colour, not the part's.
    assert_eq!(f.layers.of_node(node), Some(Layer::Observed));
    assert_eq!(Role::of_layer(Layer::Observed), Role::Observed);
    assert_ne!(Role::Observed, Role::Entity);
    // A link to the two entities of its junction, with where it joins them.
    let only = [node];
    let links: Vec<_> = f.constraints.links_of(&only).collect();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].to.len(), 2);
    assert_eq!(links[0].targets.len(), 2);
    assert!(
        f.constraints
            .items()
            .iter()
            .all(|c| !c.binds.contains(&node)),
        "no constraint binds it"
    );
    // The drawer: the status, the fit, the two entities as links, the note.
    let d = f.detail("observed", name);
    assert_eq!(
        values(&d, "Geometry", "status"),
        ["seen, not intended: not in the solve"]
    );
    assert_eq!(values(&d, "Geometry", "kind"), ["rounding"]);
    assert!(!values(&d, "Geometry", "fitted centre").is_empty());
    assert!(!values(&d, "Geometry", "fit loss if ignored").is_empty());
    let rel = d.section("Relates to").expect("the junction's entities");
    assert_eq!(rel.rows.len(), 2);
    assert!(rel
        .rows
        .iter()
        .all(|r| r.pin.is_some() && r.key == "rounds the junction of"));
    assert!(values(&d, "Evidence", "reason")[0].contains("seen, not intended"));
    // A centre line lists the centres it runs through.
    let cl = f.detail("centre_line", "CL1");
    assert!(cl.section("Relates to").is_some());
}
