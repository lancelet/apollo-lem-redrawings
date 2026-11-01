//! How things are drawn: layers, line styles, dash patterns, and the
//! palette a layer's tint refers to.
//!
//! A layer says how its lines are drawn (weight and dash, mm on the
//! paper), where they are visible (on the screen, in an export), and
//! optionally a tint: the name of a palette entry. Colour belongs to the
//! layer and to nothing else; what an item's provenance is, is shown by
//! the overlay and the queries, never by its colour. The palette is the
//! document's own declaration of each name's default appearance, so a
//! file exports with no theme at all; a theme (render crate) overrides
//! names for a screen or a printer. An appearance is either a colour or a
//! fade, the theme's ink mixed towards its paper, so "faint" is right on
//! any background. Nothing here is an RGB for one background.
//!
//! The renderer obeys the layer table and nothing else: a drawing made
//! for a [`Purpose`] holds exactly the entities whose layer is visible
//! for it.
//!
//! What a line *is* is not its layer's: an entity carries its
//! [`Convention`] (how it is drawn: visible, hidden, centre, …) and its
//! [`Role`] (what it is for), and a layer only gives the defaults for the
//! entities on it that carry none (docs/redraw-plan.md §2.8). So a hidden
//! line can sit among visible ones, and nothing reads a layer's name for
//! meaning.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// The name of a palette entry, an interned `Copy` handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PaletteId(Ustr);

impl PaletteId {
    /// A name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&str> for PaletteId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<&PaletteId> for PaletteId {
    fn from(id: &PaletteId) -> Self {
        *id
    }
}

impl std::fmt::Display for PaletteId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What can be wrong with a style.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A line weight that is not finite or not positive.
    BadWeight(f64),
    /// A dash pattern with a length that is not finite or not positive.
    BadDash(Vec<f64>),
    /// A marker size that is not finite or not positive.
    BadMarker(f64),
    /// A lettering pen that is not finite or not positive.
    BadPen(f64),
    /// A colour component outside `0..=1`.
    BadColour(Rgba),
    /// A fade outside `0..=1`.
    BadFade(f64),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// A dash pattern: dash, gap, dash, gap, … in mm on the paper; empty for
/// a solid line. An odd pattern repeats doubled, as in SVG. Where the
/// pattern starts along a line is the renderer's business (a centre line
/// begins and ends on a long dash).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<f64>", into = "Vec<f64>")]
pub struct Dash {
    pattern: Vec<f64>,
}

impl From<Dash> for Vec<f64> {
    fn from(d: Dash) -> Self {
        d.pattern
    }
}

impl TryFrom<Vec<f64>> for Dash {
    type Error = Error;

    fn try_from(pattern: Vec<f64>) -> Result<Self, Error> {
        Dash::new(pattern)
    }
}

impl Dash {
    /// A continuous line.
    pub const SOLID: Dash = Dash {
        pattern: Vec::new(),
    };

    /// A pattern; every length must be finite and positive.
    pub fn new(pattern: Vec<f64>) -> Result<Self, Error> {
        if pattern.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
            return Err(Error::BadDash(pattern));
        }
        Ok(Self { pattern })
    }

    /// Short dashes: edges behind the surface.
    pub fn hidden() -> Self {
        Self {
            pattern: vec![3.0, 1.5],
        }
    }

    /// Long dash, short dash: axes of symmetry and holes.
    pub fn centre() -> Self {
        Self {
            pattern: vec![12.0, 1.5, 1.5, 1.5],
        }
    }

    /// Long dash, two short: adjacent parts, alternate positions.
    pub fn phantom() -> Self {
        Self {
            pattern: vec![12.0, 1.5, 1.5, 1.5, 1.5, 1.5],
        }
    }

    /// Short dashes: a shortened length.
    pub fn break_line() -> Self {
        Self {
            pattern: vec![1.5, 1.5],
        }
    }

    /// The lengths, dash and gap alternating; empty when solid.
    pub fn pattern(&self) -> &[f64] {
        &self.pattern
    }

    /// Whether the line is continuous.
    pub fn is_solid(&self) -> bool {
        self.pattern.is_empty()
    }
}

