//! A dimension: a constraint shown on the sheet.
//!
//! A dimension names a constraint of the sheet or of a view and says
//! how it is shown; the value, its units and (later) its tolerance come
//! from the constraint, so a dimension can never disagree with the
//! geometry it labels, and one constraint may be shown more than once.
//! A driven constraint is a reference dimension and prints `(REF)`.
//!
//! The layout is one of three families, which must match the
//! constraint's (the document's invariant): [`Linear`] for distances,
//! and radial and angular to come. Everything else, the measured
//! points, the extension lines, the arrowheads, the text, is derived
//! from the constraint, the drafting table and the lettering.

use crate::value::centre::{CentreLineId, CentreMarkId};
use crate::value::geometry::{EntityId, Paper, P2};
use crate::value::ink::AsDrawn;
use crate::value::leader::{self, Route, Shoulder};
use crate::value::provenance::{ConstraintId, Provenance};
use crate::value::relation::DimensionKind;
use crate::value::sketch::LayerId;
use crate::value::text::{Placement, Side};
use crate::value::view::ViewId;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use ustr::Ustr;

/// The name of a dimension, an interned `Copy` handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DimensionId(Ustr);

impl DimensionId {
    /// A name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&str> for DimensionId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<&DimensionId> for DimensionId {
    fn from(id: &DimensionId) -> Self {
        *id
    }
}

impl std::fmt::Display for DimensionId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The constraint a dimension shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "in", rename_all = "snake_case")]
pub enum Of {
    /// A constraint of the sheet sketch.
    Sheet {
        /// The constraint.
        constraint: ConstraintId,
    },
    /// A constraint of a view.
    View {
        /// The view.
        view: ViewId,
        /// The constraint.
        constraint: ConstraintId,
    },
}

impl Of {
    /// A constraint of a view.
    pub fn in_view(view: impl Into<ViewId>, constraint: impl Into<ConstraintId>) -> Self {
        Of::View {
            view: view.into(),
            constraint: constraint.into(),
        }
    }

    /// A constraint of the sheet.
    pub fn on_sheet(constraint: impl Into<ConstraintId>) -> Self {
        Of::Sheet {
            constraint: constraint.into(),
        }
    }
}

/// The lettering role every dimension's value is set in.
pub const ROLE: &str = "dimension";

/// The lettering role of a radius's or diameter's note (`11/64 R
/// (TYP)`) where the table has one; else the value role.
pub const CALLOUT_ROLE: &str = "callout";

/// Which end of a dimension line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// The end at the span's first point.
    Start,
    /// The end at the span's second point.
    End,
}

/// Where a linear dimension's text goes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Place {
    /// Inline when the arrows are inside, between the extension lines
    /// when they are outside and the text fits, else on a shelf.
    Auto,
    /// In a break of the dimension line.
    Inline,
    /// Between the extension lines with nothing under it: outside
    /// arrows.
    Between,
    /// Over an unbroken line, on the side away from the feature.
    Above,
    /// Beside an unbroken line, displaced along the paper's x: the
    /// horizontal text next to a vertical line.
    Beside {
        /// Which side.
        side: Side,
    },
    /// Outside the span on a shelf: the dimension line continued past
    /// the extension line, straight when `drop` is none, else that far
    /// along the line and then horizontally to the side.
    Shelf {
        /// Which end.
        end: End,
        /// The run to the text, mm; the drafting table's when none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run: Option<f64>,
        /// A run along the line first, then the horizontal one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        drop: Option<Drop>,
    },
}

/// A shelf's run along the dimension line before its horizontal run.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drop {
    /// How far along the line, mm.
    pub along: f64,
    /// The side the horizontal run then goes.
    pub side: Side,
}

impl Place {
    /// A shelf from this end, straight on, with the standard run.
    pub fn shelf(end: End) -> Self {
        Place::Shelf {
            end,
            run: None,
            drop: None,
        }
    }

    /// A shelf from this end, `along` the line and then to the `side`,
    /// with the standard run.
    pub fn dropped_shelf(end: End, along: f64, side: Side) -> Self {
        Place::Shelf {
            end,
            run: None,
            drop: Some(Drop { along, side }),
        }
    }
}

