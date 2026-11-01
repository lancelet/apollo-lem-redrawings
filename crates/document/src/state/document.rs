//! The document: one sheet of a drawing, everything we hold about it.
//!
//! A document is the paper sketch (the format, the title block, and any
//! lines drawn in paper coordinates), the views of the part with the
//! links between them, and three tables the parts refer to by name: the
//! layer table (how each layer is drawn and whether it prints), the
//! source registry (what every citation's `DocId` names), and the frames
//! (scanned images) this sheet was read from, each with its registration
//! onto the paper ([`frame`](crate::value::frame)). The
//! user's notes, pins on the paper, are here too, and the sheet's reading
//! as an engineering drawing ([`PartReading`]: the part it depicts and its
//! features).
//!
//! The document keeps the invariants only it can see: every entity's
//! layer is in the table, every citation's document is in the registry,
//! and neither a layer nor a source in use is removed. It is changed only
//! through [`Document::apply`] with an [`Op`](crate::api::msg::Op); the methods
//! behind that re-check those references after every edit and roll the
//! edit back if one would dangle, as [`Views::edit`] does for links. A
//! file is validated on load exactly as `apply` validates.
//!
//! # Example
//!
//! ```
//! use lmsheets_document::state::document::{Document, Title};
//! use lmsheets_document::state::document::{Source, SourceKind};
//! use lmsheets_document::value::geometry::{Entity, Paper, P2};
//! use lmsheets_document::api::msg::{Op, SketchOp};
//! use lmsheets_document::value::provenance::{
//!     Agent, Citation, Date, Method, Provenance,
//! };
//!
//! let day = Date::new(2026, 10, 15)?;
//! let doc = Document::new(Title {
//!     drawing: "LDW280-23631".into(),
//!     rev: "A".into(),
//!     sheet: "3".into(),
//!     name: "BEAM PANEL INSTL".into(),
//! });
//! // The standard layers are there; a source must be registered before
//! // anything cites it.
//! assert!(doc.layer("visible").is_some());
//! let sheet = Source::new(SourceKind::Sheet, "LDW280-23631 sh 3");
//! let doc = doc.apply(&Op::add_source("sheet", sheet))?;
//! let read = Provenance::new(Method::Read, Agent::Claude, day)
//!     .cite(Citation::new("sheet", "zone C4"));
//! let border = Entity::Line {
//!     start: P2::<Paper>::new(0.0, 0.0),
//!     end: P2::new(100.0, 0.0),
//! };
//! let add = SketchOp::add_entity("border", border, "format", read.clone());
//! let doc = doc.apply(&add.on_sheet())?;
//! // A layer or a source in use stays.
//! assert!(doc.apply(&Op::RemoveLayer { id: "format".into() }).is_err());
//! assert!(doc.apply(&Op::RemoveSource { id: "sheet".into() }).is_err());
//! // An entity on an unknown layer is refused; `apply` leaves `doc` as is.
//! let stray = Entity::Line {
//!     start: P2::<Paper>::new(0.0, 1.0),
//!     end: P2::new(1.0, 1.0),
//! };
//! assert!(doc
//!     .apply(&SketchOp::add_entity("x", stray, "nope", read).on_sheet())
//!     .is_err());
//! assert_eq!(doc.sheet().geometry().len(), 1);
//! // The file round trip holds.
//! let text = doc.to_json()?;
//! assert_eq!(Document::parse(&text)?, doc);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::state::version;
use crate::state::views::{self, Views};
use crate::value::centre::{CentreLine, CentreLineId, CentreMark, CentreMarkId};
use crate::value::claims::{self, Claim, SegmentId};
use crate::value::dimension::{self, Dimension, DimensionId, Extension, Layout, Of, Serving};
use crate::value::drafting::{self, Drafting};
use crate::value::form::{self as form_value, FieldId, Form};
use crate::value::frame::{self, Frame, FrameId};
use crate::value::gaps::{self, Gap, GapId};
use crate::value::geometry::{self, Kind, Model, Paper, P2};
use crate::value::leader::{self, DrawingEntity, Leader, LeaderId, Target};
use crate::value::lettering::{self, Lettering, Role, RoleId};
use crate::value::marking::{self, MarkingRegion, MarkingRegionId};
use crate::value::measure::ParamName;
use crate::value::observed::{self, Observed, ObservedId};
use crate::value::params::{self, Param, Params};
use crate::value::part::{self, PartReading};
use crate::value::provenance::{self, Agent, Date, DocId, Provenance};
use crate::value::relation::{DimensionKind, Round, Span};
use crate::value::residuals::{self, Residuals};
use crate::value::sketch::{self, LayerId, Sketch};
use crate::value::style::{
    self, Appearance, Convention, Dash, Layer, LineStyle, Marker, PaletteId, Pen, Rgba,
};
use crate::value::text::{self, Anchor, DrawingPoint, Placement, Size, Text, TextId};
use crate::value::unmodelled::{self, Unmodelled, UnmodelledId};
use crate::value::view::ViewId;
use crate::value::weights::{self, LineKind, LineWeights, Measured};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What the title block says about the sheet.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Title {
    /// The drawing number, `LDW280-23631`.
    pub drawing: String,
    /// The revision letter.
    pub rev: String,
    /// The sheet number as printed.
    pub sheet: String,
    /// The drawing title.
    pub name: String,
}

/// What kind of thing a source is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A drawing sheet, this one or another.
    Sheet,
    /// A report or manual.
    Report,
    /// A photograph.
    Photo,
    /// The 3D model.
    Model,
    /// Anything else.
    Other,
}

/// A row of the source registry: what a [`DocId`] names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// What it is.
    pub kind: SourceKind,
    /// Where to find it, in words or as a path.
    pub locator: String,
    /// Anything worth saying about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Source {
    /// A source.
    pub fn new(kind: SourceKind, locator: &str) -> Self {
        Self {
            kind,
            locator: locator.to_string(),
            note: None,
        }
    }
}

/// A pin on the paper with the user's words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Where.
    pub at: P2<Paper>,
    /// The note.
    pub text: String,
    /// Who.
    pub by: Agent,
    /// When.
    pub when: Date,
}

impl Note {
    /// A note is somewhere: its position is finite (a NaN would not
    /// even be written to the file, which has no NaN).
    pub fn check(&self) -> Result<(), Error> {
        if self.at.x.is_finite() && self.at.y.is_finite() {
            Ok(())
        } else {
            Err(Error::Geometry(geometry::Error::NotFinite))
        }
    }
}

/// Who holds a layer or cites a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    /// An entity of the sheet.
    SheetEntity(geometry::EntityId),
    /// A constraint of the sheet.
    SheetConstraint(provenance::ConstraintId),
    /// An entity of a view.
    Entity(crate::value::view::ViewId, geometry::EntityId),
    /// A constraint of a view.
    Constraint(crate::value::view::ViewId, provenance::ConstraintId),
    /// A parameter of the document's table.
    Param(ParamName),
    /// A view's frame claim.
    Frame(crate::value::view::ViewId),
    /// A link.
    Link(views::LinkId),
    /// A layer (holding a palette entry).
    Layer(LayerId),
    /// A text.
    Text(TextId),
    /// A leader.
    Leader(LeaderId),
    /// A dimension.
    Dimension(DimensionId),
    /// A centre mark.
    CentreMark(CentreMarkId),
    /// A centre line.
    CentreLine(CentreLineId),
    /// An unmodelled mark.
    Unmodelled(UnmodelledId),
    /// An observed feature.
    Observed(ObservedId),
    /// A marking region.
    MarkingRegion(MarkingRegionId),
    /// A frame's registration.
    Registration(FrameId),
    /// A field of the placed form.
    Field(FieldId),
    /// The placed form's printed parts.
    Form,
}

/// What can go wrong with a document.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A style error: weight, dash, colour or fade out of range.
    Style(style::Error),
    /// A lettering error.
    Lettering(lettering::Error),
    /// A line weight error.
    Weights(weights::Error),
    /// A text error.
    Text(text::Error),
    /// No role has this id.
    NoSuchRole(RoleId),
    /// A text names a role that is not in the lettering table.
    RoleMissing {
        /// The role.
        role: RoleId,
        /// The text.
        by: TextId,
    },
    /// The role is used by these texts.
    RoleInUse {
        /// The role.
        role: RoleId,
        /// The texts.
        by: Vec<TextId>,
    },
    /// The `dimension` role sets every dimension's value, and these
    /// exist.
    RoleNeededByDimensions {
        /// The dimensions.
        by: Vec<DimensionId>,
    },
    /// A text with this id already exists.
    DuplicateText(TextId),
    /// No text has this id.
    NoSuchText(TextId),
    /// A text is aligned to a text that is not there.
    TextMissing {
        /// The missing text.
        text: TextId,
        /// Who aligns to it.
        by: TextId,
    },
    /// Other texts are aligned to this one.
    TextInUse {
        /// The text.
        text: TextId,
        /// Who aligns to it.
        by: Vec<TextId>,
    },
    /// Texts aligned to each other in a ring.
    TextCycle(TextId),
    /// A text sits beside a point that is not in the drawing.
    PointMissing {
        /// The point.
        point: DrawingPoint,
        /// The text.
        by: TextId,
    },
    /// A text uses a character the font has no glyph for.
    GlyphMissing {
        /// The character.
        ch: char,
        /// The text.
        by: TextId,
    },
    /// A drafting error.
    Drafting(drafting::Error),
    /// A leader error.
    Leader(leader::Error),
    /// A leader with this id already exists.
    DuplicateLeader(LeaderId),
    /// No leader has this id.
    NoSuchLeader(LeaderId),
    /// A leader leaves from or lands on a text that is not there.
    LeaderTextMissing {
        /// The text.
        text: TextId,
        /// The leader.
        by: LeaderId,
    },
    /// A leader leaves the ring of a text that has no enclosure.
    LeaderOriginNotEnclosed {
        /// The text.
        text: TextId,
        /// The leader.
        by: LeaderId,
    },
    /// A leader names an anchor the text does not have (a line past
    /// its last).
    LeaderAnchorMissing {
        /// The text.
        text: TextId,
        /// The anchor.
        anchor: Anchor,
        /// The leader.
        by: LeaderId,
    },
    /// A leader's tip lands on something not in the drawing: a missing
    /// view or entity, a point of the wrong kind, a curve that is a
    /// point.
    LeaderTargetMissing {
        /// The target.
        target: Target,
        /// The leader.
        by: LeaderId,
    },
    /// A centre mark marks something that is not a circle or an arc of
    /// the drawing.
    CentreMarkFeatureMissing {
        /// The feature.
        entity: DrawingEntity,
        /// The mark.
        by: CentreMarkId,
    },
    /// A centre mark marks circles or arcs whose centres no constraint
    /// makes one (concentric, or coincident centres) with its first.
    CentreMarkNotConcentric {
        /// The feature.
        entity: DrawingEntity,
        /// The mark.
        by: CentreMarkId,
    },
    /// A dimension's end is served by a line its measured point is not
    /// on (or by one that is not in the drawing).
    DimensionServedWrongly {
        /// Which end: 0 the span's first point, 1 its second.
        end: usize,
        /// What serves.
        by_line: Serving,
        /// The dimension.
        by: DimensionId,
    },
    /// A centre line runs through a point that is not in the drawing.
    CentreLinePointMissing {
        /// The point.
        point: DrawingPoint,
        /// The line.
        by: CentreLineId,
    },
    /// A centre mark or line is invalid on its own.
    Centre(crate::value::centre::Error),
    /// A centre mark or line of this name exists already, or none does.
    DuplicateCentre(String),
    /// No centre mark or line of this name.
    NoSuchCentre(String),
    /// An unmodelled mark is invalid on its own.
    Unmodelled(unmodelled::Error),
    /// The part reading is invalid on its own.
    PartReading(part::Error),
    /// An observed feature is invalid, or names what the document lacks.
    BadObserved {
        /// The observed feature.
        observed: ObservedId,
        /// What is wrong.
        error: observed::Error,
    },
    /// A marking region is invalid, or names what the document lacks.
    BadMarkingRegion {
        /// The marking region.
        region: MarkingRegionId,
        /// What is wrong.
        error: marking::Error,
    },
    /// An unmodelled mark of this name exists already.
    DuplicateUnmodelled(UnmodelledId),
    /// No unmodelled mark of this name.
    NoSuchUnmodelled(UnmodelledId),
    /// A leader stands for an entity that is not in the drawing.
    LeaderEntityMissing {
        /// The entity.
        entity: DrawingEntity,
        /// The leader.
        by: LeaderId,
    },
    /// Leaders leave from or land on this text.
    TextHeldByLeaders {
        /// The text.
        text: TextId,
        /// The leaders.
        by: Vec<LeaderId>,
    },
    /// A dimension error.
    Dimension(dimension::Error),
    /// A dimension with this id already exists.
    DuplicateDimension(DimensionId),
    /// No dimension has this id.
    NoSuchDimension(DimensionId),
    /// A dimension shows a constraint that is not there.
    DimensionConstraintMissing {
        /// The constraint.
        of: Of,
        /// The dimension.
        by: DimensionId,
    },
    /// A dimension shows a constraint that is not a dimension (a
    /// horizontal, a coincidence).
    DimensionConstraintNotDimensional {
        /// The constraint.
        of: Of,
        /// The dimension.
        by: DimensionId,
    },
    /// A dimension's layout is of another family than its constraint.
    DimensionKindMismatch {
        /// The constraint.
        of: Of,
        /// The constraint's family.
        constraint: DimensionKind,
        /// The layout's family.
        layout: DimensionKind,
        /// The dimension.
        by: DimensionId,
    },
    /// A diameter-across layout on a radius constraint.
    AcrossNeedsDiameter {
        /// The constraint.
        of: Of,
        /// The dimension.
        by: DimensionId,
    },
    /// A dimension's prefix or suffix uses a character the font has no
    /// glyph for.
    DimensionGlyphMissing {
        /// The character.
        ch: char,
        /// The dimension.
        by: DimensionId,
    },
    /// A radial note sits beside a point that is not in the drawing.
    DimensionPointMissing {
        /// The point.
        point: DrawingPoint,
        /// The dimension.
        by: DimensionId,
    },
    /// A radial note is aligned to a text that is not there.
    DimensionTextMissing {
        /// The text.
        text: TextId,
        /// The dimension.
        by: DimensionId,
    },
    /// A palette entry with this id already exists.
    DuplicatePalette(PaletteId),
    /// No palette entry has this id.
    NoSuchPalette(PaletteId),
    /// A layer's tint names a palette entry that is not there.
    PaletteMissing {
        /// The entry.
        entry: PaletteId,
        /// The layer.
        by: LayerId,
    },
    /// The palette entry is the tint of these layers.
    PaletteInUse {
        /// The entry.
        entry: PaletteId,
        /// The layers.
        by: Vec<LayerId>,
    },
    /// A frame whose registration fails its checks.
    BadFrame {
        /// The frame.
        frame: FrameId,
        /// What is wrong.
        error: frame::Error,
    },
    /// A claim that fails its checks.
    BadClaim {
        /// Its index.
        claim: usize,
        /// What is wrong.
        error: claims::Error,
    },
    /// A placed form that fails its checks.
    BadForm(form_value::Error),
    /// The stored title gives what the form's fields give (the drawing
    /// number or the sheet): with a form, they are the fields'.
    TitleBesideForm,
    /// A gap that fails its checks.
    BadGap {
        /// Its id.
        gap: GapId,
        /// What is wrong.
        error: gaps::Error,
    },
    /// Residuals that fail their checks.
    BadResiduals(residuals::Error),
    /// A layer with this id already exists.
    DuplicateLayer(LayerId),
    /// No layer has this id.
    NoSuchLayer(LayerId),
    /// An entity is on a layer that is not in the table.
    LayerMissing {
        /// The layer.
        layer: LayerId,
        /// Who refers to it.
        by: Holder,
    },
    /// The layer is held by these.
    LayerInUse {
        /// The layer.
        layer: LayerId,
        /// Its holders.
        by: Vec<Holder>,
    },
    /// A source with this id already exists.
    DuplicateSource(DocId),
    /// No source has this id.
    NoSuchSource(DocId),
    /// A citation names a document that is not in the registry.
    SourceMissing {
        /// The source.
        source: DocId,
        /// Who cites it.
        by: Holder,
    },
    /// The source is cited by these.
    SourceInUse {
        /// The source.
        source: DocId,
        /// Its citers.
        by: Vec<Holder>,
    },
    /// No note at this position.
    NoSuchNote(usize),
    /// A parameter error: a bad name or value, or no such parameter.
    Params(params::Error),
    /// The parameter is named by these constraints.
    ParamInUse {
        /// The parameter.
        name: ParamName,
        /// Its users.
        by: Vec<Holder>,
    },
    /// No preview to commit or drop.
    NoPreview,
    /// Nothing to undo.
    NothingToUndo,
    /// Nothing to redo.
    NothingToRedo,
    /// A sketch error.
    Sketch(sketch::Error),
    /// A views error.
    Views(views::Error),
    /// A geometry error.
    Geometry(geometry::Error),
}

