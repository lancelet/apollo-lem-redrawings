//! The hover pick: which node of the drawing is under the pointer, in
//! either pane. Pure and deterministic over already-loaded data (no I/O, no
//! clocks); the shell runs it and passes the result in [`Msg::Hover`].
//!
//! - **Scan pane, zoomed in** (`zoom >= OWNER_ZOOM`, screen px per crop px):
//!   cursor -> crop px -> owner tile -> pixel value -> node, through
//!   `owners.json`. Ink is a pixel or two wide, so the nearest owned pixel
//!   within [`PICK_PX`] screen px wins (ties: the lowest row, then column).
//!   *Known limit:* where several nodes claim a pixel the map keeps the
//!   lowest node index; the others cannot be picked there.
//! - **Scan pane, zoomed out, or the tile not loaded yet:** the cursor goes
//!   through the chain (crop px -> paper mm) to the redraw hit-test. Owner
//!   maps are fetched only when zoomed in (docs/redraw-plan.md §4), and the
//!   threshold [`OWNER_ZOOM`] is where the pyramid first serves full
//!   resolution.
//! - **Redraw pane:** [`hit_test`] over the styled primitives, each of which
//!   names its document item; [`node_key`] (in `lmsheets-render`) turns that
//!   into the owner map's `(kind, name)` and [`OwnerTable`] into a [`NodeId`].
//!
//! **One namespace.** Both panes speak [`NodeId`], an index into the bundle's
//! node table. `node_key` is the one definition, shared with blobsketch's coverage
//! check that wrote the table; the 17619 test checks that every node the
//! redraw draws is in the table.

use crate::chain::Chain;
use crate::layers::Layers;
use crate::model::{Model, NodeId, Pane};
use lmsheets_document::state::document::Document;
use lmsheets_document::value::geometry::{Paper, P2};
use lmsheets_render::nodes::node_key;
use lmsheets_render::resolve::Styled;
use lmsheets_render::scene::{Fill, Path, Plane, Primitive};
use serde_json::Value;
use std::collections::BTreeMap;

/// Below this scan zoom (screen px per crop px) the owner map is not used.
/// It is the zoom at which the pyramid starts serving full resolution
/// (`Pyramid::pick_level` returns level 0 above 0.5), so owner tiles are
/// fetched exactly when full-resolution scan tiles are.
pub const OWNER_ZOOM: f64 = 0.5;

/// How far from the pointer a line is still picked, screen px.
pub const PICK_PX: f64 = 4.0;

/// The least width of a lit member's line, screen px, at any zoom: the
/// highlight is always visible, zoomed out to the whole sheet too.
pub const LIT_MIN_PX: f32 = 3.0;

/// The same for a node that serves the group without belonging to it.
pub const SERVING_MIN_PX: f32 = 2.0;

/// The longest a piece of an outline is, paper mm: a straight paper line
/// through the scan's warp is cut this fine so it follows the drafter's
/// real distortion.
const OUTLINE_STEP_MM: f64 = 2.0;

/// The most pieces one curve of an outline is cut into.
const OUTLINE_MAX_PIECES: usize = 256;

/// The most crop px the owner search looks from the pointer.
const MAX_SEARCH_CROP_PX: f64 = 16.0;

/// The bundle's node table (`owners/owners.json`): node `i` is
/// `(kind, name)`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OwnerTable {
    nodes: Vec<(String, String)>,
    ids: BTreeMap<(String, String), NodeId>,
}

