//! Chunk 20's pick tests against 17619's real bundle (`--ignored`,
//! `LMSHEETS_BUNDLE_17619=<bundle>/LDW280-17619/sh3`): the owner lookup at
//! known pixels, the node namespace shared by the owner map and the redraw,
//! and how often the two panes' picks agree.

use lmsheets_document::state::document::Document;
use lmsheets_document::value::style::Purpose;
use lmsheets_inspector::pick::hit_test;
use lmsheets_inspector::{Chain, NodeId, OwnerTable, OwnerTile, Picker};
use lmsheets_render::conventions::Conventions;
use lmsheets_render::draw::draw;
use lmsheets_render::resolve::resolve;
use lmsheets_render::scene::Scene;
use lmsheets_render::theme::Theme;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn bundle_dir() -> PathBuf {
    std::env::var_os("LMSHEETS_BUNDLE_17619")
        .map(PathBuf::from)
        .expect("set LMSHEETS_BUNDLE_17619 to <bundle>/LDW280-17619/sh3")
}

fn picker(dir: &std::path::Path) -> Picker {
    let table =
        OwnerTable::from_json(&std::fs::read_to_string(dir.join("owners/owners.json")).unwrap())
            .unwrap();
    let doc =
        Document::parse(&std::fs::read_to_string(dir.join("document.json")).unwrap()).unwrap();
    let plane = draw(&doc, Purpose::Screen);
    let scene = Scene::of(plane);
    let styled = resolve(&scene, &doc, &Theme::screen(), Conventions::default());
    Picker::new(table, 1024, &scene.planes[0], &doc, &styled)
}

fn load_owners(p: &mut Picker, dir: &std::path::Path) {
    for y in 0..5 {
        for x in 0..7 {
            let bytes = std::fs::read(dir.join(format!("owners/{x}_{y}.png"))).unwrap();
            p.owners.insert(x, y, OwnerTile::from_png(&bytes).unwrap());
        }
    }
}

/// Every node the redraw draws is in the owner map's table (one namespace),
/// and the table's other nodes are the marks that are not the drawing's, plus
/// the claims' witness D3,D5 which the redraw (one witness per dimension)
/// does not draw.
#[test]
#[ignore = "needs the 17619 bundle (LMSHEETS_BUNDLE_17619)"]
fn the_redraw_and_the_owner_map_share_one_node_namespace() {
    let p = picker(&bundle_dir());
    let missing: Vec<usize> = (0..p.items().len())
        .filter(|&i| p.items()[i].node.is_none())
        .collect();
    let drawn: BTreeSet<NodeId> = p.items().iter().filter_map(|i| i.node).collect();
    println!(
        "{} styled items, {} without a node, {} distinct nodes drawn of {} in the table",
        p.items().len(),
        missing.len(),
        drawn.len(),
        p.table.len()
    );
    let undrawn: BTreeSet<&str> = (0..p.table.len() as u32)
        .filter(|i| !drawn.contains(&NodeId(*i)))
        .map(|i| p.table.get(NodeId(i)).unwrap().0)
        .collect();
    println!("kinds in the table not drawn: {undrawn:?}");
    for i in 0..p.table.len() as u32 {
        if !drawn.contains(&NodeId(i)) {
            println!("  undrawn {:?}", p.table.get(NodeId(i)).unwrap());
        }
    }
    let doc =
        Document::parse(&std::fs::read_to_string(bundle_dir().join("document.json")).unwrap())
            .unwrap();
    let scene = Scene::of(draw(&doc, Purpose::Screen));
    let styled = resolve(&scene, &doc, &Theme::screen(), Conventions::default());
    let keys: BTreeSet<(String, String)> = missing
        .iter()
        .map(|&i| lmsheets_render::nodes::node_key(&scene.planes[0], &doc, styled.items[i].source))
        .collect();
    println!("keys with no table node: {keys:?}");
    let table_witnesses: Vec<_> = (0..p.table.len() as u32)
        .filter_map(|i| p.table.get(NodeId(i)))
        .filter(|(k, _)| *k == "witness")
        .collect();
    println!("table witnesses: {table_witnesses:?}");
    // The bundle writer builds the table from the exported document it also
    // writes as document.json (chunk 20b), so every drawn item has its node.
    // No item is without one: even the inferred dimensions, their witnesses
    // are in the table, appended after the coverage check's nodes.
    assert!(
        keys.is_empty(),
        "{} items name no table node: {keys:?}",
        missing.len()
    );
    assert!(missing.is_empty());
}

/// Known pixels of the real owner map, and the owner map against the redraw
/// geometry through the chain.
#[test]
#[ignore = "needs the 17619 bundle (LMSHEETS_BUNDLE_17619)"]
fn the_owner_map_and_the_redraw_pick_agree_on_ink() {
    let dir = bundle_dir();
    let mut p = picker(&dir);
    load_owners(&mut p, &dir);
    let chain = Chain::load(&dir).unwrap();
    // Walk the map on a stride, every owned pixel found is checked against
    // the redraw's hit-test at its paper point.
    let (mut owned, mut agree, mut redraw_none, mut other) = (0, 0, 0, 0);
    let mut shown = 0;
    let mut first: Option<(i64, i64, NodeId)> = None;
    for y in (0..4364).step_by(7) {
        for x in (0..6623).step_by(7) {
            let Some(v) = p.owners.value_at(x, y).filter(|&v| v > 0) else {
                continue;
            };
            let node = NodeId(u32::from(v) - 1);
            first.get_or_insert((x, y, node));
            owned += 1;
            let paper = chain.scan_to_paper((x as f64 + 0.5, y as f64 + 0.5));
            match hit_test(p.items(), paper, 1.5 * 0.2) {
                Some(i) if p.items()[i].node == Some(node) => {
                    agree += 1;
                    if shown < 4
                        && p.table.get(node).is_some_and(|(k, _)| k == "entity")
                        && x > 3000
                    {
                        shown += 1;
                        println!(
                            "example: crop ({x}, {y}) paper ({:.2}, {:.2}) {:?}",
                            paper.0,
                            paper.1,
                            p.table.get(node)
                        );
                    }
                }
                Some(_) => other += 1,
                None => redraw_none += 1,
            }
        }
    }
    println!(
        "{owned} owned sample pixels: {agree} agree with the redraw's pick, {other} pick another \
         node, {redraw_none} have no redraw primitive near"
    );
    // A known pixel of the fixture: ink outside the drawing, top left.
    let known = p.owners.nearest((3290.5, 84.5), 0.5).unwrap();
    assert_eq!(p.table.get(known), Some(("foreign", "outside the drawing")));
    let (x, y, node) = first.unwrap();
    println!("first owned sample: ({x}, {y}) = {:?}", p.table.get(node));
    assert!(owned > 1000);
    assert!(
        agree * 10 >= owned * 7,
        "agreement {agree}/{owned} is under 70 %"
    );
}
