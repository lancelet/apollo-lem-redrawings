//! A theme: the paper, the ink, and overrides for the document's palette
//! by name. One for a print, one for a light screen, one for a dark one.

use lmsheets_document::state::document::Document;
use lmsheets_document::value::style::{Appearance, PaletteId, Rgba};
use std::collections::BTreeMap;

/// Colours for one kind of output.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The background.
    pub paper: Rgba,
    /// Untinted lines.
    pub ink: Rgba,
    /// Palette entries this theme overrides, by name.
    pub palette: BTreeMap<PaletteId, Appearance>,
}

/// The demonstration theme's tints by meaning, as hex colours.
pub const DEMO_TINTS: [(&str, u32); 9] = [
    // Hidden lines: a grey-blue, apart from the cyan of dimensions
    // even for deuteranopes.
    ("hidden", 0x9aa8bf),
    ("centre", 0xf2b134),
    ("measure", 0x3fd0e0),
    ("annotation", 0x8ed97a),
    ("identity", 0xb99cf7),
    ("form", 0x6b7c93),
    ("constraint", 0xf5e05b),
    ("alternative", 0xff5fa8),
    ("faint", 0x4a5568),
];

fn rgb(r: f64, g: f64, b: f64) -> Rgba {
    Rgba { r, g, b, a: 1.0 }
}

impl Theme {
    /// Black on white, the document's palette as declared.
    pub fn print() -> Self {
        Self {
            paper: rgb(1.0, 1.0, 1.0),
            ink: rgb(0.0, 0.0, 0.0),
            palette: BTreeMap::new(),
        }
    }

    /// A light screen: near-black on off-white.
    pub fn screen() -> Self {
        Self {
            paper: rgb(0.98, 0.98, 0.97),
            ink: rgb(0.08, 0.08, 0.08),
            palette: BTreeMap::new(),
        }
    }

    /// A dark screen: light ink on a dark ground; the `added` blue
    /// brightened to read on it.
    pub fn dark() -> Self {
        Self {
            paper: rgb(0.11, 0.11, 0.12),
            ink: rgb(0.90, 0.90, 0.88),
            palette: [(
                PaletteId::new("added"),
                Appearance::Colour {
                    rgba: rgb(0.45, 0.65, 1.0),
                },
            )]
            .into_iter()
            .collect(),
        }
    }

    /// The demonstration theme for reconstructed sheets: a deep
    /// blue-black ground with colour carrying meaning. Layers name their
    /// meaning by tint (see [`DEMO_TINTS`]); untinted lines are the part's
    /// geometry, near-white. Colours were checked for contrast with the
    /// ground (form 4.3:1, the rest 6:1 to 16:1) and for separation under
    /// the common colour-vision deficiencies.
    pub fn demo() -> Self {
        let hex = |h: u32| {
            rgb(
                ((h >> 16) & 0xff) as f64 / 255.0,
                ((h >> 8) & 0xff) as f64 / 255.0,
                (h & 0xff) as f64 / 255.0,
            )
        };
        Self {
            paper: hex(0x0e1623),
            ink: hex(0xeef2f7),
            palette: DEMO_TINTS
                .iter()
                .map(|(name, h)| (PaletteId::new(name), Appearance::Colour { rgba: hex(*h) }))
                .collect(),
        }
    }

    /// The ink mixed towards the paper by `by`.
    pub fn faded(&self, by: f64) -> Rgba {
        let mix = |a: f64, b: f64| a + (b - a) * by;
        Rgba {
            r: mix(self.ink.r, self.paper.r),
            g: mix(self.ink.g, self.paper.g),
            b: mix(self.ink.b, self.paper.b),
            a: self.ink.a,
        }
    }

    /// The colour of a line with this tint: the theme's override if it
    /// has one, else the document's declaration, else the ink.
    pub fn colour(&self, doc: &Document, tint: Option<PaletteId>) -> Rgba {
        let Some(id) = tint else {
            return self.ink;
        };
        let appearance = self
            .palette
            .get(&id)
            .copied()
            .or_else(|| doc.palette_entry(id).copied());
        match appearance {
            Some(Appearance::Colour { rgba }) => rgba,
            Some(Appearance::Fade { by }) => self.faded(by),
            None => self.ink,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmsheets_document::state::document::Title;

    #[test]
    fn tints_resolve_through_theme_then_document_then_ink() {
        let d = Document::new(Title::default());
        let t = Theme::print();
        assert_eq!(t.colour(&d, None), rgb(0.0, 0.0, 0.0));
        // `faint` is the document's fade: half way to white in print.
        assert_eq!(t.colour(&d, Some("faint".into())), rgb(0.5, 0.5, 0.5));
        // `added` is the document's blue in print, the theme's in dark.
        assert_eq!(t.colour(&d, Some("added".into())), rgb(0.16, 0.44, 0.86));
        assert_eq!(
            Theme::dark().colour(&d, Some("added".into())),
            rgb(0.45, 0.65, 1.0)
        );
        // An unknown name is ink.
        assert_eq!(t.colour(&d, Some("nope".into())), t.ink);
    }
}