/// Where the arrows go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrows {
    /// Inside when the span holds both heads, the text and the gaps.
    Auto,
    /// Between the extension lines.
    Inside,
    /// Outside them, pointing in.
    Outside,
}

/// Whether an extension line is drawn, and if not, what serves.
///
/// A drawn one is a witness line: a node of the drawing in its own
/// right, its foot the measured point, shared by every dimension that
/// draws the same one (the renderer draws it once over their union).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Extension {
    /// Drawn.
    Draw,
    /// Not drawn, and what serves is not named (prefer [`Extension::By`]).
    Omit,
    /// Not drawn: this line of the drawing serves.
    By(Serving),
}

/// What serves as a dimension's extension line at one end. The
/// document checks that the end's measured point is on it: a point the
/// centre line runs through, the centre of one of the mark's circles,
/// a point of the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Serving {
    /// A centre line.
    CentreLine {
        /// The line.
        line: CentreLineId,
    },
    /// A centre mark's arm.
    CentreMark {
        /// The mark.
        mark: CentreMarkId,
    },
    /// A line of the sketch the point is on: an edge, a phantom or a
    /// station line.
    Line {
        /// The line.
        line: EntityId,
    },
}

/// How the text reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    /// Horizontal, as on every orthographic view.
    Horizontal,
    /// Along the dimension line.
    Aligned,
}

/// A linear dimension's layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Linear {
    /// The dimension line's signed distance from the span's first
    /// point, paper mm, along the left normal on the paper (y down) of
    /// the measuring direction, pointed from the first point towards the
    /// second (level or plumb for a horizontal or vertical distance, the
    /// span's own way for an aligned one): a positive offset puts the
    /// line below a span measured to the right, right of one measured
    /// up, above one measured to the left.
    pub offset: f64,
    /// The text's shift from the middle of the line along it, paper mm
    /// (positive towards the span's second point); the break in the
    /// line follows it.
    #[serde(default)]
    pub along: f64,
    /// Where the text goes.
    pub place: Place,
    /// Where the arrows go.
    pub arrows: Arrows,
    /// The extension lines, at the span's first and second points.
    pub ext: [Extension; 2],
    /// The gap between a feature and the start of its extension line,
    /// paper mm, at the span's first and second points, for this
    /// dimension; the drafting table's when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext_gap: Option<[f64; 2]>,
}

/// Where a radial note's tip lands on the arc.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Land {
    /// Wherever the last run meets the arc: its point nearest the run's
    /// start.
    Nearest,
    /// At a fraction of the way round, from the arc's start.
    Fraction {
        /// The fraction, `0..=1`.
        t: f64,
    },
    /// Where the ray from the centre through the run's start meets the
    /// arc, so the run is aimed at the centre.
    Radial,
    /// A radius drawn from the centre: the run aimed at the centre ends
    /// on the arc, and a line from the centre to that point ends there
    /// in the arrowhead, pointing out, at the leader's weight. LDW280-
    /// 17446 and -17619 sh3 ("3/8 R (TYP)", "11/64 R (TYP)") are drawn
    /// so in the original; the redraw uses [`Land::Radial`] by
    /// convention, and this stays an explicit, per-note option.
    FromCentre,
}

/// Where a radial note's mark ("R", "DIA") goes about its value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    /// After the value: "11/64 R".
    #[default]
    After,
    /// Before it: "R 11/64".
    Before,
}

impl Mark {
    fn is_default(&self) -> bool {
        *self == Mark::After
    }
}

/// A radius or a diameter as a note: a leader with the value as its
/// text ("10.0 R", "8.0 DIA"). The text is placed like any text and the
/// route leaves it as a leader does. Its mark is derived from the
/// constraint; it is set after the value unless `mark` says before, and
/// `typical` adds "(TYP)": the note applies to every arc the sketch
/// states equal to this one (the constraints say which, not the text).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Radial {
    /// Where the text's attachment (an end of its line, mid cap) sits.
    pub at: Placement,
    /// Which end of the text the route leaves.
    pub side: Side,
    /// The route from the text: the shoulder and the knees.
    #[serde(flatten)]
    pub route: Route,
    /// Where the tip lands.
    pub land: Land,
    /// Where the mark goes about the value.
    #[serde(default, skip_serializing_if = "Mark::is_default")]
    pub mark: Mark,
    /// Whether the note says "(TYP)".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub typical: bool,
}

