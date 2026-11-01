//! Text on the sheet: a block of lines, sized by a role, placed by an
//! anchor.
//!
//! A text is a layout object, not geometry: its box is derived from the
//! font, its size and its lines, and the solver never sees it. Its
//! [`Anchor`] names which point of that box sits at its [`Placement`];
//! leaders and dimensions attach to the same anchors by reference, so
//! several can leave one block from either side. A placement is a paper
//! point, or beside a point of the drawing (following it when the
//! drawing moves), or aligned to another text's anchor.
//!
//! A line is a row of [`Span`]s: runs of characters, and stacks of two
//! rows about the line's middle (a fraction over its bar, limits over
//! theirs, a plus over a minus). Most lines are one run, and read and
//! write as plain strings.

use crate::value::geometry::{Paper, PointRef, P2};
use crate::value::ink::AsDrawn;
use crate::value::lettering::RoleId;
use crate::value::provenance::Provenance;
use crate::value::sketch::LayerId;
use crate::value::view::ViewId;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// The name of a text, an interned `Copy` handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TextId(Ustr);

impl TextId {
    /// A name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&str> for TextId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<&TextId> for TextId {
    fn from(id: &TextId) -> Self {
        *id
    }
}

impl std::fmt::Display for TextId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A side of a line or box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The left.
    Left,
    /// The right.
    Right,
}

/// A position on the text's box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoxPoint {
    /// Top left.
    TopLeft,
    /// Top centre.
    TopCentre,
    /// Top right.
    TopRight,
    /// Middle of the left edge.
    MidLeft,
    /// The centre.
    Centre,
    /// Middle of the right edge.
    MidRight,
    /// Bottom left.
    BottomLeft,
    /// Bottom centre.
    BottomCentre,
    /// Bottom right.
    BottomRight,
}

/// A named point of a text's derived box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Anchor {
    /// A point of the box around all the lines.
    Box {
        /// Which.
        at: BoxPoint,
    },
    /// An end of the last line's baseline.
    Baseline {
        /// Which end.
        side: Side,
    },
    /// The middle of the last line's baseline: a centred text's (a
    /// form's entry, held on the middle of the original's ink).
    BaselineCentre,
    /// An end of one line at mid cap height: where a leader attaches.
    Line {
        /// The line, from the top.
        index: usize,
        /// Which end.
        side: Side,
    },
}

impl Anchor {
    /// Which side of the text the anchor is on, if it is on one: a
    /// leader's shoulder runs away from the text on that side, so a
    /// centred anchor has none.
    pub fn side(&self) -> Option<Side> {
        match *self {
            Anchor::Box { at } => match at {
                BoxPoint::TopLeft | BoxPoint::MidLeft | BoxPoint::BottomLeft => Some(Side::Left),
                BoxPoint::TopRight | BoxPoint::MidRight | BoxPoint::BottomRight => {
                    Some(Side::Right)
                }
                BoxPoint::TopCentre | BoxPoint::Centre | BoxPoint::BottomCentre => None,
            },
            Anchor::Baseline { side } | Anchor::Line { side, .. } => Some(side),
            Anchor::BaselineCentre => None,
        }
    }
}

/// How lines sit within the block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    /// Lines share a left edge.
    Left,
    /// Lines are centred.
    Centre,
    /// Lines share a right edge.
    Right,
}

/// How big the text is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "size", rename_all = "snake_case")]
pub enum Size {
    /// The lettering table's size for a role: the norm.
    Role {
        /// The role.
        role: RoleId,
    },
    /// A stated cap height, mm: the exception.
    Cap {
        /// Millimetres.
        mm: f64,
    },
}

/// How a stack's two rows line up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StackAlign {
    /// Each row centred on the other: a fraction.
    #[default]
    Centre,
    /// On their decimal points, any leading signs in a column: limits,
    /// a plus over a minus. Rows without a point share a left edge.
    Point,
}

