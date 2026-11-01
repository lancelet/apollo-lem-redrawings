//! The two looks of the viewer (docs/redraw-plan.md §5, Themes): a light
//! "White lab" and a dark "HUD". Pure data: the chrome's CSS tokens follow
//! the same names, and the shell reads the sheet's colours from here. The
//! sheet follows the theme: the redraw is resolved against
//! [`Theme::render`], the scan's grey is mapped through a two-colour palette
//! ([`Theme::scan_palette`]), a colour mapping and never a resampling.
//!
//! Colours are written to the target as they are (the surface is not sRGB),
//! so they are the usual hex values divided by 255.

use lmsheets_document::value::style::{Appearance, PaletteId, Rgba};

/// A look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Theme {
    /// Light: white panels, hairlines, normal-case sans, one blue accent,
    /// the sheet on paper.
    #[default]
    WhiteLab,
    /// Dark: near-black, cyan hairlines, monospace capitals, the sheet dark
    /// too.
    Hud,
}

/// A colour from its hex value.
const fn hex(h: u32) -> [f32; 3] {
    [
        ((h >> 16) & 0xff) as f32 / 255.0,
        ((h >> 8) & 0xff) as f32 / 255.0,
        (h & 0xff) as f32 / 255.0,
    ]
}

fn rgba(c: [f32; 3]) -> Rgba {
    Rgba {
        r: f64::from(c[0]),
        g: f64::from(c[1]),
        b: f64::from(c[2]),
        a: 1.0,
    }
}

/// What a lit or listed part of the drawing is, for its colour: one colour
/// per role, the same everywhere (the hover, the pin, the scan's tint, the
/// overlay, the icons, the layers panel, the legend). A group takes the
/// colour of its head's role, so a dimension hovered by its witness line is
/// the dimension's colour.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Role {
    /// The part's own geometry (and constructions, observed features).
    Entity,
    /// The entities a hovered or pinned constraint binds.
    Bound,
    /// A dimension: line, arrowheads, value, witness lines.
    Dimension,
    /// Annotation text, leaders and their enclosures.
    Text,
    /// Centre lines and marks.
    Centre,
    /// The title block.
    Form,
    /// The marking region.
    Marking,
    /// An inferred dimension or constraint.
    Inferred,
    /// An observed feature (seen, not intended) or unmodelled mark.
    Observed,
}

impl Role {
    /// Every role, in the legend's order.
    pub const ALL: [Role; 9] = [
        Role::Entity,
        Role::Bound,
        Role::Dimension,
        Role::Text,
        Role::Centre,
        Role::Form,
        Role::Marking,
        Role::Inferred,
        Role::Observed,
    ];

    /// The legend's label.
    pub fn label(self) -> &'static str {
        match self {
            Role::Entity => "Part geometry",
            Role::Bound => "Bound by a constraint",
            Role::Dimension => "Dimension",
            Role::Text => "Text",
            Role::Centre => "Centre line",
            Role::Form => "Form",
            Role::Marking => "Marking region",
            Role::Inferred => "Inferred",
            Role::Observed => "Observed (seen, not intended)",
        }
    }

    /// The role of a part of the drawing from its layer category.
    pub fn of_layer(layer: crate::layers::Layer) -> Self {
        use crate::layers::Layer as L;
        match layer {
            L::Part | L::Constructions => Role::Entity,
            L::Observed => Role::Observed,
            L::Centre => Role::Centre,
            L::Dimensions => Role::Dimension,
            L::Inferred => Role::Inferred,
            L::Marking => Role::Marking,
            L::Form => Role::Form,
            L::Texts => Role::Text,
        }
    }
}

/// The role colours on paper: dark enough to read on white, apart from one
/// another, from the ink and from the blue the redraw takes over the scan.
const ROLES_LIGHT: [[f32; 3]; 9] = [
    hex(0xe0_6a_00), // entity
    hex(0xd6_1a_7a), // bound
    hex(0x00_96_88), // dimension
    hex(0x3a_a0_1e), // text
    hex(0x58_6e_a0), // centre
    hex(0x8a_5a_2b), // form
    hex(0xd3_26_1a), // marking
    hex(0x8c_40_bf), // inferred
    hex(0xcc_00_e0), // observed
];