/// A diameter across its circle: a dimension line through the centre
/// at an angle (arrows inside on the circle), or shifted outside with
/// extension lines from the circle's extreme points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Across {
    /// The line's direction on the paper, radians from +x.
    pub angle: f64,
    /// The line's signed distance from the centre along the left normal
    /// of its direction, paper mm; zero through the centre.
    pub offset: f64,
    /// Where the text goes.
    pub place: Place,
    /// Where the arrows go.
    pub arrows: Arrows,
}

/// Where an angular dimension's text goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Within {
    /// In a break of the arc when it fits, else beyond the arc.
    Auto,
    /// In a break of the arc.
    Inside,
    /// Beyond the arc, away from the vertex.
    Outside,
}

/// An angle between two lines: an arc about their vertex in the sector
/// a point names, arrows at its ends, extension lines along the rays
/// where the lines fall short of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Angular {
    /// The arc's radius from the vertex, paper mm.
    pub radius: f64,
    /// The direction from the vertex into the sector shown, radians on
    /// the paper, in `(-π, π]`.
    pub sector: f64,
    /// Where the text goes.
    pub text: Within,
    /// Where the arrows go.
    pub arrows: Arrows,
}

/// An axis of a sketch's frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// The frame's x.
    X,
    /// The frame's y.
    Y,
}

/// One coordinate of a positioned point as a station callout
/// ("X185.278"): a leader with the value as its text, the letter from
/// the view's model axis, landing on the point with an arrow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ordinate {
    /// Which coordinate.
    pub axis: Axis,
    /// Where the text's attachment sits.
    pub at: Placement,
    /// Which end of the text the route leaves.
    pub side: Side,
    /// The route from the text: the shoulder and the knees.
    #[serde(flatten)]
    pub route: Route,
}

/// The layout, by family.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Layout {
    /// A distance.
    Linear(Linear),
    /// A radius or diameter as a note.
    Radial(Radial),
    /// A diameter across its circle.
    Across(Across),
    /// An angle between two lines.
    Angular(Angular),
    /// A coordinate of a positioned point.
    Ordinate(Ordinate),
}

impl Layout {
    /// The family.
    pub fn kind(&self) -> DimensionKind {
        match self {
            Layout::Linear(_) => DimensionKind::Linear,
            Layout::Radial(_) | Layout::Across(_) => DimensionKind::Radial,
            Layout::Angular(_) => DimensionKind::Angular,
            Layout::Ordinate(_) => DimensionKind::Ordinate,
        }
    }
}

/// How the value is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// As the constraint has it: the nominal with its tolerance (a
    /// symmetric one inline, a plus over a minus, or the limits).
    #[default]
    Auto,
    /// The nominal alone.
    Nominal,
}

/// How the text is composed and set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    /// How it reads.
    pub orientation: Orientation,
    /// How the value is written.
    #[serde(default)]
    pub form: Form,
    /// Words before the value ("DRILL").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Words after the value ("(TYP)", "2 PLACES"); "(REF)" is added of
    /// itself for a driven constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

/// What can be wrong with a dimension on its own.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// An offset, run, drop, angle or knee that is not finite.
    NotFinite,
    /// A run or drop that is not positive.
    BadLength(f64),
    /// A stated shoulder that is not positive.
    BadShoulder(f64),
    /// A landing fraction outside `0..=1`.
    BadFraction(f64),
    /// A prefix or suffix with a control character in it.
    BadText(String),
    /// A builder that does not apply to the dimension's layout.
    WrongLayout {
        /// The builder.
        builder: &'static str,
        /// The layout's family.
        layout: DimensionKind,
    },
    /// An arc radius that is not positive.
    BadRadius(f64),
    /// A sector given by a point with no direction.
    NoDirection,
    /// A sector angle outside `(-π, π]`, the one form of a direction.
    BadSector(f64),
    /// Its provenance fails the record's checks.
    Provenance(crate::value::provenance::Error),
    /// Its value's ink as drawn is invalid.
    Ink(crate::value::ink::Error),
}