/// A piece of a line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Span {
    /// Characters on the baseline.
    Run {
        /// The characters.
        text: String,
    },
    /// Two rows about the line's middle, each centred on the other:
    /// a fraction, limits, a plus over a minus.
    Stack {
        /// The upper row.
        upper: String,
        /// The lower row.
        lower: String,
        /// What kind of stack: its style comes from the lettering table,
        /// or is stated outright as the exception.
        #[serde(flatten)]
        stack: StackKind,
    },
}

/// What a stack is, which the lettering table styles; or a style stated
/// outright, the exception.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stack", rename_all = "snake_case")]
pub enum StackKind {
    /// A numerator over a denominator.
    Fraction,
    /// An upper limit over a lower.
    Limits,
    /// A plus over a minus.
    PlusMinus,
    /// A stated style.
    Custom {
        /// The style.
        style: StackStyle,
    },
}

/// How a stack is set: whether a bar divides the rows at the line's
/// middle, the rows' size as a share of the cap height, how far each
/// row sits from the middle as a share of the cap, and how they line
/// up.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StackStyle {
    /// A bar between the rows.
    pub bar: bool,
    /// The rows' size, a share of the cap.
    pub scale: f64,
    /// Each row's distance from the middle, a share of the cap.
    pub gap: f64,
    /// How the rows line up.
    pub align: StackAlign,
}

impl StackStyle {
    /// A positive finite scale and a finite non-negative gap.
    pub fn check(&self) -> Result<(), Error> {
        if !(self.scale.is_finite() && self.scale > 0.0) {
            return Err(Error::BadSize(self.scale));
        }
        if !(self.gap.is_finite() && self.gap >= 0.0) {
            return Err(Error::BadSize(self.gap));
        }
        Ok(())
    }
}

impl Span {
    /// A run.
    pub fn run(text: &str) -> Self {
        Span::Run {
            text: text.to_string(),
        }
    }

    /// A fraction: numerator over denominator, styled by the lettering
    /// table.
    pub fn fraction(num: &str, den: &str) -> Self {
        Span::Stack {
            upper: num.to_string(),
            lower: den.to_string(),
            stack: StackKind::Fraction,
        }
    }

    /// Limits: the upper over the lower, styled by the lettering table.
    pub fn limits(upper: &str, lower: &str) -> Self {
        Span::Stack {
            upper: upper.to_string(),
            lower: lower.to_string(),
            stack: StackKind::Limits,
        }
    }

    /// A tolerance: a plus over a minus, styled by the lettering table.
    pub fn plus_minus(plus: &str, minus: &str) -> Self {
        Span::Stack {
            upper: plus.to_string(),
            lower: minus.to_string(),
            stack: StackKind::PlusMinus,
        }
    }

    /// A stack with a stated style: the exception.
    pub fn stacked(upper: &str, lower: &str, style: StackStyle) -> Self {
        Span::Stack {
            upper: upper.to_string(),
            lower: lower.to_string(),
            stack: StackKind::Custom { style },
        }
    }

    /// The strings in it.
    pub fn strings(&self) -> Vec<&str> {
        match self {
            Span::Run { text } => vec![text],
            Span::Stack { upper, lower, .. } => vec![upper, lower],
        }
    }
}

/// A line: a row of spans.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawLine", into = "RawLine")]
pub struct Line {
    /// The spans, left to right.
    pub spans: Vec<Span>,
}

/// The file form: a plain string for a line that is one run, else the
/// spans.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum RawLine {
    Plain(String),
    Spans(Vec<Span>),
}

impl From<Line> for RawLine {
    fn from(l: Line) -> Self {
        match l.spans.as_slice() {
            [Span::Run { text }] => RawLine::Plain(text.clone()),
            _ => RawLine::Spans(l.spans),
        }
    }
}

impl From<RawLine> for Line {
    fn from(r: RawLine) -> Self {
        match r {
            RawLine::Plain(text) => Line::plain(&text),
            RawLine::Spans(spans) => Line::of(spans),
        }
    }
}