impl OwnerTable {
    /// Read `owners.json`.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("owners.json: {e}"))?;
        let list = v["nodes"].as_array().ok_or("owners.json: no nodes")?;
        let mut t = Self::default();
        for (i, n) in list.iter().enumerate() {
            let (Some(kind), Some(name)) = (n["kind"].as_str(), n["name"].as_str()) else {
                return Err(format!("owners.json: node {i} has no kind or name"));
            };
            t.nodes.push((kind.into(), name.into()));
            t.ids
                .entry((kind.into(), name.into()))
                .or_insert(NodeId(i as u32));
        }
        Ok(t)
    }

    /// A table of these `(kind, name)` nodes, in order.
    pub fn from_keys(keys: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut t = Self::default();
        for (i, k) in keys.into_iter().enumerate() {
            t.ids.entry(k.clone()).or_insert(NodeId(i as u32));
            t.nodes.push(k);
        }
        t
    }

    /// How many nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// A node's `(kind, name)`.
    pub fn get(&self, id: NodeId) -> Option<(&str, &str)> {
        self.nodes
            .get(id.0 as usize)
            .map(|(k, n)| (k.as_str(), n.as_str()))
    }

    /// The node with this kind and name.
    pub fn id(&self, kind: &str, name: &str) -> Option<NodeId> {
        self.ids.get(&(kind.to_string(), name.to_string())).copied()
    }
}

/// One decoded owner tile: pixel values (0 paper or unclaimed, `v` is
/// `NodeId(v - 1)`), row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnerTile {
    /// Width, px.
    pub width: u32,
    /// Height, px.
    pub height: u32,
    /// The values.
    pub values: Vec<u16>,
}

impl OwnerTile {
    /// Decode a 16-bit grey PNG (the bundle's owner tile; big-endian
    /// samples). Browsers decode 16-bit PNGs to 8 bits, so this is the
    /// `png` crate's job on every platform.
    pub fn from_png(bytes: &[u8]) -> Result<Self, String> {
        let mut r = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .map_err(|e| format!("owner tile: {e}"))?;
        let mut buf = vec![0u8; r.output_buffer_size().ok_or("owner tile: too large")?];
        let info = r
            .next_frame(&mut buf)
            .map_err(|e| format!("owner tile: {e}"))?;
        if info.color_type != png::ColorType::Grayscale || info.bit_depth != png::BitDepth::Sixteen
        {
            return Err("owner tile: not 16-bit grey".into());
        }
        let values = buf[..info.buffer_size()]
            .chunks_exact(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .collect();
        Ok(Self {
            width: info.width,
            height: info.height,
            values,
        })
    }
}

/// The owner tiles held so far, by tile column and row.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OwnerMap {
    tile: u32,
    tiles: BTreeMap<(u32, u32), OwnerTile>,
}

impl OwnerMap {
    /// An empty map of tiles `tile` px on a side.
    pub fn new(tile: u32) -> Self {
        Self {
            tile: tile.max(1),
            tiles: BTreeMap::new(),
        }
    }

    /// The tile edge, px.
    pub fn tile(&self) -> u32 {
        self.tile
    }

    /// Hold a tile.
    pub fn insert(&mut self, x: u32, y: u32, tile: OwnerTile) {
        self.tiles.insert((x, y), tile);
    }

    /// Whether a tile is held.
    pub fn has(&self, x: u32, y: u32) -> bool {
        self.tiles.contains_key(&(x, y))
    }

    /// How many tiles are held.
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// The value at a crop pixel: none if its tile is not held or the pixel
    /// is outside the crop.
    pub fn value_at(&self, x: i64, y: i64) -> Option<u16> {
        if x < 0 || y < 0 {
            return None;
        }
        let t = self.tile as i64;
        let tile = self.tiles.get(&((x / t) as u32, (y / t) as u32))?;
        let (lx, ly) = ((x % t) as u32, (y % t) as u32);
        if lx >= tile.width || ly >= tile.height {
            return None;
        }
        tile.values.get((ly * tile.width + lx) as usize).copied()
    }

    /// The node of the owned pixel nearest a crop point, within `radius`
    /// crop px; ties go to the lowest row, then column. `None` if no pixel
    /// in reach is owned.
    pub fn nearest(&self, crop: (f64, f64), radius: f64) -> Option<NodeId> {
        self.nearest_where(crop, radius, |_| true)
    }

