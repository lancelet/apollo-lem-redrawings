//! The sketch: geometry and the constraints that state what it means, with
//! the provenance of every claim, kept consistent by a thin interface.
//!
//! A `Sketch<F>` holds a [`Geometry`] (the state), one [`EntityMeta`] per
//! entity (its layer and provenance) and the [`Constraint`]s (intent, each
//! a [`Relation`] with provenance). It is changed only through the
//! operations below, each of which keeps these invariants, which the file
//! form is also held to on load:
//!
//! 1. every constraint operand names an existing entity of the stated kind,
//!    and every value's unit fits the relation;
//! 2. `meta` and `geometry` have the same keys;
//! 3. an entity or constraint in use cannot be removed; the error names
//!    the users;
//! 4. no two active constraints state the same fact: their lowered
//!    equations never coincide, however the fact is spelled;
//! 5. provenance references name things that exist, and a derivation
//!    never leads back to its own claim: where a claim's provenance is
//!    replaced ([`Sketch::set_entity_provenance`]) the references are
//!    followed for that. (Dispute and Supersede may refer back by their
//!    nature: a disputed claim and its adversary name each other.)
//! 6. status moves forward only, through the sketch's `accept`, `verify`,
//!    `dispute` and `supersede`.
//!
//! A sketch owns no values beyond its constraints: a constraint may name
//! a parameter of the document's table ([`Params`]), and the sketch is
//! then an open term, closed by the table it is given. Every operation
//! that needs values takes the table: adding or revaluing a constraint,
//! lowering, solving. Invariant 4 is kept against the table given, so a
//! constraint that names a parameter is admitted only when it resolves.
//! The file form cannot be given a table, so it checks the closed
//! constraints among themselves and leaves those that name parameters
//! to the document, which calls `Sketch::check_params` on load.
//!
//! Layer ids are stored here and validated by the document, which owns the
//! layer table. [`Sketch::lower`] gives the solver its input: the
//! equations of the active, driving constraints, each with the constraint
//! it came from.
//!
//! See `docs/sketch-plan.md` for the design.
//!
//! # Example
//!
//! Section A-A of Grumman LDW280-23631 sheet 3: the upper clip with its lug,
//! the reamed pin hole, the +Y27.00 plane and the rivet line, in model
//! millimetres (model Y along x, model X up the page). Every claim carries
//! its provenance: what the sheet prints, what the drawing shows, what we
//! measured off the ink. The geometry is the tracing; the sheet's
//! dimensions, the stations and the measured sizes constrain it completely,
//! and the two `REF` dimensions are checked against the result.
//!
//! ```
//! use lmsheets_document::value::relation::{
//!     arc, circle, line, ArcEnd,
//! };
//! use lmsheets_document::state::analysis::Options;
//! use lmsheets_document::value::geometry::Model;
//! use lmsheets_document::value::measure::{Measure, Unit};
//! use lmsheets_document::value::params::Params;
//! use lmsheets_document::value::provenance::{
//!     Agent, Citation, Date, ItemRef, Method, Provenance,
//! };
//! use lmsheets_document::value::relation::*;
//! use lmsheets_document::value::sketch::EntityStatus;
//! use lmsheets_document::sketch;
//!
//! let sheet = "LDW280-23631 sh3";
//! let day = Date::new(2026, 10, 15)?;
//! let cite = |at| Citation::new(sheet, at);
//! let read = || {
//!     Provenance::new(Method::Read, Agent::Claude, day).cite(cite("SECT A-A"))
//! };
//! let traced = || {
//!     Provenance::new(Method::Measured { sigma_mm: 0.3 }, Agent::Claude, day)
//!         .cite(cite("SECT A-A, scan"))
//! };
//! let shown = || {
//!     Provenance::new(Method::Estimated, Agent::Claude, day)
//!         .cite(cite("SECT A-A"))
//! };
//! let measured = || {
//!     Provenance::new(Method::Measured { sigma_mm: 0.15 }, Agent::Claude, day)
//!         .cite(cite("SECT A-A, ink"))
//! };
//! let from_stations = || {
//!     let from = vec![ItemRef::Constraint("station".into())];
//!     Provenance::new(Method::Derived { from }, Agent::Claude, day)
//!         .cite(cite("SECT A-A, station labels"))
//! };
//! let inch = |t: &str| Measure::inches(t).unwrap();
//! let mm = |v: f64| Measure::mm(v).unwrap();
//! let deg = |v: f64| Measure::deg(v).unwrap();
//!
//! let (top_o, bend_o, leg_o, leg_end, leg_i, bend_i, top_i, flange_end) = (
//!     line("top_o"), arc("bend_o"), line("leg_o"), line("leg_end"),
//!     line("leg_i"), arc("bend_i"), line("top_i"), line("flange_end"),
//! );
//! let (lug_l, f1, lug_s, lug_arc, f2, lug_r) = (
//!     line("lug_l"), arc("f1"), line("lug_s"),
//!     arc("lug_arc"), arc("f2"), line("lug_r"),
//! );
//! let (pin, y27, rivet) = (circle("pin"), line("y27"), line("rivet_line"));
//!
//! let mut s = sketch!(Model {
//!     entities(layer: "visible", provenance: traced()) {
//!         top_o: line (710.005, 4666.488) -> (692.123, 4666.488);
//!         bend_o: arc (692.123, 4662.613),
//!             r = 3.886, from = 1.5712, sweep = 1.5703;
//!         leg_o: line (688.237, 4662.613) -> (688.237, 4653.496);
//!         leg_end: line (688.237, 4653.496) -> (689.509, 4653.496);
//!         leg_i: line (689.509, 4653.496) -> (689.510, 4662.613);
//!         bend_i: arc (692.123, 4662.612),
//!             r = 2.613, from = 1.5704, sweep = 1.5711;
//!         top_i: line (692.123, 4665.218) -> (710.005, 4665.218);
//!         flange_end: line (710.005, 4665.218) -> (710.005, 4666.488);
//!         lug_l: line (694.055, 4665.218) -> (694.055, 4656.942);
//!         f1: arc (702.176, 4656.942),
//!             r = 8.121, from = 3.1409, sweep = 0.4665;
//!         lug_s: line (694.926, 4653.291) -> (699.508, 4644.239);
//!         lug_arc: arc (705.340, 4647.176),
//!             r = 6.533, from = 0.8792, sweep = -3.5544;
//!         f2: arc (714.168, 4657.839),
//!             r = 7.333, from = 3.1421, sweep = 0.8792;
//!         lug_r: line (706.836, 4657.839) -> (706.836, 4665.218);
//!         pin: circle (704.850, 4583.938), r = 3.175;
//!     }
//!     entities(layer: "phantom", provenance: traced()) {
//!         y27: line (685.799, 4680.467) -> (685.798, 4447.281);
//!         rivet_line: line (700.077, 4674.021) -> (700.092, 4507.316);
//!     }
//!     // What the sheet prints: the pin's stations, the reamed hole, the
//!     // dimensions, and two REF dimensions to check rather than solve.
//!     constraints(provenance: read()) {
//!         station: position(pin.centre(), inch("27.750"), inch("180.470"));
//!         ream: diameter(pin, inch(".2500"));
//!         d3200: vertical_distance(pin.centre(), top_i.end(), inch("3.200"));
//!         d188: horizontal_distance(
//!             rivet.start(), pin.centre(), inch(".188"),
//!         );
//!         d750: horizontal_distance(y27.start(), pin.centre(), inch(".750"))
//!             .driven()?;
//!         x183: vertical_distance(pin.centre(), top_o.start(), inch("3.250"))
//!             .driven()?;
//!     }
//!     // The +Y27.00 plane is .750 from a pin at Y27.750: a difference of
//!     // stations, which the .750 REF above confirms.
//!     constraints(provenance: from_stations()) {
//!         y27_at: horizontal_distance(
//!             y27.start(), pin.centre(), inch(".750"),
//!         );
//!     }
//!     // What the drawing shows: the clip's shape.
//!     constraints(provenance: shown()) {
//!         h1: horizontal(top_o);
//!         h2: horizontal(top_i);
//!         h3: horizontal(leg_end);
//!         v1: vertical(leg_o);
//!         v2: vertical(leg_i);
//!         v3: vertical(flange_end);
//!         v4: vertical(lug_l);
//!         v5: vertical(lug_r);
//!         v6: vertical(y27);
//!         v7: vertical(rivet);
//!         j1: tangent_join(top_o, LineEnd::End, bend_o, ArcEnd::Start);
//!         j2: tangent_join(leg_o, LineEnd::Start, bend_o, ArcEnd::End);
//!         c1: coincident(leg_o.end(), leg_end.start());
//!         c2: coincident(leg_end.end(), leg_i.start());
//!         j3: tangent_join(leg_i, LineEnd::End, bend_i, ArcEnd::End);
//!         j4: tangent_join(top_i, LineEnd::Start, bend_i, ArcEnd::Start);
//!         c3: coincident(top_i.end(), flange_end.start());
//!         c4: coincident(flange_end.end(), top_o.start());
//!         cc: concentric(bend_o, bend_i);
//!         o1: on_line(lug_l.start(), top_i);
//!         o2: on_line(lug_r.end(), top_i);
//!         j5: tangent_join(lug_l, LineEnd::End, f1, ArcEnd::Start);
//!         j6: tangent_join(lug_s, LineEnd::Start, f1, ArcEnd::End);
//!         j7: tangent_join(lug_s, LineEnd::End, lug_arc, ArcEnd::End);
//!         j8: tangent_arc_join(lug_arc, ArcEnd::Start, f2, ArcEnd::End);
//!         j9: tangent_join(lug_r, LineEnd::Start, f2, ArcEnd::Start);
//!     }
//!     // Measured off the ink: the sizes the sheet does not give. The leg's
//!     // thickness is not stated again; concentric bends imply it.
//!     constraints(provenance: measured()) {
//!         t1: length(flange_end, inch(".050"));
//!         rb: radius(bend_o, mm(3.886));
//!         fl: length(top_o, mm(17.882));
//!         ll: length(leg_o, mm(9.117));
//!         lat: horizontal_distance(y27.start(), leg_o.start(), mm(2.438));
//!         slope: angle(lug_s, top_i, deg(63.16));
//!         lx1: horizontal_distance(
//!             top_i.start(), lug_l.start(), mm(1.932),
//!         );
//!         lx2: horizontal_distance(
//!             top_i.start(), lug_r.end(), mm(14.713),
//!         );
//!         ly1: length(lug_l, mm(8.276));
//!         ly2: length(lug_r, mm(7.379));
//!         rf1: radius(f1, mm(8.121));
//!         rf2: radius(f2, mm(7.333));
//!         rh: radius(lug_arc, mm(6.533));
//!         ye: vertical_distance(pin.centre(), y27.start(), mm(96.529));
//!         yl: length(y27, mm(233.186));
//!         re: vertical_distance(pin.centre(), rivet.start(), mm(90.083));
//!         rl: length(rivet, mm(166.705));
//!     }
//! });
//!
//! let a = s.solve(Options::default(), &Params::NONE)?;
//! // Fully constrained: nothing free, nowhere to slide, nothing redundant.
//! assert_eq!((a.internal_free, a.placement.count()), (0, 0));
//! assert!(a.groups.is_empty());
//! assert!(a.status.values().all(|st| *st == EntityStatus::Fixed));
//! assert!(a.report.residual_max < 1e-6);
//! // The sheet's REF dimensions agree with the figure to a fraction of a mm.
//! for check in &a.reference {
//!     assert!(check.off_by.iter().all(|v| v.abs() < 0.2), "{check:?}");
//! }
//! // The flange's inner face sits 3.200 in above the pin, as printed.
//! let face = s.geometry().position(&top_i.end())?;
//! let hole = s.geometry().position(&pin.centre())?;
//! assert!((face.y - hole.y - 81.28).abs() < 1e-9);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::solver::equation::{self as eq, Equation};
use crate::solver::near::{self, Prior};
use crate::solver::solve::{self as solver, Options, Placement, Report, Solution, Value};
use crate::value::geometry::{self, Entity, EntityId, Frame, Geometry};
use crate::value::measure::{self, Measure, ParamName, Tolerance};
use crate::value::params::Params;
use crate::value::provenance::{self, Check, ConstraintId, ItemRef, Method, Provenance, Status};
use crate::value::relation::{self, Relation};
use crate::value::style::{Convention, Layer, LineStyle, Role};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ustr::Ustr;

