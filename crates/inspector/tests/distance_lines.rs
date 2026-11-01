//! A distance to a line, or between lines, is held by a segment square to
//! the line(s), revealed beside its icon like a distance between points.

use lmsheets_document::examples::dimensions;
use lmsheets_document::value::style::Purpose;
use lmsheets_inspector::{Constraints, OwnerTable};
use lmsheets_render::draw::draw;
use lmsheets_render::nodes::node_key;
use lmsheets_render::scene::Scene;

#[test]
fn a_distance_to_lines_is_a_segment_square_to_them() {
    let doc = dimensions();
    let scene = Scene::of(draw(&doc, Purpose::Screen));
    let mut keys: Vec<(String, String)> = Vec::new();
    for it in &scene.planes[0].items {
        let k = node_key(&scene.planes[0], &doc, it.source);
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    let table = OwnerTable::from_keys(keys);
    let constraints = Constraints::new(&doc, &scene.planes[0], &table);
    let segment = |id: &str| {
        constraints
            .items()
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("no constraint {id}"))
            .relation_line
            .unwrap_or_else(|| panic!("{id} has no relation line"))
    };
    // "h": the plate's level bottom and top edges, 30 model mm = 75 paper
    // mm apart: a plumb segment of that length.
    let ((ax, ay), (bx, by)) = segment("h");
    assert!((ax - bx).abs() < 1e-6, "plumb: {ax} {bx}");
    assert!(((ay - by).abs() - 75.0).abs() < 1e-6, "{ay} {by}");
    // "hr": the hole's centre to the right edge (plumb), 40 model mm =
    // 100 paper mm: a level segment of that length.
    let ((ax, ay), (bx, by)) = segment("hr");
    assert!((ay - by).abs() < 1e-6, "level: {ay} {by}");
    assert!(((ax - bx).abs() - 100.0).abs() < 1e-6, "{ax} {bx}");
}