    /// [`OwnerMap::nearest`] over the nodes `keep` accepts (the visible
    /// ones: a hidden node's pixels are not targets, and the next owned
    /// pixel in reach is).
    pub fn nearest_where(
        &self,
        crop: (f64, f64),
        radius: f64,
        keep: impl Fn(NodeId) -> bool,
    ) -> Option<NodeId> {
        let (cx, cy) = (crop.0.floor() as i64, crop.1.floor() as i64);
        let r = radius.ceil() as i64;
        let mut best: Option<(f64, i64, i64, u16)> = None;
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                let Some(v) = self
                    .value_at(x, y)
                    .filter(|&v| v > 0 && keep(NodeId(u32::from(v) - 1)))
                else {
                    continue;
                };
                // Distance from the point to the pixel's centre.
                let d =
                    ((x as f64 + 0.5 - crop.0).powi(2) + (y as f64 + 0.5 - crop.1).powi(2)).sqrt();
                if d > radius.max(0.5) {
                    continue;
                }
                if best.is_none_or(|(bd, by, bx, _)| (d, y, x) < (bd, by, bx)) {
                    best = Some((d, y, x, v));
                }
            }
        }
        best.map(|(.., v)| NodeId(u32::from(v) - 1))
    }
}

/// A styled primitive with the node it draws.
#[derive(Clone, Debug, PartialEq)]
pub struct HitItem {
    /// What is drawn.
    pub primitive: Primitive,
    /// Its node, if the owner table has it.
    pub node: Option<NodeId>,
}

/// How far a paper point is from a primitive's ink, mm (0 on or in it).
fn distance(primitive: &Primitive, p: P2<Paper>) -> f64 {
    let len = |a: P2<Paper>, b: P2<Paper>| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    match primitive {
        Primitive::Stroke { path, weight, .. } => (len(path.nearest(p), p) - weight / 2.0).max(0.0),
        Primitive::Marker { at, marker, .. } => (len(*at, p) - marker.size / 2.0).max(0.0),
        Primitive::Fill { fill, outline } => {
            let half = outline.unwrap_or(0.0) / 2.0;
            match fill {
                Fill::Disc { centre, radius } => (len(*centre, p) - radius - half).max(0.0),
                Fill::Polygon { points } => {
                    let n = points.len();
                    let mut inside = false;
                    let mut edge = f64::MAX;
                    for i in 0..n {
                        let (a, b) = (points[i], points[(i + 1) % n]);
                        if (a.y > p.y) != (b.y > p.y)
                            && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x
                        {
                            inside = !inside;
                        }
                        edge = edge.min(segment_distance(a, b, p));
                    }
                    if inside {
                        0.0
                    } else {
                        (edge - half).max(0.0)
                    }
                }
            }
        }
    }
}

fn segment_distance(a: P2<Paper>, b: P2<Paper>, p: P2<Paper>) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a.x + t * dx - p.x).powi(2) + (a.y + t * dy - p.y).powi(2)).sqrt()
}

/// The item nearest a paper point within `tol` mm of its ink: the least
/// distance wins, and among equals the one drawn last (on top).
pub fn hit_test(items: &[HitItem], at: (f64, f64), tol: f64) -> Option<usize> {
    hit_test_where(items, at, tol, |_| true)
}

/// [`hit_test`] over the items `keep` accepts (by index).
pub fn hit_test_where(
    items: &[HitItem],
    at: (f64, f64),
    tol: f64,
    keep: impl Fn(usize) -> bool,
) -> Option<usize> {
    let p = P2::<Paper>::new(at.0, at.1);
    let mut best: Option<(f64, usize)> = None;
    for (i, item) in items.iter().enumerate() {
        let Some(b) = item.primitive.ink_bounds() else {
            continue;
        };
        if !b.grow(tol).contains(p) || !keep(i) {
            continue;
        }
        let d = distance(&item.primitive, p);
        if d <= tol && best.is_none_or(|(bd, _)| d <= bd) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| i)
}

/// Everything the pick reads: the node table, the owner tiles held, and the
/// redraw's primitives with their nodes. The shell owns one and feeds the
/// owner tiles in as they load.
#[derive(Clone, Debug, Default)]
pub struct Picker {
    /// The bundle's node table.
    pub table: OwnerTable,
    /// The owner tiles held.
    pub owners: OwnerMap,
    items: Vec<HitItem>,
    layers: Layers,
}