/// The role colours on the dark sheet: bright, apart from one another, from
/// the ink and from the cyan the redraw takes over the scan.
const ROLES_DARK: [[f32; 3]; 9] = [
    hex(0xff_5a_3c), // entity
    hex(0xff_3d_c8), // bound
    hex(0x7d_ff_3d), // dimension
    hex(0xb5_7b_ff), // text
    hex(0x3d_ff_b0), // centre
    hex(0x4d_8c_ff), // form
    hex(0xff_ff_ff), // marking
    hex(0xff_b4_54), // inferred
    hex(0x1a_b8_48), // observed
];

impl Theme {
    /// The other theme.
    pub fn toggled(self) -> Self {
        match self {
            Theme::WhiteLab => Theme::Hud,
            Theme::Hud => Theme::WhiteLab,
        }
    }

    /// Whether the sheet is drawn dark.
    pub fn is_dark(self) -> bool {
        self == Theme::Hud
    }

    /// The name shown on the switch.
    pub fn label(self) -> &'static str {
        match self {
            Theme::WhiteLab => "White lab",
            Theme::Hud => "HUD",
        }
    }

    /// The redraw's colours: the paper, the ink, and the palette entries
    /// this look overrides (the working layers' tints).
    pub fn render(self) -> lmsheets_render::theme::Theme {
        match self {
            Theme::WhiteLab => lmsheets_render::theme::Theme::screen(),
            Theme::Hud => lmsheets_render::theme::Theme {
                paper: rgba(hex(0x07_0b_10)),
                ink: rgba(hex(0xb9_e8_f5)),
                palette: [
                    ("added", hex(0x5f_e0_ff)),
                    ("inferred", hex(0xff_b4_54)),
                    ("notes", hex(0xff_b4_54)),
                ]
                .into_iter()
                .map(|(n, c)| (PaletteId::new(n), Appearance::Colour { rgba: rgba(c) }))
                .collect(),
            },
        }
    }

    /// The scan's two colours, `(ink, paper)`: the tile shader mixes them by
    /// the tile's grey. In `muted` form (under an overlay) the ink is
    /// drawn half way back to the paper, so the redraw reads over it.
    pub fn scan_palette(self, muted: bool) -> ([f32; 4], [f32; 4]) {
        let (ink, paper) = match self {
            Theme::WhiteLab => (hex(0x20_25_2a), hex(0xfb_fb_f8)),
            Theme::Hud => (hex(0xb9_e8_f5), hex(0x07_0b_10)),
        };
        let ink = if muted {
            let k = if self.is_dark() { 0.62 } else { 0.5 };
            [0, 1, 2].map(|i| ink[i] + (paper[i] - ink[i]) * k)
        } else {
            ink
        };
        (
            [ink[0], ink[1], ink[2], 1.0],
            [paper[0], paper[1], paper[2], 1.0],
        )
    }

    /// The colour of the redraw drawn over the scan: one accent, and for
    /// inferred dimensions a second.
    pub fn overlay(self, inferred: bool) -> [f32; 3] {
        match (self, inferred) {
            (Theme::WhiteLab, false) => hex(0x1f_6f_eb),
            (Theme::WhiteLab, true) => self.role(Role::Inferred),
            (Theme::Hud, false) => hex(0x5f_e0_ff),
            (Theme::Hud, true) => self.role(Role::Inferred),
        }
    }

    /// The colour of a constraint icon of a trust class: the inferred one
    /// is the look's inferred colour.
    pub fn constraint(self, trust: crate::constraints::ConstraintTrust) -> [f32; 3] {
        use crate::constraints::ConstraintTrust as T;
        match (self, trust) {
            (Theme::WhiteLab, T::Read) => hex(0x1a_8a_4a),
            (Theme::WhiteLab, T::Drawn) => hex(0x6b_77_85),
            (Theme::WhiteLab, T::Stated) => hex(0x1f_6f_eb),
            (Theme::WhiteLab, T::Inferred) => self.role(Role::Inferred),
            (Theme::Hud, T::Read) => hex(0x7d_ff_9d),
            (Theme::Hud, T::Drawn) => hex(0x8f_ae_ba),
            (Theme::Hud, T::Stated) => hex(0x5f_e0_ff),
            (Theme::Hud, T::Inferred) => self.role(Role::Inferred),
        }
    }

    /// The colour of a role.
    pub fn role(self, role: Role) -> [f32; 3] {
        let table = if self.is_dark() {
            &ROLES_DARK
        } else {
            &ROLES_LIGHT
        };
        table[role as usize]
    }

    /// The default for a system that prefers dark or light.
    pub fn for_scheme(prefers_dark: bool) -> Self {
        if prefers_dark {
            Theme::Hud
        } else {
            Theme::WhiteLab
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative luminance of a (display) colour.
    fn lum(c: [f32; 3]) -> f32 {
        let f = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
    }

    fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
        let (x, y) = (lum(a) + 0.05, lum(b) + 0.05);
        x.max(y) / x.min(y)
    }

    fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    #[test]
    fn every_role_colour_reads_on_its_sheet_and_is_distinct() {
        for theme in [Theme::WhiteLab, Theme::Hud] {
            let (ink, paper) = theme.scan_palette(false);
            let (ink, paper) = ([ink[0], ink[1], ink[2]], [paper[0], paper[1], paper[2]]);
            let accent = theme.overlay(false);
            let mut all: Vec<(String, [f32; 3])> = Role::ALL
                .iter()
                .map(|r| (format!("{r:?}"), theme.role(*r)))
                .collect();
            for (name, c) in &all {
                let k = contrast(*c, paper);
                assert!(k >= 3.0, "{theme:?} {name}: contrast {k:.2}");
            }
            // The base ink and the overlay's accent are colours too.
            all.push(("ink".into(), ink));
            all.push(("accent".into(), accent));
            for (i, (an, a)) in all.iter().enumerate() {
                for (bn, b) in &all[i + 1..] {
                    let d = dist(*a, *b);
                    assert!(d >= 0.28, "{theme:?}: {an} and {bn} are {d:.2} apart");
                }
            }
        }
    }

    #[test]
    fn the_inferred_colours_are_one_colour() {
        use crate::constraints::ConstraintTrust;
        for theme in [Theme::WhiteLab, Theme::Hud] {
            assert_eq!(
                theme.constraint(ConstraintTrust::Inferred),
                theme.role(Role::Inferred)
            );
            assert_eq!(theme.overlay(true), theme.role(Role::Inferred));
        }
    }

    #[test]
    fn every_constraint_colour_reads_on_its_sheet() {
        use crate::constraints::ConstraintTrust;
        for theme in [Theme::WhiteLab, Theme::Hud] {
            let (_, paper) = theme.scan_palette(false);
            let paper = [paper[0], paper[1], paper[2]];
            for t in ConstraintTrust::ALL {
                let c = contrast(theme.constraint(t), paper);
                assert!(c >= 3.0, "{theme:?} {t:?}: contrast {c:.2}");
            }
        }
    }

    #[test]
    fn the_scan_palette_runs_from_ink_to_paper_and_mutes_towards_paper() {
        for theme in [Theme::WhiteLab, Theme::Hud] {
            let (ink, paper) = theme.scan_palette(false);
            let (muted, paper2) = theme.scan_palette(true);
            assert_eq!(paper, paper2);
            assert!(contrast([ink[0], ink[1], ink[2]], [paper[0], paper[1], paper[2]]) > 7.0);
            assert!(
                contrast(
                    [muted[0], muted[1], muted[2]],
                    [paper[0], paper[1], paper[2]]
                ) < contrast([ink[0], ink[1], ink[2]], [paper[0], paper[1], paper[2]])
            );
        }
        assert_eq!(Theme::for_scheme(true), Theme::Hud);
        assert_eq!(Theme::Hud.toggled().toggled(), Theme::Hud);
    }
}