impl Line {
    /// A line of one run.
    pub fn plain(text: &str) -> Self {
        Line {
            spans: vec![Span::run(text)],
        }
    }

    /// A line of these spans, in canonical form: adjacent runs joined,
    /// empty runs dropped, and an empty line one empty run. So one line
    /// has one representation, in memory and in a file.
    pub fn of(spans: Vec<Span>) -> Self {
        let mut out: Vec<Span> = Vec::with_capacity(spans.len());
        for s in spans {
            match (&s, out.last_mut()) {
                (Span::Run { text }, _) if text.is_empty() => {}
                (Span::Run { text }, Some(Span::Run { text: last })) => last.push_str(text),
                _ => out.push(s),
            }
        }
        if out.is_empty() {
            out.push(Span::run(""));
        }
        Line { spans: out }
    }

    /// The line as plain text, a stack written as `upper/lower`: for
    /// messages and tests.
    pub fn to_plain(&self) -> String {
        self.spans
            .iter()
            .map(|s| match s {
                Span::Run { text } => text.clone(),
                Span::Stack { upper, lower, .. } => format!("{upper}/{lower}"),
            })
            .collect()
    }

    /// Every string in the line.
    pub fn strings(&self) -> impl Iterator<Item = &str> {
        self.spans.iter().flat_map(|s| s.strings())
    }
}

/// A point of the drawing a text can sit beside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "in", rename_all = "snake_case")]
pub enum DrawingPoint {
    /// A point of the sheet sketch.
    Sheet {
        /// The point.
        point: PointRef,
    },
    /// A point of a view, placed through the view's transform.
    View {
        /// The view.
        view: ViewId,
        /// The point.
        point: PointRef,
    },
}

/// Where a text's anchor sits.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Placement {
    /// A point on the paper.
    Paper {
        /// The point, mm.
        point: P2<Paper>,
    },
    /// Beside a point of the drawing, offset on the paper: the text
    /// follows the point.
    Beside {
        /// The point.
        point: DrawingPoint,
        /// The offset from it, paper mm.
        offset: P2<Paper>,
    },
    /// At another text's anchor, offset on the paper: notes in a column.
    AlignedTo {
        /// The other text.
        text: TextId,
        /// Its anchor.
        anchor: Anchor,
        /// The offset from it, paper mm.
        offset: P2<Paper>,
    },
}

impl Placement {
    /// A point on the paper.
    pub fn paper(x: f64, y: f64) -> Self {
        Placement::Paper {
            point: P2::new(x, y),
        }
    }

    /// Beside a point of a view, offset `(dx, dy)` on the paper.
    pub fn beside_in(view: impl Into<ViewId>, point: PointRef, dx: f64, dy: f64) -> Self {
        Placement::Beside {
            point: DrawingPoint::View {
                view: view.into(),
                point,
            },
            offset: P2::new(dx, dy),
        }
    }

    /// Beside a point of the sheet, offset `(dx, dy)` on the paper.
    pub fn beside_on_sheet(point: PointRef, dx: f64, dy: f64) -> Self {
        Placement::Beside {
            point: DrawingPoint::Sheet { point },
            offset: P2::new(dx, dy),
        }
    }
}

/// What can be wrong with a text.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// No lines.
    NoLines,
    /// A string with a control character or a newline in it.
    BadLine(String),
    /// A cap height, width factor, stroke or stack scale that is not
    /// finite or not positive.
    BadSize(f64),
    /// A value that is not finite.
    NotFinite,
    /// A line anchor past the last line.
    NoSuchLine(usize),
    /// A list item that does not start at a line of the text, is out of
    /// order, or whose first line does not begin with its number, or an
    /// indent that is negative or not finite.
    BadItem(usize),
    /// An enclosed text has this many lines; it must have one.
    EnclosedNotOneLine(usize),
    /// A text aligned to itself.
    SelfAligned(TextId),
    /// Its provenance fails the record's checks.
    Provenance(crate::value::provenance::Error),
    /// Its ink as drawn is invalid.
    Ink(crate::value::ink::Error),
}