impl Picker {
    /// A picker over the styled redraw (`styled.items` in order, so item
    /// indices are the renderer's) of `doc`, with `plane` the scene's plane
    /// (witness lines name their users there).
    pub fn new(
        table: OwnerTable,
        tile: u32,
        plane: &Plane,
        doc: &Document,
        styled: &Styled,
    ) -> Self {
        let items = styled
            .items
            .iter()
            .map(|it| {
                let (kind, name) = node_key(plane, doc, it.source);
                HitItem {
                    primitive: it.primitive.clone(),
                    node: table.id(&kind, &name),
                }
            })
            .collect();
        let layers = Layers::new(doc, plane, &styled.items, &table);
        Self {
            table,
            owners: OwnerMap::new(tile),
            items,
            layers,
        }
    }

    /// A picker over ready hit items (tests, and shells with their own).
    pub fn from_items(table: OwnerTable, tile: u32, items: Vec<HitItem>) -> Self {
        Self {
            table,
            owners: OwnerMap::new(tile),
            items,
            layers: Layers::default(),
        }
    }

    /// Set the layer classification the pick and the highlight respect.
    pub fn set_layers(&mut self, layers: Layers) {
        self.layers = layers;
    }

    /// The classification of the drawing's parts.
    pub fn layers(&self) -> &Layers {
        &self.layers
    }

    /// Whether the model shows styled item `i`.
    pub fn item_visible(&self, m: &Model, i: usize) -> bool {
        self.layers.of_item(i).is_none_or(|l| m.layer_visible(l))
    }

    /// Whether the model shows a node (a node in no layer always is).
    pub fn node_visible(&self, m: &Model, n: NodeId) -> bool {
        self.layers.of_node(n).is_none_or(|l| m.layer_visible(l))
    }

    /// The redraw's items with their nodes.
    pub fn items(&self) -> &[HitItem] {
        &self.items
    }