/// The shape of a point marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    /// A small filled dot.
    Dot,
    /// A small cross: construction points.
    Cross,
}

/// How a layer draws its points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// The shape.
    pub kind: MarkerKind,
    /// Its size across, mm on the paper.
    pub size: f64,
}

impl Marker {
    /// A 1 mm dot.
    pub const DOT: Marker = Marker {
        kind: MarkerKind::Dot,
        size: 1.0,
    };
    /// A 1.5 mm cross.
    pub const CROSS: Marker = Marker {
        kind: MarkerKind::Cross,
        size: 1.5,
    };

    /// A marker; the size must be finite and positive.
    pub fn new(kind: MarkerKind, size: f64) -> Result<Self, Error> {
        let m = Self { kind, size };
        m.check()?;
        Ok(m)
    }

    /// Finite positive size.
    pub fn check(&self) -> Result<(), Error> {
        if self.size.is_finite() && self.size > 0.0 {
            Ok(())
        } else {
            Err(Error::BadMarker(self.size))
        }
    }
}

/// The pen lettering is drawn with on a layer: a separate property from
/// the line weight, since a letter's stroke goes with its size (a Leroy
/// pen with its template) and not with the lines beside it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pen {
    /// A share of the cap height: the norm, the same look at every size.
    OfCap {
        /// The share.
        share: f64,
    },
    /// A fixed width, mm.
    Mm {
        /// Millimetres.
        mm: f64,
    },
}

impl Pen {
    /// The standard pen: 0.14 of the cap height, as measured on the
    /// Grumman sheets.
    pub const STANDARD: Pen = Pen::OfCap { share: 0.14 };

    /// The stroke, mm, for lettering of this cap height.
    pub fn width(&self, cap: f64) -> f64 {
        match *self {
            Pen::OfCap { share } => share * cap,
            Pen::Mm { mm } => mm,
        }
    }

    /// Finite positive.
    pub fn check(&self) -> Result<(), Error> {
        let v = match *self {
            Pen::OfCap { share } => share,
            Pen::Mm { mm } => mm,
        };
        if v.is_finite() && v > 0.0 {
            Ok(())
        } else {
            Err(Error::BadPen(v))
        }
    }
}

/// How a layer's lines, points and lettering are drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineStyle {
    /// Line weight, mm on the paper.
    pub weight: f64,
    /// The dash pattern.
    pub dash: Dash,
    /// How points on the layer are marked.
    pub points: Marker,
    /// The pen lettering on the layer is drawn with.
    pub pen: Pen,
}

impl LineStyle {
    /// A style with points as 1 mm dots and the standard pen; the weight
    /// must be finite and positive.
    pub fn new(weight: f64, dash: Dash) -> Result<Self, Error> {
        let s = Self {
            weight,
            dash,
            points: Marker::DOT,
            pen: Pen::STANDARD,
        };
        s.check()?;
        Ok(s)
    }

    /// The same style with these point markers.
    pub fn with_points(mut self, points: Marker) -> Self {
        self.points = points;
        self
    }

    /// The same style with this pen.
    pub fn with_pen(mut self, pen: Pen) -> Result<Self, Error> {
        self.pen = pen;
        self.check()?;
        Ok(self)
    }

    /// Finite positive weight, marker size and pen; the dash is valid by
    /// construction, but a file is checked again.
    pub fn check(&self) -> Result<(), Error> {
        if !(self.weight.is_finite() && self.weight > 0.0) {
            return Err(Error::BadWeight(self.weight));
        }
        self.points.check()?;
        self.pen.check()?;
        Dash::new(self.dash.pattern.clone()).map(|_| ())
    }
}

/// What a drawing is made for; decides which layers are in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// The screen.
    Screen,
    /// An export: SVG, PDF, a print.
    Export,
}

/// Where a layer is visible. Extensible: locking and selectability will
/// join these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visibility {
    /// On the screen.
    pub screen: bool,
    /// In an export.
    pub export: bool,
}

