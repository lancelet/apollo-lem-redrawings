//! Selection groups on 17619's stored document (the v4 fixture): the
//! dimension groups, the centre groups and shared witness lines.

use lmsheets_document::state::document::Document;
use lmsheets_document::value::style::Purpose;
use lmsheets_inspector::{Groups, NodeId, OwnerTable};
use lmsheets_render::draw::draw;
use lmsheets_render::nodes::node_key;
use lmsheets_render::scene::Plane;

struct Fixture {
    table: OwnerTable,
    groups: Groups,
}

impl Fixture {
    fn new() -> Self {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../document/tests/fixtures/v4/LDW280-17619-sh3.document.json"
        );
        let doc = Document::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let plane: Plane = draw(&doc, Purpose::Screen);
        let mut keys: Vec<(String, String)> = Vec::new();
        for it in &plane.items {
            let k = node_key(&plane, &doc, it.source);
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        let table = OwnerTable::from_keys(keys);
        let groups = Groups::new(&doc, &plane, &table);
        Self { table, groups }
    }

    fn id(&self, kind: &str, name: &str) -> NodeId {
        self.table
            .id(kind, name)
            .unwrap_or_else(|| panic!("no node {kind} {name}"))
    }

    /// The `kind:name` of each node, sorted.
    fn names(&self, ids: &[NodeId]) -> Vec<String> {
        let mut v: Vec<String> = ids
            .iter()
            .map(|&i| {
                let (k, n) = self.table.get(i).unwrap();
                format!("{k}:{n}")
            })
            .collect();
        v.sort();
        v
    }
}

#[test]
fn a_dimension_takes_its_parts_and_marks_the_centre_line_serving_it() {
    let f = Fixture::new();
    let h = f.groups.highlight(f.id("dimension", "D1"), false);
    let members = f.names(&h.members);
    // The dimension (line, heads, value) and the witness lines it draws,
    // alone or shared with D4.
    assert!(members.contains(&"dimension:D1".to_string()));
    assert!(
        members.contains(&"witness:D1,D4".to_string()),
        "{members:?}"
    );
    assert!(members
        .iter()
        .all(|m| m.starts_with("dimension:") || m.starts_with("witness:")));
    // CL1 is D1's second extension line, not drawn by it: serving.
    assert_eq!(f.names(&h.serving), ["centre_line:CL1"]);
    assert!(h.contains(f.id("centre_line", "CL1")));
    assert!(!h.members.contains(&f.id("centre_line", "CL1")));
    // Alt takes the dimension alone.
    let one = f.groups.highlight(f.id("dimension", "D1"), true);
    assert_eq!(f.names(&one.members), ["dimension:D1"]);
    assert!(one.serving.is_empty());
}

#[test]
fn hovering_a_serving_line_does_not_take_the_dimension() {
    let f = Fixture::new();
    // CL1 takes its own centre's group (with CL4), not D1.
    let h = f.groups.highlight(f.id("centre_line", "CL1"), false);
    assert_eq!(f.names(&h.members), ["centre_line:CL1", "centre_line:CL4"]);
    assert!(h.serving.is_empty());
}

#[test]
fn centre_lines_through_one_centre_group_together() {
    let f = Fixture::new();
    let (cl2, cl3, cl1) = (
        f.id("centre_line", "CL2"),
        f.id("centre_line", "CL3"),
        f.id("centre_line", "CL1"),
    );
    for n in [cl2, cl3] {
        let h = f.groups.highlight(n, false);
        assert!(h.members.contains(&cl2) && h.members.contains(&cl3));
        assert!(!h.members.contains(&cl1));
    }
    // The left end's mark: CL1 and the said vertical CL4, whose dashes lie
    // off the arc's fitted centre (docs/redraw-plan.md §5).
    let cl4 = f.id("centre_line", "CL4");
    let h = f.groups.highlight(cl4, false);
    assert_eq!(f.names(&h.members), ["centre_line:CL1", "centre_line:CL4"]);
}

#[test]
fn a_shared_witness_line_is_in_every_group_that_uses_it() {
    let f = Fixture::new();
    // 17619 draws one witness line for D2 and D5, and one for D1 and D4.
    for (shared, users) in [("D2,D5", ["D2", "D5"]), ("D1,D4", ["D1", "D4"])] {
        let w = f.id("witness", shared);
        assert_eq!(f.groups.groups_of(w).count(), 2, "{shared}");
        let h = f.groups.highlight(w, false);
        for d in users {
            assert!(h.members.contains(&f.id("dimension", d)), "{shared}: {d}");
        }
        // Each user's own group is lit with it.
        for d in users {
            let g = f.groups.highlight(f.id("dimension", d), false);
            assert!(g.members.contains(&w));
            assert!(g.members.iter().all(|m| h.members.contains(m)));
        }
    }
}

#[test]
fn every_dimension_has_a_group_and_the_pieces_are_disjoint_from_serving() {
    let f = Fixture::new();
    for g in f.groups.groups() {
        assert!(g.members.contains(&g.head));
        assert!(g.serving.is_disjoint(&g.members));
    }
    let h = f.groups.highlight(f.id("dimension", "D3"), false);
    assert!(h.members.windows(2).all(|w| w[0] < w[1]), "sorted");
    // A node in no group is its own.
    let lone = f.groups.highlight(f.id("dimension", "R1"), false);
    assert!(lone.members.contains(&f.id("dimension", "R1")));
}

#[test]
fn the_left_extension_line_of_1_3_4_and_39_64_takes_both() {
    // D3 (1 3/4) and D5 (39/64) measure from the same extreme of the left
    // end, one construction, so their extension line is one witness.
    let f = Fixture::new();
    let h = f.groups.highlight(f.id("witness", "D3,D5"), false);
    let members = f.names(&h.members);
    assert!(members.contains(&"dimension:D3".to_string()), "{members:?}");
    assert!(members.contains(&"dimension:D5".to_string()), "{members:?}");
}