    /// The styled-item indices that draw a node.
    pub fn items_of(&self, node: NodeId) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.items[i].node == Some(node))
            .collect()
    }

    /// The styled-item indices that draw any of `nodes`, ascending.
    pub fn items_in(&self, nodes: &[NodeId]) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.items[i].node.is_some_and(|n| nodes.contains(&n)))
            .collect()
    }

    /// The redraw geometry of `nodes` carried through the chain onto the
    /// scan, as straight pieces in crop px: how the scan pane shows a
    /// hovered group where the owner map is out of reach (zoomed out, or
    /// its tiles not held). Lines are cut into pieces of at most
    /// [`OUTLINE_STEP_MM`] and curves finely enough to follow the warp.
    /// Drawn as a thick outline, it stays visible at any zoom; it is the
    /// registration's image of the vectors, not the ink's own pixels.
    pub fn outline(&self, nodes: &[NodeId], chain: &Chain) -> Vec<[(f64, f64); 2]> {
        let mut out = Vec::new();
        let mut polyline = |pts: &[(f64, f64)], closed: bool| {
            let n = pts.len();
            let end = if closed { n } else { n.saturating_sub(1) };
            for i in 0..end {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                let len = (b.0 - a.0).hypot(b.1 - a.1);
                let k = ((len / OUTLINE_STEP_MM).ceil() as usize).clamp(1, OUTLINE_MAX_PIECES);
                let at =
                    |t: f64| chain.paper_to_scan((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
                for j in 0..k {
                    out.push([at(j as f64 / k as f64), at((j + 1) as f64 / k as f64)]);
                }
            }
        };
        for item in self.items_in(nodes).into_iter().map(|i| &self.items[i]) {
            match &item.primitive {
                Primitive::Stroke { path, .. } => {
                    let pieces = match path {
                        Path::Line { .. } => 1,
                        p => ((p.length() / OUTLINE_STEP_MM).ceil() as usize)
                            .clamp(8, OUTLINE_MAX_PIECES),
                    };
                    let pts: Vec<(f64, f64)> = (0..=pieces)
                        .map(|i| {
                            let q = path.at(i as f64 / pieces as f64);
                            (q.x, q.y)
                        })
                        .collect();
                    polyline(&pts, false);
                }
                Primitive::Marker { at, marker, .. } => {
                    let h = marker.size / 2.0;
                    polyline(&[(at.x - h, at.y), (at.x + h, at.y)], false);
                    polyline(&[(at.x, at.y - h), (at.x, at.y + h)], false);
                }
                Primitive::Fill { fill, .. } => match fill {
                    Fill::Polygon { points } => {
                        let pts: Vec<(f64, f64)> = points.iter().map(|p| (p.x, p.y)).collect();
                        polyline(&pts, true);
                    }
                    Fill::Disc { centre, radius } => {
                        let pts: Vec<(f64, f64)> = (0..16)
                            .map(|i| {
                                let a = std::f64::consts::TAU * i as f64 / 16.0;
                                (centre.x + radius * a.cos(), centre.y + radius * a.sin())
                            })
                            .collect();
                        polyline(&pts, true);
                    }
                },
            }
        }
        out
    }

    /// The node under pane pixel `at` of `pane` in the model's current
    /// cameras.
    pub fn pick(&self, m: &Model, pane: Pane, at: (f64, f64)) -> Option<NodeId> {
        match pane {
            Pane::Redraw => {
                let paper = m.paper_at(at)?;
                let i = hit_test_where(&self.items, paper, PICK_PX / m.scale(), |i| {
                    self.item_visible(m, i)
                })?;
                self.items[i].node
            }
            Pane::Scan => {
                let cam = m.scan_camera()?;
                let crop = m.crop_at(at)?;
                if cam.zoom >= OWNER_ZOOM {
                    let (x, y) = (crop.0.floor() as i64, crop.1.floor() as i64);
                    // Authoritative once the pointer's tile is held.
                    if self.owners.value_at(x, y).is_some() {
                        let r = (PICK_PX / cam.zoom).min(MAX_SEARCH_CROP_PX);
                        return self
                            .owners
                            .nearest_where(crop, r, |n| self.node_visible(m, n));
                    }
                }
                let paper = m.chain().scan_to_paper(crop);
                let per_mm = Chain::scan_per_mm(m.chain(), paper);
                let tol = PICK_PX / cam.zoom / per_mm;
                let i = hit_test_where(&self.items, paper, tol, |i| self.item_visible(m, i))?;
                self.items[i].node
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmsheets_document::value::style::Dash;
    use lmsheets_render::scene::Path;

    fn line(a: (f64, f64), b: (f64, f64), weight: f64, node: u32) -> HitItem {
        HitItem {
            primitive: Primitive::Stroke {
                path: Path::Line {
                    a: P2::new(a.0, a.1),
                    b: P2::new(b.0, b.1),
                },
                weight,
                dash: Dash::hidden(),
                anchors: vec![],
            },
            node: Some(NodeId(node)),
        }
    }

    #[test]
    fn the_nearest_line_within_tolerance_is_hit_and_ties_go_to_the_top() {
        let items = [
            line((0.0, 0.0), (10.0, 0.0), 0.2, 0),
            line((0.0, 1.0), (10.0, 1.0), 0.2, 1),
            line((5.0, -5.0), (5.0, 5.0), 0.2, 2),
        ];
        assert_eq!(hit_test(&items, (2.0, 0.1), 0.3), Some(0));
        assert_eq!(hit_test(&items, (2.0, 0.8), 0.3), Some(1));
        assert_eq!(hit_test(&items, (2.0, 0.5), 0.3), None);
        // The crossing at (5, 0): lines 0 and 2 both at distance 0; the
        // later (drawn on top) wins.
        assert_eq!(hit_test(&items, (5.0, 0.0), 0.3), Some(2));
        // A thick line is hit across its width.
        let thick = [line((0.0, 0.0), (10.0, 0.0), 2.0, 0)];
        assert_eq!(hit_test(&thick, (3.0, 0.9), 0.0), Some(0));
        assert_eq!(hit_test(&thick, (3.0, 1.1), 0.0), None);
    }

    #[test]
    fn a_polygon_is_hit_inside_and_a_disc_in_its_radius() {
        let tri = HitItem {
            primitive: Primitive::Fill {
                fill: Fill::Polygon {
                    points: vec![P2::new(0.0, 0.0), P2::new(4.0, 0.0), P2::new(0.0, 4.0)],
                },
                outline: None,
            },
            node: Some(NodeId(7)),
        };
        assert_eq!(
            hit_test(std::slice::from_ref(&tri), (1.0, 1.0), 0.0),
            Some(0)
        );
        assert_eq!(hit_test(std::slice::from_ref(&tri), (3.0, 3.0), 0.0), None);
        assert_eq!(hit_test(&[tri], (3.0, 3.0), 1.5), Some(0));
        let dot = HitItem {
            primitive: Primitive::Fill {
                fill: Fill::Disc {
                    centre: P2::new(1.0, 1.0),
                    radius: 0.5,
                },
                outline: None,
            },
            node: None,
        };
        assert_eq!(
            hit_test(std::slice::from_ref(&dot), (1.4, 1.0), 0.0),
            Some(0)
        );
        assert_eq!(hit_test(&[dot], (1.6, 1.0), 0.0), None);
    }

    fn map() -> OwnerMap {
        // Tiles of 4 px; tile (0, 0) has node 3 at (1, 1) and node 5 at
        // (3, 3); tile (1, 0) has node 0 at its (0, 1), i.e. crop (4, 1).
        let mut m = OwnerMap::new(4);
        let mut a = vec![0u16; 16];
        a[4 + 1] = 4;
        a[3 * 4 + 3] = 6;
        let mut b = vec![0u16; 16];
        b[4] = 1;
        for (x, v) in [(0, a), (1, b)] {
            m.insert(
                x,
                0,
                OwnerTile {
                    width: 4,
                    height: 4,
                    values: v,
                },
            );
        }
        m
    }

    #[test]
    fn the_owner_lookup_reads_known_pixels_and_searches_nearby() {
        let m = map();
        assert_eq!(m.value_at(1, 1), Some(4));
        assert_eq!(m.value_at(4, 1), Some(1));
        assert_eq!(m.value_at(0, 0), Some(0));
        assert_eq!(m.value_at(0, 4), None, "tile not held");
        assert_eq!(m.value_at(-1, 0), None);
        // On the pixel; beside it; across the tile border.
        assert_eq!(m.nearest((1.5, 1.5), 0.5), Some(NodeId(3)));
        assert_eq!(m.nearest((2.4, 1.5), 1.5), Some(NodeId(3)));
        assert_eq!(m.nearest((3.9, 1.5), 1.0), Some(NodeId(0)));
        assert_eq!(m.nearest((2.0, 2.0), 0.6), None);
        // Equidistant (3, 3)-ish ties break by row then column; here the
        // nearer one wins.
        assert_eq!(m.nearest((3.5, 3.5), 3.0), Some(NodeId(5)));
        // Deterministic.
        assert_eq!(m.nearest((2.5, 2.5), 3.0), m.nearest((2.5, 2.5), 3.0));
    }

    #[test]
    fn the_node_table_reads_owners_json() {
        let t = OwnerTable::from_json(
            r#"{"nodes":[{"kind":"entity","name":"a"},{"kind":"text","name":"t1"}]}"#,
        )
        .unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.id("text", "t1"), Some(NodeId(1)));
        assert_eq!(t.get(NodeId(0)), Some(("entity", "a")));
        assert_eq!(t.id("text", "zz"), None);
        assert!(OwnerTable::from_json("{}").is_err());
    }

    #[test]
    fn an_owner_tile_decodes_from_a_16_bit_png() {
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 3, 2);
            e.set_color(png::ColorType::Grayscale);
            e.set_depth(png::BitDepth::Sixteen);
            let mut w = e.write_header().unwrap();
            let vals: [u16; 6] = [0, 1, 258, 1000, 65534, 7];
            let be: Vec<u8> = vals.iter().flat_map(|v| v.to_be_bytes()).collect();
            w.write_image_data(&be).unwrap();
        }
        let t = OwnerTile::from_png(&bytes).unwrap();
        assert_eq!((t.width, t.height), (3, 2));
        assert_eq!(t.values, [0, 1, 258, 1000, 65534, 7]);
    }
}