impl Visibility {
    /// Visible everywhere.
    pub const ALL: Visibility = Visibility {
        screen: true,
        export: true,
    };
    /// On the screen only: our working layers.
    pub const SCREEN: Visibility = Visibility {
        screen: true,
        export: false,
    };
    /// Nowhere.
    pub const NONE: Visibility = Visibility {
        screen: false,
        export: false,
    };

    /// Whether the layer is in a drawing made for `purpose`.
    pub fn is_on(&self, purpose: Purpose) -> bool {
        match purpose {
            Purpose::Screen => self.screen,
            Purpose::Export => self.export,
        }
    }
}

/// A line's drafting convention: how it is drawn and read. The set is the
/// drawing's line work in the standard layer table
/// ([`standard_layers`](crate::state::document::Document::standard_layers)),
/// each with the style of its row there ([`style`](Self::style)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Convention {
    /// Thick, solid: edges in view.
    Visible,
    /// Thin, short dashes: edges behind the surface.
    Hidden,
    /// Thin, long dash and short: axes of symmetry and holes.
    Centre,
    /// Thin, long dash and two short: adjacent parts, alternate positions.
    Phantom,
    /// Thick, long dash and two short: where a section is cut.
    CuttingPlane,
    /// Thin, short dashes: a shortened length.
    Break,
    /// Thick, solid: the sheet's format (border, title block).
    Format,
    /// Ours, thin, on the screen only: a line to constrain against.
    Construction,
}

impl Convention {
    /// Its standard style: weight and dash as in the standard layer table
    /// (which is built from this), points as dots (construction's as
    /// crosses), the standard pen.
    pub fn style(self) -> LineStyle {
        let (weight, dash) = match self {
            Convention::Visible | Convention::Format => (0.7, Dash::SOLID),
            Convention::Hidden => (0.35, Dash::hidden()),
            Convention::Centre => (0.35, Dash::centre()),
            Convention::Phantom => (0.35, Dash::phantom()),
            Convention::CuttingPlane => (0.7, Dash::phantom()),
            Convention::Break => (0.35, Dash::break_line()),
            Convention::Construction => (0.18, Dash::SOLID),
        };
        let points = match self {
            Convention::Construction => Marker::CROSS,
            _ => Marker::DOT,
        };
        LineStyle {
            weight,
            dash,
            points,
            pen: Pen::STANDARD,
        }
    }
}

/// What a line is for in the drawing, beside how it is drawn. Only the
/// roles the export gives entities so far: a dimension's, a leader's and
/// a centre mark's lines are nodes of their own, not entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// An edge: of the part, or a line of the format.
    Outline,
    /// An axis: a centre line of symmetry or through a hole.
    Axis,
    /// Where a dimension measures from: a line through a feature's
    /// extreme, across the measuring direction.
    Extension,
}

/// A row of the layer table: style and visibility only, never origin
/// (that is provenance).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// How its lines are drawn.
    pub style: LineStyle,
    /// Where it is visible.
    pub visible: Visibility,
    /// The palette entry its lines take, if any; `None` is the theme's
    /// ink.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<PaletteId>,
    /// The convention of the entities on it that carry none, if it gives
    /// one; `style` is how lines of this convention are drawn here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convention: Option<Convention>,
    /// The role of the entities on it that carry none, if it gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
}

impl Layer {
    /// A layer visible everywhere, in ink: the drawing's own stack.
    pub fn drawn(style: LineStyle) -> Self {
        Self {
            style,
            visible: Visibility::ALL,
            tint: None,
            convention: None,
            role: None,
        }
    }

    /// A layer visible on the screen only: our working stack.
    pub fn working(style: LineStyle) -> Self {
        Self {
            style,
            visible: Visibility::SCREEN,
            tint: None,
            convention: None,
            role: None,
        }
    }

    /// The same layer with this visibility.
    pub fn visible(mut self, visible: Visibility) -> Self {
        self.visible = visible;
        self
    }

    /// The same layer giving these defaults to the entities on it.
    pub fn with_defaults(mut self, convention: Convention, role: Option<Role>) -> Self {
        self.convention = Some(convention);
        self.role = role;
        self
    }

