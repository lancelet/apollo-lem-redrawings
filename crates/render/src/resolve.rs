//! Apply a theme once: the styled scene, with concrete colours and dash
//! patterns fitted to each curve, is what every writer takes.

use crate::conventions::{Conventions, DashFit};
use crate::dashes::{self, grid_of};
use crate::scene::{Primitive, Scene, Source, Termination};
use crate::theme::Theme;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::sketch::LayerId;
use lmsheets_document::value::style::Rgba;

pub use crate::dashes::fit_dash;

/// A primitive with its colour and its dash as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct StyledItem {
    /// What is drawn.
    pub primitive: Primitive,
    /// The dashes and gaps as drawn, mm, dash first, laid from the
    /// curve's start; empty if solid.
    pub dashes: Vec<f64>,
    /// The colour.
    pub colour: Rgba,
    /// Its layer.
    pub layer: LayerId,
    /// The document item it draws.
    pub source: Source,
}

/// The scene with the theme and the conventions applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Styled {
    /// The background.
    pub paper: Rgba,
    /// How to cap and join strokes.
    pub conventions: Conventions,
    /// The items, in draw order.
    pub items: Vec<StyledItem>,
    /// Which stroke each terminator among the items ends (indices into
    /// `items`).
    pub terminations: Vec<Termination>,
}

/// The scene with `theme` and `conventions` applied, palette defaults
/// from `doc`. Dashes are fitted by the rules in [`dashes`]: every
/// dashed curve starts and ends on a dash, crosses its anchors on its
/// short group, and keeps every crossing with another curve inside a
/// dash. The crossings come from a grid over the scene's curves, built
/// once here.
pub fn resolve(scene: &Scene, doc: &Document, theme: &Theme, conventions: Conventions) -> Styled {
    let grid = match conventions.dash_fit {
        DashFit::EndsOnDash => Some(grid_of(scene.items())),
        DashFit::Plain => None,
    };
    let mut seen = vec![0u32; grid.as_ref().map_or(0, |g| g.len())];
    let mut stamp = 0u32;
    let items = scene
        .items()
        .enumerate()
        .map(|(index, i)| {
            let dashes = match (&i.primitive, &grid) {
                (
                    Primitive::Stroke {
                        path,
                        dash,
                        anchors,
                        ..
                    },
                    Some(grid),
                ) if !dash.is_solid() => {
                    stamp += 1;
                    let crossings = grid.crossings(index, path, &mut seen, stamp);
                    dashes::fit(
                        dash.pattern(),
                        path.length(),
                        path.is_closed(),
                        anchors,
                        &crossings,
                        &conventions,
                    )
                }
                (Primitive::Stroke { dash, .. }, None) => dash.pattern().to_vec(),
                _ => Vec::new(),
            };
            StyledItem {
                primitive: i.primitive.clone(),
                dashes,
                colour: theme.colour(doc, i.tint),
                layer: i.layer,
                source: i.source,
            }
        })
        .collect();
    // The planes' terminations, their indices moved on past the items
    // of the planes before.
    let mut terminations = Vec::new();
    let mut by = 0;
    for p in &scene.planes {
        terminations.extend(p.terminations.iter().map(|t| t.offset(by)));
        by += p.items.len();
    }
    Styled {
        paper: theme.paper,
        conventions,
        items,
        terminations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::draw;
    use crate::scene::Path;
    use lmsheets_document::examples::dimensions;
    use lmsheets_document::value::style::Purpose;

    #[test]
    fn a_centre_line_crosses_the_part_and_its_hole_on_dashes() {
        let d = dimensions();
        let scene = Scene::of(draw(&d, Purpose::Export));
        let conv = Conventions::default();
        let styled = resolve(&scene, &d, &Theme::print(), conv);
        let cv = styled
            .items
            .iter()
            .find(|i| i.source == Source::View("linear".into(), "cv".into()))
            .expect("the hole's centre line");
        let Primitive::Stroke {
            path: Path::Line { a, b },
            ..
        } = cv.primitive
        else {
            panic!()
        };
        // The line is vertical; distances along it from its start.
        let along = |y: f64| (y - a.y) * if b.y > a.y { 1.0 } else { -1.0 };
        let len = (b.y - a.y).abs();
        let mut at = 0.0;
        let dashes: Vec<(f64, f64)> = cv
            .dashes
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let r = (at, at + l);
                at += l;
                (i % 2 == 0 && *l > 0.0).then_some(r)
            })
            .collect();
        assert!((at - len).abs() < 1e-6);
        // It crosses the hole's circle twice (y = 72.5, 92.5), the plate's
        // bottom edge (y = 120) and the chain's line (y = 132): each inside
        // a dash with clear room; and the centre (y = 82.5) on a short one.
        for y in [72.5, 92.5, 120.0, 132.0] {
            let x = along(y);
            assert!(
                dashes.iter().any(
                    |(p, q)| x - p >= conv.dash_clear - 1e-3 && q - x >= conv.dash_clear - 1e-3
                ),
                "crossing at y = {y} ({x} along) is not inside a dash: {:?}",
                cv.dashes
            );
        }
        let centre = along(82.5);
        assert!(dashes
            .iter()
            .any(|(p, q)| ((p + q) / 2.0 - centre).abs() < 1e-3 && q - p < 2.0));
    }
}