impl From<crate::value::provenance::Error> for Error {
    fn from(e: crate::value::provenance::Error) -> Self {
        Error::Provenance(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// A numbered item of a list set in a text (a general note): its lines
/// run from `first` to the next item's first (or the last line). The
/// first line begins with `number` and is set `indent` cap heights from
/// the block's left edge; the item's other lines are set under the body
/// (the first letter after the number and a space), a hanging indent,
/// not under the number. Only in a left-aligned text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// The item's first line, from the top.
    pub first: usize,
    /// Its number as it begins that line (`1.`).
    pub number: String,
    /// How far the item's lines are indented from the block's left
    /// edge, cap heights.
    pub indent: f64,
}

/// A block of text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct Text {
    lines: Vec<Line>,
    items: Vec<Item>,
    size: Size,
    width: Option<f64>,
    weight: Option<f64>,
    align: Align,
    anchor: Anchor,
    at: Placement,
    angle: f64,
    enclosure: Option<Enclosure>,
    enclosure_mm: Option<f64>,
    layer: LayerId,
    provenance: Provenance,
    drawn: Option<AsDrawn>,
}

/// An enclosure drawn round a text: the ring of a balloon round a dash
/// number or a find number, the box round a flag note's number. It is
/// centred on the text's box, at the text's own enclosure size or else
/// the drafting table's balloon diameter (a square's side), or larger
/// when the box's corners would come within the balloon clearance of
/// it; an enclosed text is one line. A leader may leave the enclosure
/// instead of the text (see
/// [`Origin::Enclosure`](crate::value::leader::Origin::Enclosure)); a
/// leader landing on an enclosed text lands on its box, not its ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enclosure {
    /// A circle: the balloon of the drawings.
    Circle,
    /// A square with sharp corners: a flag note's box (LDW280-18426
    /// sh3's boxed 1 and 2).
    Square,
    /// A square of dashes, its corners rounded, each corner one dash
    /// turning round it and a dash in the middle of each side: a flag
    /// note's box in the lettering pen (LDW280-18065 sh3).
    DashedSquare,
}

#[derive(Serialize, Deserialize)]
struct Raw {
    lines: Vec<Line>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    items: Vec<Item>,
    #[serde(flatten)]
    size: Size,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
    align: Align,
    anchor: Anchor,
    at: Placement,
    angle: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enclosure: Option<Enclosure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enclosure_mm: Option<f64>,
    layer: LayerId,
    provenance: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drawn: Option<AsDrawn>,
}

impl From<Text> for Raw {
    fn from(t: Text) -> Self {
        Raw {
            lines: t.lines,
            items: t.items,
            size: t.size,
            width: t.width,
            weight: t.weight,
            align: t.align,
            anchor: t.anchor,
            at: t.at,
            angle: t.angle,
            enclosure: t.enclosure,
            enclosure_mm: t.enclosure_mm,
            layer: t.layer,
            provenance: t.provenance,
            drawn: t.drawn,
        }
    }
}

impl TryFrom<Raw> for Text {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        let t = Text {
            lines: r.lines,
            items: r.items,
            size: r.size,
            width: r.width,
            weight: r.weight,
            align: r.align,
            anchor: r.anchor,
            at: r.at,
            angle: r.angle,
            enclosure: r.enclosure,
            enclosure_mm: r.enclosure_mm,
            layer: r.layer,
            provenance: r.provenance,
            drawn: r.drawn,
        };
        t.check()?;
        Ok(t)
    }
}

impl Text {
    /// A text of these plain lines in a role, left-aligned, its
    /// bottom-left at a paper point, on the `text` layer. The builders
    /// below change the rest.
    pub fn new(
        lines: &[&str],
        role: impl Into<RoleId>,
        at: P2<Paper>,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        Self::with_lines(
            lines.iter().map(|l| Line::plain(l)).collect(),
            role,
            at,
            provenance,
        )
    }