impl From<crate::value::provenance::Error> for Error {
    fn from(e: crate::value::provenance::Error) -> Self {
        Error::Provenance(e)
    }
}

impl From<leader::Error> for Error {
    fn from(e: leader::Error) -> Self {
        match e {
            leader::Error::BadShoulder(mm) => Error::BadShoulder(mm),
            leader::Error::NotFinite => Error::NotFinite,
            // A route has no tips, targets or texts to be wrong.
            other => unreachable!(
                "a route's check gives only shoulder and finiteness errors, not {other:?}"
            ),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// A dimension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct Dimension {
    of: Of,
    layout: Layout,
    text: TextStyle,
    layer: LayerId,
    provenance: Provenance,
    drawn: Option<AsDrawn>,
}

#[derive(Serialize, Deserialize)]
struct Raw {
    of: Of,
    layout: Layout,
    text: TextStyle,
    layer: LayerId,
    provenance: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drawn: Option<AsDrawn>,
}

impl From<Dimension> for Raw {
    fn from(d: Dimension) -> Self {
        Raw {
            of: d.of,
            layout: d.layout,
            text: d.text,
            layer: d.layer,
            provenance: d.provenance,
            drawn: d.drawn,
        }
    }
}

impl TryFrom<Raw> for Dimension {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        let d = Dimension {
            of: r.of,
            layout: r.layout,
            text: r.text,
            layer: r.layer,
            provenance: r.provenance,
            drawn: r.drawn,
        };
        d.check()?;
        Ok(d)
    }
}

impl Dimension {
    /// A linear dimension of a constraint, its line `offset` mm from the
    /// span, text and arrows placed by the rules, both extension lines
    /// drawn, horizontal text, on the `dimension` layer. The builders
    /// below change the rest.
    pub fn linear(of: Of, offset: f64, provenance: Provenance) -> Result<Self, Error> {
        let d = Self {
            of,
            layout: Layout::Linear(Linear {
                offset,
                along: 0.0,
                place: Place::Auto,
                arrows: Arrows::Auto,
                ext: [Extension::Draw, Extension::Draw],
                ext_gap: None,
            }),
            text: TextStyle {
                orientation: Orientation::Horizontal,
                form: Form::Auto,
                prefix: None,
                suffix: None,
            },
            layer: LayerId::new("dimension"),
            provenance,
            drawn: None,
        };
        d.check()?;
        Ok(d)
    }

    /// A radius or diameter note of a constraint: the text's attachment
    /// at `at`, the route leaving the text's `side` with the standard
    /// shoulder and landing nearest its run, horizontal text, on the
    /// `dimension` layer.
    pub fn radial(
        of: Of,
        at: Placement,
        side: Side,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        let d = Self {
            of,
            layout: Layout::Radial(Radial {
                at,
                side,
                route: Route::standard(),
                land: Land::Nearest,
                mark: Mark::After,
                typical: false,
            }),
            text: TextStyle {
                orientation: Orientation::Horizontal,
                form: Form::Auto,
                prefix: None,
                suffix: None,
            },
            layer: LayerId::new("dimension"),
            provenance,
            drawn: None,
        };
        d.check()?;
        Ok(d)
    }

    /// A diameter across its circle at `angle` (paper radians), the line
    /// `offset` from the centre (zero: through it), text and arrows by
    /// the rules, on the `dimension` layer.
    pub fn across(of: Of, angle: f64, offset: f64, provenance: Provenance) -> Result<Self, Error> {
        let d = Self {
            of,
            layout: Layout::Across(Across {
                angle,
                offset,
                place: Place::Auto,
                arrows: Arrows::Auto,
            }),
            text: TextStyle {
                orientation: Orientation::Horizontal,
                form: Form::Auto,
                prefix: None,
                suffix: None,
            },
            layer: LayerId::new("dimension"),
            provenance,
            drawn: None,
        };
        d.check()?;
        Ok(d)
    }