impl From<style::Error> for Error {
    fn from(e: style::Error) -> Self {
        Error::Style(e)
    }
}

impl From<weights::Error> for Error {
    fn from(e: weights::Error) -> Self {
        Error::Weights(e)
    }
}

impl From<lettering::Error> for Error {
    fn from(e: lettering::Error) -> Self {
        Error::Lettering(e)
    }
}

impl From<text::Error> for Error {
    fn from(e: text::Error) -> Self {
        Error::Text(e)
    }
}

impl From<drafting::Error> for Error {
    fn from(e: drafting::Error) -> Self {
        Error::Drafting(e)
    }
}

impl From<leader::Error> for Error {
    fn from(e: leader::Error) -> Self {
        Error::Leader(e)
    }
}

impl From<dimension::Error> for Error {
    fn from(e: dimension::Error) -> Self {
        Error::Dimension(e)
    }
}

impl From<sketch::Error> for Error {
    fn from(e: sketch::Error) -> Self {
        Error::Sketch(e)
    }
}

impl From<params::Error> for Error {
    fn from(e: params::Error) -> Self {
        Error::Params(e)
    }
}

impl From<views::Error> for Error {
    fn from(e: views::Error) -> Self {
        Error::Views(e)
    }
}

impl From<geometry::Error> for Error {
    fn from(e: geometry::Error) -> Self {
        Error::Geometry(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// One sheet of a drawing. See the module documentation.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(into = "Raw")]
pub struct Document {
    title: Title,
    frames: BTreeMap<FrameId, Frame>,
    sheet: Sketch<Paper>,
    views: Views,
    params: Params,
    layers: BTreeMap<LayerId, Layer>,
    palette: BTreeMap<PaletteId, Appearance>,
    lettering: Lettering,
    weights: LineWeights,
    drafting: Drafting,
    texts: BTreeMap<TextId, Text>,
    leaders: BTreeMap<LeaderId, Leader>,
    dimensions: BTreeMap<DimensionId, Dimension>,
    centre_marks: BTreeMap<CentreMarkId, CentreMark>,
    centre_lines: BTreeMap<CentreLineId, CentreLine>,
    unmodelled: BTreeMap<UnmodelledId, Unmodelled>,
    observed: BTreeMap<ObservedId, Observed>,
    marking_regions: BTreeMap<MarkingRegionId, MarkingRegion>,
    sources: BTreeMap<DocId, Source>,
    notes: Vec<Note>,
    reading: Option<PartReading>,
    claims: Vec<Claim>,
    gaps: BTreeMap<GapId, Gap>,
    form: Option<Form>,
    residuals: Option<Residuals>,
}

/// The file form at [`version::VERSION`], checked on the way in.
#[derive(Serialize, Deserialize)]
struct Raw {
    /// The header, written by every save; [`version::upgrade`] has read
    /// it before the typed parse, so the parse passes over it.
    format: String,
    version: u64,
    title: Title,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    frames: BTreeMap<FrameId, Frame>,
    sheet: Sketch<Paper>,
    views: Views,
    params: Params,
    layers: BTreeMap<LayerId, Layer>,
    palette: BTreeMap<PaletteId, Appearance>,
    lettering: Lettering,
    #[serde(default, skip_serializing_if = "LineWeights::is_empty")]
    weights: LineWeights,
    drafting: Drafting,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    texts: BTreeMap<TextId, Text>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    leaders: BTreeMap<LeaderId, Leader>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    dimensions: BTreeMap<DimensionId, Dimension>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    centre_marks: BTreeMap<CentreMarkId, CentreMark>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    centre_lines: BTreeMap<CentreLineId, CentreLine>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    unmodelled: BTreeMap<UnmodelledId, Unmodelled>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    observed: BTreeMap<ObservedId, Observed>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    marking_regions: BTreeMap<MarkingRegionId, MarkingRegion>,
    sources: BTreeMap<DocId, Source>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    notes: Vec<Note>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reading: Option<PartReading>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    claims: Vec<Claim>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    gaps: BTreeMap<GapId, Gap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    form: Option<Form>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    residuals: Option<Residuals>,
}

impl From<Document> for Raw {
    fn from(d: Document) -> Self {
        Raw {
            format: version::FORMAT.into(),
            version: version::VERSION,
            title: d.title,
            frames: d.frames,
            sheet: d.sheet,
            views: d.views,
            params: d.params,
            layers: d.layers,
            palette: d.palette,
            lettering: d.lettering,
            weights: d.weights,
            drafting: d.drafting,
            texts: d.texts,
            leaders: d.leaders,
            dimensions: d.dimensions,
            centre_marks: d.centre_marks,
            centre_lines: d.centre_lines,
            unmodelled: d.unmodelled,
            observed: d.observed,
            marking_regions: d.marking_regions,
            sources: d.sources,
            notes: d.notes,
            reading: d.reading,
            claims: d.claims,
            gaps: d.gaps,
            form: d.form,
            residuals: d.residuals,
        }
    }
}

/// A file of any version this reader knows, upgraded, then parsed and
/// checked at [`version::VERSION`]. The typed pass runs on the upgraded
/// value, which has no lines, so its errors name the path instead
/// (`views.front.sketch…: …`).
impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let file =
            version::upgrade(serde_json::Value::deserialize(d)?).map_err(D::Error::custom)?;
        let raw: Raw = serde_path_to_error::deserialize(file).map_err(D::Error::custom)?;
        Document::try_from(raw).map_err(D::Error::custom)
    }
}

impl TryFrom<Raw> for Document {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        for l in r.layers.values() {
            l.style.check()?;
        }
        for a in r.palette.values() {
            a.check()?;
        }
        for (id, f) in &r.frames {
            check_frame(*id, f)?;
        }
        for n in &r.notes {
            n.check()?;
        }
        if let Some(p) = &r.reading {
            p.check().map_err(Error::PartReading)?;
        }
        r.weights.check()?;
        let d = Document {
            title: r.title,
            frames: r.frames,
            sheet: r.sheet,
            views: r.views,
            params: r.params,
            layers: r.layers,
            palette: r.palette,
            lettering: r.lettering,
            weights: r.weights,
            drafting: r.drafting,
            texts: r.texts,
            leaders: r.leaders,
            dimensions: r.dimensions,
            centre_marks: r.centre_marks,
            centre_lines: r.centre_lines,
            unmodelled: r.unmodelled,
            observed: r.observed,
            marking_regions: r.marking_regions,
            sources: r.sources,
            notes: r.notes,
            reading: r.reading,
            claims: r.claims,
            gaps: r.gaps,
            form: r.form,
            residuals: r.residuals,
        };
        // The sketches loaded as open terms; the table closes them.
        d.check_params_in(&d.params)?;
        d.check_references()?;
        Ok(d)
    }
}

/// A frame valid, or why not.
fn check_frame(id: FrameId, f: &Frame) -> Result<(), Error> {
    f.check()
        .map_err(|error| Error::BadFrame { frame: id, error })
}

impl Document {
    /// An empty sheet with the standard layer table and palette, and no
    /// sources.
    pub fn new(title: Title) -> Self {
        Self {
            title,
            frames: BTreeMap::new(),
            sheet: Sketch::new(),
            views: Views::new(),
            params: Params::new(),
            layers: Self::standard_layers(),
            palette: Self::standard_palette(),
            lettering: Lettering::standard(),
            weights: LineWeights::default(),
            drafting: Drafting::standard(),
            texts: BTreeMap::new(),
            leaders: BTreeMap::new(),
            dimensions: BTreeMap::new(),
            centre_marks: BTreeMap::new(),
            centre_lines: BTreeMap::new(),
            unmodelled: BTreeMap::new(),
            observed: BTreeMap::new(),
            marking_regions: BTreeMap::new(),
            sources: BTreeMap::new(),
            notes: Vec::new(),
            reading: None,
            claims: Vec::new(),
            gaps: BTreeMap::new(),
            form: None,
            residuals: None,
        }
    }

    /// The standard palette: `faint` for construction, `added` for our
    /// own dimensions and geometry, `notes` for the user's pins. A theme
    /// may override any by name.
    pub fn standard_palette() -> BTreeMap<PaletteId, Appearance> {
        [
            ("faint", Appearance::Fade { by: 0.5 }),
            (
                "added",
                Appearance::Colour {
                    rgba: Rgba {
                        r: 0.16,
                        g: 0.44,
                        b: 0.86,
                        a: 1.0,
                    },
                },
            ),
            (
                "notes",
                Appearance::Colour {
                    rgba: Rgba {
                        r: 0.95,
                        g: 0.55,
                        b: 0.05,
                        a: 1.0,
                    },
                },
            ),
            (
                "inferred",
                Appearance::Colour {
                    rgba: Rgba {
                        r: 0.55,
                        g: 0.25,
                        b: 0.75,
                        a: 1.0,
                    },
                },
            ),
        ]
        .into_iter()
        .map(|(id, a)| (PaletteId::new(id), a))
        .collect()
    }

    /// The standard layers: the drawing's own stack (the line conventions
    /// of the format, in ink, visible everywhere) and our working stack
    /// (screen only, tinted): construction, added dimensions and
    /// geometry, notes. In the drawing's stack, `balloon` carries the
    /// rings of dash-number balloons at the visible weight, `bend` the
    /// bend lines of flat patterns, and `symbol` the drawn symbols (a
    /// fastener's box with an X, a section arrow) at the thin weight.
    pub fn standard_layers() -> BTreeMap<LayerId, Layer> {
        use Convention as C;
        let style = |weight, dash| LineStyle {
            weight,
            dash,
            points: Marker::DOT,
            pen: Pen::STANDARD,
        };
        let drawn = |w, d| Layer::drawn(style(w, d));
        let working = |w, d| Layer::working(style(w, d));
        // The line work's layers, each the default for its convention.
        let lines = |c: Convention, role| Layer::drawn(c.style()).with_defaults(c, role);
        [
            ("visible", lines(C::Visible, Some(style::Role::Outline))),
            ("hidden", lines(C::Hidden, Some(style::Role::Outline))),
            ("centre", lines(C::Centre, Some(style::Role::Axis))),
            ("phantom", lines(C::Phantom, Some(style::Role::Outline))),
            ("cutting_plane", lines(C::CuttingPlane, None)),
            ("break", lines(C::Break, None)),
            ("dimension", drawn(0.35, Dash::SOLID)),
            ("leader", drawn(0.35, Dash::SOLID)),
            ("text", drawn(0.35, Dash::SOLID)),
            ("balloon", drawn(0.7, Dash::SOLID)),
            ("bend", drawn(0.35, Dash::SOLID)),
            ("symbol", drawn(0.35, Dash::SOLID)),
            ("format", lines(C::Format, Some(style::Role::Outline))),
            (
                "construction",
                Layer::working(C::Construction.style())
                    .with_defaults(C::Construction, None)
                    .tinted("faint"),
            ),
            ("added", working(0.35, Dash::SOLID).tinted("added")),
            ("notes", working(0.35, Dash::SOLID).tinted("notes")),
            // The inferred constraints' dimensions (K10): shown,
            // hidden and inspected apart from the genuine ones.
            ("inferred", working(0.35, Dash::SOLID).tinted("inferred")),
            // The observed features (K11): seen, not intended. A drawn
            // rounding replaces its sharp corner on the part, so the
            // layer is the drawing's own (ink, visible everywhere) at
            // the outline's weight; hiding it gives the sharp corners
            // back.
            ("observed", lines(C::Visible, Some(style::Role::Outline))),
            ("unmodelled", working(0.18, Dash::SOLID).tinted("notes")),
        ]
        .into_iter()
        .map(|(id, l)| (LayerId::new(id), l))
        .collect()
    }

    // ---- reading

    /// The title block: a function of the placed form's fields where it
    /// has a cell for them (the drawing number, the sheet), over what is
    /// stored for the rest (the name and revision, which ENG-73.1 6-64
    /// has no cell for); without a form, what is stored.
    pub fn title(&self) -> Title {
        let mut t = self.title.clone();
        if let Some(f) = &self.form {
            let has = |field: &str| f.fields.contains_key(&FieldId::new(field));
            if has(DRAWING_FIELD) {
                t.drawing = f.value(DRAWING_FIELD).unwrap_or_default();
            }
            if has(SHEET_FIELD) {
                t.sheet = f.value(SHEET_FIELD).unwrap_or_default();
            }
        }
        t
    }

    /// The printed form placed on the sheet, with its fields.
    pub fn form(&self) -> Option<&Form> {
        self.form.as_ref()
    }

    /// The frames, by id.
    pub fn frames(&self) -> impl Iterator<Item = (&FrameId, &Frame)> {
        self.frames.iter()
    }

    /// A frame.
    pub fn frame(&self, id: impl Into<FrameId>) -> Option<&Frame> {
        self.frames.get(&id.into())
    }

    /// The claims on the frames' ink, by node ([`claims`]).
    pub fn claims(&self) -> &[Claim] {
        &self.claims
    }

    /// The sheet's gaps, by id ([`gaps`]); its unmodelled marks are gaps
    /// too, not listed here.
    pub fn gaps(&self) -> &BTreeMap<GapId, Gap> {
        &self.gaps
    }

    /// The residuals of the solve that designed a view, if stored
    /// ([`residuals`]).
    pub fn residuals(&self) -> Option<&Residuals> {
        self.residuals.as_ref()
    }

    /// The paper sketch.
    pub fn sheet(&self) -> &Sketch<Paper> {
        &self.sheet
    }

    /// The parameter table: the document's named, shared values, which
    /// constraints in any sketch may name.
    pub fn params(&self) -> &Params {
        &self.params
    }

    /// A parameter.
    pub fn param(&self, name: impl Into<ParamName>) -> Option<&Param> {
        self.params.get(name)
    }

    /// The constraints, in any sketch, whose values name the parameter.
    pub fn users_of_param(&self, name: impl Into<ParamName>) -> Vec<Holder> {
        let name = name.into();
        let sheet = self
            .sheet
            .users_of_param(name)
            .into_iter()
            .map(Holder::SheetConstraint);
        let views = self.views.views().flat_map(|(vid, v)| {
            v.sketch
                .users_of_param(name)
                .into_iter()
                .map(move |c| Holder::Constraint(*vid, c))
        });
        sheet.chain(views).collect()
    }

    /// The views and links.
    pub fn views(&self) -> &Views {
        &self.views
    }

    /// A layer.
    pub fn layer(&self, id: impl Into<LayerId>) -> Option<&Layer> {
        let id = &id.into();
        self.layers.get(id)
    }

    /// The layer table, in id order.
    pub fn layers(&self) -> impl Iterator<Item = (&LayerId, &Layer)> {
        self.layers.iter()
    }

    /// A palette entry.
    pub fn palette_entry(&self, id: impl Into<PaletteId>) -> Option<&Appearance> {
        self.palette.get(&id.into())
    }

    /// The palette, in id order.
    pub fn palette(&self) -> impl Iterator<Item = (&PaletteId, &Appearance)> {
        self.palette.iter()
    }

    /// A source.
    pub fn source(&self, id: impl Into<DocId>) -> Option<&Source> {
        let id = &id.into();
        self.sources.get(id)
    }

    /// The source registry, in id order.
    pub fn sources(&self) -> impl Iterator<Item = (&DocId, &Source)> {
        self.sources.iter()
    }

    /// The sheet's measured line weights.
    pub fn weights(&self) -> &LineWeights {
        &self.weights
    }

    /// The lettering table.
    pub fn lettering(&self) -> &Lettering {
        &self.lettering
    }

    /// A text.
    pub fn text(&self, id: impl Into<TextId>) -> Option<&Text> {
        self.texts.get(&id.into())
    }

    /// The texts, in id order.
    pub fn texts(&self) -> impl Iterator<Item = (&TextId, &Text)> {
        self.texts.iter()
    }

    /// The drafting table.
    pub fn drafting(&self) -> &Drafting {
        &self.drafting
    }

    /// A leader.
    pub fn leader(&self, id: impl Into<LeaderId>) -> Option<&Leader> {
        self.leaders.get(&id.into())
    }