    /// A text of these lines, as [`new`](Self::new).
    pub fn with_lines(
        lines: Vec<Line>,
        role: impl Into<RoleId>,
        at: P2<Paper>,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        let t = Self {
            lines,
            items: Vec::new(),
            size: Size::Role { role: role.into() },
            width: None,
            weight: None,
            align: Align::Left,
            anchor: Anchor::Box {
                at: BoxPoint::BottomLeft,
            },
            at: Placement::Paper { point: at },
            angle: 0.0,
            enclosure: None,
            enclosure_mm: None,
            layer: LayerId::new("text"),
            provenance,
            drawn: None,
        };
        t.check()?;
        Ok(t)
    }

    /// The same text with the ink its drafter drew for it (for a
    /// scan-faithful rendering; it still prints in our lettering).
    pub fn as_drawn(mut self, drawn: AsDrawn) -> Self {
        self.drawn = Some(drawn);
        self
    }

    /// The ink as drawn, if recorded.
    pub fn drawn(&self) -> Option<&AsDrawn> {
        self.drawn.as_ref()
    }

    /// The same text with its numbered items (see [`Item`]).
    pub fn itemised(mut self, items: Vec<Item>) -> Result<Self, Error> {
        self.items = items;
        self.check()?;
        Ok(self)
    }

    /// The numbered items.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The same text with this size.
    pub fn sized(mut self, size: Size) -> Result<Self, Error> {
        self.size = size;
        self.check()?;
        Ok(self)
    }

    /// The same text with this width factor, overriding the table's.
    pub fn width_factor(mut self, width: f64) -> Result<Self, Error> {
        self.width = Some(width);
        self.check()?;
        Ok(self)
    }

    /// The same text with this stroke, mm, overriding the layer's pen.
    pub fn weight(mut self, weight: f64) -> Result<Self, Error> {
        self.weight = Some(weight);
        self.check()?;
        Ok(self)
    }