    /// The same layer tinted by a palette entry.
    pub fn tinted(mut self, tint: impl Into<PaletteId>) -> Self {
        self.tint = Some(tint.into());
        self
    }
}

/// A colour with straight alpha, components in `0..=1`. A theme's output
/// and a palette entry's default; never attached to an entity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    /// Red.
    pub r: f64,
    /// Green.
    pub g: f64,
    /// Blue.
    pub b: f64,
    /// Alpha.
    pub a: f64,
}

impl Rgba {
    /// A colour, checked: every component in `0..=1`.
    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Result<Self, Error> {
        let c = Self { r, g, b, a };
        c.check()?;
        Ok(c)
    }

    /// An opaque colour.
    pub fn opaque(r: f64, g: f64, b: f64) -> Result<Self, Error> {
        Self::new(r, g, b, 1.0)
    }

    /// Every component in `0..=1`.
    pub fn check(&self) -> Result<(), Error> {
        let ok = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
        if ok(self.r) && ok(self.g) && ok(self.b) && ok(self.a) {
            Ok(())
        } else {
            Err(Error::BadColour(*self))
        }
    }
}

/// What a palette entry does to a line's colour.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Appearance {
    /// Replace the semantic colour with this one.
    Colour {
        /// The colour.
        rgba: Rgba,
    },
    /// The theme's ink mixed towards its paper by this much: `0` is ink,
    /// `1` is invisible.
    Fade {
        /// The mix.
        by: f64,
    },
}

impl Appearance {
    /// A fade, checked: `0..=1`.
    pub fn fade(by: f64) -> Result<Self, Error> {
        let a = Self::Fade { by };
        a.check()?;
        Ok(a)
    }

    /// A colour.
    pub fn colour(rgba: Rgba) -> Self {
        Self::Colour { rgba }
    }

    /// Components and fades in range.
    pub fn check(&self) -> Result<(), Error> {
        match self {
            Appearance::Colour { rgba } => rgba.check(),
            Appearance::Fade { by } => {
                if by.is_finite() && (0.0..=1.0).contains(by) {
                    Ok(())
                } else {
                    Err(Error::BadFade(*by))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes_are_checked_and_round_trip_as_lists() {
        assert!(Dash::SOLID.is_solid());
        assert_eq!(Dash::hidden().pattern(), &[3.0, 1.5]);
        assert_eq!(
            Dash::new(vec![3.0, 0.0]),
            Err(Error::BadDash(vec![3.0, 0.0]))
        );
        assert!(Dash::new(vec![3.0, f64::NAN]).is_err());
        let json = serde_json::to_string(&Dash::centre()).unwrap();
        assert_eq!(json, "[12.0,1.5,1.5,1.5]");
        assert_eq!(serde_json::from_str::<Dash>(&json).unwrap(), Dash::centre());
        assert!(serde_json::from_str::<Dash>("[1.0,-1.0]").is_err());
        assert_eq!(LineStyle::new(0.0, Dash::SOLID), Err(Error::BadWeight(0.0)));
        assert_eq!(
            LineStyle::new(0.5, Dash::SOLID)
                .unwrap()
                .with_pen(Pen::Mm { mm: 0.0 }),
            Err(Error::BadPen(0.0))
        );
        assert!((Pen::STANDARD.width(5.0) - 0.7).abs() < 1e-12);
        assert_eq!(Pen::Mm { mm: 0.3 }.width(5.0), 0.3);
    }

    #[test]
    fn appearances_are_checked() {
        assert!(Appearance::fade(0.5).is_ok());
        assert_eq!(Appearance::fade(1.5), Err(Error::BadFade(1.5)));
        assert!(Rgba::opaque(0.2, 0.4, 0.6).is_ok());
        assert!(matches!(
            Rgba::new(1.2, 0.0, 0.0, 1.0),
            Err(Error::BadColour(_))
        ));
        let a = Appearance::colour(Rgba::opaque(0.95, 0.55, 0.05).unwrap());
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"kind\":\"colour\""));
        assert_eq!(serde_json::from_str::<Appearance>(&json).unwrap(), a);
    }
}