/// The name of a layer in the document's layer table, an interned `Copy`
/// handle. The sketch stores it; the document validates it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LayerId(Ustr);

impl LayerId {
    /// A layer name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&LayerId> for LayerId {
    fn from(id: &LayerId) -> Self {
        *id
    }
}

impl From<&str> for LayerId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// What the sketch knows about an entity besides its values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityMeta {
    /// Where it is kept: organisation, and the defaults below.
    pub layer: LayerId,
    /// Where it came from.
    pub provenance: Provenance,
    /// How it is drawn, if it says; else its layer's
    /// ([`convention_on`](Self::convention_on)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convention: Option<Convention>,
    /// What it is for, if it says; else its layer's
    /// ([`role_on`](Self::role_on)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
}

impl EntityMeta {
    /// A layer and a provenance; the convention and role the layer's.
    pub fn new(layer: impl Into<LayerId>, provenance: Provenance) -> Self {
        Self {
            layer: layer.into(),
            provenance,
            convention: None,
            role: None,
        }
    }

    /// The same, with its own convention.
    pub fn with_convention(mut self, convention: Convention) -> Self {
        self.convention = Some(convention);
        self
    }

    /// The same, with its own role.
    pub fn with_role(mut self, role: Role) -> Self {
        self.role = Some(role);
        self
    }

    /// Its convention on `layer` (the table's row for its layer): its own,
    /// else the layer's default.
    pub fn convention_on(&self, layer: &Layer) -> Option<Convention> {
        self.convention.or(layer.convention)
    }

    /// Its role on `layer`: its own, else the layer's default.
    pub fn role_on(&self, layer: &Layer) -> Option<Role> {
        self.role.or(layer.role)
    }

    /// How it is drawn on `layer`, in a document whose layer table is
    /// `table`: the layer's style, unless the entity has a convention of
    /// its own other than the layer's; then the style of the table's row
    /// that is that convention's default (the first by id), else the
    /// convention's standard style.
    pub fn style_on<'a>(
        &self,
        layer: &Layer,
        mut table: impl Iterator<Item = &'a Layer>,
    ) -> LineStyle {
        match self.convention {
            Some(c) if layer.convention != Some(c) => table
                .find(|l| l.convention == Some(c))
                .map_or_else(|| c.style(), |l| l.style.clone()),
            _ => layer.style.clone(),
        }
    }
}

/// A stored constraint: a relation and its provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    relation: Relation,
    provenance: Provenance,
}

impl Constraint {
    /// The relation.
    pub fn relation(&self) -> &Relation {
        &self.relation
    }

    /// The provenance.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Whether the constraint is solved for: in force (not disputed or
    /// superseded) and driving.
    pub fn active(&self) -> bool {
        self.provenance.active() && self.relation.driving()
    }
}

/// The equations of the active constraints, each with the constraint it
/// came from. What the solver takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Lowered {
    /// The equations, canonical.
    pub equations: Vec<Equation>,
    /// For each equation, the constraint that produced it.
    pub owners: Vec<ConstraintId>,
}

/// An entity's status from how it can move: nothing, with its body only,
/// some of its coordinates, or all of them.
pub(crate) fn entity_status(m: Option<&solver::Motion>, width: usize) -> EntityStatus {
    match m {
        None => EntityStatus::Fixed,
        Some(m) if m.free == 0 => EntityStatus::Fixed,
        Some(m) if !m.beyond_placement => EntityStatus::FixedUpToPlacement,
        Some(m) if m.values.len() >= width => EntityStatus::Free,
        Some(m) => EntityStatus::Partial(m.values.clone()),
    }
}

/// How far an entity is determined by the constraints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EntityStatus {
    /// Every value is determined.
    Fixed,
    /// Determined except for where the whole figure sits.
    FixedUpToPlacement,
    /// These values can still move (besides placement).
    Partial(Vec<Value>),
    /// No value is determined (besides placement).
    Free,
}

/// Constraints that are not independent at the solution. With zero
/// residual they repeat one another; otherwise they conflict, and
/// `culprits` are those whose removal alone would resolve it.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The constraints, in id order.
    pub constraints: Vec<ConstraintId>,
    /// The largest residual among them, mm.
    pub residual: f64,
    /// Members whose removal alone resolves the conflict (empty for a
    /// redundancy).
    pub culprits: Vec<ConstraintId>,
}

impl Analysis {
    /// One entity's status.
    pub fn entity(&self, id: impl Into<EntityId>) -> Option<&EntityStatus> {
        self.status.get(&id.into())
    }
}

/// A driven (`REF`) dimension checked against the figure.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceCheck {
    /// The dimension.
    pub constraint: ConstraintId,
    /// The figure's value less the claimed one, per equation, in mm or
    /// rad; zero when the figure agrees with the sheet.
    pub off_by: Vec<f64>,
}

/// What a solve or an analysis found, in the sketch's terms.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    /// The solver's report, in equation terms.
    pub report: Report,
    /// What the figure is free to do as a whole.
    pub placement: Placement,
    /// Freedoms other than placement; zero means constrained up to
    /// placement, or fully if placement is none.
    pub internal_free: usize,
    /// How far each entity is determined.
    pub status: BTreeMap<EntityId, EntityStatus>,
    /// Redundancies and conflicts, as constraints.
    pub groups: Vec<Group>,
    /// Driven dimensions against the figure.
    pub reference: Vec<ReferenceCheck>,
}

/// What [`Sketch::solve_near`] found.
#[derive(Clone, Debug, PartialEq)]
pub struct Near {
    /// The report, as for [`Sketch::solve`].
    pub analysis: Analysis,
    /// The weighted departure from the prior, `½ (x − p)ᵀ W (x − p)`.
    pub cost: f64,
    /// Whether the constraints all hold at the point the solve came to
    /// rest at (a first-order KKT point); if not, `analysis` says why
    /// (conflicts, a stall).
    pub converged: bool,
    /// The multiplier of each constraint's equations, by constraint: the
    /// largest magnitude over its rows, in cost per unit of residual. How
    /// hard each pulls the design from the prior.
    pub multipliers: BTreeMap<ConstraintId, f64>,
    /// Directions that are free in the constraints and weighed by
    /// nothing; see [`Nearest::unheld`](crate::state::analysis::Nearest).
    pub unheld: usize,
}

/// A dependency group with a residual above this, mm, is a conflict rather
/// than a redundancy.
pub(crate) const CONFLICT: f64 = 1e-6;

/// What can go wrong changing a sketch.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A geometry error: a bad entity, a missing or wrong-kind operand.
    Geometry(geometry::Error),
    /// A relation error: units, parameters, lowering.
    Relation(relation::Error),
    /// A provenance error: evidence, status transitions.
    Provenance(provenance::Error),
    /// A measure error.
    Measure(measure::Error),
    /// An equation or solver error.
    Equation(eq::Error),
    /// A constraint with this id already exists.
    DuplicateConstraint(ConstraintId),
    /// No constraint has this id.
    NoSuchConstraint(ConstraintId),
    /// A provenance refers to an item that does not exist.
    NoSuchItem(ItemRef),
    /// A provenance would make an item depend on itself, through the
    /// items it refers to.
    ProvenanceCycle(ItemRef),
    /// An entity's meta is missing or an extra meta has no entity (file).
    MetaMismatch(EntityId),
    /// The entity is an operand of a constraint, or cited by an item.
    EntityInUse {
        /// The entity.
        id: EntityId,
        /// Its users.
        by: Vec<ItemRef>,
    },
    /// The constraint is cited by these items' provenance.
    ConstraintInUse {
        /// The constraint.
        id: ConstraintId,
        /// Its users.
        by: Vec<ItemRef>,
    },
    /// The fact is already stated by an active constraint.
    AlreadyStated {
        /// The constraint that states it.
        by: ConstraintId,
    },
}

impl From<geometry::Error> for Error {
    fn from(e: geometry::Error) -> Self {
        Error::Geometry(e)
    }
}

impl From<relation::Error> for Error {
    fn from(e: relation::Error) -> Self {
        Error::Relation(e)
    }
}

impl From<provenance::Error> for Error {
    fn from(e: provenance::Error) -> Self {
        Error::Provenance(e)
    }
}

impl From<measure::Error> for Error {
    fn from(e: measure::Error) -> Self {
        Error::Measure(e)
    }
}