    /// The leaders, in id order.
    pub fn leaders(&self) -> impl Iterator<Item = (&LeaderId, &Leader)> {
        self.leaders.iter()
    }

    /// A centre mark by name.
    pub fn centre_mark(&self, id: impl Into<CentreMarkId>) -> Option<&CentreMark> {
        self.centre_marks.get(&id.into())
    }

    /// The centre marks, in id order.
    pub fn centre_marks(&self) -> impl Iterator<Item = (&CentreMarkId, &CentreMark)> {
        self.centre_marks.iter()
    }

    /// A centre line by name.
    pub fn centre_line(&self, id: impl Into<CentreLineId>) -> Option<&CentreLine> {
        self.centre_lines.get(&id.into())
    }

    /// The centre lines, in id order.
    pub fn centre_lines(&self) -> impl Iterator<Item = (&CentreLineId, &CentreLine)> {
        self.centre_lines.iter()
    }

    /// An unmodelled mark.
    pub fn unmodelled(&self, id: impl Into<UnmodelledId>) -> Option<&Unmodelled> {
        self.unmodelled.get(&id.into())
    }

    /// The observed features, by id (docs/redraw-plan.md §8.3): seen on
    /// the sheet, not intended, and never part of the design.
    pub fn observed(&self) -> &BTreeMap<ObservedId, Observed> {
        &self.observed
    }

    /// The marking regions, by id (docs/redraw-plan.md K5): the boxed X
    /// that owns its rectangle's and diagonals' lines.
    pub fn marking_regions(&self) -> &BTreeMap<MarkingRegionId, MarkingRegion> {
        &self.marking_regions
    }

    /// The marking region that owns `entity` of `view`, if any.
    pub fn marking_region_of(
        &self,
        view: &str,
        entity: &str,
    ) -> Option<(&MarkingRegionId, &MarkingRegion)> {
        let (view, entity) = (ViewId::new(view), geometry::EntityId::new(entity));
        self.marking_regions
            .iter()
            .find(|(_, r)| r.view == view && r.has(entity))
    }