    /// An angle between two lines: the arc `radius` from their vertex in
    /// the sector that `towards` (an offset from the vertex on the paper)
    /// points into, text and arrows by the rules, on the `dimension`
    /// layer.
    pub fn angular(
        of: Of,
        radius: f64,
        towards: P2<Paper>,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        if !(towards.x.is_finite() && towards.y.is_finite()) {
            return Err(Error::NotFinite);
        }
        if towards.x == 0.0 && towards.y == 0.0 {
            return Err(Error::NoDirection);
        }
        let sector = towards.y.atan2(towards.x);
        let d = Self {
            of,
            layout: Layout::Angular(Angular {
                radius,
                sector,
                text: Within::Auto,
                arrows: Arrows::Auto,
            }),
            text: TextStyle {
                orientation: Orientation::Horizontal,
                form: Form::Auto,
                prefix: None,
                suffix: None,
            },
            layer: LayerId::new("dimension"),
            provenance,
            drawn: None,
        };
        d.check()?;
        Ok(d)
    }

    /// The same angular dimension with its text so.
    pub fn within(mut self, text: Within) -> Result<Self, Error> {
        match &mut self.layout {
            Layout::Angular(a) => a.text = text,
            _ => return Err(self.wrong("within")),
        }
        Ok(self)
    }

    /// The error for a builder that does not apply to this layout.
    fn wrong(&self, builder: &'static str) -> Error {
        Error::WrongLayout {
            builder,
            layout: self.layout.kind(),
        }
    }

    fn linear_mut(&mut self, builder: &'static str) -> Result<&mut Linear, Error> {
        let wrong = self.wrong(builder);
        match &mut self.layout {
            Layout::Linear(l) => Ok(l),
            _ => Err(wrong),
        }
    }

    /// A coordinate of a positioned point as a callout: the text's
    /// attachment at `at`, the route leaving the text's `side` with the
    /// standard shoulder, on the `dimension` layer.
    pub fn ordinate(
        of: Of,
        axis: Axis,
        at: Placement,
        side: Side,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        let d = Self {
            of,
            layout: Layout::Ordinate(Ordinate {
                axis,
                at,
                side,
                route: Route::standard(),
            }),
            text: TextStyle {
                orientation: Orientation::Horizontal,
                form: Form::Auto,
                prefix: None,
                suffix: None,
            },
            layer: LayerId::new("dimension"),
            provenance,
            drawn: None,
        };
        d.check()?;
        Ok(d)
    }

    fn radial_mut(&mut self, builder: &'static str) -> Result<&mut Radial, Error> {
        let wrong = self.wrong(builder);
        match &mut self.layout {
            Layout::Radial(r) => Ok(r),
            _ => Err(wrong),
        }
    }

    /// The route of a layout that has one: its shoulder and knees.
    fn route_mut(&mut self, builder: &'static str) -> Result<&mut Route, Error> {
        let wrong = self.wrong(builder);
        match &mut self.layout {
            Layout::Radial(r) => Ok(&mut r.route),
            Layout::Ordinate(o) => Ok(&mut o.route),
            _ => Err(wrong),
        }
    }

    /// The same linear dimension with its own gaps between the features
    /// and the start of their extension lines, paper mm, at the span's
    /// first and second points.
    pub fn gapped(mut self, gap: [f64; 2]) -> Result<Self, Error> {
        self.linear_mut("gapped")?.ext_gap = Some(gap);
        self.check()?;
        Ok(self)
    }

    /// The same dimension with its text placed so (linear and across).
    pub fn placed(mut self, place: Place) -> Result<Self, Error> {
        match &mut self.layout {
            Layout::Linear(l) => l.place = place,
            Layout::Across(a) => a.place = place,
            Layout::Radial(_) | Layout::Angular(_) | Layout::Ordinate(_) => {
                return Err(self.wrong("placed"))
            }
        }
        self.check()?;
        Ok(self)
    }

    /// The same radial note landing so.
    pub fn landed(mut self, land: Land) -> Result<Self, Error> {
        self.radial_mut("landed")?.land = land;
        self.check()?;
        Ok(self)
    }

    /// The same radial note with its mark so about the value.
    pub fn marked(mut self, mark: Mark) -> Result<Self, Error> {
        self.radial_mut("marked")?.mark = mark;
        Ok(self)
    }