impl From<eq::Error> for Error {
    fn from(e: eq::Error) -> Self {
        Error::Equation(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// Geometry and constraints, kept consistent. See the module documentation
/// for the invariants and operations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "", try_from = "Raw<F>", into = "Raw<F>")]
pub struct Sketch<F: Frame> {
    geometry: Geometry<F>,
    meta: BTreeMap<EntityId, EntityMeta>,
    constraints: BTreeMap<ConstraintId, Constraint>,
}

/// The file form: the three parts, checked on the way in by rebuilding the
/// sketch through its own operations.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
struct Raw<F: Frame> {
    geometry: Geometry<F>,
    meta: BTreeMap<EntityId, EntityMeta>,
    constraints: BTreeMap<ConstraintId, Constraint>,
}

impl<F: Frame> From<Sketch<F>> for Raw<F> {
    fn from(s: Sketch<F>) -> Self {
        Raw {
            geometry: s.geometry,
            meta: s.meta,
            constraints: s.constraints,
        }
    }
}

/// Whether a constraint names no parameter: checkable without a table.
fn closed(c: &Constraint) -> bool {
    c.relation.params().is_empty()
}

impl<F: Frame> TryFrom<Raw<F>> for Sketch<F> {
    type Error = Error;

    fn try_from(r: Raw<F>) -> Result<Self, Error> {
        // Entities and their meta must match one to one.
        for id in r.meta.keys() {
            if r.geometry.get(id).is_none() {
                return Err(Error::MetaMismatch(*id));
            }
        }
        // Entities may cite one another in any order, like constraints:
        // place them all (each entity and provenance checked), then check
        // the references against the whole.
        let mut s = Sketch::default();
        for (id, e) in r.geometry.iter() {
            let meta = r.meta.get(id).ok_or(Error::MetaMismatch(*id))?;
            meta.provenance.check()?;
            s.geometry.add(*id, e.clone())?;
            s.meta.insert(*id, meta.clone());
        }
        // Constraints may cite one another in any order: place them all,
        // then check each against the whole. The closed ones are checked
        // for duplicates among themselves; those that name parameters
        // wait for the document's table (`check_params`).
        for (id, c) in &r.constraints {
            if s.constraints.contains_key(id) {
                return Err(Error::DuplicateConstraint(*id));
            }
            s.constraints.insert(*id, c.clone());
        }
        for (id, c) in &r.constraints {
            s.check_structure(*id, &c.relation, &c.provenance)?;
            if closed(c) {
                s.check_stated(*id, &c.relation, &c.provenance, &Params::NONE, &closed)?;
            }
        }
        for (id, meta) in &r.meta {
            s.check_refs(ItemRef::Entity(*id), &meta.provenance)?;
        }
        Ok(s)
    }
}

impl<F: Frame> Default for Sketch<F> {
    fn default() -> Self {
        Self {
            geometry: Geometry::default(),
            meta: BTreeMap::new(),
            constraints: BTreeMap::new(),
        }
    }
}

impl<F: Frame> Sketch<F> {
    /// An empty sketch.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a sketch by adding to an empty one in `f`; the first error
    /// stops the build. What [`try_sketch!`](crate::try_sketch) expands to.
    pub fn build(f: impl FnOnce(&mut Sketch<F>) -> Result<(), Error>) -> Result<Sketch<F>, Error> {
        let mut s = Sketch::new();
        f(&mut s)?;
        Ok(s)
    }

    // ---- solving

    /// Solve the active constraints in the environment `params`, apply the
    /// result to the geometry, and report. The geometry the solver returns
    /// has the same entities with values that passed every entity check,
    /// so taking it keeps the invariants.
    pub fn solve(&mut self, o: Options, params: &Params) -> Result<Analysis, Error> {
        let lowered = self.lower(params)?;
        let solved = solver::solve(&self.geometry, &lowered.equations, o)?;
        self.set_geometry(solved.geometry);
        self.analysis(&lowered, solved.report, o, params)
    }

    /// Move the geometry to the design nearest `prior` (weighted by its
    /// weights) on which every active constraint holds exactly, apply it,
    /// and report: see [`solver::near`]. The prior must have the sketch's
    /// entities, each of its kind. Where `solve` finds the solution
    /// nearest the current values in the plain norm, this finds it nearest
    /// the ink in the ink's own precision, starting from the ink.
    pub fn solve_near(
        &mut self,
        prior: &Prior<F>,
        o: Options,
        params: &Params,
    ) -> Result<Near, Error> {
        // The same entities, each of its kind: the first id that differs.
        let same: Result<(), geometry::Error> = {
            let (mut ours, mut theirs) = (self.geometry.iter(), prior.geometry.iter());
            loop {
                match (ours.next(), theirs.next()) {
                    (None, None) => break Ok(()),
                    (Some((a, x)), Some((b, y))) if a == b && x.kind() == y.kind() => {}
                    (Some((a, _)), Some((b, _))) if a == b => {
                        break Err(geometry::Error::KindChanged(*a))
                    }
                    (Some((a, _)), Some((b, _))) => break Err(geometry::Error::NoSuch(*a.min(b))),
                    (Some((a, _)), None) | (None, Some((a, _))) => {
                        break Err(geometry::Error::NoSuch(*a))
                    }
                }
            }
        };
        same?;
        let lowered = self.lower(params)?;
        let near = near::solve_near(prior, &lowered.equations, o)?;
        self.set_geometry(near.solution.geometry);
        let mut multipliers = BTreeMap::new();
        for (owner, l) in lowered.owners.iter().zip(&near.multipliers) {
            let m: &mut f64 = multipliers.entry(*owner).or_insert(0.0);
            *m = m.max(*l);
        }
        Ok(Near {
            analysis: self.analysis(&lowered, near.solution.report, o, params)?,
            cost: near.cost,
            converged: near.converged,
            multipliers,
            unheld: near.unheld,
        })
    }

    /// Take a geometry with the same entities and values that passed every
    /// entity check (the solver's result, or one view's share of a system's
    /// result). The keys must match; nothing else is checked, because the
    /// solver built it through `set`.
    pub(crate) fn set_geometry(&mut self, g: Geometry<F>) {
        debug_assert!(g.iter().all(|(id, _)| self.meta.contains_key(id)));
        debug_assert_eq!(g.len(), self.meta.len());
        self.geometry = g;
    }

    /// The same report at the current state, without moving anything.
    pub fn analyse(&self, o: Options, params: &Params) -> Result<Analysis, Error> {
        let lowered = self.lower(params)?;
        let at = solver::analyse(&self.geometry, &lowered.equations, o)?;
        self.analysis(&lowered, at.report, o, params)
    }

    /// [`Sketch::analyse`] with the entities in separate `bodies`, whose
    /// placement is judged apart: a body free to move rigidly on its own
    /// is placement, not under-constraint, so the internal freedoms left
    /// are those of each body and between them. K10 uses two, the
    /// design's and annotation layout's, to see what the design leaves
    /// open apart from the layout.
    pub fn analyse_bodies(
        &self,
        bodies: &[Vec<EntityId>],
        o: Options,
        params: &Params,
    ) -> Result<Analysis, Error> {
        let lowered = self.lower(params)?;
        let at = solver::analyse_bodies(&self.geometry, &lowered.equations, bodies, o)?;
        self.analysis(&lowered, at.report, o, params)
    }

    /// The largest residual of the active constraints at the current
    /// state: [`Sketch::analyse`]'s `residual_max` without the rest of
    /// the report (its rank and freedoms take an SVD of the whole
    /// sketch).
    pub fn residual_max(&self, params: &Params) -> Result<f64, Error> {
        let lowered = self.lower(params)?;
        let r = solver::residuals(&self.geometry, &lowered.equations)?;
        Ok(r.iter().flatten().fold(0.0, |m, v| m.max(v.abs())))
    }

    /// Solve with extra equations on top of the active constraints (a
    /// drag, a what-if), returning the result without applying it.
    pub fn solve_with(
        &self,
        extra: &[Equation],
        o: Options,
        params: &Params,
    ) -> Result<Solution<F>, Error> {
        let mut eqs = self.lower(params)?.equations;
        eqs.extend(extra.iter().copied());
        Ok(solver::solve(&self.geometry, &eqs, o)?)
    }

    /// Translate a solver report into the sketch's terms. For a
    /// conflicting group, each member is removed in turn and the rest
    /// re-solved; those whose removal resolves it are the culprits.
    fn analysis(
        &self,
        lowered: &Lowered,
        report: Report,
        o: Options,
        params: &Params,
    ) -> Result<Analysis, Error> {
        let status = self
            .geometry
            .iter()
            .map(|(id, e)| (*id, entity_status(report.motion.get(id), e.values())))
            .collect();
        let mut groups = Vec::new();
        for d in &report.dependencies {
            let mut constraints: Vec<ConstraintId> =
                d.equations.iter().map(|&i| lowered.owners[i]).collect();
            constraints.sort_unstable();
            constraints.dedup();
            let mut culprits = Vec::new();
            if d.residual > CONFLICT {
                for c in &constraints {
                    let rest: Vec<Equation> = lowered
                        .equations
                        .iter()
                        .zip(&lowered.owners)
                        .filter(|(_, owner)| *owner != c)
                        .map(|(e, _)| *e)
                        .collect();
                    let trial = solver::solve(&self.geometry, &rest, o)?;
                    if trial.report.residual_max <= CONFLICT {
                        culprits.push(*c);
                    }
                }
            }
            let g = Group {
                constraints,
                residual: d.residual,
                culprits,
            };
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        let driven = self.lower_driven(params)?;
        let residuals = solver::residuals(&self.geometry, &driven.equations)?;
        let mut reference: Vec<ReferenceCheck> = Vec::new();
        for (owner, r) in driven.owners.iter().zip(residuals) {
            match reference.last_mut() {
                Some(last) if last.constraint == *owner => last.off_by.extend(r),
                _ => reference.push(ReferenceCheck {
                    constraint: *owner,
                    off_by: r,
                }),
            }
        }
        Ok(Analysis {
            placement: report.placement,
            internal_free: report.internal_free,
            report,
            status,
            groups,
            reference,
        })
    }

    // ---- queries

    /// The geometry.
    pub fn geometry(&self) -> &Geometry<F> {
        &self.geometry
    }

    /// An entity's meta.
    pub fn meta(&self, id: impl Into<EntityId>) -> Option<&EntityMeta> {
        let id = &id.into();
        self.meta.get(id)
    }

    /// All entities with their meta, in id order.
    pub fn entities(&self) -> impl Iterator<Item = (&EntityId, &Entity<F>, &EntityMeta)> {
        self.geometry.iter().map(|(id, e)| (id, e, &self.meta[id]))
    }

    /// A constraint.
    pub fn constraint(&self, id: impl Into<ConstraintId>) -> Option<&Constraint> {
        let id = &id.into();
        self.constraints.get(id)
    }

    /// All constraints, in id order.
    pub fn constraints(&self) -> impl Iterator<Item = (&ConstraintId, &Constraint)> {
        self.constraints.iter()
    }

    /// The items that use the entity: constraints that have it as an
    /// operand or cite it, and entities whose provenance cites it.
    pub fn users_of_entity(&self, id: impl Into<EntityId>) -> Vec<ItemRef> {
        let id = &id.into();
        let item = ItemRef::Entity(*id);
        let from_constraints = self
            .constraints
            .iter()
            .filter(|(_, c)| {
                c.relation.operands().iter().any(|(e, _)| e == id)
                    || refs_of(&c.provenance).contains(&item)
            })
            .map(|(cid, _)| ItemRef::Constraint(*cid));
        let from_entities = self
            .meta
            .iter()
            .filter(|(eid, m)| *eid != id && refs_of(&m.provenance).contains(&item))
            .map(|(eid, _)| ItemRef::Entity(*eid));
        from_constraints.chain(from_entities).collect()
    }

    /// The constraints whose values refer to the parameter.
    pub fn users_of_param(&self, name: impl Into<ParamName>) -> Vec<ConstraintId> {
        let name = &name.into();
        self.constraints
            .iter()
            .filter(|(_, c)| c.relation.params().contains(name))
            .map(|(cid, _)| *cid)
            .collect()
    }

    /// The items whose provenance cites the constraint.
    pub fn users_of_constraint(&self, id: impl Into<ConstraintId>) -> Vec<ItemRef> {
        let id = &id.into();
        let item = ItemRef::Constraint(*id);
        let from_constraints = self
            .constraints
            .iter()
            .filter(|(cid, c)| *cid != id && refs_of(&c.provenance).contains(&item))
            .map(|(cid, _)| ItemRef::Constraint(*cid));
        let from_entities = self
            .meta
            .iter()
            .filter(|(_, m)| refs_of(&m.provenance).contains(&item))
            .map(|(eid, _)| ItemRef::Entity(*eid));
        from_constraints.chain(from_entities).collect()
    }

    /// The equations of the active, driven (`REF`) constraints, with
    /// their owners, in constraint order: what to check the figure
    /// against.
    pub(crate) fn lower_driven(&self, params: &Params) -> Result<Lowered, Error> {
        let lookup = params.lookup();
        let mut equations = Vec::new();
        let mut owners = Vec::new();
        for (id, c) in &self.constraints {
            if c.provenance.active() && !c.relation.driving() {
                for e in c.relation.lower(&self.geometry, &lookup)? {
                    equations.push(e);
                    owners.push(*id);
                }
            }
        }
        Ok(Lowered { equations, owners })
    }

    /// The equations of the active, driving constraints, with their
    /// owners, their parameters resolved in `params`.
    pub fn lower(&self, params: &Params) -> Result<Lowered, Error> {
        let lookup = params.lookup();
        let mut equations = Vec::new();
        let mut owners = Vec::new();
        for (id, c) in &self.constraints {
            if !c.active() {
                continue;
            }
            for e in c.relation.lower(&self.geometry, &lookup)? {
                equations.push(e);
                owners.push(*id);
            }
        }
        Ok(Lowered { equations, owners })
    }

    // ---- entities

    /// Add an entity with its meta. Both are checked.
    pub fn add_entity(
        &mut self,
        id: EntityId,
        e: Entity<F>,
        meta: EntityMeta,
    ) -> Result<(), Error> {
        meta.provenance.check()?;
        self.check_refs(ItemRef::Entity(id), &meta.provenance)?;
        self.geometry.add(id, e)?;
        self.meta.insert(id, meta);
        Ok(())
    }

    /// Replace an entity's values (not its kind), returning the old ones.
    pub fn set_entity(
        &mut self,
        id: impl Into<EntityId>,
        e: Entity<F>,
    ) -> Result<Entity<F>, Error> {
        let id = &id.into();
        let old = self
            .geometry
            .get(id)
            .cloned()
            .ok_or(geometry::Error::NoSuch(*id))?;
        self.geometry.set(id, e)?;
        Ok(old)
    }

    /// Remove an entity and its meta. Refused while a constraint uses or
    /// cites it.
    pub fn remove_entity(
        &mut self,
        id: impl Into<EntityId>,
    ) -> Result<(Entity<F>, EntityMeta), Error> {
        let id = &id.into();
        if self.geometry.get(id).is_none() {
            return Err(geometry::Error::NoSuch(*id).into());
        }
        let by = self.users_of_entity(id);
        if !by.is_empty() {
            return Err(Error::EntityInUse { id: *id, by });
        }
        let e = self.geometry.remove(id)?;
        let meta = self.meta.remove(id).expect("meta matches geometry");
        Ok((e, meta))
    }

    /// Replace an entity's provenance, returning the old. Checked as a new
    /// entity's is, and the references must not lead back to the entity
    /// (through any provenance they reach): a derivation is acyclic. The
    /// claim keeps where it stood: a replacement that is merely
    /// `Provisional` leaves the old status; any other status replaces it.
    pub fn set_entity_provenance(
        &mut self,
        id: impl Into<EntityId>,
        provenance: Provenance,
    ) -> Result<Provenance, Error> {
        let id = id.into();
        provenance.check()?;
        self.check_refs(ItemRef::Entity(id), &provenance)?;
        let old = self
            .meta
            .get(&id)
            .ok_or(geometry::Error::NoSuch(id))?
            .provenance
            .clone();
        let mut seen = std::collections::BTreeSet::new();
        let mut todo = refs_of(&provenance);
        while let Some(r) = todo.pop() {
            if r == ItemRef::Entity(id) {
                return Err(Error::ProvenanceCycle(r));
            }
            if seen.insert(r) {
                let there = match r {
                    ItemRef::Entity(e) => self.meta.get(&e).map(|m| &m.provenance),
                    ItemRef::Constraint(c) => self.constraints.get(&c).map(|k| &k.provenance),
                };
                todo.extend(there.map(refs_of).unwrap_or_default());
            }
        }
        let provenance = match provenance.status() {
            Status::Provisional => provenance.with_status(old.status().clone()),
            _ => provenance,
        };
        let meta = self.meta.get_mut(&id).ok_or(geometry::Error::NoSuch(id))?;
        Ok(std::mem::replace(&mut meta.provenance, provenance))
    }

    /// Move an entity to a layer, returning the old layer.
    pub fn set_layer(&mut self, id: impl Into<EntityId>, layer: LayerId) -> Result<LayerId, Error> {
        let id = &id.into();
        let meta = self.meta.get_mut(id).ok_or(geometry::Error::NoSuch(*id))?;
        Ok(std::mem::replace(&mut meta.layer, layer))
    }

    // ---- constraints

    /// Everything a constraint must satisfy against the rest of the sketch
    /// in the environment `params`: operands, units, provenance and its
    /// references, that its parameters resolve, and that no other active
    /// constraint already states it.
    fn check_constraint(
        &self,
        id: ConstraintId,
        relation: &Relation,
        provenance: &Provenance,
        params: &Params,
    ) -> Result<(), Error> {
        self.check_structure(id, relation, provenance)?;
        self.check_stated(id, relation, provenance, params, &|_| true)
    }

    /// The checks that need no values: operands, units, provenance and
    /// its references.
    fn check_structure(
        &self,
        id: ConstraintId,
        relation: &Relation,
        provenance: &Provenance,
    ) -> Result<(), Error> {
        relation.check(&self.geometry)?;
        provenance.check()?;
        self.check_refs(ItemRef::Constraint(id), provenance)
    }

    /// The checks that need values: the relation lowers in `params`
    /// (its parameters resolve), and if it is active and driving, no
    /// other active constraint `among` those considered states the same
    /// fact.
    fn check_stated(
        &self,
        id: ConstraintId,
        relation: &Relation,
        provenance: &Provenance,
        params: &Params,
        among: &dyn Fn(&Constraint) -> bool,
    ) -> Result<(), Error> {
        let lookup = params.lookup();
        let mine = relation.lower(&self.geometry, &lookup)?;
        // Every position it reads exists now (an arc's extreme lies in
        // its sweep, a spline has the point).
        for e in &mine {
            for p in e.point_refs() {
                self.geometry.position(&p)?;
            }
        }
        if provenance.active() && relation.driving() {
            for (other, c) in &self.constraints {
                if *other == id || !c.active() || !among(c) {
                    continue;
                }
                let theirs = c.relation.lower(&self.geometry, &lookup)?;
                if mine.iter().any(|e| theirs.contains(e)) {
                    return Err(Error::AlreadyStated { by: *other });
                }
            }
        }
        Ok(())
    }

    /// The document's check, after the file form or a change to the
    /// table: every constraint that names a parameter resolves in
    /// `params` and states nothing another active constraint states.
    pub(crate) fn check_params(&self, params: &Params) -> Result<(), Error> {
        for (id, c) in &self.constraints {
            if !closed(c) {
                self.check_stated(*id, &c.relation, &c.provenance, params, &|_| true)?;
            }
        }
        Ok(())
    }

    fn item_exists(&self, r: ItemRef) -> bool {
        match r {
            ItemRef::Entity(e) => self.geometry.get(e).is_some(),
            ItemRef::Constraint(c) => self.constraints.contains_key(&c),
        }
    }

    /// Every item a provenance refers to must exist and not be the item
    /// itself.
    fn check_refs(&self, item: ItemRef, p: &Provenance) -> Result<(), Error> {
        for r in refs_of(p) {
            if !self.item_exists(r) || r == item {
                return Err(Error::NoSuchItem(r));
            }
        }
        Ok(())
    }

    /// Add a constraint. Checked against everything (see the module doc)
    /// in the environment `params`.
    pub fn add_constraint(
        &mut self,
        id: ConstraintId,
        relation: Relation,
        provenance: Provenance,
        params: &Params,
    ) -> Result<(), Error> {
        if self.constraints.contains_key(&id) {
            return Err(Error::DuplicateConstraint(id));
        }
        self.check_constraint(id, &relation, &provenance, params)?;
        self.constraints.insert(
            id,
            Constraint {
                relation,
                provenance,
            },
        );
        Ok(())
    }

    /// Remove a constraint. Refused while another item's provenance cites
    /// it.
    pub fn remove_constraint(&mut self, id: impl Into<ConstraintId>) -> Result<Constraint, Error> {
        let id = &id.into();
        if !self.constraints.contains_key(id) {
            return Err(Error::NoSuchConstraint(*id));
        }
        let by = self.users_of_constraint(id);
        if !by.is_empty() {
            return Err(Error::ConstraintInUse { id: *id, by });
        }
        Ok(self.constraints.remove(id).expect("checked above"))
    }

    /// Replace a dimension's values, returning the old ones. The new values
    /// are checked like a new constraint's.
    pub fn set_values(
        &mut self,
        id: impl Into<ConstraintId>,
        values: Vec<Measure>,
        params: &Params,
    ) -> Result<Vec<Measure>, Error> {
        let id = &id.into();
        let c = self
            .constraints
            .get(id)
            .ok_or(Error::NoSuchConstraint(*id))?;
        let mut relation = c.relation.clone();
        let old = relation.set_values(values)?;
        self.check_constraint(*id, &relation, &c.provenance, params)?;
        self.constraints
            .get_mut(id)
            .expect("checked above")
            .relation = relation;
        Ok(old)
    }

    /// Replace a dimension's tolerance, returning the old one.
    pub fn set_tolerance(
        &mut self,
        id: impl Into<ConstraintId>,
        tolerance: Tolerance,
        params: &Params,
    ) -> Result<Tolerance, Error> {
        let id = &id.into();
        let c = self
            .constraints
            .get(id)
            .ok_or(Error::NoSuchConstraint(*id))?;
        let mut relation = c.relation.clone();
        let old = relation.set_tolerance(tolerance)?;
        self.check_constraint(*id, &relation, &c.provenance, params)?;
        self.constraints
            .get_mut(id)
            .expect("checked above")
            .relation = relation;
        Ok(old)
    }

    // ---- provenance transitions

    fn provenance_mut(&mut self, item: ItemRef) -> Result<&mut Provenance, Error> {
        match item {
            ItemRef::Entity(id) => self
                .meta
                .get_mut(&id)
                .map(|m| &mut m.provenance)
                .ok_or(Error::NoSuchItem(item)),
            ItemRef::Constraint(id) => self
                .constraints
                .get_mut(&id)
                .map(|c| &mut c.provenance)
                .ok_or(Error::NoSuchItem(item)),
        }
    }

    /// Accept a provisional claim.
    pub fn accept(&mut self, item: ItemRef) -> Result<(), Error> {
        Ok(self.provenance_mut(item)?.accept()?)
    }

    /// Record a verification of a claim.
    pub fn verify(&mut self, item: ItemRef, check: Check) -> Result<(), Error> {
        Ok(self.provenance_mut(item)?.verify(check)?)
    }

    /// Dispute a claim against named others, which must exist.
    pub fn dispute(&mut self, item: ItemRef, against: Vec<ItemRef>) -> Result<(), Error> {
        for r in &against {
            if !self.item_exists(*r) || *r == item {
                return Err(Error::NoSuchItem(*r));
            }
        }
        Ok(self.provenance_mut(item)?.dispute(against)?)
    }

    /// Supersede a claim by a named one, which must exist.
    pub fn supersede(&mut self, item: ItemRef, by: ItemRef) -> Result<(), Error> {
        if !self.item_exists(by) || by == item {
            return Err(Error::NoSuchItem(by));
        }
        Ok(self.provenance_mut(item)?.supersede(by)?)
    }
}

/// The items a provenance refers to.
fn refs_of(p: &Provenance) -> Vec<ItemRef> {
    let mut out = Vec::new();
    if let Method::Derived { from } = p.method() {
        out.extend(from.iter().copied());
    }
    match p.status() {
        Status::Disputed { against } => out.extend(against.iter().copied()),
        Status::Superseded { by } => out.push(*by),
        _ => {}
    }
    out
}

/// Build a `Sketch` declaratively: blocks of entities (with a layer and a
/// provenance) and constraints, each going through the checked operations,
/// so nothing bypasses the invariants. Blocks may repeat. A `params(table);`
/// block names the table the constraints after it are checked in; without
/// one they are checked in the empty table, so they may name no parameter.
///
/// ```
/// use lmsheets_document::value::relation::{arc, line, ArcEnd};
/// use lmsheets_document::value::geometry::Model;
/// use lmsheets_document::value::measure::{Measure, Unit};
/// use lmsheets_document::value::params::{Param, Params};
/// use lmsheets_document::value::provenance::{Agent, Date, Method, Provenance};
/// use lmsheets_document::value::relation::*;
/// use lmsheets_document::try_sketch;
///
/// let day = Date::new(2026, 10, 15)?;
/// let ours = || Provenance::new(Method::Authored, Agent::Claude, day);
/// let mut table = Params::new();
/// table.set("rb".into(), Param::new(Measure::mm(4.0)?, ours())?)?;
/// let s = try_sketch!(Model {
///     entities(layer: "visible", provenance: ours()) {
///         top:  line (0.0, 0.0) -> (10.0, 0.0);
///         bend: arc (10.0, 5.0), r = 5.0, from = -1.5708, sweep = 1.5708;
///     }
///     params(&table);
///     constraints(provenance: ours()) {
///         h: horizontal(line("top"));
///         j: tangent_join(
///             line("top"), LineEnd::End, arc("bend"), ArcEnd::Start,
///         );
///         r: radius(arc("bend"), Measure::param("rb".into())?);
///     }
/// })?;
/// assert_eq!(s.constraints().count(), 3);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Returns `Result<Sketch<F>, Error>`; [`sketch!`](macro@crate::sketch) is the
/// same but panics on error, for tests.
#[macro_export]
macro_rules! try_sketch {
    ($frame:ty { $($body:tt)* }) => {
        $crate::value::sketch::Sketch::<$frame>::build(|s| {
            let _ = &s;
            let params: &$crate::value::params::Params = &$crate::value::params::Params::NONE;
            let _ = params;
            $crate::__sketch_blocks!(s, params; $($body)*);
            Ok(())
        })
    };
}

/// [`try_sketch!`](crate::try_sketch) that panics on error. For tests and
/// examples.
#[macro_export]
macro_rules! sketch {
    ($($t:tt)*) => {
        $crate::try_sketch!($($t)*).expect("sketch! failed")
    };
}

/// The blocks of a [`try_sketch!`](crate::try_sketch) invocation, one per
/// arm. Not for direct use.
#[doc(hidden)]
#[macro_export]
macro_rules! __sketch_blocks {
    ($s:ident, $p:ident;) => {};
    ($s:ident, $p:ident; entities(layer: $layer:expr, provenance: $prov:expr) { $($b:tt)* }
        $($rest:tt)*) => {
        {
            let g = $crate::value::geometry::Geometry::build(|g| {
                let _ = &g;
                $crate::__geometry_body!(g; $($b)*);
                Ok(())
            })?;
            let layer = $crate::value::sketch::LayerId::from($layer);
            let prov = $prov;
            for (id, e) in g.iter() {
                $s.add_entity(
                    *id,
                    e.clone(),
                    $crate::value::sketch::EntityMeta::new(layer, prov.clone()),
                )?;
            }
        }
        $crate::__sketch_blocks!($s, $p; $($rest)*);
    };
    ($s:ident, $p:ident; params($table:expr); $($rest:tt)*) => {
        {
            let $p: &$crate::value::params::Params = $table;
            $crate::__sketch_blocks!($s, $p; $($rest)*);
        }
    };
    ($s:ident, $p:ident; constraints(provenance: $prov:expr) { $($id:ident : $rel:expr;)* }
        $($rest:tt)*) => {
        {
            let prov = $prov;
            $(
                $s.add_constraint(
                    $crate::value::provenance::ConstraintId::from(stringify!($id)),
                    $rel,
                    prov.clone(),
                    $p,
                )?;
            )*
        }
        $crate::__sketch_blocks!($s, $p; $($rest)*);
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry;
    use crate::sketch;
    use crate::solver::equation::{arc, circle, line, ArcEnd};
    use crate::value::geometry::{Model, P2};
    use crate::value::params::Param;
    use crate::value::provenance::{Agent, CheckKind, Citation, Date};
    use crate::value::relation::{
        angle, coincident, concentric, diameter, horizontal, horizontal_distance,
        horizontal_points, length, on_line, position, radius, tangent_arc_join, tangent_join,
        vertical, vertical_distance, LineEnd,
    };
    use std::f64::consts::FRAC_PI_2;

    const SHEET: &str = "LDW280-23631 sh3";

    fn day() -> Date {
        Date::new(2026, 10, 15).unwrap()
    }

    fn read() -> Provenance {
        Provenance::new(Method::Read, Agent::Claude, day()).cite(Citation::new(SHEET, "zone C4"))
    }

    fn traced() -> Provenance {
        Provenance::new(Method::Measured { sigma_mm: 0.3 }, Agent::Claude, day())
            .cite(Citation::new(SHEET, "scan"))
    }

    fn authored() -> Provenance {
        Provenance::new(Method::Authored, Agent::Claude, day())
    }

    fn visible() -> EntityMeta {
        EntityMeta::new("visible", traced())
    }

    fn mm(v: f64) -> Measure {
        Measure::mm(v).unwrap()
    }

    /// A clip's top, bend and leg, and a pin, traced.
    fn clip() -> Sketch<Model> {
        let g = geometry!(Model {
            top: line (0.0, 0.0) -> (10.0, 0.0);
            bend: arc (10.0, 5.0), r = 5.0, from = -FRAC_PI_2, sweep = FRAC_PI_2;
            leg: line (15.0, 5.0) -> (15.0, 20.0);
            pin: circle (5.0, 10.0), r = 1.0;
        });
        let mut s = Sketch::new();
        for (id, e) in g.iter() {
            s.add_entity(*id, e.clone(), visible()).unwrap();
        }
        s
    }

    #[test]
    fn convention_and_role_default_from_the_layer() {
        use crate::state::document::Document;
        use crate::value::style::Dash;
        let layers = Document::standard_layers();
        let on = |id: &str| &layers[&LayerId::new(id)];
        // No convention of its own: the layer's, drawn in the layer's style.
        let m = visible();
        assert_eq!(m.convention_on(on("visible")), Some(Convention::Visible));
        assert_eq!(m.role_on(on("visible")), Some(Role::Outline));
        let table = || layers.values();
        assert_eq!(m.style_on(on("visible"), table()), on("visible").style);
        let c = EntityMeta::new("centre", traced());
        assert_eq!(c.convention_on(on("centre")), Some(Convention::Centre));
        assert_eq!(c.role_on(on("centre")), Some(Role::Axis));
        // A layer that gives no role leaves it unsaid.
        let k = EntityMeta::new("construction", traced());
        assert_eq!(k.role_on(on("construction")), None);
        // Its own overrides the layer's: a hidden line among visible ones,
        // drawn hidden, still an outline.
        let h = visible().with_convention(Convention::Hidden);
        assert_eq!(h.convention_on(on("visible")), Some(Convention::Hidden));
        assert_eq!(h.role_on(on("visible")), Some(Role::Outline));
        // Drawn in the table's hidden style, the standard one without it.
        let mut own = layers.clone();
        own.get_mut(&LayerId::new("hidden")).unwrap().style.weight = 0.25;
        assert_eq!(h.style_on(on("visible"), own.values()).weight, 0.25);
        assert_eq!(h.style_on(on("visible"), own.values()).dash, Dash::hidden());
        assert_eq!(
            h.style_on(on("visible"), std::iter::empty()),
            Convention::Hidden.style()
        );
        assert_eq!(h.style_on(on("hidden"), table()), on("hidden").style);
        let x = k.with_role(Role::Extension);
        assert_eq!(x.role_on(on("construction")), Some(Role::Extension));
        // Each convention's style is its standard layer's.
        for (id, l) in &layers {
            if let Some(c) = l.convention {
                assert_eq!(c.style(), l.style, "{}", id.as_str());
            }
        }
        // In the file: stated only when the entity says; read back.
        assert!(!serde_json::to_string(&m).unwrap().contains("convention"));
        let s = serde_json::to_string(&h.clone().with_role(Role::Outline)).unwrap();
        assert!(s.contains(r#""convention":"hidden""#) && s.contains(r#""role":"outline""#));
        assert_eq!(
            serde_json::from_str::<EntityMeta>(&s).unwrap(),
            h.with_role(Role::Outline)
        );
    }

    #[test]
    fn entities_and_meta_move_together() {
        let mut s = clip();
        assert_eq!(s.geometry().len(), 4);
        assert_eq!(s.entities().count(), 4);
        assert_eq!(s.meta("top").unwrap().layer, "visible".into());
        // A bad entity adds no meta; a bad provenance adds no entity.
        let bad = Entity::Circle {
            centre: P2::new(0.0, 0.0),
            radius: 0.0,
        };
        assert!(s.add_entity("c".into(), bad, visible()).is_err());
        assert!(s.meta("c").is_none());
        let no_evidence =
            EntityMeta::new("visible", Provenance::new(Method::Read, Agent::User, day()));
        let pt = Entity::Point {
            at: P2::new(1.0, 1.0),
        };
        assert_eq!(
            s.add_entity("p".into(), pt.clone(), no_evidence),
            Err(Error::Provenance(provenance::Error::NoEvidence))
        );
        assert!(s.geometry().get("p").is_none());
        // set returns the old values; remove returns both parts.
        let old = s
            .set_entity(
                "pin",
                Entity::Circle {
                    centre: P2::new(6.0, 10.0),
                    radius: 1.0,
                },
            )
            .unwrap();
        assert!(matches!(old, Entity::Circle { centre, .. } if centre == P2::new(5.0, 10.0)));
        let (e, m) = s.remove_entity("pin").unwrap();
        assert!(matches!(e, Entity::Circle { .. }));
        assert_eq!(m.layer, "visible".into());
        assert_eq!(s.geometry().len(), 3);
        assert!(s.meta("pin").is_none());
        assert_eq!(s.set_layer("top", "hidden".into()), Ok("visible".into()));
        assert_eq!(s.meta("top").unwrap().layer, "hidden".into());
    }

    #[test]
    fn constraints_are_checked_against_the_sketch() {
        let mut s = clip();
        let (top, bend, leg) = (line("top"), arc("bend"), line("leg"));
        s.add_constraint("c1".into(), horizontal(top), read(), &Params::NONE)
            .unwrap();
        // Unknown operand, wrong kind, wrong unit.
        assert!(matches!(
            s.add_constraint("c2".into(), horizontal(line("nope")), read(), &Params::NONE),
            Err(Error::Relation(relation::Error::Geometry(_)))
        ));
        assert!(matches!(
            s.add_constraint("c2".into(), horizontal(line("bend")), read(), &Params::NONE),
            Err(Error::Relation(relation::Error::Geometry(_)))
        ));
        let deg = Measure::deg(1.0).unwrap();
        assert!(matches!(
            s.add_constraint("c2".into(), radius(bend, deg), read(), &Params::NONE),
            Err(Error::Relation(relation::Error::Measure(_)))
        ));
        // Duplicate id, missing parameter, bad provenance.
        assert_eq!(
            s.add_constraint("c1".into(), vertical(leg), read(), &Params::NONE),
            Err(Error::DuplicateConstraint("c1".into()))
        );
        let r = Measure::param("r_bend".into()).unwrap();
        assert_eq!(
            s.add_constraint("c2".into(), radius(bend, r), read(), &Params::NONE),
            Err(Error::Relation(relation::Error::UnknownParam(
                "r_bend".into()
            )))
        );
        assert!(matches!(
            s.add_constraint(
                "c2".into(),
                vertical(leg),
                Provenance::new(Method::Read, Agent::User, day()),
                &Params::NONE,
            ),
            Err(Error::Provenance(provenance::Error::NoEvidence))
        ));
        assert_eq!(s.constraints().count(), 1);
    }

    #[test]
    fn one_fact_is_stated_once() {
        let mut s = clip();
        let (top, bend, leg, pin) = (line("top"), arc("bend"), line("leg"), circle("pin"));
        s.add_constraint("h".into(), horizontal(top), read(), &Params::NONE)
            .unwrap();
        // The same fact, spelled differently, is refused and names the
        // constraint that states it.
        assert_eq!(
            s.add_constraint(
                "h2".into(),
                horizontal_points(top.end(), top.start()),
                read(),
                &Params::NONE,
            ),
            Err(Error::AlreadyStated { by: "h".into() })
        );
        // A compound whose lowering overlaps an existing fact is refused.
        s.add_constraint(
            "j".into(),
            tangent_join(top, LineEnd::End, bend, ArcEnd::Start),
            read(),
            &Params::NONE,
        )
        .unwrap();
        assert_eq!(
            s.add_constraint(
                "c".into(),
                coincident(bend.start(), top.end()),
                read(),
                &Params::NONE
            ),
            Err(Error::AlreadyStated { by: "j".into() })
        );
        // A different value on the same operands is a different equation
        // (a conflict for the solver, not a duplicate for the store).
        s.add_constraint("r1".into(), radius(bend, mm(4.0)), read(), &Params::NONE)
            .unwrap();
        s.add_constraint(
            "r2".into(),
            radius(bend, mm(5.0)),
            authored(),
            &Params::NONE,
        )
        .unwrap();
        // The same value through a parameter is the same fact.
        let mut table = Params::new();
        table
            .set("rb".into(), Param::new(mm(4.0), authored()).unwrap())
            .unwrap();
        let rb = Measure::param("rb".into()).unwrap();
        assert_eq!(
            s.add_constraint("r3".into(), radius(bend, rb), read(), &table),
            Err(Error::AlreadyStated { by: "r1".into() })
        );
        // Driven dimensions and inactive constraints do not count.
        s.add_constraint(
            "l".into(),
            length(leg, mm(15.0)).driven().unwrap(),
            read(),
            &Params::NONE,
        )
        .unwrap();
        s.add_constraint("l2".into(), length(leg, mm(15.0)), read(), &Params::NONE)
            .unwrap();
        s.supersede(
            ItemRef::Constraint("r1".into()),
            ItemRef::Constraint("r2".into()),
        )
        .unwrap();
        s.add_constraint("r4".into(), radius(bend, mm(4.0)), read(), &Params::NONE)
            .unwrap();
        // Changing a value into a duplicate is refused too.
        assert_eq!(
            s.set_values("r4", vec![mm(5.0)], &Params::NONE),
            Err(Error::AlreadyStated { by: "r2".into() })
        );
        assert_eq!(
            s.set_values("r4", vec![mm(4.5)], &Params::NONE),
            Ok(vec![mm(4.0)])
        );
        let _ = pin;
    }

    #[test]
    fn things_in_use_cannot_be_removed() {
        let mut s = clip();
        let (top, bend) = (line("top"), arc("bend"));
        s.add_constraint("h".into(), horizontal(top), read(), &Params::NONE)
            .unwrap();
        assert_eq!(
            s.remove_entity("top"),
            Err(Error::EntityInUse {
                id: "top".into(),
                by: vec![ItemRef::Constraint("h".into())]
            })
        );
        // A constraint cited by another's provenance.
        let derived = Provenance::new(
            Method::Derived {
                from: vec![ItemRef::Constraint("h".into())],
            },
            Agent::Claude,
            day(),
        )
        .cite(Citation::new(SHEET, "zone C4"));
        s.add_constraint("v".into(), vertical(line("leg")), derived, &Params::NONE)
            .unwrap();
        assert_eq!(
            s.remove_constraint("h"),
            Err(Error::ConstraintInUse {
                id: "h".into(),
                by: vec![ItemRef::Constraint("v".into())]
            })
        );
        // Remove the user, then the used.
        s.remove_constraint("v").unwrap();
        s.remove_constraint("h").unwrap();
        s.remove_entity("top").unwrap();
        let _ = bend;
        assert_eq!(
            s.remove_constraint("h"),
            Err(Error::NoSuchConstraint("h".into()))
        );
    }

    #[test]
    fn entities_citing_entities() {
        // A hidden line derived from a visible one: the file may list the
        // cited entity after the citing one, and the cited one cannot be
        // removed while cited.
        let mut s = clip();
        let derived = EntityMeta::new(
            "hidden",
            Provenance::new(
                Method::Derived {
                    from: vec![ItemRef::Entity("top".into())],
                },
                Agent::Claude,
                day(),
            )
            .cite(Citation::new(SHEET, "zone C4")),
        );
        // "a_shadow" sorts before "top".
        s.add_entity(
            "a_shadow".into(),
            Entity::Line {
                start: P2::new(0.0, 1.0),
                end: P2::new(10.0, 1.0),
            },
            derived,
        )
        .unwrap();
        assert_eq!(
            s.remove_entity("top"),
            Err(Error::EntityInUse {
                id: "top".into(),
                by: vec![ItemRef::Entity("a_shadow".into())]
            })
        );
        let json = serde_json::to_string(&s).unwrap();
        let back: Sketch<Model> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        // A citation of a missing entity is refused on load.
        let bad = json.replace(
            r#"{"kind":"entity","id":"top"}"#,
            r#"{"kind":"entity","id":"nope"}"#,
        );
        assert_ne!(bad, json);
        assert!(serde_json::from_str::<Sketch<Model>>(&bad).is_err());
    }

    #[test]
    fn an_entity_provenance_is_replaced_with_its_references_checked() {
        let mut s = clip();
        let derived = |from: Vec<ItemRef>| {
            Provenance::new(Method::Derived { from }, Agent::Claude, day())
                .cite(Citation::new(SHEET, "zone C4"))
        };
        let (top, bottom) = (ItemRef::Entity("top".into()), ItemRef::Entity("leg".into()));
        // A missing entity, a missing reference, a self-reference.
        assert_eq!(
            s.set_entity_provenance("nope", traced()),
            Err(Error::Geometry(geometry::Error::NoSuch("nope".into())))
        );
        let nope = ItemRef::Constraint("nope".into());
        assert_eq!(
            s.set_entity_provenance("top", derived(vec![nope])),
            Err(Error::NoSuchItem(nope))
        );
        assert_eq!(
            s.set_entity_provenance("top", derived(vec![top])),
            Err(Error::NoSuchItem(top))
        );
        // A cycle through another claim: bottom derived from top, then top
        // from bottom.
        s.set_entity_provenance("leg", derived(vec![top])).unwrap();
        assert_eq!(
            s.set_entity_provenance("top", derived(vec![bottom])),
            Err(Error::ProvenanceCycle(top))
        );
        // Where the claim stood is kept: accepted stays accepted.
        s.accept(top).unwrap();
        s.set_entity_provenance("top", traced()).unwrap();
        assert_eq!(
            s.meta("top").unwrap().provenance.status(),
            &Status::Accepted
        );
    }

    #[test]
    fn provenance_references_must_exist() {
        let mut s = clip();
        let top = line("top");
        let derived = |from: Vec<ItemRef>| {
            Provenance::new(Method::Derived { from }, Agent::Claude, day())
                .cite(Citation::new(SHEET, "zone C4"))
        };
        assert_eq!(
            s.add_constraint(
                "h".into(),
                horizontal(top),
                derived(vec![ItemRef::Constraint("nope".into())]),
                &Params::NONE,
            ),
            Err(Error::NoSuchItem(ItemRef::Constraint("nope".into())))
        );
        // A claim cannot derive from itself.
        assert_eq!(
            s.add_constraint(
                "h".into(),
                horizontal(top),
                derived(vec![ItemRef::Constraint("h".into())]),
                &Params::NONE,
            ),
            Err(Error::NoSuchItem(ItemRef::Constraint("h".into())))
        );
        s.add_constraint(
            "h".into(),
            horizontal(top),
            derived(vec![ItemRef::Entity("top".into())]),
            &Params::NONE,
        )
        .unwrap();
        // Transitions through the sketch validate their references.
        let h = ItemRef::Constraint("h".into());
        assert_eq!(
            s.dispute(h, vec![ItemRef::Constraint("nope".into())]),
            Err(Error::NoSuchItem(ItemRef::Constraint("nope".into())))
        );
        assert_eq!(s.supersede(h, h), Err(Error::NoSuchItem(h)));
        s.accept(h).unwrap();
        s.verify(
            h,
            Check {
                kind: CheckKind::Overlay,
                cite: Citation::new(SHEET, "overlay"),
            },
        )
        .unwrap();
        assert!(matches!(
            s.constraint("h").unwrap().provenance().status(),
            Status::Verified { .. }
        ));
        assert_eq!(
            s.accept(h),
            Err(Error::Provenance(provenance::Error::BadTransition {
                from: s.constraint("h").unwrap().provenance().status().clone(),
                attempted: "accept"
            }))
        );
        // Entities have provenance too.
        s.accept(ItemRef::Entity("top".into())).unwrap();
        assert_eq!(
            s.accept(ItemRef::Entity("nope".into())),
            Err(Error::NoSuchItem(ItemRef::Entity("nope".into())))
        );
    }

    #[test]
    fn a_sketch_is_closed_by_the_table_it_is_given() {
        let mut s = clip();
        let bend = arc("bend");
        let rb = Measure::param("rb".into()).unwrap();
        // Without the table, a constraint naming a parameter is refused.
        assert_eq!(
            s.add_constraint("r".into(), radius(bend, rb.clone()), read(), &Params::NONE),
            Err(Error::Relation(relation::Error::UnknownParam("rb".into())))
        );
        let table = |v: Measure| {
            let mut t = Params::new();
            t.set("rb".into(), Param::new(v, authored()).unwrap())
                .unwrap();
            t
        };
        let four = table(mm(4.0));
        s.add_constraint("r".into(), radius(bend, rb), read(), &four)
            .unwrap();
        assert_eq!(s.users_of_param("rb"), vec!["r".into()]);
        // The document's check: its table, or a replacement, must fit
        // every constraint that names a parameter.
        assert_eq!(s.check_params(&four), Ok(()));
        assert!(matches!(
            s.check_params(&Params::NONE),
            Err(Error::Relation(relation::Error::UnknownParam(_)))
        ));
        assert!(matches!(
            s.check_params(&table(Measure::deg(45.0).unwrap())),
            Err(Error::Relation(_))
        ));
        // Lowering takes the table too.
        assert!(s.lower(&Params::NONE).is_err());
        assert_eq!(s.lower(&four).unwrap().equations.len(), 1);
        // A table whose value makes the constraint restate a literal fact
        // is refused, as the literal would be.
        s.add_constraint("r5".into(), radius(bend, mm(5.0)), authored(), &four)
            .unwrap();
        assert_eq!(
            s.check_params(&table(mm(5.0))),
            Err(Error::AlreadyStated { by: "r5".into() })
        );
    }

    #[test]
    fn lowering_takes_the_active_driving_constraints() {
        let mut s = clip();
        let (top, bend, leg) = (line("top"), arc("bend"), line("leg"));
        s.add_constraint("h".into(), horizontal(top), read(), &Params::NONE)
            .unwrap();
        s.add_constraint(
            "j".into(),
            tangent_join(top, LineEnd::End, bend, ArcEnd::Start),
            read(),
            &Params::NONE,
        )
        .unwrap();
        s.add_constraint(
            "l".into(),
            length(leg, mm(15.0)).driven().unwrap(),
            read(),
            &Params::NONE,
        )
        .unwrap();
        s.add_constraint("r".into(), radius(bend, mm(5.0)), read(), &Params::NONE)
            .unwrap();
        s.add_constraint("r_old".into(), radius(bend, mm(6.0)), read(), &Params::NONE)
            .unwrap();
        s.supersede(
            ItemRef::Constraint("r_old".into()),
            ItemRef::Constraint("r".into()),
        )
        .unwrap();
        let low = s.lower(&Params::NONE).unwrap();
        // h: 1, j: 2, r: 1; the driven length and the superseded radius
        // are out.
        assert_eq!(low.equations.len(), 4);
        assert_eq!(
            low.owners,
            vec!["h".into(), "j".into(), "j".into(), "r".into()]
        );
        assert!(!s.constraint("l").unwrap().active());
        assert!(!s.constraint("r_old").unwrap().active());
        assert!(s.constraint("r").unwrap().active());
    }

    #[test]
    fn file_form_is_rebuilt_through_the_interface() {
        let mut s = clip();
        let (top, bend) = (line("top"), arc("bend"));
        let mut table = Params::new();
        table
            .set("rb".into(), Param::new(mm(4.0), authored()).unwrap())
            .unwrap();
        s.add_constraint("h".into(), horizontal(top), read(), &Params::NONE)
            .unwrap();
        s.add_constraint(
            "r".into(),
            radius(bend, Measure::param("rb".into()).unwrap()),
            read(),
            &table,
        )
        .unwrap();
        // A constraint deriving from one that sorts after it: order in the
        // file must not matter.
        let derived = Provenance::new(
            Method::Derived {
                from: vec![ItemRef::Constraint("r".into())],
            },
            Agent::Claude,
            day(),
        )
        .cite(Citation::new(SHEET, "zone C4"));
        // Once a sketch names a parameter, every check needs the table:
        // whether a new fact is already stated depends on the values.
        assert!(matches!(
            s.add_constraint(
                "a".into(),
                vertical(line("leg")),
                derived.clone(),
                &Params::NONE
            ),
            Err(Error::Relation(relation::Error::UnknownParam(_)))
        ));
        s.add_constraint("a".into(), vertical(line("leg")), derived, &table)
            .unwrap();
        let json = serde_json::to_string_pretty(&s).unwrap();
        assert!(json.contains("\"frame\": \"model\""));
        let back: Sketch<Model> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        // The file is an open term: it loads without the table, and the
        // document closes it.
        assert_eq!(back.check_params(&table), Ok(()));
        assert!(back.check_params(&Params::NONE).is_err());
        // Hand-edited files are held to the invariants.
        let missing_meta =
            json.replace("\"top\": {\n      \"layer\"", "\"tip\": {\n      \"layer\"");
        assert_ne!(missing_meta, json);
        assert!(serde_json::from_str::<Sketch<Model>>(&missing_meta)
            .unwrap_err()
            .to_string()
            .contains("MetaMismatch"));
        let dup = json
            .replace("\"rel\": \"vertical\"", "\"rel\": \"horizontal\"")
            .replace("\"leg\"", "\"top\"");
        assert!(serde_json::from_str::<Sketch<Model>>(&dup)
            .unwrap_err()
            .to_string()
            .contains("AlreadyStated"));
    }
    #[test]
    fn macro_builds_through_the_interface() {
        let mut table = Params::new();
        table
            .set("rb".into(), Param::new(mm(4.0), authored()).unwrap())
            .unwrap();
        let s = sketch!(Model {
            entities(layer: "visible", provenance: traced()) {
                top: line (0.0, 0.0) -> (10.0, 0.0);
                bend: arc (10.0, 5.0), r = 5.0, from = -FRAC_PI_2, sweep = FRAC_PI_2;
            }
            entities(layer: "construction", provenance: authored()) {
                axis: line (0.0, -5.0) -> (0.0, 25.0);
            }
            constraints(provenance: read()) {
                h: horizontal(line("top"));
                j: tangent_join(line("top"), LineEnd::End, arc("bend"), ArcEnd::Start);
            }
            params(&table);
            constraints(provenance: authored()) {
                r: radius(arc("bend"), Measure::param("rb".into()).unwrap());
            }
        });
        assert_eq!(s.geometry().len(), 3);
        assert_eq!(s.meta("axis").unwrap().layer, "construction".into());
        assert_eq!(s.constraints().count(), 3);
        assert_eq!(s.users_of_param("rb"), vec!["r".into()]);
        assert!(matches!(
            s.constraint("r").unwrap().provenance().method(),
            Method::Authored
        ));
        // Errors come out as the store's errors.
        let r = crate::try_sketch!(Model {
            entities(layer: "visible", provenance: traced()) {
                top: line (0.0, 0.0) -> (10.0, 0.0);
            }
            constraints(provenance: read()) {
                h: horizontal(line("top"));
                h2: horizontal_points(line("top").start(), line("top").end());
            }
        });
        assert_eq!(r.err(), Some(Error::AlreadyStated { by: "h".into() }));
    }

    /// A clip's top, bend and leg from a rough tracing, with the relations
    /// the drawing implies and its dimensions.
    fn rough_clip() -> Sketch<Model> {
        sketch!(Model {
            entities(layer: "visible", provenance: traced()) {
                top: line (0.1, 0.2) -> (9.0, -0.3);
                bend: arc (9.5, 5.0), r = 5.0, from = -1.6, sweep = 1.7;
                leg: line (15.0, 5.5) -> (14.0, 20.0);
            }
            constraints(provenance: read()) {
                h: horizontal(line("top"));
                v: vertical(line("leg"));
                j1: tangent_join(line("top"), LineEnd::End, arc("bend"), ArcEnd::Start);
                j2: tangent_join(line("leg"), LineEnd::Start, arc("bend"), ArcEnd::End);
                w: length(line("top"), mm(10.0));
                r: radius(arc("bend"), mm(4.0));
                l: length(line("leg"), mm(15.0));
            }
        })
    }

    #[test]
    fn solve_applies_and_reports_in_sketch_terms() {
        let mut s = rough_clip();
        let a = s.solve(Options::default(), &Params::NONE).unwrap();
        assert!(a.report.residual_max < 1e-9, "{:?}", a.report);
        // Rigid, but nowhere in particular: constrained up to placement.
        assert_eq!(
            a.placement,
            Placement {
                x: true,
                y: true,
                rotation: false
            }
        );
        assert_eq!(a.internal_free, 0);
        assert!(a
            .status
            .values()
            .all(|st| *st == EntityStatus::FixedUpToPlacement));
        assert!(a.groups.is_empty());
        // The geometry moved onto the dimensions.
        match s.geometry().get("bend").unwrap() {
            Entity::Arc { radius, .. } => assert!((radius - 4.0).abs() < 1e-9),
            _ => unreachable!(),
        }
        // A fiducial removes the placement; now everything is fixed.
        s.add_constraint(
            "o".into(),
            position(line("top").start(), mm(0.0), mm(0.0)),
            read(),
            &Params::NONE,
        )
        .unwrap();
        let a = s.solve(Options::default(), &Params::NONE).unwrap();
        assert_eq!(a.placement.count(), 0);
        assert!(a.status.values().all(|st| *st == EntityStatus::Fixed));
        // Analysis without solving agrees with the solved state.
        let b = s.analyse(Options::default(), &Params::NONE).unwrap();
        assert_eq!(b.status, a.status);
        assert!(b.report.residual_max < 1e-9);
    }

    #[test]
    fn solve_near_stays_near_the_ink_where_it_is_weighed() {
        use nalgebra::DMatrix;
        let ink = rough_clip();
        let prior = |top: f64, leg: f64| Prior {
            geometry: ink.geometry().clone(),
            weights: BTreeMap::from([
                ("top".into(), DMatrix::identity(4, 4) * top),
                ("leg".into(), DMatrix::identity(4, 4) * leg),
                ("bend".into(), DMatrix::identity(5, 5)),
            ]),
        };
        // How far the top line's start moved from the ink.
        let moved = |s: &Sketch<Model>| match (ink.geometry().get("top"), s.geometry().get("top")) {
            (Some(Entity::Line { start: a, .. }), Some(Entity::Line { start: b, .. })) => {
                (a.x - b.x).hypot(a.y - b.y)
            }
            _ => unreachable!(),
        };
        let (mut firm, mut loose) = (rough_clip(), rough_clip());
        let a = firm
            .solve_near(&prior(1e3, 1.0), Options::default(), &Params::NONE)
            .unwrap();
        let b = loose
            .solve_near(&prior(1.0, 1e3), Options::default(), &Params::NONE)
            .unwrap();
        for n in [&a, &b] {
            assert!(n.converged, "{:?}", n.analysis.report);
            assert!(n.analysis.report.residual_max < 1e-9);
            assert!(n.cost > 0.0);
        }
        assert!(moved(&firm) < moved(&loose));
        // A prior of other entities is refused, the sketch untouched.
        let mut s = rough_clip();
        let before = s.clone();
        let other = Prior {
            geometry: crate::geometry!(Model { top: point (0.0, 0.0); }),
            weights: BTreeMap::new(),
        };
        // One with an entity too many, and one with an entity of another
        // kind: all refused, the sketch untouched.
        let mut extra = ink.geometry().clone();
        extra
            .add(
                "zz".into(),
                Entity::Point {
                    at: crate::value::geometry::P2::new(0.0, 0.0),
                },
            )
            .unwrap();
        let reshaped = crate::geometry!(Model {
            top: line (0.1, 0.2) -> (9.0, -0.3);
            bend: arc (9.5, 5.0), r = 5.0, from = -1.6, sweep = 1.7;
            leg: point (15.0, 5.5);
        });
        for geometry in [other.geometry, extra, reshaped] {
            let prior = Prior {
                geometry,
                weights: BTreeMap::new(),
            };
            let r = s.solve_near(&prior, Options::default(), &Params::NONE);
            assert!(matches!(r, Err(Error::Geometry(_))));
            assert_eq!(s, before);
        }
    }

    #[test]
    fn partial_entities_are_named() {
        let mut s = rough_clip();
        s.remove_constraint("l").unwrap();
        s.add_constraint(
            "o".into(),
            position(line("top").start(), mm(0.0), mm(0.0)),
            read(),
            &Params::NONE,
        )
        .unwrap();
        let a = s.solve(Options::default(), &Params::NONE).unwrap();
        assert_eq!(a.internal_free, 1);
        assert_eq!(a.entity("top"), Some(&EntityStatus::Fixed));
        assert_eq!(a.entity("bend"), Some(&EntityStatus::Fixed));
        // Only the leg's end can move: along the leg.
        match a.entity("leg").unwrap() {
            EntityStatus::Partial(vals) => {
                assert!(vals.iter().all(|v| matches!(v, Value::EndX | Value::EndY)))
            }
            st => panic!("{st:?}"),
        }
    }

    #[test]
    fn conflicts_name_the_culprit() {
        let mut s = sketch!(Model {
            entities(layer: "visible", provenance: traced()) {
                l: line (0.0, 0.0) -> (10.0, 0.0);
            }
            constraints(provenance: read()) {
                a: position(line("l").start(), mm(0.0), mm(0.0));
                b: position(line("l").end(), mm(10.0), mm(0.0));
                good: length(line("l"), mm(10.0));
            }
        });
        // A consistent extra statement is a redundancy, no culprit.
        let a = s.solve(Options::default(), &Params::NONE).unwrap();
        assert_eq!(a.groups.len(), 1);
        assert!(a.groups[0].residual < CONFLICT);
        assert!(a.groups[0].culprits.is_empty());
        assert_eq!(
            a.groups[0].constraints,
            vec!["a".into(), "b".into(), "good".into()]
        );
        // A wrong one is a conflict, and the only member whose removal
        // alone resolves it.
        s.remove_constraint("good").unwrap();
        s.add_constraint(
            "bad".into(),
            length(line("l"), mm(12.0)),
            authored(),
            &Params::NONE,
        )
        .unwrap();
        s.add_constraint(
            "good".into(),
            length(line("l"), mm(10.0)),
            read(),
            &Params::NONE,
        )
        .unwrap();
        let a = s.solve(Options::default(), &Params::NONE).unwrap();
        let conflict = a.groups.iter().find(|g| g.residual > CONFLICT).unwrap();
        assert!(conflict.constraints.contains(&"bad".into()));
        assert_eq!(conflict.culprits, vec!["bad".into()]);
    }

    #[test]
    fn reference_dimensions_are_checked_not_solved() {
        let mut s = rough_clip();
        s.add_constraint(
            "ref".into(),
            length(line("leg"), mm(14.0)).driven().unwrap(),
            read(),
            &Params::NONE,
        )
        .unwrap();
        let a = s.solve(Options::default(), &Params::NONE).unwrap();
        // The leg is 15 by its driving length; the sheet's REF says 14.
        assert_eq!(a.reference.len(), 1);
        assert_eq!(a.reference[0].constraint, "ref".into());
        assert!((a.reference[0].off_by[0] - 1.0).abs() < 1e-9);
        // The REF did not take part in the solve.
        assert!(a.report.residual_max < 1e-9);
        // A what-if: with the top's start as a fiducial, pushing the leg's
        // end elsewhere cannot hold. Nothing is applied.
        s.add_constraint(
            "o".into(),
            position(line("top").start(), mm(0.0), mm(0.0)),
            read(),
            &Params::NONE,
        )
        .unwrap();
        s.solve(Options::default(), &Params::NONE).unwrap();
        let what_if = s
            .solve_with(
                &[eq::pinned(line("leg").end(), 15.0, 25.0)],
                Options::default(),
                &Params::NONE,
            )
            .unwrap();
        assert!(what_if.report.residual_max > 0.1, "{:?}", what_if.report);
        let b = s.analyse(Options::default(), &Params::NONE).unwrap();
        assert!(b.report.residual_max < 1e-9);
    }
    /// Section A-A of LDW280-23631 sheet 3: the upper clip with its lug, the
    /// reamed pin hole, the +Y27.00 plane and the rivet line. Model mm:
    /// model Y along x, model X along y (up the page).
    fn aa_upper() -> Sketch<Model> {
        let inch = |t: &str| Measure::inches(t).unwrap();
        let (top_o, bend_o, leg_o, leg_end, leg_i, bend_i, top_i, flange_end) = (
            line("top_o"),
            arc("bend_o"),
            line("leg_o"),
            line("leg_end"),
            line("leg_i"),
            arc("bend_i"),
            line("top_i"),
            line("flange_end"),
        );
        let (lug_l, f1, lug_s, lug_arc, f2, lug_r) = (
            line("lug_l"),
            arc("f1"),
            line("lug_s"),
            arc("lug_arc"),
            arc("f2"),
            line("lug_r"),
        );
        let (pin, y27, rivet) = (circle("pin"), line("y27"), line("rivet_line"));
        sketch!(Model {
            entities(layer: "visible", provenance: traced()) {
                top_o: line (710.005, 4666.488) -> (692.123, 4666.488);
                bend_o: arc (692.123, 4662.613), r = 3.886, from = 1.5712, sweep = 1.5703;
                leg_o: line (688.237, 4662.613) -> (688.237, 4653.496);
                leg_end: line (688.237, 4653.496) -> (689.509, 4653.496);
                leg_i: line (689.509, 4653.496) -> (689.510, 4662.613);
                bend_i: arc (692.123, 4662.612), r = 2.613, from = 1.5704, sweep = 1.5711;
                top_i: line (692.123, 4665.218) -> (710.005, 4665.218);
                flange_end: line (710.005, 4665.218) -> (710.005, 4666.488);
                lug_l: line (694.055, 4665.218) -> (694.055, 4656.942);
                f1: arc (702.176, 4656.942), r = 8.121, from = 3.1409, sweep = 0.4665;
                lug_s: line (694.926, 4653.291) -> (699.508, 4644.239);
                lug_arc: arc (705.340, 4647.176), r = 6.533, from = 0.8792, sweep = -3.5544;
                f2: arc (714.168, 4657.839), r = 7.333, from = 3.1421, sweep = 0.8792;
                lug_r: line (706.836, 4657.839) -> (706.836, 4665.218);
                pin: circle (704.850, 4583.938), r = 3.175;
            }
            entities(layer: "phantom", provenance: traced()) {
                y27: line (685.799, 4680.467) -> (685.798, 4447.281);
                rivet_line: line (700.077, 4674.021) -> (700.092, 4507.316);
            }
            // What the sheet says.
            constraints(provenance: read()) {
                station: position(pin.centre(), inch("27.750"), inch("180.470"));
                ream: diameter(pin, inch(".2500"));
                d3200: vertical_distance(pin.centre(), top_i.end(), inch("3.200"));
                d188: horizontal_distance(rivet.start(), pin.centre(), inch(".188"));
                d750: horizontal_distance(y27.start(), pin.centre(), inch(".750")).driven().unwrap();
                x183: vertical_distance(pin.centre(), top_o.start(), inch("3.250")).driven().unwrap();
            }
            // The +Y27.00 plane is .750 from the pin at Y27.750: a station
            // difference, not a printed dimension (the .750 REF checks it).
            constraints(provenance: derived_from_station()) {
                y27_at: horizontal_distance(y27.start(), pin.centre(), inch(".750"));
            }
            // What the drawing shows: the clip's shape.
            constraints(provenance: estimated()) {
                h1: horizontal(top_o);
                h2: horizontal(top_i);
                h3: horizontal(leg_end);
                v1: vertical(leg_o);
                v2: vertical(leg_i);
                v3: vertical(flange_end);
                v4: vertical(lug_l);
                v5: vertical(lug_r);
                v6: vertical(y27);
                v7: vertical(rivet);
                j1: tangent_join(top_o, LineEnd::End, bend_o, ArcEnd::Start);
                j2: tangent_join(leg_o, LineEnd::Start, bend_o, ArcEnd::End);
                c1: coincident(leg_o.end(), leg_end.start());
                c2: coincident(leg_end.end(), leg_i.start());
                j3: tangent_join(leg_i, LineEnd::End, bend_i, ArcEnd::End);
                j4: tangent_join(top_i, LineEnd::Start, bend_i, ArcEnd::Start);
                c3: coincident(top_i.end(), flange_end.start());
                c4: coincident(flange_end.end(), top_o.start());
                cc: concentric(bend_o, bend_i);
                o1: on_line(lug_l.start(), top_i);
                o2: on_line(lug_r.end(), top_i);
                j5: tangent_join(lug_l, LineEnd::End, f1, ArcEnd::Start);
                j6: tangent_join(lug_s, LineEnd::Start, f1, ArcEnd::End);
                j7: tangent_join(lug_s, LineEnd::End, lug_arc, ArcEnd::End);
                j8: tangent_arc_join(lug_arc, ArcEnd::Start, f2, ArcEnd::End);
                j9: tangent_join(lug_r, LineEnd::Start, f2, ArcEnd::Start);
            }
            // Measured off the ink: sizes the sheet does not give.
            constraints(provenance: measured()) {
                t1: length(flange_end, inch(".050"));
                rb: radius(bend_o, mm(3.886));
                fl: length(top_o, mm(17.882));
                ll: length(leg_o, mm(9.117));
                lat: horizontal_distance(y27.start(), leg_o.start(), mm(2.438));
                slope: angle(lug_s, top_i, Measure::deg(63.16).unwrap());
                lx1: horizontal_distance(top_i.start(), lug_l.start(), mm(1.932));
                lx2: horizontal_distance(top_i.start(), lug_r.end(), mm(14.713));
                ly1: length(lug_l, mm(8.276));
                ly2: length(lug_r, mm(7.379));
                rf1: radius(f1, mm(8.121));
                rf2: radius(f2, mm(7.333));
                rh: radius(lug_arc, mm(6.533));
                ye: vertical_distance(pin.centre(), y27.start(), mm(96.529));
                yl: length(y27, mm(233.186));
                re: vertical_distance(pin.centre(), rivet.start(), mm(90.083));
                rl: length(rivet, mm(166.705));
            }
        })
    }

    fn derived_from_station() -> Provenance {
        Provenance::new(
            Method::Derived {
                from: vec![ItemRef::Constraint("station".into())],
            },
            Agent::Claude,
            day(),
        )
        .cite(Citation::new(SHEET, "SECT A-A, station labels"))
    }

    fn measured() -> Provenance {
        Provenance::new(Method::Measured { sigma_mm: 0.15 }, Agent::Claude, day())
            .cite(Citation::new(SHEET, "SECT A-A, ink"))
    }

    fn estimated() -> Provenance {
        Provenance::new(Method::Estimated, Agent::Claude, day())
            .cite(Citation::new(SHEET, "SECT A-A"))
    }

    #[test]
    fn aa_upper_clip_is_fully_constrained() {
        let mut s = aa_upper();
        let a = s.solve(Options::default(), &Params::NONE).unwrap();
        let bad: Vec<_> = a
            .status
            .iter()
            .filter(|(_, st)| **st != EntityStatus::Fixed)
            .collect();
        assert!(
            a.internal_free == 0 && a.placement.count() == 0 && a.groups.is_empty(),
            "free {} placement {:?}\nstatus {:?}\ngroups {:?}\nfreedoms {:?}\nreport {:?}",
            a.internal_free,
            a.placement,
            bad,
            a.groups,
            a.report.freedoms,
            (
                a.report.rank,
                a.report.parameters,
                a.report.equations,
                a.report.termination.clone(),
                a.report.residual_max
            )
        );
        assert!(a.report.residual_max < 1e-6, "{:?}", a.report);
        for r in &a.reference {
            assert!(r.off_by.iter().all(|v| v.abs() < 0.2), "{r:?}");
        }
    }
}
