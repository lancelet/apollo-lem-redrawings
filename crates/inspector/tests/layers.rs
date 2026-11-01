//! The layers panel's categories on 17619's stored document (the v4
//! fixture).

use lmsheets_document::state::document::Document;
use lmsheets_document::value::style::Purpose;
use lmsheets_inspector::{Layer, Layers, OwnerTable};
use lmsheets_render::conventions::Conventions;
use lmsheets_render::draw::draw;
use lmsheets_render::nodes::node_key;
use lmsheets_render::resolve::{resolve, Styled};
use lmsheets_render::scene::Scene;
use lmsheets_render::theme::Theme;

struct Fixture {
    doc: Document,
    scene: Scene,
    styled: Styled,
    table: OwnerTable,
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
        Self {
            doc,
            scene,
            styled,
            table,
        }
    }

    fn layers(&self) -> Layers {
        Layers::new(
            &self.doc,
            &self.scene.planes[0],
            &self.styled.items,
            &self.table,
        )
    }
}

#[test]
fn every_item_has_a_layer_and_the_counts_are_the_documents() {
    let f = Fixture::new();
    let layers = f.layers();
    for i in 0..f.styled.items.len() {
        assert!(layers.of_item(i).is_some(), "item {i} has no layer");
    }
    // 17619: six genuine dimensions (D1-D5, R1), two inferred, three
    // centre lines (and their marks), one marking region, two texts.
    // Nodes, not primitives: 17619's six genuine dimensions (D1-D5, R1)
    // and their witness lines, two inferred dimensions with theirs, three
    // centre lines with their marks, one marking region, two texts. (Lower
    // bounds where the sketch's markup may still gain or merge lines.)
    assert!(layers.count(Layer::Dimensions) >= 6);
    assert!(layers.count(Layer::Inferred) >= 2);
    assert!(layers.count(Layer::Centre) >= 3);
    assert!(layers.count(Layer::Part) >= 8);
    assert!(layers.count(Layer::Form) >= 40);
    assert_eq!(layers.count(Layer::Marking), 1);
    assert_eq!(layers.count(Layer::Texts), 2);
    assert_eq!(layers.count(Layer::Observed), 4);
    // A node's layer is its items' layer.
    for (i, item) in f.scene.planes[0].items.iter().enumerate() {
        let (k, n) = node_key(&f.scene.planes[0], &f.doc, item.source);
        let id = f.table.id(&k, &n).unwrap();
        assert_eq!(layers.of_node(id), layers.of_item(i), "{k} {n}");
    }
    let named = |kind: &str, name: &str| {
        layers.of_node(
            f.table
                .id(kind, name)
                .unwrap_or_else(|| panic!("{kind} {name}")),
        )
    };
    assert_eq!(named("dimension", "D1"), Some(Layer::Dimensions));
    assert_eq!(named("centre_line", "CL1"), Some(Layer::Centre));
    assert_eq!(
        named("dimension", "inferred_vertical_distance_E68.start_E68.end"),
        Some(Layer::Inferred)
    );
}