    /// The marking regions the printed form item `item` (its name,
    /// [`FormItem::name`](form_value::FormItem::name)) names: the other
    /// end of [`NamedBy`](marking::NamedBy).
    pub fn regions_named_by<'a>(
        &'a self,
        item: &'a str,
    ) -> impl Iterator<Item = (&'a MarkingRegionId, &'a MarkingRegion)> + 'a {
        self.marking_regions.iter().filter(move |(_, r)| {
            r.named_by
                .as_ref()
                .is_some_and(|n| n.note == item || n.box_x.iter().any(|b| b == item))
        })
    }

    /// The unmodelled marks.
    pub fn unmodelled_marks(&self) -> impl Iterator<Item = (&UnmodelledId, &Unmodelled)> {
        self.unmodelled.iter()
    }

    /// A dimension.
    pub fn dimension(&self, id: impl Into<DimensionId>) -> Option<&Dimension> {
        self.dimensions.get(&id.into())
    }

    /// The dimensions, in id order.
    pub fn dimensions(&self) -> impl Iterator<Item = (&DimensionId, &Dimension)> {
        self.dimensions.iter()
    }

    /// The constraint a dimension shows, if it is there.
    pub fn constraint_of(&self, of: Of) -> Option<&sketch::Constraint> {
        match of {
            Of::Sheet { constraint } => self.sheet.constraint(constraint),
            Of::View { view, constraint } => self
                .views
                .view(view)
                .and_then(|v| v.sketch.constraint(constraint)),
        }
    }

    /// The font every text is lettered in.
    pub fn font() -> &'static lmsheets_font::Font {
        static FONT: std::sync::OnceLock<lmsheets_font::Font> = std::sync::OnceLock::new();
        FONT.get_or_init(lmsheets_font::Font::lm_lettering)
    }

    /// The user's notes, in the order made.
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// The sheet read as an engineering drawing, if it has been.
    pub fn part_reading(&self) -> Option<&PartReading> {
        self.reading.as_ref()
    }

    /// The file form, pretty JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// A document from its file form, of any version this reader knows
    /// ([`version`]), validated.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    // ---- the tables

    /// Set the title block.
    pub(crate) fn set_title(&mut self, title: Title) {
        self.title = title;
    }

    /// Set or remove a frame.
    pub(crate) fn set_frame(&mut self, id: FrameId, frame: Option<Frame>) -> Result<(), Error> {
        match frame {
            Some(f) => {
                check_frame(id, &f)?;
                let old = self.frames.insert(id, f);
                if let Err(e) = self.check_references() {
                    match old {
                        Some(o) => self.frames.insert(id, o),
                        None => self.frames.remove(&id),
                    };
                    return Err(e);
                }
            }
            None => {
                let Some(old) = self.frames.remove(&id) else {
                    return Ok(());
                };
                if let Err(e) = self.check_references() {
                    self.frames.insert(id, old);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// Set the claims, all of them.
    pub(crate) fn set_claims(&mut self, claims: Vec<Claim>) -> Result<(), Error> {
        let old = std::mem::replace(&mut self.claims, claims);
        if let Err(e) = self.check_references() {
            self.claims = old;
            return Err(e);
        }
        Ok(())
    }

    /// Place or remove the printed form.
    pub(crate) fn set_form(&mut self, form: Option<Form>) -> Result<(), Error> {
        let old = std::mem::replace(&mut self.form, form);
        if let Err(e) = self.check_references() {
            self.form = old;
            return Err(e);
        }
        Ok(())
    }

    /// Set the observed features, all of them.
    pub(crate) fn set_observed(
        &mut self,
        observed: BTreeMap<ObservedId, Observed>,
    ) -> Result<(), Error> {
        let old = std::mem::replace(&mut self.observed, observed);
        if let Err(e) = self.check_references() {
            self.observed = old;
            return Err(e);
        }
        Ok(())
    }

    /// Set the marking regions, all of them.
    pub(crate) fn set_marking_regions(
        &mut self,
        regions: BTreeMap<MarkingRegionId, MarkingRegion>,
    ) -> Result<(), Error> {
        let old = std::mem::replace(&mut self.marking_regions, regions);
        if let Err(e) = self.check_references() {
            self.marking_regions = old;
            return Err(e);
        }
        Ok(())
    }

    /// Set the gaps, all of them.
    pub(crate) fn set_gaps(&mut self, gaps: BTreeMap<GapId, Gap>) -> Result<(), Error> {
        let old = std::mem::replace(&mut self.gaps, gaps);
        if let Err(e) = self.check_references() {
            self.gaps = old;
            return Err(e);
        }
        Ok(())
    }

    /// Set the residuals, or remove them.
    pub(crate) fn set_residuals(&mut self, residuals: Option<Residuals>) -> Result<(), Error> {
        let old = std::mem::replace(&mut self.residuals, residuals);
        if let Err(e) = self.check_residuals() {
            self.residuals = old;
            return Err(e);
        }
        Ok(())
    }

    /// Add a layer; its tint, if any, must be in the palette.
    pub(crate) fn add_layer(&mut self, id: LayerId, layer: Layer) -> Result<(), Error> {
        if self.layers.contains_key(&id) {
            return Err(Error::DuplicateLayer(id));
        }
        layer.style.check()?;
        self.check_tint(id, &layer)?;
        self.layers.insert(id, layer);
        Ok(())
    }

    fn check_tint(&self, id: LayerId, layer: &Layer) -> Result<(), Error> {
        match layer.tint {
            Some(entry) if !self.palette.contains_key(&entry) => {
                Err(Error::PaletteMissing { entry, by: id })
            }
            _ => Ok(()),
        }
    }

    /// Add a palette entry.
    pub(crate) fn add_palette(&mut self, id: PaletteId, a: Appearance) -> Result<(), Error> {
        if self.palette.contains_key(&id) {
            return Err(Error::DuplicatePalette(id));
        }
        a.check()?;
        self.palette.insert(id, a);
        Ok(())
    }

    /// Change a palette entry.
    pub(crate) fn set_palette(
        &mut self,
        id: impl Into<PaletteId>,
        a: Appearance,
    ) -> Result<Appearance, Error> {
        let id = &id.into();
        a.check()?;
        let slot = self.palette.get_mut(id).ok_or(Error::NoSuchPalette(*id))?;
        Ok(std::mem::replace(slot, a))
    }

    /// Remove a palette entry. Refused while a layer is tinted by it.
    pub(crate) fn remove_palette(&mut self, id: impl Into<PaletteId>) -> Result<Appearance, Error> {
        let id = &id.into();
        let by: Vec<LayerId> = self
            .layers
            .iter()
            .filter(|(_, l)| l.tint.as_ref() == Some(id))
            .map(|(lid, _)| *lid)
            .collect();
        if !by.is_empty() {
            return Err(Error::PaletteInUse { entry: *id, by });
        }
        self.palette.remove(id).ok_or(Error::NoSuchPalette(*id))
    }

    /// Change a layer's style or handling.
    pub(crate) fn set_layer(
        &mut self,
        id: impl Into<LayerId>,
        layer: Layer,
    ) -> Result<Layer, Error> {
        let id = &id.into();
        layer.style.check()?;
        self.check_tint(*id, &layer)?;
        let slot = self.layers.get_mut(id).ok_or(Error::NoSuchLayer(*id))?;
        Ok(std::mem::replace(slot, layer))
    }

    /// Remove a layer. Refused while an entity is on it.
    pub(crate) fn remove_layer(&mut self, id: impl Into<LayerId>) -> Result<Layer, Error> {
        let id = &id.into();
        let by: Vec<Holder> = self
            .layer_holders()
            .filter(|(l, _)| l == id)
            .map(|(_, h)| h)
            .collect();
        if !by.is_empty() {
            return Err(Error::LayerInUse { layer: *id, by });
        }
        self.layers.remove(id).ok_or(Error::NoSuchLayer(*id))
    }

    /// Add a source.
    pub(crate) fn add_source(&mut self, id: DocId, source: Source) -> Result<(), Error> {
        if self.sources.contains_key(&id) {
            return Err(Error::DuplicateSource(id));
        }
        self.sources.insert(id, source);
        Ok(())
    }

    /// Change a source's row.
    pub(crate) fn set_source(
        &mut self,
        id: impl Into<DocId>,
        source: Source,
    ) -> Result<Source, Error> {
        let id = &id.into();
        let slot = self.sources.get_mut(id).ok_or(Error::NoSuchSource(*id))?;
        Ok(std::mem::replace(slot, source))
    }

    /// Remove a source. Refused while a citation names it.
    pub(crate) fn remove_source(&mut self, id: impl Into<DocId>) -> Result<Source, Error> {
        let id = &id.into();
        let by: Vec<Holder> = self
            .citations()
            .filter(|(d, _)| d == id)
            .map(|(_, h)| h)
            .collect();
        if !by.is_empty() {
            return Err(Error::SourceInUse { source: *id, by });
        }
        self.sources.remove(id).ok_or(Error::NoSuchSource(*id))
    }

    /// Add or replace a lettering role.
    pub(crate) fn set_role(
        &mut self,
        id: impl Into<RoleId>,
        role: Role,
    ) -> Result<Option<Role>, Error> {
        Ok(self.lettering.set_role(id, role)?)
    }

    /// Set the width factor all lettering shares.
    pub(crate) fn set_width_factor(&mut self, width_factor: f64) -> Result<(), Error> {
        Ok(self.lettering.set_width_factor(width_factor)?)
    }

    /// Set the line pitch all lettering shares.
    pub(crate) fn set_pitch_of_cap(&mut self, pitch_of_cap: f64) -> Result<(), Error> {
        Ok(self.lettering.set_pitch_of_cap(pitch_of_cap)?)
    }

    /// Set the sheet's one fraction digit size and row spacing.
    pub(crate) fn set_fraction(&mut self, cap: f64, gap_of_cap: f64) -> Result<(), Error> {
        Ok(self.lettering.set_fraction(cap, gap_of_cap)?)
    }

    /// Set, or clear, the measured pen of a kind of line.
    pub(crate) fn set_line_weight(
        &mut self,
        kind: LineKind,
        weight: Option<Measured>,
    ) -> Result<(), Error> {
        Ok(self.weights.set(kind, weight)?)
    }

    /// Remove a role. Refused while a text uses it.
    pub(crate) fn remove_role(&mut self, id: impl Into<RoleId>) -> Result<Role, Error> {
        let id = id.into();
        let by: Vec<TextId> = self
            .texts
            .iter()
            .filter(|(_, t)| matches!(t.size(), Size::Role { role } if *role == id))
            .map(|(tid, _)| *tid)
            .collect();
        if !by.is_empty() {
            return Err(Error::RoleInUse { role: id, by });
        }
        if id == RoleId::new(dimension::ROLE) && !self.dimensions.is_empty() {
            return Err(Error::RoleNeededByDimensions {
                by: self.dimensions.keys().copied().collect(),
            });
        }
        self.lettering.remove_role(id).ok_or(Error::NoSuchRole(id))
    }

    /// Add a text; everything it refers to must exist.
    pub(crate) fn add_text(&mut self, id: TextId, text: Text) -> Result<(), Error> {
        if self.texts.contains_key(&id) {
            return Err(Error::DuplicateText(id));
        }
        self.texts.insert(id, text);
        if let Err(e) = self.check_references() {
            self.texts.remove(&id);
            return Err(e);
        }
        Ok(())
    }

    /// Replace a text.
    pub(crate) fn set_text(&mut self, id: impl Into<TextId>, text: Text) -> Result<Text, Error> {
        let id = id.into();
        let old = self.texts.insert(id, text).ok_or(Error::NoSuchText(id))?;
        if let Err(e) = self.check_references() {
            self.texts.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Remove a text. Refused while another is aligned to it or a leader
    /// leaves from or lands on it.
    pub(crate) fn remove_text(&mut self, id: impl Into<TextId>) -> Result<Text, Error> {
        let id = id.into();
        let by: Vec<TextId> = self
            .texts
            .iter()
            .filter(|(_, t)| matches!(t.at(), Placement::AlignedTo { text, .. } if text == id))
            .map(|(tid, _)| *tid)
            .collect();
        if !by.is_empty() {
            return Err(Error::TextInUse { text: id, by });
        }
        let by: Vec<LeaderId> = self
            .leaders
            .iter()
            .filter(|(_, l)| l.texts().any(|t| t == id))
            .map(|(lid, _)| *lid)
            .collect();
        if !by.is_empty() {
            return Err(Error::TextHeldByLeaders { text: id, by });
        }
        let old = self.texts.remove(&id).ok_or(Error::NoSuchText(id))?;
        // A claim on its ink keeps it.
        if let Err(e) = self.check_references() {
            self.texts.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Replace the drafting table.
    pub(crate) fn set_drafting(&mut self, drafting: Drafting) -> Result<Drafting, Error> {
        drafting.check()?;
        Ok(std::mem::replace(&mut self.drafting, drafting))
    }

    /// Add a leader; everything it refers to must exist.
    pub(crate) fn add_leader(&mut self, id: LeaderId, leader: Leader) -> Result<(), Error> {
        if self.leaders.contains_key(&id) {
            return Err(Error::DuplicateLeader(id));
        }
        self.leaders.insert(id, leader);
        if let Err(e) = self.check_references() {
            self.leaders.remove(&id);
            return Err(e);
        }
        Ok(())
    }

    /// Replace a leader.
    pub(crate) fn set_leader(
        &mut self,
        id: impl Into<LeaderId>,
        leader: Leader,
    ) -> Result<Leader, Error> {
        let id = id.into();
        let old = self
            .leaders
            .insert(id, leader)
            .ok_or(Error::NoSuchLeader(id))?;
        if let Err(e) = self.check_references() {
            self.leaders.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Remove a leader. Only a claim on its ink refers to a leader.
    pub(crate) fn remove_leader(&mut self, id: impl Into<LeaderId>) -> Result<Leader, Error> {
        let id = id.into();
        let old = self.leaders.remove(&id).ok_or(Error::NoSuchLeader(id))?;
        if let Err(e) = self.check_references() {
            self.leaders.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Add a centre mark; what it marks must be circles or arcs of the
    /// drawing.
    pub(crate) fn add_centre_mark(&mut self, id: CentreMarkId, m: CentreMark) -> Result<(), Error> {
        if self.centre_marks.contains_key(&id) {
            return Err(Error::DuplicateCentre(id.to_string()));
        }
        self.centre_marks.insert(id, m);
        if let Err(e) = self.check_references() {
            self.centre_marks.remove(&id);
            return Err(e);
        }
        Ok(())
    }

    /// Replace a centre mark.
    pub(crate) fn set_centre_mark(
        &mut self,
        id: CentreMarkId,
        m: CentreMark,
    ) -> Result<CentreMark, Error> {
        let old = self
            .centre_marks
            .insert(id, m)
            .ok_or_else(|| Error::NoSuchCentre(id.to_string()))?;
        if let Err(e) = self.check_references() {
            self.centre_marks.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Remove a centre mark.
    pub(crate) fn remove_centre_mark(&mut self, id: CentreMarkId) -> Result<CentreMark, Error> {
        let old = self
            .centre_marks
            .remove(&id)
            .ok_or_else(|| Error::NoSuchCentre(id.to_string()))?;
        // A dimension it serves keeps it.
        if let Err(e) = self.check_references() {
            self.centre_marks.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Add a centre line; its points must be in the drawing.
    pub(crate) fn add_centre_line(&mut self, id: CentreLineId, l: CentreLine) -> Result<(), Error> {
        if self.centre_lines.contains_key(&id) {
            return Err(Error::DuplicateCentre(id.to_string()));
        }
        self.centre_lines.insert(id, l);
        if let Err(e) = self.check_references() {
            self.centre_lines.remove(&id);
            return Err(e);
        }
        Ok(())
    }

    /// Replace a centre line.
    pub(crate) fn set_centre_line(
        &mut self,
        id: CentreLineId,
        l: CentreLine,
    ) -> Result<CentreLine, Error> {
        let old = self
            .centre_lines
            .insert(id, l)
            .ok_or_else(|| Error::NoSuchCentre(id.to_string()))?;
        if let Err(e) = self.check_references() {
            self.centre_lines.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Remove a centre line.
    pub(crate) fn remove_centre_line(&mut self, id: CentreLineId) -> Result<CentreLine, Error> {
        let old = self
            .centre_lines
            .remove(&id)
            .ok_or_else(|| Error::NoSuchCentre(id.to_string()))?;
        // A dimension it serves keeps it.
        if let Err(e) = self.check_references() {
            self.centre_lines.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Add an unmodelled mark.
    pub(crate) fn add_unmodelled(&mut self, id: UnmodelledId, u: Unmodelled) -> Result<(), Error> {
        u.check().map_err(Error::Unmodelled)?;
        if self.unmodelled.contains_key(&id) {
            return Err(Error::DuplicateUnmodelled(id));
        }
        self.unmodelled.insert(id, u);
        if let Err(e) = self.check_references() {
            self.unmodelled.remove(&id);
            return Err(e);
        }
        Ok(())
    }

    /// Replace an unmodelled mark.
    pub(crate) fn set_unmodelled(
        &mut self,
        id: UnmodelledId,
        u: Unmodelled,
    ) -> Result<Unmodelled, Error> {
        u.check().map_err(Error::Unmodelled)?;
        let old = self
            .unmodelled
            .insert(id, u)
            .ok_or(Error::NoSuchUnmodelled(id))?;
        if let Err(e) = self.check_references() {
            self.unmodelled.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Remove an unmodelled mark.
    pub(crate) fn remove_unmodelled(&mut self, id: UnmodelledId) -> Result<Unmodelled, Error> {
        let old = self
            .unmodelled
            .remove(&id)
            .ok_or(Error::NoSuchUnmodelled(id))?;
        // A claim on its ink keeps it.
        if let Err(e) = self.check_references() {
            self.unmodelled.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Add a dimension; its constraint must exist and match its family.
    pub(crate) fn add_dimension(&mut self, id: DimensionId, d: Dimension) -> Result<(), Error> {
        if self.dimensions.contains_key(&id) {
            return Err(Error::DuplicateDimension(id));
        }
        self.dimensions.insert(id, d);
        if let Err(e) = self.check_references() {
            self.dimensions.remove(&id);
            return Err(e);
        }
        Ok(())
    }

    /// Replace a dimension.
    pub(crate) fn set_dimension(
        &mut self,
        id: impl Into<DimensionId>,
        d: Dimension,
    ) -> Result<Dimension, Error> {
        let id = id.into();
        let old = self
            .dimensions
            .insert(id, d)
            .ok_or(Error::NoSuchDimension(id))?;
        if let Err(e) = self.check_references() {
            self.dimensions.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Remove a dimension. Only a claim on its ink (or its witness
    /// lines') refers to a dimension.
    pub(crate) fn remove_dimension(
        &mut self,
        id: impl Into<DimensionId>,
    ) -> Result<Dimension, Error> {
        let id = id.into();
        let old = self
            .dimensions
            .remove(&id)
            .ok_or(Error::NoSuchDimension(id))?;
        if let Err(e) = self.check_references() {
            self.dimensions.insert(id, old);
            return Err(e);
        }
        Ok(old)
    }

    /// Everything a dimension refers to: its layer, and a constraint of
    /// its family.
    fn check_dimension(&self, id: DimensionId, d: &Dimension) -> Result<(), Error> {
        if !self.layers.contains_key(&d.layer()) {
            return Err(Error::LayerMissing {
                layer: d.layer(),
                by: Holder::Dimension(id),
            });
        }
        let of = d.of();
        let rel = self
            .constraint_of(of)
            .map(|c| c.relation())
            .ok_or(Error::DimensionConstraintMissing { of, by: id })?;
        let kind = rel
            .dimension_kind()
            .ok_or(Error::DimensionConstraintNotDimensional { of, by: id })?;
        if kind != d.layout().kind() {
            return Err(Error::DimensionKindMismatch {
                of,
                constraint: kind,
                layout: d.layout().kind(),
                by: id,
            });
        }
        if self.lettering.role(dimension::ROLE).is_none() {
            return Err(Error::NoSuchRole(RoleId::new(dimension::ROLE)));
        }
        let font = Self::font();
        for s in [&d.text().prefix, &d.text().suffix].into_iter().flatten() {
            if let Some(ch) = s.chars().find(|c| font.glyph(*c).is_none()) {
                return Err(Error::DimensionGlyphMissing { ch, by: id });
            }
        }
        match d.layout() {
            // Across is a diameter's layout only.
            Layout::Across(_) if !matches!(rel.circle(), Some((_, Round::Diameter))) => {
                return Err(Error::AcrossNeedsDiameter { of, by: id });
            }
            Layout::Radial(_) | Layout::Ordinate(_) => {
                let at = match d.layout() {
                    Layout::Radial(r) => r.at,
                    Layout::Ordinate(o) => o.at,
                    _ => unreachable!(),
                };
                match at {
                    Placement::Beside { point, .. } if !self.has_point(point) => {
                        return Err(Error::DimensionPointMissing { point, by: id });
                    }
                    Placement::AlignedTo { text, .. } if !self.texts.contains_key(&text) => {
                        return Err(Error::DimensionTextMissing { text, by: id });
                    }
                    _ => {}
                }
            }
            Layout::Linear(l) => {
                if let Some(span) = rel.span() {
                    for (end, ext) in l.ext.iter().enumerate() {
                        if let Extension::By(by_line) = *ext {
                            if !self.serves(of, span, end, by_line) {
                                return Err(Error::DimensionServedWrongly {
                                    end,
                                    by_line,
                                    by: id,
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether `by` can serve as the extension line at a linear span's
    /// `end` (0 its first, 1 its second): the measured point is on it.
    fn serves(&self, of: Of, span: Span, end: usize, by: Serving) -> bool {
        // The end measured: a point, or a line (a foot on it).
        let (point, line) = match (span, end) {
            (Span::Points { a, .. }, 0) | (Span::PointLine { p: a, .. }, 0) => (Some(a), None),
            (Span::Points { b, .. }, _) => (Some(b), None),
            (Span::PointLine { l, .. }, _) => (None, Some(l.id())),
            (Span::Lines { l, .. }, 0) => (None, Some(l.id())),
            (Span::Lines { m, .. }, _) => (None, Some(m.id())),
        };
        let at = |point| match of {
            Of::Sheet { .. } => DrawingPoint::Sheet { point },
            Of::View { view, .. } => DrawingPoint::View { view, point },
        };
        let entity = |id| match of {
            Of::Sheet { .. } => DrawingEntity::Sheet { id },
            Of::View { view, .. } => DrawingEntity::View { view, id },
        };
        match by {
            Serving::CentreLine { line: cl } => point.is_some_and(|p| {
                self.centre_lines
                    .get(&cl)
                    .is_some_and(|c| c.through().contains(&at(p)))
            }),
            Serving::CentreMark { mark } => point.and_then(|p| p.centre_of()).is_some_and(|e| {
                self.centre_marks
                    .get(&mark)
                    .is_some_and(|m| m.of().contains(&entity(e)))
            }),
            Serving::Line { line: id } => {
                point.map_or(line, |p| Some(p.id())) == Some(id)
                    && self.entity_kind(entity(id)) == Some(Kind::Line)
            }
        }
    }

    /// Add a note; it must be somewhere on the paper.
    pub(crate) fn add_note(&mut self, note: Note) -> Result<(), Error> {
        note.check()?;
        self.notes.push(note);
        Ok(())
    }

    /// Remove a note by position.
    pub(crate) fn remove_note(&mut self, index: usize) -> Option<Note> {
        (index < self.notes.len()).then(|| self.notes.remove(index))
    }

    /// Set the part reading (or take it away), returning the old one.
    pub(crate) fn set_part_reading(
        &mut self,
        reading: Option<PartReading>,
    ) -> Result<Option<PartReading>, Error> {
        if let Some(r) = &reading {
            r.check().map_err(Error::PartReading)?;
        }
        Ok(std::mem::replace(&mut self.reading, reading))
    }

    // ---- the sketch and the views

    /// Edit the paper sketch through `f`, which is given the parameter
    /// table. Layers and citations are re-checked afterwards; if one
    /// would dangle, the edit is rolled back and the error returned.
    pub(crate) fn edit_sheet<T>(
        &mut self,
        f: impl FnOnce(&mut Sketch<Paper>, &Params) -> Result<T, sketch::Error>,
    ) -> Result<T, Error> {
        let before = self.sheet.clone();
        let out = match f(&mut self.sheet, &self.params) {
            Ok(t) => t,
            Err(e) => {
                self.sheet = before;
                return Err(e.into());
            }
        };
        if let Err(e) = self.check_references() {
            self.sheet = before;
            return Err(e);
        }
        Ok(out)
    }

    /// Edit one view's sketch through `f`, which is given the parameter
    /// table, with the same re-check and rollback as
    /// [`edit_sheet`](Self::edit_sheet).
    pub(crate) fn edit_view<T>(
        &mut self,
        id: impl Into<ViewId>,
        f: impl FnOnce(&mut Sketch<Model>, &Params) -> Result<T, sketch::Error>,
    ) -> Result<T, Error> {
        let id = id.into();
        let before = self.views.clone();
        let params = &self.params;
        let out = match self.views.edit(id, |s| f(s, params)) {
            Ok(t) => t,
            Err(e) => {
                self.views = before;
                return Err(e.into());
            }
        };
        if let Err(e) = self.check_references() {
            self.views = before;
            return Err(e);
        }
        Ok(out)
    }

    // ---- parameters

    /// Set or add a parameter; returns the old one if any. The new value
    /// must fit every constraint, in any sketch, that names it, and its
    /// citations must resolve; otherwise nothing changes.
    pub(crate) fn set_param(
        &mut self,
        name: ParamName,
        param: Param,
    ) -> Result<Option<Param>, Error> {
        let mut table = self.params.clone();
        let old = table.set(name, param)?;
        self.check_params_in(&table)?;
        let before = std::mem::replace(&mut self.params, table);
        if let Err(e) = self.check_references() {
            self.params = before;
            return Err(e);
        }
        Ok(old)
    }

    /// Remove a parameter. Refused while a constraint names it.
    pub(crate) fn remove_param(&mut self, name: impl Into<ParamName>) -> Result<Param, Error> {
        let name = name.into();
        if self.params.get(name).is_none() {
            return Err(params::Error::NoSuchParam(name).into());
        }
        let by = self.users_of_param(name);
        if !by.is_empty() {
            return Err(Error::ParamInUse { name, by });
        }
        Ok(self.params.remove(name)?)
    }

    /// Every constraint in every sketch that names a parameter resolves
    /// in `table` and restates nothing.
    fn check_params_in(&self, table: &Params) -> Result<(), Error> {
        self.sheet.check_params(table)?;
        for (_, v) in self.views.views() {
            v.sketch.check_params(table)?;
        }
        Ok(())
    }

    /// Edit the views through `f`, with the same re-check and rollback as
    /// [`edit_sheet`](Self::edit_sheet).
    pub(crate) fn edit_views<T>(
        &mut self,
        f: impl FnOnce(&mut Views) -> Result<T, views::Error>,
    ) -> Result<T, Error> {
        let before = self.views.clone();
        let out = match f(&mut self.views) {
            Ok(t) => t,
            Err(e) => {
                self.views = before;
                return Err(e.into());
            }
        };
        if let Err(e) = self.check_references() {
            self.views = before;
            return Err(e);
        }
        Ok(out)
    }

    /// Solve the sheet sketch and the views as a system, moving geometry
    /// to the solutions; the two analyses. Both or neither: on an error
    /// the document is as it was.
    pub(crate) fn solve(
        &mut self,
        o: crate::solver::solve::Options,
    ) -> Result<(sketch::Analysis, views::SystemAnalysis), Error> {
        let (mut sheet, mut views) = (self.sheet.clone(), self.views.clone());
        let analyses = (sheet.solve(o, &self.params)?, views.solve(o, &self.params)?);
        (self.sheet, self.views) = (sheet, views);
        Ok(analyses)
    }

    /// The same analyses at the current state, without moving anything.
    pub fn analyse(
        &self,
        o: crate::solver::solve::Options,
    ) -> Result<(sketch::Analysis, views::SystemAnalysis), Error> {
        Ok((
            self.sheet.analyse(o, &self.params)?,
            self.views.analyse(o, &self.params)?,
        ))
    }

    // ---- invariants

    /// Everything a text refers to: its layer, its role, the point it sits
    /// beside, the text it is aligned to (and no ring of those), and a
    /// glyph for every character.
    fn check_text(&self, id: TextId, t: &Text) -> Result<(), Error> {
        if !self.layers.contains_key(&t.layer()) {
            return Err(Error::LayerMissing {
                layer: t.layer(),
                by: Holder::Text(id),
            });
        }
        if let Size::Role { role } = t.size() {
            if self.lettering.role(*role).is_none() {
                return Err(Error::RoleMissing {
                    role: *role,
                    by: id,
                });
            }
        }
        match t.at() {
            Placement::Paper { .. } => {}
            Placement::Beside { point, .. } => {
                if !self.has_point(point) {
                    return Err(Error::PointMissing { point, by: id });
                }
            }
            Placement::AlignedTo { text, .. } => {
                if text == id {
                    return Err(Error::Text(text::Error::SelfAligned(id)));
                }
                // Follow the chain; it must end at a text that is placed
                // on the paper or beside the drawing, never come back.
                let mut seen = vec![id];
                let mut next = text;
                loop {
                    let Some(t) = self.texts.get(&next) else {
                        return Err(Error::TextMissing {
                            text: next,
                            by: *seen.last().expect("one"),
                        });
                    };
                    if seen.contains(&next) {
                        return Err(Error::TextCycle(id));
                    }
                    seen.push(next);
                    match t.at() {
                        Placement::AlignedTo { text, .. } => next = text,
                        _ => break,
                    }
                }
            }
        }
        let font = Self::font();
        if let Some(ch) = t.chars().find(|c| font.glyph(*c).is_none()) {
            return Err(Error::GlyphMissing { ch, by: id });
        }
        Ok(())
    }

    /// Whether a point of the drawing is there, of the kind named.
    fn has_point(&self, p: DrawingPoint) -> bool {
        match p {
            DrawingPoint::Sheet { point } => self.sheet.geometry().position(&point).is_ok(),
            DrawingPoint::View { view, point } => self
                .views
                .view(view)
                .is_some_and(|v| v.sketch.geometry().position(&point).is_ok()),
        }
    }

    /// The kind of an entity of the drawing, if it is there.
    fn entity_kind(&self, e: DrawingEntity) -> Option<Kind> {
        match e {
            DrawingEntity::Sheet { id } => self.sheet.geometry().get(id).map(|e| e.kind()),
            DrawingEntity::View { view, id } => self
                .views
                .view(view)
                .and_then(|v| v.sketch.geometry().get(id))
                .map(|e| e.kind()),
        }
    }

    /// Everything a centre mark refers to: circles and arcs of the
    /// drawing, all of one sketch and concentric with the first through
    /// the sketch's constraints, so a re-solve keeps them on one centre.
    fn check_centre_mark(&self, id: CentreMarkId, m: &CentreMark) -> Result<(), Error> {
        for e in m.of() {
            if !matches!(self.entity_kind(*e), Some(Kind::Arc | Kind::Circle)) {
                return Err(Error::CentreMarkFeatureMissing { entity: *e, by: id });
            }
        }
        if let Some((first, rest)) = m.of().split_first() {
            for e in rest {
                if !self.concentric(*first, *e) {
                    return Err(Error::CentreMarkNotConcentric { entity: *e, by: id });
                }
            }
        }
        Ok(())
    }

    /// Whether two circles or arcs are one entity, or of one sketch and
    /// held on one centre by its constraints (through any chain of
    /// them).
    fn concentric(&self, a: DrawingEntity, b: DrawingEntity) -> bool {
        let (sketch, a, b) = match (a, b) {
            (DrawingEntity::Sheet { id: a }, DrawingEntity::Sheet { id: b }) => {
                (&self.sheet.constraints().collect::<Vec<_>>(), a, b)
            }
            (DrawingEntity::View { view, id: a }, DrawingEntity::View { view: w, id: b })
                if view == w =>
            {
                let Some(v) = self.views.view(view) else {
                    return false;
                };
                (&v.sketch.constraints().collect::<Vec<_>>(), a, b)
            }
            _ => return false,
        };
        let pairs: Vec<_> = sketch
            .iter()
            .filter_map(|(_, c)| c.relation().same_centre())
            .collect();
        // A walk from `a` over the pairs.
        let mut seen = vec![a];
        let mut i = 0;
        while i < seen.len() {
            let e = seen[i];
            for &(x, y) in &pairs {
                for (p, q) in [(x, y), (y, x)] {
                    if p == e && !seen.contains(&q) {
                        seen.push(q);
                    }
                }
            }
            i += 1;
        }
        seen.contains(&b)
    }

    fn check_centre_line(&self, id: CentreLineId, l: &CentreLine) -> Result<(), Error> {
        for p in l.through() {
            if !self.has_point(*p) {
                return Err(Error::CentreLinePointMissing { point: *p, by: id });
            }
        }
        Ok(())
    }

    /// Everything a leader refers to: its layer, the text it leaves and
    /// any it lands on (with their anchors), the points and curves its
    /// tips land on, the views its free points sit in, and the entities
    /// it stands for.
    fn check_leader(&self, id: LeaderId, l: &Leader) -> Result<(), Error> {
        if !self.layers.contains_key(&l.layer()) {
            return Err(Error::LayerMissing {
                layer: l.layer(),
                by: Holder::Leader(id),
            });
        }
        let anchored = |text: TextId, anchor: Anchor| {
            let Some(t) = self.texts.get(&text) else {
                return Err(Error::LeaderTextMissing { text, by: id });
            };
            match anchor {
                Anchor::Line { index, .. } if index >= t.lines().len() => {
                    Err(Error::LeaderAnchorMissing {
                        text,
                        anchor,
                        by: id,
                    })
                }
                _ => Ok(()),
            }
        };
        // A target (or a point the leader leaves) the drawing has.
        let target = |target: Target| -> Result<(), Error> {
            let missing = Error::LeaderTargetMissing { target, by: id };
            match target {
                Target::Point { point } => {
                    if !self.has_point(point) {
                        return Err(missing);
                    }
                }
                Target::Along { entity, .. } => match self.entity_kind(entity) {
                    Some(Kind::Line | Kind::Arc | Kind::Circle) => {}
                    _ => return Err(missing),
                },
                Target::Text { text, anchor } => anchored(text, anchor)?,
                Target::Paper { .. } => {}
                Target::InView { view, .. } => {
                    if self.views.view(view).is_none() {
                        return Err(missing);
                    }
                }
            }
            Ok(())
        };
        match l.from() {
            crate::value::leader::Origin::Text { text, anchor } => anchored(text, anchor)?,
            crate::value::leader::Origin::Enclosure { text, .. } => match self.texts.get(&text) {
                None => return Err(Error::LeaderTextMissing { text, by: id }),
                Some(t) if t.enclosure().is_none() => {
                    return Err(Error::LeaderOriginNotEnclosed { text, by: id })
                }
                Some(_) => {}
            },
            crate::value::leader::Origin::Point { at } => target(at)?,
        }
        for tip in l.tips() {
            target(tip.target)?;
        }
        for e in l.refers_to() {
            if self.entity_kind(*e).is_none() {
                return Err(Error::LeaderEntityMissing { entity: *e, by: id });
            }
        }
        Ok(())
    }

    /// Every (layer, holder) pair in the document: entities, texts and
    /// leaders.
    fn layer_holders(&self) -> impl Iterator<Item = (LayerId, Holder)> + '_ {
        let sheet = self
            .sheet
            .entities()
            .map(|(id, _, m)| (m.layer, Holder::SheetEntity(*id)));
        let views = self.views.views().flat_map(|(vid, v)| {
            v.sketch
                .entities()
                .map(move |(id, _, m)| (m.layer, Holder::Entity(*vid, *id)))
        });
        let texts = self
            .texts
            .iter()
            .map(|(id, t)| (t.layer(), Holder::Text(*id)));
        let leaders = self
            .leaders
            .iter()
            .map(|(id, l)| (l.layer(), Holder::Leader(*id)));
        let dimensions = self
            .dimensions
            .iter()
            .map(|(id, d)| (d.layer(), Holder::Dimension(*id)));
        let marks = self
            .centre_marks
            .iter()
            .map(|(id, m)| (m.layer(), Holder::CentreMark(*id)));
        let lines = self
            .centre_lines
            .iter()
            .map(|(id, l)| (l.layer(), Holder::CentreLine(*id)));
        let unmodelled = self
            .unmodelled
            .iter()
            .map(|(id, u)| (u.layer(), Holder::Unmodelled(*id)));
        let observed = self
            .observed
            .iter()
            .map(|(id, o)| (o.layer, Holder::Observed(*id)));
        sheet
            .chain(views)
            .chain(texts)
            .chain(leaders)
            .chain(dimensions)
            .chain(marks)
            .chain(lines)
            .chain(unmodelled)
            .chain(observed)
    }

    /// Every (cited document, holder) pair in the document.
    fn citations(&self) -> impl Iterator<Item = (DocId, Holder)> + '_ {
        fn cited(p: &Provenance, h: Holder) -> impl Iterator<Item = (DocId, Holder)> + '_ {
            p.evidence().iter().map(move |c| (c.doc, h.clone()))
        }
        let sheet_entities = self
            .sheet
            .entities()
            .flat_map(|(id, _, m)| cited(&m.provenance, Holder::SheetEntity(*id)));
        let sheet_constraints = self
            .sheet
            .constraints()
            .flat_map(|(id, c)| cited(c.provenance(), Holder::SheetConstraint(*id)));
        let params = self
            .params
            .iter()
            .flat_map(|(n, p)| cited(p.provenance(), Holder::Param(*n)));
        let views = self.views.views().flat_map(|(vid, v)| {
            let entities = v
                .sketch
                .entities()
                .flat_map(move |(id, _, m)| cited(&m.provenance, Holder::Entity(*vid, *id)));
            let constraints = v
                .sketch
                .constraints()
                .flat_map(move |(id, c)| cited(c.provenance(), Holder::Constraint(*vid, *id)));
            let frame = v
                .model_frame
                .iter()
                .flat_map(move |p| cited(p, Holder::Frame(*vid)));
            entities.chain(constraints).chain(frame)
        });
        let links = self
            .views
            .links()
            .flat_map(|(lid, l)| cited(l.provenance(), Holder::Link(*lid)));
        let texts = self.texts.iter().flat_map(|(tid, t)| {
            let drawn = t.drawn().map(|d| d.provenance());
            std::iter::once(t.provenance())
                .chain(drawn)
                .flat_map(move |p| cited(p, Holder::Text(*tid)))
        });
        let leaders = self
            .leaders
            .iter()
            .flat_map(|(lid, l)| cited(l.provenance(), Holder::Leader(*lid)));
        let dimensions = self.dimensions.iter().flat_map(|(did, d)| {
            let drawn = d.drawn().map(|a| a.provenance());
            std::iter::once(d.provenance())
                .chain(drawn)
                .flat_map(move |p| cited(p, Holder::Dimension(*did)))
        });
        let marks = self
            .centre_marks
            .iter()
            .flat_map(|(id, m)| cited(m.provenance(), Holder::CentreMark(*id)));
        let lines = self
            .centre_lines
            .iter()
            .flat_map(|(id, l)| cited(l.provenance(), Holder::CentreLine(*id)));
        let unmodelled = self
            .unmodelled
            .iter()
            .flat_map(|(id, u)| cited(u.provenance(), Holder::Unmodelled(*id)));
        let observed = self
            .observed
            .iter()
            .flat_map(|(id, o)| cited(&o.provenance, Holder::Observed(*id)));
        let regions = self
            .marking_regions
            .iter()
            .flat_map(|(id, r)| cited(&r.provenance, Holder::MarkingRegion(*id)));
        let frames = self.frames.iter().flat_map(|(id, f)| {
            let r = &f.registration;
            [
                &r.crop.provenance,
                &r.warp.provenance,
                &r.rotation.provenance,
                &r.paper.provenance,
            ]
            .into_iter()
            .flat_map(move |p| cited(p, Holder::Registration(*id)))
        });
        let form = self.form.iter().flat_map(|f| {
            let fields = f.entries().flat_map(|(field, t)| {
                let drawn = t.drawn().map(|d| d.provenance());
                std::iter::once(t.provenance())
                    .chain(drawn)
                    .flat_map(move |p| cited(p, Holder::Field(field)))
            });
            cited(&f.printed, Holder::Form).chain(fields)
        });
        sheet_entities
            .chain(sheet_constraints)
            .chain(params)
            .chain(views)
            .chain(links)
            .chain(texts)
            .chain(leaders)
            .chain(dimensions)
            .chain(marks)
            .chain(lines)
            .chain(unmodelled)
            .chain(observed)
            .chain(regions)
            .chain(frames)
            .chain(form)
    }

    /// Every layer held exists; every tint is in the palette; every
    /// document cited is registered.
    fn check_references(&self) -> Result<(), Error> {
        for (layer, by) in self.layer_holders() {
            if !self.layers.contains_key(&layer) {
                return Err(Error::LayerMissing { layer, by });
            }
        }
        for (id, l) in &self.layers {
            self.check_tint(*id, l)?;
        }
        for (id, t) in &self.texts {
            self.check_text(*id, t)?;
        }
        for (id, l) in &self.leaders {
            self.check_leader(*id, l)?;
        }
        for (id, d) in &self.dimensions {
            self.check_dimension(*id, d)?;
        }
        for (id, m) in &self.centre_marks {
            self.check_centre_mark(*id, m)?;
        }
        for (id, l) in &self.centre_lines {
            self.check_centre_line(*id, l)?;
        }
        for (source, by) in self.citations() {
            if !self.sources.contains_key(&source) {
                return Err(Error::SourceMissing { source, by });
            }
        }
        self.check_form()?;
        self.check_observed()?;
        self.check_marking_regions()?;
        self.check_claims()?;
        self.check_gaps()?;
        self.check_residuals()
    }

    /// Each observed feature valid on its own, in a view the document
    /// has, its junction two lines of that view, and what it graduated
    /// to (if anything) the view's.
    fn check_observed(&self) -> Result<(), Error> {
        use crate::value::provenance::ItemRef;
        for (id, o) in &self.observed {
            let bad = |error| Error::BadObserved {
                observed: *id,
                error,
            };
            o.check().map_err(bad)?;
            let view = self
                .views
                .view(o.view)
                .ok_or(bad(observed::Error::NoView(o.view)))?;
            for line in o.junction().iter().flatten() {
                if !matches!(
                    view.sketch.geometry().get(*line),
                    Some(geometry::Entity::Line { .. })
                ) {
                    return Err(bad(observed::Error::NoLine(*line)));
                }
            }
            match o.graduated_to {
                Some(ItemRef::Entity(e)) if view.sketch.geometry().get(e).is_none() => {
                    return Err(bad(observed::Error::NoGraduate(ItemRef::Entity(e))));
                }
                Some(ItemRef::Constraint(c)) if view.sketch.constraint(c).is_none() => {
                    return Err(bad(observed::Error::NoGraduate(ItemRef::Constraint(c))));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Each marking region valid on its own, in a view the document has,
    /// its six lines lines of that view and no other region's, and the
    /// form items that name it items of the placed form.
    fn check_marking_regions(&self) -> Result<(), Error> {
        for (id, r) in &self.marking_regions {
            let bad = |error| Error::BadMarkingRegion { region: *id, error };
            r.check().map_err(bad)?;
            let view = self
                .views
                .view(r.view)
                .ok_or(bad(marking::Error::NoView(r.view)))?;
            for line in r.lines() {
                if !matches!(
                    view.sketch.geometry().get(line),
                    Some(geometry::Entity::Line { .. })
                ) {
                    return Err(bad(marking::Error::NoLine(line)));
                }
                let owner = self
                    .marking_regions
                    .iter()
                    .find(|(other, o)| *other != id && o.view == r.view && o.has(line));
                if owner.is_some() {
                    return Err(bad(marking::Error::Shared(line)));
                }
            }
            if let Some(n) = &r.named_by {
                let template = self.form.as_ref().and_then(|f| f.template().ok());
                for name in std::iter::once(&n.note).chain(&n.box_x) {
                    let held = template.as_ref().is_some_and(|t| {
                        form_value::FormItem::parse(name).is_some_and(|i| t.has(i))
                    });
                    if !held {
                        return Err(bad(marking::Error::NoFormItem(name.clone())));
                    }
                }
            }
        }
        Ok(())
    }

    /// The placed form valid against its template, its entries' layers
    /// the table's, and the stored title not giving what its fields give.
    fn check_form(&self) -> Result<(), Error> {
        let Some(f) = &self.form else {
            return Ok(());
        };
        f.check().map_err(Error::BadForm)?;
        for (field, t) in f.entries() {
            if !self.layers.contains_key(&t.layer()) {
                return Err(Error::LayerMissing {
                    layer: t.layer(),
                    by: Holder::Field(field),
                });
            }
        }
        let has = |field: &str| f.fields.contains_key(&FieldId::new(field));
        if (has(DRAWING_FIELD) && !self.title.drawing.is_empty())
            || (has(SHEET_FIELD) && !self.title.sheet.is_empty())
        {
            return Err(Error::TitleBesideForm);
        }
        Ok(())
    }

    /// Each gap valid on its own, its region on a frame the document has,
    /// and its nodes the document's.
    fn check_gaps(&self) -> Result<(), Error> {
        for (id, g) in &self.gaps {
            let bad = |error| Error::BadGap { gap: *id, error };
            g.check().map_err(bad)?;
            if let Some(r) = g
                .region
                .as_ref()
                .filter(|r| !self.frames.contains_key(&r.frame))
            {
                return Err(bad(gaps::Error::NoFrame(r.frame)));
            }
            if let Some(n) = g.nodes.iter().find(|n| !self.has_node(n)) {
                return Err(bad(gaps::Error::NoNode(n.clone())));
            }
        }
        Ok(())
    }

    /// The residuals valid on their own, and every id they name the
    /// document's: the view, its dimensions, its constraints and entities.
    fn check_residuals(&self) -> Result<(), Error> {
        let Some(r) = &self.residuals else {
            return Ok(());
        };
        let bad = Error::BadResiduals;
        r.check().map_err(bad)?;
        let view = self
            .views
            .view(r.view)
            .ok_or(bad(residuals::Error::NoView(r.view)))?;
        let constraint = |c: &provenance::ConstraintId| {
            view.sketch
                .constraint(*c)
                .map(|_| ())
                .ok_or(bad(residuals::Error::NoConstraint(*c)))
        };
        for id in r.dimensions.keys() {
            if !self.dimensions.contains_key(id) {
                return Err(bad(residuals::Error::NoDimension(*id)));
            }
        }
        for id in r.relations.keys() {
            constraint(id)?;
        }
        for (id, e) in &r.entities {
            if view.sketch.geometry().get(*id).is_none() {
                return Err(bad(residuals::Error::NoEntity(*id)));
            }
            if let Some(m) = &e.moved_by {
                constraint(&m.constraint)?;
            }
        }
        Ok(())
    }

    /// Each claim citing a frame's evidence, valid against it, and naming
    /// a node the document has.
    fn check_claims(&self) -> Result<(), Error> {
        let mut known: BTreeMap<FrameId, std::collections::HashSet<SegmentId>> = BTreeMap::new();
        for (k, c) in self.claims.iter().enumerate() {
            let bad = |error| Error::BadClaim { claim: k, error };
            let evidence = self
                .frames
                .get(&c.evidence)
                .and_then(|f| f.evidence.as_ref())
                .ok_or(bad(claims::Error::NoEvidence(c.evidence)))?;
            let segments = known
                .entry(c.evidence)
                .or_insert_with(|| evidence.segments.iter().copied().collect());
            c.check(evidence, segments).map_err(bad)?;
            if !self.has_node(&c.node) {
                return Err(bad(claims::Error::NoNode(c.node.clone())));
            }
        }
        Ok(())
    }

    /// Whether the document has a node a claim names: a drawing node by
    /// its id (an entity of a view as `view/id`, a witness line by its
    /// dimensions'), and any non-drawing node.
    fn has_node(&self, n: &claims::Node) -> bool {
        use claims::Kind;
        let name = n.name.as_str();
        match n.kind {
            Kind::Entity => match name.split_once('/') {
                Some((v, id)) => self
                    .views
                    .view(v)
                    .is_some_and(|v| v.sketch.geometry().get(id).is_some()),
                None => self.sheet.geometry().get(name).is_some(),
            },
            Kind::Text => self.texts.contains_key(&name.into()),
            Kind::Leader => self.leaders.contains_key(&name.into()),
            Kind::Dimension => self.dimensions.contains_key(&name.into()),
            Kind::Witness => name
                .split(',')
                .all(|d| self.dimensions.contains_key(&d.into())),
            Kind::CentreMark => self.centre_marks.contains_key(&name.into()),
            Kind::CentreLine => self.centre_lines.contains_key(&name.into()),
            Kind::Unmodelled => self.unmodelled.contains_key(&name.into()),
            Kind::Observed => self.observed.contains_key(&name.into()),
            Kind::MarkingRegion => self.marking_regions.contains_key(&name.into()),
            Kind::Form => self.form.as_ref().is_some_and(|f| {
                form_value::FormItem::parse(name)
                    .is_some_and(|i| f.template().is_ok_and(|t| t.has(i)))
            }),
            Kind::Field => self.form.as_ref().is_some_and(|f| {
                f.fields
                    .get(&FieldId::new(name))
                    .is_some_and(Option::is_some)
            }),
            Kind::Noise | Kind::Foreign | Kind::Pencil | Kind::Ignored => true,
        }
    }
}

/// The field a form's drawing number is entered in.
const DRAWING_FIELD: &str = "drawing_number";
/// The field a form's sheet number is entered in.
const SHEET_FIELD: &str = "sheet";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::geometry::{Entity, Model};
    use crate::value::provenance::{Citation, Method};
    use crate::value::sketch::EntityMeta;
    use crate::value::view::{ModelAxis, Transform, View};
    use approx::assert_abs_diff_eq;

    fn day() -> Date {
        Date::new(2026, 10, 15).unwrap()
    }

    fn ours() -> Provenance {
        Provenance::new(Method::Authored, Agent::Claude, day())
    }

    fn cited(doc: &str) -> Provenance {
        Provenance::new(Method::Read, Agent::Claude, day()).cite(Citation::new(doc, "zone A1"))
    }

    fn line<F: geometry::Frame>(x: f64) -> Entity<F> {
        Entity::Line {
            start: P2::new(x, 0.0),
            end: P2::new(x + 10.0, 0.0),
        }
    }

    fn doc_with_view() -> Document {
        let mut d = Document::new(Title::default());
        d.add_source("sheet".into(), Source::new(SourceKind::Sheet, "this"))
            .unwrap();
        let mut sketch = Sketch::<Model>::new();
        sketch
            .add_entity(
                "a".into(),
                line(0.0),
                EntityMeta::new("hidden", cited("sheet")),
            )
            .unwrap();
        let view = View::new(
            (ModelAxis::X, ModelAxis::Y),
            sketch,
            Transform::at(0.0, 0.0),
        )
        .unwrap()
        .claim_model(ours())
        .unwrap();
        d.edit_views(|v| v.add_view("front".into(), view)).unwrap();
        d
    }

    #[test]
    fn parameters_belong_to_the_document() {
        use crate::api::msg::Op;
        use crate::value::measure::Measure;
        use crate::value::relation::{self, length, line};
        let mut d = doc_with_view();
        let mm = |v: f64| Measure::mm(v).unwrap();
        let p = |v: Measure| Param::new(v, ours()).unwrap();
        let w = Measure::param("w".into()).unwrap();
        let len = |m: Measure| length(line("a"), m);
        let user = Holder::Constraint("front".into(), "len".into());
        // A view's constraint may name only a parameter the document has.
        assert!(matches!(
            d.edit_view("front", |s, t| s.add_constraint(
                "len".into(),
                len(w.clone()),
                ours(),
                t
            )),
            Err(Error::Views(views::Error::Sketch(sketch::Error::Relation(
                relation::Error::UnknownParam(_)
            ))))
        ));
        assert_eq!(d.set_param("w".into(), p(mm(10.0))), Ok(None));
        d.edit_view("front", |s, t| {
            s.add_constraint("len".into(), len(w.clone()), ours(), t)
        })
        .unwrap();
        assert_eq!(d.users_of_param("w"), vec![user.clone()]);
        // A replacement must fit every user; a wrong quantity is refused
        // and nothing changes. A fitting one returns the old.
        assert!(matches!(
            d.set_param("w".into(), p(Measure::deg(1.0).unwrap())),
            Err(Error::Sketch(_))
        ));
        assert_eq!(d.param("w").unwrap().value(), &mm(10.0));
        let old = d.apply(&Op::set_param("w", p(mm(12.0)))).unwrap();
        assert_eq!(old.param("w").unwrap().value(), &mm(12.0));
        d = old;
        // Removal is refused while a constraint names it.
        assert_eq!(
            d.remove_param("w"),
            Err(Error::ParamInUse {
                name: "w".into(),
                by: vec![user]
            })
        );
        assert_eq!(
            d.apply(&Op::remove_param("nope")).err(),
            Some(Error::Params(params::Error::NoSuchParam("nope".into())))
        );
        // The solver reads the table: the line becomes 12 long.
        d.solve(Default::default()).unwrap();
        let g = d.views().view("front").unwrap().sketch.geometry();
        let (a, b) = (
            g.position(&line("a").start()).unwrap(),
            g.position(&line("a").end()).unwrap(),
        );
        assert_abs_diff_eq!(
            ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt(),
            12.0,
            epsilon = 1e-9
        );
        // The file round trip holds; the view's constraint loads as an
        // open term and the table closes it, so a table without its
        // parameter is refused on load.
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        let bad = json.replacen("\"w\": {", "\"ww\": {", 1);
        assert_ne!(bad, json);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("UnknownParam"), "{e}");
        // The user goes, then the parameter.
        d.edit_view("front", |s, _| s.remove_constraint("len").map(|_| ()))
            .unwrap();
        d.remove_param("w").unwrap();
        assert!(d.params().is_empty());
    }

    #[test]
    fn layers_and_sources_in_use_by_a_view_stay() {
        let mut d = doc_with_view();
        assert_eq!(
            d.remove_layer("hidden"),
            Err(Error::LayerInUse {
                layer: "hidden".into(),
                by: vec![Holder::Entity("front".into(), "a".into())]
            })
        );
        assert_eq!(
            d.remove_source("sheet"),
            Err(Error::SourceInUse {
                source: "sheet".into(),
                by: vec![Holder::Entity("front".into(), "a".into())]
            })
        );
        // Unused ones go; unknown ones are named.
        d.remove_layer("break").unwrap();
        assert_eq!(
            d.remove_layer("break"),
            Err(Error::NoSuchLayer("break".into()))
        );
        d.add_source("photo".into(), Source::new(SourceKind::Photo, "p"))
            .unwrap();
        d.remove_source("photo").unwrap();
        assert_eq!(
            d.remove_source("photo"),
            Err(Error::NoSuchSource("photo".into()))
        );
        assert_eq!(
            d.add_layer("hidden".into(), d.layer("hidden").cloned().unwrap()),
            Err(Error::DuplicateLayer("hidden".into()))
        );
        // A tint must be in the palette; an entry in use stays.
        let faint = Layer::working(LineStyle::new(0.2, Dash::SOLID).unwrap()).tinted("nope");
        assert_eq!(
            d.add_layer("guide".into(), faint),
            Err(Error::PaletteMissing {
                entry: "nope".into(),
                by: "guide".into()
            })
        );
        assert_eq!(
            d.remove_palette("faint"),
            Err(Error::PaletteInUse {
                entry: "faint".into(),
                by: vec!["construction".into()]
            })
        );
        d.add_palette("grey".into(), Appearance::fade(0.3).unwrap())
            .unwrap();
        d.remove_palette("grey").unwrap();
        assert_eq!(
            d.remove_palette("grey"),
            Err(Error::NoSuchPalette("grey".into()))
        );
        assert_eq!(
            d.add_palette("bad".into(), Appearance::Fade { by: 2.0 }),
            Err(Error::Style(style::Error::BadFade(2.0)))
        );
    }

    #[test]
    fn view_edits_that_would_dangle_roll_back() {
        let mut d = doc_with_view();
        let before = d.clone();
        // A citation of an unregistered document, through a view's sketch.
        let r = d.edit_views(|v| {
            v.edit("front", |s| {
                s.add_entity(
                    "b".into(),
                    line(20.0),
                    EntityMeta::new("visible", cited("nowhere")),
                )
            })
        });
        assert_eq!(
            r,
            Err(Error::SourceMissing {
                source: "nowhere".into(),
                by: Holder::Entity("front".into(), "b".into())
            })
        );
        assert_eq!(d, before);
        // A frame claim citing an unregistered document.
        let view = View::new(
            (ModelAxis::X, ModelAxis::Z),
            Sketch::new(),
            Transform::at(0.0, 0.0),
        )
        .unwrap()
        .claim_model(cited("nowhere"))
        .unwrap();
        let r = d.edit_views(|v| v.add_view("side".into(), view));
        assert_eq!(
            r,
            Err(Error::SourceMissing {
                source: "nowhere".into(),
                by: Holder::Frame("side".into())
            })
        );
        assert_eq!(d, before);
        // An inner error is passed through and nothing changes.
        let r = d.edit_views(|v| v.remove_view("nope").map(|_| ()));
        assert!(matches!(r, Err(Error::Views(views::Error::NoSuchView(_)))));
        assert_eq!(d, before);
    }

    #[test]
    fn a_file_with_a_dangling_reference_is_refused() {
        let d = doc_with_view();
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        // Rename the layer in the table only: the entity's layer dangles.
        let bad = json.replacen("\"hidden\": {", "\"hidden2\": {", 1);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("LayerMissing"), "{e}");
        // Rename the source in the registry only.
        let bad = json.replacen(
            "\"sources\": {\n    \"sheet\"",
            "\"sources\": {\n    \"sheet2\"",
            1,
        );
        assert_ne!(bad, json);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("SourceMissing"), "{e}");
        // A bad weight in the table.
        let bad = json.replacen("\"weight\": 0.7", "\"weight\": -0.7", 1);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("BadWeight"), "{e}");
        // A tint naming an entry the palette lacks.
        let bad = json.replacen("\"tint\": \"faint\"", "\"tint\": \"nope\"", 1);
        assert_ne!(bad, json);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("PaletteMissing"), "{e}");
        // A fade out of range.
        let bad = json.replacen("\"by\": 0.5", "\"by\": 5.0", 1);
        assert_ne!(bad, json);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("BadFade"), "{e}");
    }

    #[test]
    fn texts_keep_their_references() {
        use crate::value::relation::line;
        use crate::value::text::{Anchor, BoxPoint, DrawingPoint, Placement, Size, Text};
        let mut d = doc_with_view();
        let text = |lines: &[&str]| Text::new(lines, "text", P2::new(10.0, 10.0), ours()).unwrap();
        d.add_text("a".into(), text(&["NOTE 1"])).unwrap();
        // The role, the layer, the point beside, the glyphs: all checked.
        assert_eq!(
            d.add_text(
                "b".into(),
                text(&["X"])
                    .sized(Size::Role {
                        role: "nope".into()
                    })
                    .unwrap()
            ),
            Err(Error::RoleMissing {
                role: "nope".into(),
                by: "b".into()
            })
        );
        assert_eq!(
            d.add_text("b".into(), text(&["X"]).on_layer("nope")),
            Err(Error::LayerMissing {
                layer: "nope".into(),
                by: Holder::Text("b".into())
            })
        );
        let beside = DrawingPoint::View {
            view: "front".into(),
            point: line("zz").start(),
        };
        assert_eq!(
            d.add_text(
                "b".into(),
                text(&["X"])
                    .placed(Placement::Beside {
                        point: beside,
                        offset: P2::new(0.0, 0.0)
                    })
                    .unwrap()
            ),
            Err(Error::PointMissing {
                point: beside,
                by: "b".into()
            })
        );
        assert_eq!(
            d.add_text("b".into(), text(&["½"])),
            Err(Error::GlyphMissing {
                ch: '½',
                by: "b".into()
            })
        );
        // Aligned texts form a chain that must end on the paper.
        let aligned = |to: &str| {
            text(&["NOTE 2"])
                .placed(Placement::AlignedTo {
                    text: to.into(),
                    anchor: Anchor::Box {
                        at: BoxPoint::BottomLeft,
                    },
                    offset: P2::new(0.0, 8.0),
                })
                .unwrap()
        };
        d.add_text("b".into(), aligned("a")).unwrap();
        assert_eq!(
            d.add_text("c".into(), aligned("zz")),
            Err(Error::TextMissing {
                text: "zz".into(),
                by: "c".into()
            })
        );
        assert_eq!(
            d.set_text("a", aligned("b")),
            Err(Error::TextCycle("a".into()))
        );
        assert_eq!(
            d.remove_text("a"),
            Err(Error::TextInUse {
                text: "a".into(),
                by: vec!["b".into()]
            })
        );
        assert_eq!(
            d.remove_role("text"),
            Err(Error::RoleInUse {
                role: "text".into(),
                by: vec!["a".into(), "b".into()]
            })
        );
        d.remove_text("b").unwrap();
        d.remove_text("a").unwrap();
        d.remove_role("text").unwrap();
        assert_eq!(d.remove_role("text"), Err(Error::NoSuchRole("text".into())));
        // The file round trip holds and a dangling role is refused on load.
        d.set_role(
            "text",
            crate::value::lettering::Role::new(5.0, false).unwrap(),
        )
        .unwrap();
        d.add_text("a".into(), text(&["NOTE 1"])).unwrap();
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        let bad = json.replacen("\"role\": \"text\"", "\"role\": \"nope\"", 1);
        assert_ne!(bad, json);
        assert!(Document::parse(&bad)
            .unwrap_err()
            .to_string()
            .contains("RoleMissing"));
    }

    #[test]
    fn centre_marks_and_lines_keep_their_references() {
        use crate::value::centre::{CentreLine, CentreMark};
        use crate::value::geometry::{ArcPos, EntityId, PointRef};
        use crate::value::leader::DrawingEntity;
        use crate::value::text::DrawingPoint;
        let mut d = doc_with_view();
        for (id, x) in [("c1", 0.0), ("c2", 50.0)] {
            d.edit_view("front", |s, _| {
                s.add_entity(
                    id.into(),
                    Entity::Arc {
                        centre: P2::new(x, 0.0),
                        radius: 5.0,
                        start_angle: 0.0,
                        sweep: 3.0,
                    },
                    EntityMeta::new("visible", ours()),
                )
            })
            .unwrap();
        }
        let arc = |id: &str| DrawingEntity::View {
            view: "front".into(),
            id: id.into(),
        };
        let centre = |id: &str| DrawingPoint::View {
            view: "front".into(),
            point: PointRef::arc(id, ArcPos::Centre),
        };
        // A mark on an arc; not on a line.
        d.add_centre_mark(
            "m1".into(),
            CentreMark::new(vec![arc("c1")], ours()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            d.add_centre_mark(
                "m2".into(),
                CentreMark::new(vec![arc("a")], ours()).unwrap()
            ),
            Err(Error::CentreMarkFeatureMissing {
                entity: arc("a"),
                by: "m2".into()
            })
        );
        // A line through both centres; not through a missing one.
        d.add_centre_line(
            "l1".into(),
            CentreLine::new(vec![centre("c1"), centre("c2")], ours()).unwrap(),
        )
        .unwrap();
        assert!(d
            .add_centre_line(
                "l2".into(),
                CentreLine::new(vec![centre("c1"), centre("zz")], ours()).unwrap(),
            )
            .is_err());
        // What they name cannot go while they do.
        assert!(d
            .edit_view("front", |s, _| s.remove_entity(EntityId::new("c2")))
            .is_err());
        // And the document survives its file.
        let back = Document::parse(&d.to_json().unwrap()).unwrap();
        assert_eq!(back.centre_marks().count(), 1);
        assert_eq!(back.centre_lines().count(), 1);
    }

    #[test]
    fn leaders_keep_their_references() {
        use crate::value::leader::{Along, DrawingEntity, Leader, Target, Tip};
        use crate::value::relation::line;
        use crate::value::text::{Anchor, BoxPoint, Side, Text};
        let mut d = doc_with_view();
        d.add_text(
            "n".into(),
            Text::new(&["NOTE"], "text", P2::new(10.0, 10.0), ours()).unwrap(),
        )
        .unwrap();
        d.edit_view("front", |s, _| {
            s.add_entity(
                "p".into(),
                Entity::Point {
                    at: P2::new(1.0, 1.0),
                },
                EntityMeta::new("visible", ours()),
            )
        })
        .unwrap();
        let on = |view: &str, id: &str| Target::Along {
            entity: DrawingEntity::View {
                view: view.into(),
                id: id.into(),
            },
            at: Along::Nearest,
        };
        let line0 = Anchor::Line {
            index: 0,
            side: Side::Left,
        };
        let leader = |text: &str, anchor, target| {
            Leader::new(text, anchor, Tip::arrow(target), ours()).unwrap()
        };
        d.add_leader("l".into(), leader("n", line0, on("front", "a")))
            .unwrap();
        let missing = |target| {
            Err(Error::LeaderTargetMissing {
                target,
                by: "m".into(),
            })
        };
        // The text, its anchor, the layer, and every target are checked.
        assert_eq!(
            d.add_leader("m".into(), leader("zz", line0, on("front", "a"))),
            Err(Error::LeaderTextMissing {
                text: "zz".into(),
                by: "m".into()
            })
        );
        let line3 = Anchor::Line {
            index: 3,
            side: Side::Left,
        };
        assert_eq!(
            d.add_leader("m".into(), leader("n", line3, on("front", "a"))),
            Err(Error::LeaderAnchorMissing {
                text: "n".into(),
                anchor: line3,
                by: "m".into()
            })
        );
        assert_eq!(
            d.add_leader(
                "m".into(),
                leader("n", line0, on("front", "a")).on_layer("nope")
            ),
            Err(Error::LayerMissing {
                layer: "nope".into(),
                by: Holder::Leader("m".into())
            })
        );
        for target in [on("front", "zz"), on("nope", "a"), on("front", "p")] {
            // (The last: a point is not a curve to land along.)
            assert_eq!(
                d.add_leader("m".into(), leader("n", line0, target)),
                missing(target)
            );
        }
        // A leader from a ring needs an enclosed text.
        let from_ring = |text: &str| {
            Leader::balloon(text, Side::Right, Tip::arrow(on("front", "a")), ours()).unwrap()
        };
        assert_eq!(
            d.add_leader("m".into(), from_ring("n")),
            Err(Error::LeaderOriginNotEnclosed {
                text: "n".into(),
                by: "m".into()
            })
        );
        assert_eq!(
            d.add_leader("m".into(), from_ring("zz")),
            Err(Error::LeaderTextMissing {
                text: "zz".into(),
                by: "m".into()
            })
        );
        d.add_text(
            "b".into(),
            Text::balloon("-15", P2::new(30.0, 10.0), ours()).unwrap(),
        )
        .unwrap();
        d.add_leader("bl".into(), from_ring("b")).unwrap();
        // While its leader leaves the ring, the balloon may not lose its
        // enclosure.
        let plain = Text::balloon("-15", P2::new(30.0, 10.0), ours())
            .unwrap()
            .unenclosed();
        assert_eq!(
            d.set_text(TextId::from("b"), plain),
            Err(Error::LeaderOriginNotEnclosed {
                text: "b".into(),
                by: "bl".into()
            })
        );
        d.remove_leader("bl").unwrap();
        d.remove_text("b").unwrap();
        let at_p = Target::Point {
            point: DrawingPoint::View {
                view: "front".into(),
                point: crate::value::geometry::PointRef::point("p"),
            },
        };
        d.add_leader("m".into(), leader("n", line0, at_p)).unwrap();
        d.remove_leader("m").unwrap();
        let at_zz = Target::Point {
            point: DrawingPoint::View {
                view: "front".into(),
                point: line("zz").start(),
            },
        };
        assert_eq!(
            d.add_leader("m".into(), leader("n", line0, at_zz)),
            missing(at_zz)
        );
        let in_nope = Target::InView {
            view: "nope".into(),
            point: P2::new(0.0, 0.0),
        };
        assert_eq!(
            d.add_leader("m".into(), leader("n", line0, in_nope)),
            missing(in_nope)
        );
        let zz = DrawingEntity::Sheet { id: "zz".into() };
        assert_eq!(
            d.add_leader(
                "m".into(),
                leader("n", line0, on("front", "a")).standing_for(vec![zz])
            ),
            Err(Error::LeaderEntityMissing {
                entity: zz,
                by: "m".into()
            })
        );
        // A tip may land on another text's anchor.
        d.add_text(
            "n2".into(),
            Text::new(&["SEE NOTE"], "text", P2::new(40.0, 10.0), ours()).unwrap(),
        )
        .unwrap();
        let to_n2 = Target::Text {
            text: "n2".into(),
            anchor: Anchor::Box {
                at: BoxPoint::MidLeft,
            },
        };
        assert_eq!(d.add_leader("m".into(), leader("n", line0, to_n2)), Ok(()));
        assert_eq!(
            d.add_leader("m".into(), leader("n", line0, at_p)),
            Err(Error::DuplicateLeader("m".into()))
        );
        // What a leader touches stays: both texts, its layer, and the
        // entity it lands on (that edit rolls back).
        assert_eq!(
            d.remove_text("n2"),
            Err(Error::TextHeldByLeaders {
                text: "n2".into(),
                by: vec!["m".into()]
            })
        );
        d.remove_leader("m").unwrap();
        assert_eq!(d.remove_leader("m"), Err(Error::NoSuchLeader("m".into())));
        d.remove_text("n2").unwrap();
        assert_eq!(
            d.remove_text("n"),
            Err(Error::TextHeldByLeaders {
                text: "n".into(),
                by: vec!["l".into()]
            })
        );
        assert_eq!(
            d.remove_layer("leader"),
            Err(Error::LayerInUse {
                layer: "leader".into(),
                by: vec![Holder::Leader("l".into())]
            })
        );
        assert_eq!(
            d.remove_layer("text"),
            Err(Error::LayerInUse {
                layer: "text".into(),
                by: vec![Holder::Text("n".into())]
            })
        );
        let before = d.clone();
        assert_eq!(
            d.edit_view("front", |s, _| s.remove_entity("a").map(|_| ())),
            Err(Error::LeaderTargetMissing {
                target: on("front", "a"),
                by: "l".into()
            })
        );
        assert_eq!(d, before);
        // The file round trip holds; a dangling target is refused on load.
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        let bad = json.replacen("\"id\": \"a\"", "\"id\": \"zz\"", 1);
        assert_ne!(bad, json);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("LeaderTargetMissing"), "{e}");
        let bad = json.replacen("\"dot\": 1.27", "\"dot\": 0.0", 1);
        assert_ne!(bad, json);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("BadSize"), "{e}");
        d.remove_leader("l").unwrap();
        d.remove_text("n").unwrap();
    }

    #[test]
    fn dimensions_keep_their_constraints() {
        use crate::value::dimension::{Dimension, Of};
        use crate::value::measure::Measure;
        use crate::value::relation::{circle, horizontal, length, line, radius};
        let mut d = doc_with_view();
        d.edit_view("front", |s, p| {
            s.add_entity(
                "c".into(),
                Entity::Circle {
                    centre: P2::new(5.0, 5.0),
                    radius: 2.0,
                },
                EntityMeta::new("visible", ours()),
            )?;
            s.add_constraint(
                "len".into(),
                length(line("a"), Measure::mm(10.0).unwrap()),
                ours(),
                p,
            )?;
            s.add_constraint("h".into(), horizontal(line("a")), ours(), p)?;
            s.add_constraint(
                "r".into(),
                radius(circle("c"), Measure::mm(2.0).unwrap()),
                ours(),
                p,
            )
        })
        .unwrap();
        let of = |view: &str, c: &str| Of::View {
            view: view.into(),
            constraint: c.into(),
        };
        let lin = |of| Dimension::linear(of, 5.0, ours()).unwrap();
        d.add_dimension("d".into(), lin(of("front", "len")))
            .unwrap();
        // The constraint must be there and be a dimension of the family.
        assert_eq!(
            d.add_dimension("e".into(), lin(of("front", "h"))),
            Err(Error::DimensionConstraintNotDimensional {
                of: of("front", "h"),
                by: "e".into()
            })
        );
        for bad in [
            of("front", "zz"),
            of("nope", "len"),
            Of::Sheet {
                constraint: "len".into(),
            },
        ] {
            assert_eq!(
                d.add_dimension("e".into(), lin(bad)),
                Err(Error::DimensionConstraintMissing {
                    of: bad,
                    by: "e".into()
                })
            );
        }
        assert_eq!(
            d.add_dimension("e".into(), lin(of("front", "r"))),
            Err(Error::DimensionKindMismatch {
                of: of("front", "r"),
                constraint: DimensionKind::Radial,
                layout: DimensionKind::Linear,
                by: "e".into()
            })
        );
        assert_eq!(
            d.add_dimension(
                "e".into(),
                Dimension::across(of("front", "r"), 0.0, 0.0, ours()).unwrap()
            ),
            Err(Error::AcrossNeedsDiameter {
                of: of("front", "r"),
                by: "e".into()
            })
        );
        assert_eq!(
            d.add_dimension("e".into(), lin(of("front", "len")).suffixed("☃").unwrap()),
            Err(Error::DimensionGlyphMissing {
                ch: '☃',
                by: "e".into()
            })
        );
        assert_eq!(
            d.add_dimension("e".into(), lin(of("front", "len")).on_layer("nope")),
            Err(Error::LayerMissing {
                layer: "nope".into(),
                by: Holder::Dimension("e".into())
            })
        );
        assert_eq!(
            d.add_dimension("d".into(), lin(of("front", "len"))),
            Err(Error::DuplicateDimension("d".into()))
        );
        // Its layer and its constraint stay (that edit rolls back).
        assert_eq!(
            d.remove_layer("dimension"),
            Err(Error::LayerInUse {
                layer: "dimension".into(),
                by: vec![Holder::Dimension("d".into())]
            })
        );
        let before = d.clone();
        assert_eq!(
            d.edit_view("front", |s, _| s.remove_constraint("len").map(|_| ())),
            Err(Error::DimensionConstraintMissing {
                of: of("front", "len"),
                by: "d".into()
            })
        );
        assert_eq!(d, before);
        // The file round trip holds; a dangling constraint is refused.
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        let bad = json.replacen("\"constraint\": \"len\"", "\"constraint\": \"zz\"", 1);
        assert_ne!(bad, json);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("DimensionConstraintMissing"), "{e}");
        d.remove_dimension("d").unwrap();
        assert_eq!(
            d.remove_dimension("d"),
            Err(Error::NoSuchDimension("d".into()))
        );
    }

    #[test]
    fn frames_round_trip_and_are_checked() {
        use crate::value::frame::{Registration, Similarity, Space};
        let paper = Similarity::new([100.0, 50.0], 8.0, 0.1).unwrap();
        let r = Registration::similarity(
            [10, 10],
            paper,
            Provenance::new(Method::Assumed, Agent::User, day()),
        );
        for p in [P2::new(0.0, 0.0), P2::new(25.4, 12.7), P2::new(-3.0, 7.0)] {
            assert_abs_diff_eq!(r.to_paper(r.from_paper(p, Space::Frame), Space::Frame), p);
        }
        assert_eq!(r.from_paper(P2::new(0.0, 0.0), Space::Frame), [100.0, 50.0]);
        let mut d = Document::new(Title::default());
        let frame = Frame {
            file: "a.png".into(),
            size_px: [10, 10],
            hash: None,
            registration: r,
            hops: Vec::new(),
            evidence: None,
        };
        d.set_frame("scan".into(), Some(frame.clone())).unwrap();
        assert_eq!(d.frame("scan"), Some(&frame));
        d.add_note(Note {
            at: P2::new(1.0, 2.0),
            text: "check this".into(),
            by: Agent::User,
            when: day(),
        })
        .unwrap();
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        assert_eq!(d.remove_note(0).map(|n| n.text), Some("check this".into()));
        assert_eq!(d.remove_note(0), None);
        let bad = json.replacen("\"px_per_mm\": 8.0", "\"px_per_mm\": 0.0", 1);
        let e = Document::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("BadFrame") && e.contains("BadScale(0.0)"), "{e}");
        let mut cropped = frame;
        cropped.registration.crop.map.box_px = [0, 0, 11, 10];
        assert_eq!(
            d.set_frame("scan".into(), Some(cropped)),
            Err(Error::BadFrame {
                frame: "scan".into(),
                error: frame::Error::BadCrop([0, 0, 11, 10])
            })
        );
        d.set_frame("scan".into(), None).unwrap();
        assert_eq!(d.frames().count(), 0);
    }

    /// Claims round trip through the file; one citing a segment the
    /// evidence lacks, a frame with no evidence, or a node the document
    /// lacks is refused, and the evidence cited cannot go.
    /// 17619's form: ENG-73.1 6-64 with its six fields as read, the
    /// check left empty when `check` is false.
    fn form_17619(check: bool) -> Form {
        use crate::value::form::{FormPlacement, FormTemplate};
        let t = FormTemplate::builtin("ENG-73.1_6-64").unwrap();
        let entry = |v: &str| Some(Text::new(&[v], "text", P2::new(300.0, 250.0), ours()).unwrap());
        let mut fields: BTreeMap<FieldId, Option<Text>> = [
            ("size", "B"),
            ("code_ident", "26512"),
            ("drawing_number", "LDW280-17619"),
            ("scale", "NONE"),
            ("sheet", "3"),
            ("check", "✓"),
        ]
        .into_iter()
        .map(|(f, v)| (FieldId::new(f), entry(v)))
        .collect();
        if !check {
            fields.insert(FieldId::new("check"), None);
        }
        Form {
            edition: t.edition.id.clone(),
            hash: t.hash(),
            placement: FormPlacement::IDENTITY,
            printed: ours(),
            fields,
        }
    }

    /// A placed form round-trips; its fields are exactly its template's,
    /// each filled or empty; the title is a function of them (the name
    /// and revision, which the form has no cell for, stored), and a
    /// stored drawing number beside them is refused; a claim may name a
    /// printed item the template has, or a filled field.
    #[test]
    fn the_form_is_placed_with_its_fields() {
        use crate::value::claims::tests::{claim, evidence};
        use crate::value::claims::Kind;
        use crate::value::form::Error as F;
        use crate::value::frame::{Registration, Similarity};
        let mut d = Document::new(Title {
            name: "LINK".into(),
            ..Title::default()
        });
        let form = form_17619(false);
        d.set_form(Some(form.clone())).unwrap();
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        assert_eq!(d.form(), Some(&form));
        assert_eq!(
            d.title(),
            Title {
                drawing: "LDW280-17619".into(),
                rev: String::new(),
                sheet: "3".into(),
                name: "LINK".into(),
            }
        );
        assert_eq!(form.value("scale").as_deref(), Some("NONE"));
        assert_eq!(form.value("check"), None);
        // Its fields, exactly.
        let mut short = form.clone();
        short.fields.remove(&FieldId::new("scale"));
        assert_eq!(d.set_form(Some(short)), Err(Error::BadForm(F::Fields)));
        let mut extra = form.clone();
        extra.fields.insert(FieldId::new("title"), None);
        assert_eq!(d.set_form(Some(extra)), Err(Error::BadForm(F::Fields)));
        // Its template by edition and hash.
        let mut other = form.clone();
        other.hash = "fnv1a64:0".into();
        assert!(matches!(
            d.set_form(Some(other)),
            Err(Error::BadForm(F::Hash { .. }))
        ));
        // The title's drawing number is the field's alone.
        d.set_title(Title {
            drawing: "LDW280-17619".into(),
            ..Title::default()
        });
        assert_eq!(d.check_references(), Err(Error::TitleBesideForm));
        d.set_title(Title::default());
        // Claims on its items and fields.
        let paper = Similarity::new([0.0, 0.0], 8.0, 0.0).unwrap();
        d.set_frame(
            "scan".into(),
            Some(Frame {
                file: "a.png".into(),
                size_px: [10, 10],
                hash: None,
                registration: Registration::similarity([10, 10], paper, ours()),
                hops: Vec::new(),
                evidence: Some(evidence()),
            }),
        )
        .unwrap();
        for (kind, name) in [
            (Kind::Form, "rule/0/3"),
            (Kind::Form, "label/0/6"),
            (Kind::Form, "printed/1/5"),
            (Kind::Form, "frame/2"),
            (Kind::Field, "sheet"),
        ] {
            d.set_claims(vec![claim(kind, name)]).unwrap();
        }
        for (kind, name) in [
            (Kind::Form, "rule/0/99"),
            (Kind::Form, "label/0/2"),
            (Kind::Form, "frame/4"),
            (Kind::Form, "nonsense"),
            (Kind::Field, "check"),
        ] {
            assert!(d.set_claims(vec![claim(kind, name)]).is_err(), "{name}");
        }
        // The claimed form cannot go.
        d.set_claims(vec![claim(Kind::Field, "sheet")]).unwrap();
        assert!(d.set_form(None).is_err());
        d.set_claims(Vec::new()).unwrap();
        d.set_form(None).unwrap();
        assert_eq!(d.title().drawing, "");
    }

    #[test]
    fn claims_round_trip_and_are_checked() {
        use crate::value::claims::tests::{claim, evidence};
        use crate::value::claims::{Error as E, Kind};
        use crate::value::frame::{Registration, Similarity};
        let paper = Similarity::new([0.0, 0.0], 8.0, 0.0).unwrap();
        let mut d = Document::new(Title::default());
        let mut frame = Frame {
            file: "a.png".into(),
            size_px: [10, 10],
            hash: None,
            registration: Registration::similarity([10, 10], paper, ours()),
            hops: Vec::new(),
            evidence: None,
        };
        d.set_frame("scan".into(), Some(frame.clone())).unwrap();
        d.add_text(
            "T1".into(),
            Text::new(&["NOTE"], "text", P2::new(10.0, 10.0), ours()).unwrap(),
        )
        .unwrap();
        let text = claim(Kind::Text, "T1");
        let bad = |error| Err(Error::BadClaim { claim: 0, error });
        assert_eq!(
            d.set_claims(vec![text.clone()]),
            bad(E::NoEvidence("scan".into()))
        );
        frame.evidence = Some(evidence());
        d.set_frame("scan".into(), Some(frame)).unwrap();
        let stamp = claim(Kind::Foreign, "NAS 9-1100 stamp");
        d.set_claims(vec![text.clone(), stamp]).unwrap();
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        assert_eq!(d.claims().len(), 2);
        let mut unknown: serde_json::Value = serde_json::from_str(&json).unwrap();
        unknown["claims"][0]["segments"][0] = "tffffffffff0".into();
        let e = Document::parse(&unknown.to_string())
            .unwrap_err()
            .to_string();
        assert!(e.contains("UnknownSegment"), "{e}");
        let elsewhere = Claim {
            evidence: "other".into(),
            ..text.clone()
        };
        assert_eq!(
            d.set_claims(vec![elsewhere]),
            bad(E::NoEvidence("other".into()))
        );
        let nobody = claim(Kind::Dimension, "D9");
        assert_eq!(
            d.set_claims(vec![nobody.clone()]),
            bad(E::NoNode(nobody.node))
        );
        assert!(matches!(
            d.set_frame("scan".into(), None),
            Err(Error::BadClaim { .. })
        ));
        assert_eq!(d.claims().len(), 2);
        d.set_claims(Vec::new()).unwrap();
        d.set_frame("scan".into(), None).unwrap();
    }

    /// A marking region owns six lines of one view (a rectangle's four
    /// sides and two diagonals), none another region's, all lines; it is
    /// named by items of the placed form, which must exist; it round
    /// trips through the file, and the lines it owns are found from it.
    #[test]
    fn a_marking_region_owns_six_lines_and_is_named_by_the_form() {
        use crate::api::msg::SketchOp;
        use crate::value::marking::{Error as M, Meaning, NamedBy};
        let mut d = crate::examples::block();
        let diagonal = |name: &str, a: (f64, f64), b: (f64, f64)| {
            let line = Entity::Line {
                start: P2::new(a.0, a.1),
                end: P2::new(b.0, b.1),
            };
            SketchOp::add_entity(name, line, "visible", ours()).in_view("top")
        };
        d.apply_in_place(&diagonal("d1", (0.0, 0.0), (40.0, 20.0)))
            .unwrap();
        d.apply_in_place(&diagonal("d2", (0.0, 20.0), (40.0, 0.0)))
            .unwrap();
        let region = |sides: [&str; 4], diagonals: [&str; 2]| MarkingRegion {
            view: "top".into(),
            rectangle: sides.map(Into::into),
            diagonals: diagonals.map(Into::into),
            meaning: Meaning::PartNumberAndMarkings,
            named_by: None,
            provenance: ours(),
        };
        let set =
            |d: &mut Document, r: MarkingRegion| d.set_marking_regions([("MR".into(), r)].into());
        let bad = |error| {
            Err(Error::BadMarkingRegion {
                region: "MR".into(),
                error,
            })
        };
        let good = region(["tb", "tr", "tt", "tl"], ["d1", "d2"]);
        assert_eq!(
            set(&mut d, region(["tb", "tr", "tt", "tb"], ["d1", "d2"])),
            bad(M::Repeated("tb".into()))
        );
        assert_eq!(
            set(&mut d, region(["tb", "tr", "tt", "none"], ["d1", "d2"])),
            bad(M::NoLine("none".into()))
        );
        // A point is no line.
        assert_eq!(
            set(&mut d, region(["tb", "tr", "tt", "hw"], ["d1", "d2"])),
            bad(M::NoLine("hw".into()))
        );
        assert!(d.marking_regions().is_empty());
        set(&mut d, good.clone()).unwrap();
        // Another region cannot take a line of it.
        let rival = region(["tb", "fr", "ft", "fl"], ["d1", "d2"]);
        let two: BTreeMap<_, _> = [("MR".into(), good.clone()), ("MR2".into(), rival)].into();
        assert!(matches!(
            d.set_marking_regions(two),
            Err(Error::BadMarkingRegion {
                error: M::Shared(_),
                ..
            })
        ));
        assert_eq!(d.marking_regions().len(), 1);
        assert_eq!(
            d.marking_region_of("top", "d2").map(|r| r.0.as_str()),
            Some("MR")
        );
        assert!(d.marking_region_of("top", "hole").is_none());
        assert!(d.marking_region_of("front", "d2").is_none());
        // Named by the form: it needs the form, and the items in it.
        let link = NamedBy {
            note: "printed/1/5".into(),
            box_x: (4..10).map(|k| format!("rule/1/{k}")).collect(),
        };
        let mut named = good.clone();
        named.named_by = Some(link.clone());
        assert_eq!(
            set(&mut d, named.clone()),
            bad(M::NoFormItem("printed/1/5".into()))
        );
        d.set_title(Title::default());
        d.set_form(Some(form_17619(false))).unwrap();
        set(&mut d, named.clone()).unwrap();
        let mut stray = named.clone();
        stray
            .named_by
            .as_mut()
            .unwrap()
            .box_x
            .push("rule/1/99".into());
        assert_eq!(set(&mut d, stray), bad(M::NoFormItem("rule/1/99".into())));
        let mut empty = named.clone();
        empty.named_by.as_mut().unwrap().box_x.clear();
        assert_eq!(set(&mut d, empty), bad(M::BadLink));
        // The other end of the link.
        let by_note: Vec<_> = d
            .regions_named_by("printed/1/5")
            .map(|r| r.0.as_str())
            .collect();
        assert_eq!(by_note, ["MR"]);
        assert_eq!(d.regions_named_by("rule/1/7").count(), 1);
        assert_eq!(d.regions_named_by("rule/0/0").count(), 0);
        // The file keeps it; a line it owns cannot go.
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        assert!(d
            .apply_in_place(&SketchOp::<Model>::remove_entity("d1").in_view("top"))
            .is_err());
        // Its meaning reads back from words.
        assert_eq!(
            Meaning::parse("other(a plate)"),
            Some(Meaning::Other("a plate".into()))
        );
        assert_eq!(Meaning::parse("other()"), None);
        assert_eq!(
            Meaning::parse("part-number-and-markings"),
            Some(Meaning::PartNumberAndMarkings)
        );
    }

    /// Gaps round trip through the file; one naming a node the document
    /// lacks, or a region on a frame it lacks, is refused, and a node a
    /// gap names cannot go.
    #[test]
    fn gaps_round_trip_and_are_checked() {
        use crate::value::claims::Kind;
        use crate::value::gaps::{tests::gap, Error as E, Region};
        let mut d = Document::new(Title::default());
        d.add_text(
            "T1".into(),
            Text::new(&["NOTE"], "text", P2::new(10.0, 10.0), ours()).unwrap(),
        )
        .unwrap();
        let g = gap(&[(Kind::Text, "T1")]);
        d.set_gaps([("marking-region".into(), g.clone())].into())
            .unwrap();
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        assert_eq!(d.gaps().len(), 1);
        let bad = |error| {
            Err(Error::BadGap {
                gap: "marking-region".into(),
                error,
            })
        };
        let dangling = gap(&[(Kind::Text, "T9")]);
        assert_eq!(
            d.set_gaps([("marking-region".into(), dangling.clone())].into()),
            bad(E::NoNode(dangling.nodes[0].clone()))
        );
        let elsewhere = Gap {
            region: Some(Region {
                frame: "scan".into(),
                px: [0.0, 0.0, 5.0, 5.0],
            }),
            ..g.clone()
        };
        assert_eq!(
            d.set_gaps([("marking-region".into(), elsewhere)].into()),
            bad(E::NoFrame("scan".into()))
        );
        let mut file: serde_json::Value = serde_json::from_str(&json).unwrap();
        file["gaps"]["marking-region"]["nodes"][0]["name"] = "T9".into();
        let e = Document::parse(&file.to_string()).unwrap_err().to_string();
        assert!(e.contains("NoNode"), "{e}");
        assert!(matches!(d.remove_text("T1"), Err(Error::BadGap { .. })));
        assert_eq!(d.gaps()[&GapId::new("marking-region")], g);
    }
    /// Residuals round trip through the file, and one naming a view,
    /// dimension, constraint or entity the document lacks is refused.
    #[test]
    fn residuals_round_trip_and_name_the_documents_ids() {
        use crate::value::residuals::{
            tests::residuals, DimensionResidual, Error as E, MovedBy, RelationResidual,
        };
        let mut d = crate::examples::dimensions();
        assert!(d.residuals().is_none());
        let (view, v) = d.views().views().next().unwrap();
        let (view, v) = (*view, v.clone());
        let dim = *d.dimensions().next().unwrap().0;
        let con = *v.sketch.constraints().next().unwrap().0;
        let ent = *v.sketch.geometry().iter().next().unwrap().0;
        let mut r = residuals();
        r.view = view;
        r.dimensions.insert(
            dim,
            DimensionResidual {
                read_in: 1.0,
                ink_in: 0.99,
                delta_in: 0.01,
                delta_px: 2.0,
                scale_sample: 2.0,
                scale_without: None,
                pull: 3.0,
            },
        );
        r.relations.insert(
            con,
            RelationResidual {
                ink_px: 1.0,
                pull: 2.0,
            },
        );
        r.entities.insert(
            ent,
            crate::value::residuals::EntityResidual {
                rms_px: 1.0,
                max_px: 2.0,
                moved_by: Some(MovedBy {
                    constraint: con,
                    rms_px: 0.5,
                }),
            },
        );
        d.set_residuals(Some(r.clone())).unwrap();
        let json = d.to_json().unwrap();
        let back = Document::parse(&json).unwrap();
        assert_eq!(back, d);
        assert_eq!(back.residuals(), Some(&r));
        assert_eq!(back.to_json().unwrap(), json);
        let bad = |error| Err(Error::BadResiduals(error));
        let mut x = r.clone();
        x.view = "nowhere".into();
        assert_eq!(d.set_residuals(Some(x)), bad(E::NoView("nowhere".into())));
        let mut x = r.clone();
        let row = x.dimensions.remove(&dim).unwrap();
        x.dimensions.insert("D99".into(), row);
        assert_eq!(d.set_residuals(Some(x)), bad(E::NoDimension("D99".into())));
        let mut x = r.clone();
        x.relations.insert(
            "c99".into(),
            RelationResidual {
                ink_px: 0.0,
                pull: 0.0,
            },
        );
        assert_eq!(d.set_residuals(Some(x)), bad(E::NoConstraint("c99".into())));
        let mut x = r.clone();
        x.entities
            .get_mut(&ent)
            .unwrap()
            .moved_by
            .as_mut()
            .unwrap()
            .constraint = "c99".into();
        assert_eq!(d.set_residuals(Some(x)), bad(E::NoConstraint("c99".into())));
        let mut x = r.clone();
        let row = x.entities.remove(&ent).unwrap();
        x.entities.insert("e99".into(), row);
        assert_eq!(d.set_residuals(Some(x)), bad(E::NoEntity("e99".into())));
        let mut file: serde_json::Value = serde_json::from_str(&json).unwrap();
        file["residuals"]["view"] = "nowhere".into();
        let e = Document::parse(&file.to_string()).unwrap_err().to_string();
        assert!(e.contains("NoView"), "{e}");
        assert_eq!(d.residuals(), Some(&r));
    }

    /// An observed rounding is kept out of the design, on its own layer,
    /// in a view, between two lines of it; one that names what the
    /// document lacks is refused whole, as is the layer it is on while
    /// it is held; it round trips through the file.
    #[test]
    fn an_observed_rounding_needs_its_view_lines_and_layer() {
        use crate::value::observed::{self as ob, Observed};
        let mut d = crate::examples::block();
        let fit = ob::Fit {
            ink_radius_mm: 3.0,
            pen_mm: 0.3,
            rms_sharp_mm: 0.2,
            rms_rounded_mm: 0.05,
            rms_best_mm: 0.04,
            loss_mm: 0.5,
            points: 30,
            sigma_mm: 0.0,
            own: None,
        };
        let evidence = Provenance::new(Method::Measured { sigma_mm: 0.1 }, Agent::Claude, day())
            .cite(Citation::new("brief", "the corner"));
        let seen = |view: &str, a: &str, b: &str| {
            Observed::rounding(
                view.into(),
                [a.into(), b.into()],
                2.5,
                P2::new(37.5, 2.5),
                fit.clone(),
                evidence.clone(),
            )
        };
        let set = |d: &mut Document, o: Observed| d.set_observed([("OB1".into(), o)].into());
        let n = d.observed().len();
        let bad = |error| {
            Err(Error::BadObserved {
                observed: "OB1".into(),
                error,
            })
        };
        assert_eq!(
            set(&mut d, seen("nowhere", "tb", "tr")),
            bad(ob::Error::NoView("nowhere".into()))
        );
        assert_eq!(
            set(&mut d, seen("top", "tb", "none")),
            bad(ob::Error::NoLine("none".into()))
        );
        assert_eq!(d.observed().len(), n);
        set(&mut d, seen("top", "tb", "tr")).unwrap();
        assert_eq!(d.observed().len(), 1);
        // It is on the observed layer, which cannot go while it is held.
        assert!(d.layer("observed").is_some());
        assert!(matches!(
            d.remove_layer("observed"),
            Err(Error::LayerInUse { .. })
        ));
        // Its citation must be a registered source.
        let mut stray = seen("top", "tb", "tr");
        stray.provenance =
            Provenance::new(Method::Measured { sigma_mm: 0.1 }, Agent::Claude, day())
                .cite(Citation::new("unregistered", "x"));
        assert!(matches!(
            set(&mut d, stray),
            Err(Error::SourceMissing { .. })
        ));
        // The file form keeps it.
        let json = d.to_json().unwrap();
        assert_eq!(Document::parse(&json).unwrap(), d);
        // Nothing of the design changed: the same constraints and entities.
        let plain = crate::examples::block();
        assert_eq!(
            d.views().view("top").unwrap().sketch,
            plain.views().view("top").unwrap().sketch
        );
    }
}