    /// The same radial note saying "(TYP)": typical of the arcs the
    /// sketch states equal.
    pub fn typical(mut self) -> Result<Self, Error> {
        self.radial_mut("typical")?.typical = true;
        Ok(self)
    }

    /// The same note (radial or ordinate) with this shoulder.
    pub fn shouldered(mut self, shoulder: Shoulder) -> Result<Self, Error> {
        self.route_mut("shouldered")?.shoulder = shoulder;
        self.check()?;
        Ok(self)
    }

    /// The same note (radial or ordinate) with these knees after the
    /// shoulder, offsets from the attachment in paper mm.
    pub fn via(mut self, knees: Vec<P2<Paper>>) -> Result<Self, Error> {
        self.route_mut("via")?.knees = knees;
        self.check()?;
        Ok(self)
    }

    /// The same linear dimension with its text shifted this far from
    /// the middle of the line along it, paper mm.
    pub fn shifted(mut self, along: f64) -> Result<Self, Error> {
        self.linear_mut("shifted")?.along = along;
        self.check()?;
        Ok(self)
    }

    /// The same dimension with its arrows so (linear, across, angular).
    pub fn arrowed(mut self, arrows: Arrows) -> Result<Self, Error> {
        match &mut self.layout {
            Layout::Linear(l) => l.arrows = arrows,
            Layout::Across(a) => a.arrows = arrows,
            Layout::Angular(a) => a.arrows = arrows,
            Layout::Radial(_) | Layout::Ordinate(_) => return Err(self.wrong("arrowed")),
        }
        Ok(self)
    }

    /// The same linear dimension with these extension lines.
    pub fn extended(mut self, ext: [Extension; 2]) -> Result<Self, Error> {
        self.linear_mut("extended")?.ext = ext;
        Ok(self)
    }

    /// The same dimension with its value written so.
    pub fn formed(mut self, form: Form) -> Self {
        self.text.form = form;
        self
    }

    /// The same dimension with its text along the line.
    pub fn aligned(mut self) -> Self {
        self.text.orientation = Orientation::Aligned;
        self
    }

    /// The same dimension with words before the value.
    pub fn prefixed(mut self, prefix: &str) -> Result<Self, Error> {
        self.text.prefix = Some(prefix.to_string());
        self.check()?;
        Ok(self)
    }

    /// The same dimension with words after the value.
    pub fn suffixed(mut self, suffix: &str) -> Result<Self, Error> {
        self.text.suffix = Some(suffix.to_string());
        self.check()?;
        Ok(self)
    }

    /// The same dimension on another layer.
    pub fn on_layer(mut self, layer: impl Into<LayerId>) -> Self {
        self.layer = layer.into();
        self
    }