    /// The same text with its lines aligned so.
    pub fn aligned(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// The same text with this anchor at its placement.
    pub fn anchored(mut self, anchor: Anchor) -> Result<Self, Error> {
        self.anchor = anchor;
        self.check()?;
        Ok(self)
    }

    /// The same text placed so.
    pub fn placed(mut self, at: Placement) -> Result<Self, Error> {
        self.at = at;
        self.check()?;
        Ok(self)
    }

    /// The same text turned by `angle` radians on the paper (positive
    /// clockwise on a page whose y runs down, like every other angle).
    pub fn turned(mut self, angle: f64) -> Result<Self, Error> {
        self.angle = angle;
        self.check()?;
        Ok(self)
    }

    /// The same text on another layer.
    pub fn on_layer(mut self, layer: impl Into<LayerId>) -> Self {
        self.layer = layer.into();
        self
    }

    /// The same text with an enclosure drawn round it. The text must be
    /// one line.
    pub fn enclosed(mut self, enclosure: Enclosure) -> Result<Self, Error> {
        self.enclosure = Some(enclosure);
        self.check()?;
        Ok(self)
    }

    /// The same text with no enclosure.
    pub fn unenclosed(mut self) -> Self {
        self.enclosure = None;
        self
    }

    /// The same text with its enclosure this size, mm (a circle's
    /// diameter, a square's side), instead of the drafting table's
    /// balloon diameter: a revision balloon smaller than the dash
    /// numbers', a flag note's box.
    pub fn enclosure_sized(mut self, mm: f64) -> Result<Self, Error> {
        self.enclosure_mm = Some(mm);
        self.check()?;
        Ok(self)
    }

    /// A balloon: one line in the `dimension` role (dash numbers are
    /// lettered like values), centred on a paper point, ringed, on the
    /// `balloon` layer (whose heavy pen draws the ring). The builders
    /// change the rest.
    pub fn balloon(label: &str, at: P2<Paper>, provenance: Provenance) -> Result<Self, Error> {
        Self::new(&[label], "dimension", at, provenance)?
            .anchored(Anchor::Box {
                at: BoxPoint::Centre,
            })?
            .enclosed(Enclosure::Circle)
            .map(|t| t.on_layer("balloon"))
    }

    /// Everything a text must satisfy on its own; the document checks
    /// what it refers to.
    pub fn check(&self) -> Result<(), Error> {
        self.provenance.check()?;
        if let Some(d) = &self.drawn {
            d.check().map_err(Error::Ink)?;
        }
        let mut prev: Option<usize> = None;
        for it in &self.items {
            let begins = self
                .lines
                .get(it.first)
                .is_some_and(|l| l.to_plain().starts_with(&it.number));
            if !(begins
                && prev.is_none_or(|p| p < it.first)
                && it.indent.is_finite()
                && it.indent >= 0.0)
            {
                return Err(Error::BadItem(it.first));
            }
            prev = Some(it.first);
        }
        if self.lines.is_empty() {
            return Err(Error::NoLines);
        }
        let positive = |v: f64| v.is_finite() && v > 0.0;
        for l in &self.lines {
            for s in l.strings() {
                if s.chars().any(|c| c.is_control()) {
                    return Err(Error::BadLine(s.to_string()));
                }
            }
            for s in &l.spans {
                if let Span::Stack {
                    stack: StackKind::Custom { style },
                    ..
                } = s
                {
                    style.check()?;
                }
            }
        }
        if let Size::Cap { mm } = self.size {
            if !positive(mm) {
                return Err(Error::BadSize(mm));
            }
        }
        for v in [self.width, self.weight, self.enclosure_mm]
            .into_iter()
            .flatten()
        {
            if !positive(v) {
                return Err(Error::BadSize(v));
            }
        }
        if !self.angle.is_finite() {
            return Err(Error::NotFinite);
        }
        let finite = |p: P2<Paper>| p.x.is_finite() && p.y.is_finite();
        match self.at {
            Placement::Paper { point } if !finite(point) => return Err(Error::NotFinite),
            Placement::Beside { offset, .. } | Placement::AlignedTo { offset, .. }
                if !finite(offset) =>
            {
                return Err(Error::NotFinite)
            }
            _ => {}
        }
        if let Anchor::Line { index, .. } = self.anchor {
            if index >= self.lines.len() {
                return Err(Error::NoSuchLine(index));
            }
        }
        if self.enclosure.is_some() && self.lines.len() != 1 {
            return Err(Error::EnclosedNotOneLine(self.lines.len()));
        }
        Ok(())
    }

    /// The enclosure round the text, if any.
    pub fn enclosure(&self) -> Option<Enclosure> {
        self.enclosure
    }

    /// The enclosure's own size, mm, if it has one (else the drafting
    /// table's balloon diameter).
    pub fn enclosure_mm(&self) -> Option<f64> {
        self.enclosure_mm
    }

    /// The lines.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The size.
    pub fn size(&self) -> &Size {
        &self.size
    }

    /// The width factor override, if any.
    pub fn width(&self) -> Option<f64> {
        self.width
    }

    /// The stroke override, mm, if any.
    pub fn weight_mm(&self) -> Option<f64> {
        self.weight
    }

    /// How the lines sit.
    pub fn align(&self) -> Align {
        self.align
    }

    /// The anchor.
    pub fn anchor(&self) -> Anchor {
        self.anchor
    }

    /// The placement.
    pub fn at(&self) -> Placement {
        self.at
    }

    /// The angle on the paper, radians.
    pub fn angle(&self) -> f64 {
        self.angle
    }

    /// The layer.
    pub fn layer(&self) -> LayerId {
        self.layer
    }

    /// The provenance.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Every character drawn, each once.
    pub fn chars(&self) -> impl Iterator<Item = char> + '_ {
        let mut seen = std::collections::BTreeSet::new();
        self.lines
            .iter()
            .flat_map(|l| l.strings().flat_map(|s| s.chars()))
            .filter(move |c| seen.insert(*c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::provenance::{Agent, Date, Method};

    fn ours() -> Provenance {
        Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 15).unwrap(),
        )
    }

    #[test]
    fn texts_are_checked_and_round_trip() {
        let t = Text::new(&["TOP"], "view_title", P2::new(10.0, 20.0), ours())
            .unwrap()
            .aligned(Align::Centre)
            .anchored(Anchor::Box {
                at: BoxPoint::TopCentre,
            })
            .unwrap();
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"size\":\"role\""), "{json}");
        // A plain line is a plain string in the file.
        assert!(json.contains("\"lines\":[\"TOP\"]"), "{json}");
        assert_eq!(serde_json::from_str::<Text>(&json).unwrap(), t);
        assert_eq!(
            Text::new(&[], "text", P2::new(0.0, 0.0), ours()).err(),
            Some(Error::NoLines)
        );
        assert_eq!(
            Text::new(&["a\nb"], "text", P2::new(0.0, 0.0), ours()).err(),
            Some(Error::BadLine("a\nb".into()))
        );
        assert_eq!(
            t.clone().sized(Size::Cap { mm: -1.0 }).err(),
            Some(Error::BadSize(-1.0))
        );
        assert_eq!(
            t.clone()
                .anchored(Anchor::Line {
                    index: 1,
                    side: Side::Left
                })
                .err(),
            Some(Error::NoSuchLine(1))
        );
        assert_eq!(t.chars().collect::<String>(), "TOP");
        // A cap override serialises as its own size kind.
        let c = t.sized(Size::Cap { mm: 3.0 }).unwrap();
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"size\":\"cap\""), "{json}");
        assert_eq!(serde_json::from_str::<Text>(&json).unwrap(), c);
    }

    #[test]
    fn a_balloon_is_one_enclosed_line() {
        let b = Text::balloon("-15", P2::new(0.0, 0.0), ours()).unwrap();
        assert_eq!(b.enclosure(), Some(Enclosure::Circle));
        assert_eq!(b.layer(), "balloon".into());
        assert_eq!(
            b.anchor(),
            Anchor::Box {
                at: BoxPoint::Centre
            }
        );
        // The enclosure is a plain field in the file.
        let v: serde_json::Value = serde_json::to_value(&b).unwrap();
        assert_eq!(v["enclosure"], "circle");
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(serde_json::from_str::<Text>(&json).unwrap(), b);
        // A text without one writes no field.
        let plain = b.clone().unenclosed();
        assert!(!serde_json::to_string(&plain).unwrap().contains("enclosure"));
        assert_eq!(
            Text::new(&["A", "B"], "text", P2::new(0.0, 0.0), ours())
                .unwrap()
                .enclosed(Enclosure::Circle)
                .err(),
            Some(Error::EnclosedNotOneLine(2))
        );
    }

    #[test]
    fn lines_may_hold_stacks() {
        let line = Line::of(vec![
            Span::run("2 "),
            Span::fraction("5", "16"),
            Span::run(" R"),
        ]);
        assert_eq!(line.to_plain(), "2 5/16 R");
        let t = Text::with_lines(vec![line], "text", P2::new(0.0, 0.0), ours()).unwrap();
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"kind\":\"stack\""), "{json}");
        assert_eq!(serde_json::from_str::<Text>(&json).unwrap(), t);
        assert_eq!(t.chars().collect::<String>(), "2 516R");
        let bad = Line::of(vec![Span::stacked(
            "1",
            "2",
            StackStyle {
                bar: true,
                scale: 0.0,
                gap: 0.2,
                align: StackAlign::Centre,
            },
        )]);
        assert_eq!(
            Text::with_lines(vec![bad], "text", P2::new(0.0, 0.0), ours()).err(),
            Some(Error::BadSize(0.0))
        );
    }
}