    /// Everything a dimension must satisfy on its own; the document
    /// checks the constraint.
    pub fn check(&self) -> Result<(), Error> {
        self.provenance.check()?;
        if let Some(d) = &self.drawn {
            d.check().map_err(Error::Ink)?;
        }
        let finite = |p: &P2<Paper>| p.x.is_finite() && p.y.is_finite();
        let shelf = |place: Place| {
            if let Place::Shelf { run, drop, .. } = place {
                for v in [run, drop.map(|d| d.along)].into_iter().flatten() {
                    if !v.is_finite() {
                        return Err(Error::NotFinite);
                    }
                    if v <= 0.0 {
                        return Err(Error::BadLength(v));
                    }
                }
            }
            Ok(())
        };
        match &self.layout {
            Layout::Linear(l) => {
                if !(l.offset.is_finite() && l.along.is_finite()) {
                    return Err(Error::NotFinite);
                }
                for g in l.ext_gap.into_iter().flatten() {
                    if !g.is_finite() {
                        return Err(Error::NotFinite);
                    }
                    if g < 0.0 {
                        return Err(Error::BadLength(g));
                    }
                }
                shelf(l.place)?;
            }
            Layout::Radial(r) => {
                let placed = match r.at {
                    Placement::Paper { point } => point,
                    Placement::Beside { offset, .. } | Placement::AlignedTo { offset, .. } => {
                        offset
                    }
                };
                if !finite(&placed) {
                    return Err(Error::NotFinite);
                }
                r.route.check()?;
                if let Land::Fraction { t } = r.land {
                    if !t.is_finite() {
                        return Err(Error::NotFinite);
                    }
                    if !(0.0..=1.0).contains(&t) {
                        return Err(Error::BadFraction(t));
                    }
                }
            }
            Layout::Across(a) => {
                if !(a.angle.is_finite() && a.offset.is_finite()) {
                    return Err(Error::NotFinite);
                }
                shelf(a.place)?;
            }
            Layout::Angular(a) => {
                if !(a.radius.is_finite() && a.sector.is_finite()) {
                    return Err(Error::NotFinite);
                }
                if a.radius <= 0.0 {
                    return Err(Error::BadRadius(a.radius));
                }
                if a.sector <= -PI || a.sector > PI {
                    return Err(Error::BadSector(a.sector));
                }
            }
            Layout::Ordinate(o) => {
                let placed = match o.at {
                    Placement::Paper { point } => point,
                    Placement::Beside { offset, .. } | Placement::AlignedTo { offset, .. } => {
                        offset
                    }
                };
                if !finite(&placed) {
                    return Err(Error::NotFinite);
                }
                o.route.check()?;
            }
        }
        for s in [&self.text.prefix, &self.text.suffix].into_iter().flatten() {
            if s.chars().any(|c| c.is_control()) {
                return Err(Error::BadText(s.clone()));
            }
        }
        Ok(())
    }

    /// The constraint shown.
    pub fn of(&self) -> Of {
        self.of
    }

    /// The layout.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The text style.
    pub fn text(&self) -> &TextStyle {
        &self.text
    }

    /// The layer.
    pub fn layer(&self) -> LayerId {
        self.layer
    }

    /// The same dimension with the ink its drafter drew for its value
    /// (for a scan-faithful rendering; it still prints in our lettering).
    pub fn as_drawn(mut self, drawn: AsDrawn) -> Self {
        self.drawn = Some(drawn);
        self
    }

    /// Its value's ink as drawn, if recorded.
    pub fn drawn(&self) -> Option<&AsDrawn> {
        self.drawn.as_ref()
    }

    /// The provenance.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
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
    fn an_extension_gap_is_checked_and_round_trips() {
        let of = Of::View {
            view: "top".into(),
            constraint: "w".into(),
        };
        let d = Dimension::linear(of, 5.0, ours()).unwrap();
        // None says nothing: older documents read as before.
        let plain = serde_json::to_string(&d).unwrap();
        assert!(!plain.contains("ext_gap"), "{plain}");
        let g = d.clone().gapped([3.4, 1.6]).unwrap();
        let json = serde_json::to_string(&g).unwrap();
        assert!(json.contains("\"ext_gap\":[3.4,1.6]"), "{json}");
        assert_eq!(serde_json::from_str::<Dimension>(&json).unwrap(), g);
        assert_eq!(
            d.clone().gapped([1.0, -1.0]).err(),
            Some(Error::BadLength(-1.0))
        );
        assert_eq!(
            d.clone().gapped([f64::NAN, 1.0]).err(),
            Some(Error::NotFinite)
        );
        let radial = Dimension::radial(
            of,
            Placement::Paper {
                point: P2::new(0.0, 0.0),
            },
            Side::Right,
            ours(),
        )
        .unwrap();
        assert!(radial.gapped([1.0, 1.0]).is_err());
    }

    #[test]
    fn dimensions_are_checked_and_round_trip() {
        let of = Of::View {
            view: "top".into(),
            constraint: "w".into(),
        };
        let d = Dimension::linear(of, -12.0, ours())
            .unwrap()
            .placed(Place::Shelf {
                end: End::End,
                run: Some(8.0),
                drop: None,
            })
            .unwrap()
            .suffixed("(TYP)")
            .unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"layout\":{\"kind\":\"linear\""), "{json}");
        assert_eq!(serde_json::from_str::<Dimension>(&json).unwrap(), d);
        assert_eq!(d.layout().kind(), DimensionKind::Linear);
        assert_eq!(
            Dimension::linear(of, f64::NAN, ours()).err(),
            Some(Error::NotFinite)
        );
        assert_eq!(
            d.clone().shifted(f64::INFINITY).err(),
            Some(Error::NotFinite)
        );
        assert_eq!(
            d.clone()
                .placed(Place::Shelf {
                    end: End::Start,
                    run: Some(0.0),
                    drop: None,
                })
                .err(),
            Some(Error::BadLength(0.0))
        );
        assert_eq!(
            d.clone().prefixed("a\nb").err(),
            Some(Error::BadText("a\nb".into()))
        );
        let bad = json.replacen("\"offset\":-12.0", "\"offset\":null", 1);
        assert_ne!(bad, json);
        assert!(serde_json::from_str::<Dimension>(&bad).is_err());
        // Radial and across layouts, and the builders that fit them.
        let r = Dimension::radial(
            of,
            Placement::Paper {
                point: P2::new(1.0, 2.0),
            },
            Side::Right,
            ours(),
        )
        .unwrap()
        .landed(Land::Radial)
        .unwrap()
        .via(vec![P2::new(3.0, 4.0)])
        .unwrap()
        .marked(Mark::Before)
        .unwrap()
        .typical()
        .unwrap();
        assert_eq!(r.layout().kind(), DimensionKind::Radial);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"mark\":\"before\"") && json.contains("\"typical\":true"));
        // The defaults are left out.
        let plain = Dimension::radial(
            of,
            Placement::Paper {
                point: P2::new(1.0, 2.0),
            },
            Side::Right,
            ours(),
        )
        .unwrap();
        let plain_json = serde_json::to_string(&plain).unwrap();
        assert!(!plain_json.contains("mark") && !plain_json.contains("typical"));
        assert_eq!(
            d.clone().typical().err(),
            Some(Error::WrongLayout {
                builder: "typical",
                layout: DimensionKind::Linear
            })
        );
        assert_eq!(serde_json::from_str::<Dimension>(&json).unwrap(), r);
        assert_eq!(
            r.clone().landed(Land::Fraction { t: 2.0 }).err(),
            Some(Error::BadFraction(2.0))
        );
        assert_eq!(
            r.clone().shifted(1.0).err(),
            Some(Error::WrongLayout {
                builder: "shifted",
                layout: DimensionKind::Radial
            })
        );
        assert_eq!(
            d.clone().landed(Land::Nearest).err(),
            Some(Error::WrongLayout {
                builder: "landed",
                layout: DimensionKind::Linear
            })
        );
        let a = Dimension::across(of, 0.5, 0.0, ours())
            .unwrap()
            .placed(Place::Above)
            .unwrap();
        assert_eq!(a.layout().kind(), DimensionKind::Radial);
        assert_eq!(
            Dimension::across(of, f64::NAN, 0.0, ours()).err(),
            Some(Error::NotFinite)
        );
        let g = Dimension::angular(of, 20.0, P2::new(1.0, -0.5), ours())
            .unwrap()
            .within(Within::Outside)
            .unwrap();
        assert_eq!(g.layout().kind(), DimensionKind::Angular);
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<Dimension>(&json).unwrap(), g);
        assert_eq!(
            Dimension::angular(of, 0.0, P2::new(1.0, 0.0), ours()).err(),
            Some(Error::BadRadius(0.0))
        );
        assert_eq!(
            Dimension::angular(of, 5.0, P2::new(0.0, 0.0), ours()).err(),
            Some(Error::NoDirection)
        );
        assert_eq!(
            d.within(Within::Inside).err(),
            Some(Error::WrongLayout {
                builder: "within",
                layout: DimensionKind::Linear
            })
        );
        let o = Dimension::ordinate(
            of,
            Axis::X,
            Placement::Paper {
                point: P2::new(1.0, 2.0),
            },
            Side::Left,
            ours(),
        )
        .unwrap()
        .shouldered(Shoulder::None)
        .unwrap();
        assert_eq!(o.layout().kind(), DimensionKind::Ordinate);
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<Dimension>(&json).unwrap(), o);
        assert_eq!(
            o.clone().shouldered(Shoulder::Length { mm: -1.0 }).err(),
            Some(Error::BadShoulder(-1.0))
        );
    }
}
