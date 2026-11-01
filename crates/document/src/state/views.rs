//! Views of one model, linked along the model axes they share, solved as
//! one system.
//!
//! A drawing's views are projections of the same part, so a feature's
//! model X in the top view is its model X in the front view. A `Link`
//! states that: `aligned(top, p, front, q, X)` says the two points have
//! the same model X, which is one equation, `same`, between one
//! coordinate of each. Solving is one system: the views' geometries are
//! merged under generated ids (a table maps each back to its view and
//! entity, so names are never parsed and nothing is forbidden in them),
//! every sketch's constraints and every link are lowered into it, the
//! solver runs once with each view as a body (so a view sliding on its
//! own is placement of that view), and the values go back to the
//! sketches. The merged geometry is never stored.
//!
//! The store keeps its invariants on every path in: a link's views exist,
//! both show the named axis, both points exist in their sketches, and a
//! view in use by a link is not removed. A view's sketch is edited through
//! [`Views::edit`], which re-checks the links that touch it and rolls the
//! edit back if one would dangle.
//!
//! # Example
//!
//! A block 40 × 20 × 10 with a Ø6 hole through it, in three views. The top
//! view (model X, Y) carries the width, the depth, the hole and a station;
//! the front view (X, Z) carries the height; the side view (Y, Z) carries
//! nothing but its shape. Every other size reaches a view through a link:
//! the front's width from the top along X, the side's depth from the top
//! along Y and its height from the front along Z, and the hidden lines of
//! the hole from four construction points on the circle in the top view.
//! The top view is fixed by its station; front and side have no station in
//! Z, so the pair can slide in Z together: placement of those two bodies,
//! reported as such, with nothing else free.
//!
//! ```
//! use lmsheets_document::value::relation::{circle, line, point};
//! use lmsheets_document::state::analysis::Options;
//! use lmsheets_document::value::geometry::{Model, P2};
//! use lmsheets_document::value::measure::{Measure, Unit};
//! use lmsheets_document::value::params::Params;
//! use lmsheets_document::value::provenance::{Agent, Date, Method, Provenance};
//! use lmsheets_document::value::relation::*;
//! use lmsheets_document::value::sketch::EntityStatus;
//! use lmsheets_document::value::view::{ModelAxis, Transform, View};
//! use lmsheets_document::state::views::{aligned, in_view, Views};
//! use lmsheets_document::sketch;
//!
//! let day = Date::new(2026, 10, 15)?;
//! let ours = || Provenance::new(Method::Authored, Agent::Claude, day);
//! let mm = |v: f64| Measure::mm(v).unwrap();
//! let rect = |b: &str, r: &str, t: &str, l: &str| {
//!     let (b, r, t, l) = (line(b), line(r), line(t), line(l));
//!     vec![
//!         coincident(b.end(), r.start()),
//!         coincident(r.end(), t.start()),
//!         coincident(t.end(), l.start()),
//!         coincident(l.end(), b.start()),
//!         horizontal(b),
//!         vertical(r),
//!         horizontal(t),
//!         vertical(l),
//!     ]
//! };
//!
//! // Top view, (X, Y): the rectangle, the hole, and four points on the
//! // hole where the hidden lines of the other views come from.
//! let mut top = sketch!(Model {
//!     entities(layer: "visible", provenance: ours()) {
//!         tb: line (0.0, 0.0) -> (40.0, 0.0);
//!         tr: line (40.0, 0.0) -> (40.0, 20.0);
//!         tt: line (40.0, 20.0) -> (0.0, 20.0);
//!         tl: line (0.0, 20.0) -> (0.0, 0.0);
//!         hole: circle (10.0, 10.0), r = 3.0;
//!     }
//!     entities(layer: "construction", provenance: ours()) {
//!         hw: point (7.0, 10.0);
//!         he: point (13.0, 10.0);
//!         hs: point (10.0, 7.0);
//!         hn: point (10.0, 13.0);
//!     }
//!     constraints(provenance: ours()) {
//!         origin: position(line("tb").start(), mm(0.0), mm(0.0));
//!         width: horizontal_distance(
//!             line("tb").start(), line("tb").end(), mm(40.0),
//!         );
//!         depth: vertical_distance(
//!             line("tr").start(), line("tr").end(), mm(20.0),
//!         );
//!         hx: horizontal_distance(
//!             line("tb").start(), circle("hole").centre(), mm(10.0),
//!         );
//!         hy: vertical_distance(
//!             line("tb").start(), circle("hole").centre(), mm(10.0),
//!         );
//!         hd: diameter(circle("hole"), mm(6.0));
//!         w1: on_circle(point("hw"), circle("hole"));
//!         w2: horizontal_points(point("hw"), circle("hole").centre());
//!         e1: on_circle(point("he"), circle("hole"));
//!         e2: horizontal_points(point("he"), circle("hole").centre());
//!         s1: on_circle(point("hs"), circle("hole"));
//!         s2: vertical_points(point("hs"), circle("hole").centre());
//!         n1: on_circle(point("hn"), circle("hole"));
//!         n2: vertical_points(point("hn"), circle("hole").centre());
//!     }
//! });
//! for (i, c) in rect("tb", "tr", "tt", "tl").into_iter().enumerate() {
//!     let id = format!("r{i}");
//!     top.add_constraint(id.as_str().into(), c, ours(), &Params::NONE)?;
//! }
//!
//! // Front view, (X, Z): the rectangle with the height, and the hole as
//! // two hidden lines from edge to edge.
//! let mut front = sketch!(Model {
//!     entities(layer: "visible", provenance: ours()) {
//!         fb: line (0.0, 0.0) -> (40.0, 0.0);
//!         fr: line (40.0, 0.0) -> (40.0, 10.0);
//!         ft: line (40.0, 10.0) -> (0.0, 10.0);
//!         fl: line (0.0, 10.0) -> (0.0, 0.0);
//!     }
//!     entities(layer: "hidden", provenance: ours()) {
//!         fw: line (7.0, 0.0) -> (7.0, 10.0);
//!         fe: line (13.0, 0.0) -> (13.0, 10.0);
//!     }
//!     constraints(provenance: ours()) {
//!         height: vertical_distance(
//!             line("fr").start(), line("fr").end(), mm(10.0),
//!         );
//!         v1: vertical(line("fw"));
//!         v2: vertical(line("fe"));
//!         o1: on_line(line("fw").start(), line("fb"));
//!         o2: on_line(line("fw").end(), line("ft"));
//!         o3: on_line(line("fe").start(), line("fb"));
//!         o4: on_line(line("fe").end(), line("ft"));
//!     }
//! });
//! for (i, c) in rect("fb", "fr", "ft", "fl").into_iter().enumerate() {
//!     let id = format!("r{i}");
//!     front.add_constraint(id.as_str().into(), c, ours(), &Params::NONE)?;
//! }
//!
//! // Side view, (Y, Z): only its shape; depth, height and the hidden
//! // lines all arrive through links.
//! let mut side = sketch!(Model {
//!     entities(layer: "visible", provenance: ours()) {
//!         sb: line (0.0, 0.0) -> (20.0, 0.0);
//!         sr: line (20.0, 0.0) -> (20.0, 10.0);
//!         st: line (20.0, 10.0) -> (0.0, 10.0);
//!         sl: line (0.0, 10.0) -> (0.0, 0.0);
//!     }
//!     entities(layer: "hidden", provenance: ours()) {
//!         ss: line (7.0, 0.0) -> (7.0, 10.0);
//!         sn: line (13.0, 0.0) -> (13.0, 10.0);
//!     }
//!     constraints(provenance: ours()) {
//!         v1: vertical(line("ss"));
//!         v2: vertical(line("sn"));
//!         o1: on_line(line("ss").start(), line("sb"));
//!         o2: on_line(line("ss").end(), line("st"));
//!         o3: on_line(line("sn").start(), line("sb"));
//!         o4: on_line(line("sn").end(), line("st"));
//!     }
//! });
//! for (i, c) in rect("sb", "sr", "st", "sl").into_iter().enumerate() {
//!     let id = format!("r{i}");
//!     side.add_constraint(id.as_str().into(), c, ours(), &Params::NONE)?;
//! }
//!
//! let view = |axes, sketch, at| {
//!     let at = Transform::at(at, 100.0);
//!     View::new(axes, sketch, at)?.claim_model(ours())
//! };
//! let mut views = Views::new();
//! let top = view((ModelAxis::X, ModelAxis::Y), top, 100.0)?;
//! views.add_view("top".into(), top)?;
//! let front = view((ModelAxis::X, ModelAxis::Z), front, 100.0)?;
//! views.add_view("front".into(), front)?;
//! let side = view((ModelAxis::Y, ModelAxis::Z), side, 200.0)?;
//! views.add_view("side".into(), side)?;
//!
//! // Projection lines: what the drawing's layout says.
//! let t = |p| in_view("top", p);
//! let f = |p| in_view("front", p);
//! let s = |p| in_view("side", p);
//! let links = [
//!     ("x0", aligned(
//!         f(line("fb").start()),
//!         t(line("tb").start()),
//!         ModelAxis::X
//!     )),
//!     ("x1", aligned(f(line("fb").end()), t(line("tb").end()), ModelAxis::X)),
//!     ("xw", aligned(f(line("fw").start()), t(point("hw")), ModelAxis::X)),
//!     ("xe", aligned(f(line("fe").start()), t(point("he")), ModelAxis::X)),
//!     ("y0", aligned(
//!         s(line("sb").start()),
//!         t(line("tb").start()),
//!         ModelAxis::Y
//!     )),
//!     ("y1", aligned(s(line("sb").end()), t(line("tr").end()), ModelAxis::Y)),
//!     ("ys", aligned(s(line("ss").start()), t(point("hs")), ModelAxis::Y)),
//!     ("yn", aligned(s(line("sn").start()), t(point("hn")), ModelAxis::Y)),
//!     ("z0", aligned(
//!         s(line("sr").start()),
//!         f(line("fr").start()),
//!         ModelAxis::Z
//!     )),
//!     ("z1", aligned(s(line("sr").end()), f(line("fr").end()), ModelAxis::Z)),
//! ];
//! for (id, link) in links {
//!     views.add_link(id.into(), link, ours())?;
//! }
//!
//! let a = views.solve(Options::default(), &Params::NONE)?;
//! assert!(a.residual_max < 1e-9);
//! assert!(a.groups.is_empty());
//! // Nothing is free but where front and side sit in Z, together.
//! assert_eq!(a.internal_free, 0);
//! assert_eq!(a.free, 1);
//! let top = a.view("top").unwrap();
//! assert!(top.status.values().all(|s| *s == EntityStatus::Fixed));
//! assert!(!top.placement.y);
//! let front = &a.view("front").unwrap().placement;
//! assert!(front.y && !front.x);
//! let side = &a.view("side").unwrap().placement;
//! assert!(side.y && !side.x);
//! assert!(a.view("side").unwrap()
//!     .status
//!     .values()
//!     .all(|s| *s == EntityStatus::FixedUpToPlacement));
//!
//! // The side view's hidden lines landed at the hole's Y extent, 7 and 13,
//! // though the side view never mentions the hole.
//! let g = views.view("side").unwrap().sketch.geometry();
//! assert!((g.position(&line("ss").start())?.x - 7.0).abs() < 1e-9);
//! assert!((g.position(&line("sn").start())?.x - 13.0).abs() < 1e-9);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::solver::equation::{self as eq, Axis, Equation};
use crate::solver::solve::{self as solver, Options, Placement, Report, Termination};
use crate::value::geometry::{self, EntityId, Geometry, Model, PointRef};
use crate::value::params::Params;
use crate::value::provenance::{self, ConstraintId, Provenance};
use crate::value::sketch::{self, EntityStatus, Sketch, CONFLICT};
use crate::value::view::{self, ModelAxis, View, ViewId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ustr::Ustr;

/// The name of a link, an interned `Copy` handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LinkId(Ustr);

impl LinkId {
    /// A name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&LinkId> for LinkId {
    fn from(id: &LinkId) -> Self {
        *id
    }
}

impl From<&str> for LinkId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// A point in a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewPoint {
    /// The view.
    pub view: ViewId,
    /// The point in its sketch.
    pub point: PointRef,
}

/// A point in a view.
pub fn in_view(view: impl Into<ViewId>, point: PointRef) -> ViewPoint {
    ViewPoint {
        view: view.into(),
        point,
    }
}

/// A relation between views. Opaque; built by [`aligned`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Link(LinkKind);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "link", rename_all = "snake_case")]
enum LinkKind {
    Aligned {
        a: ViewPoint,
        b: ViewPoint,
        axis: ModelAxis,
    },
}

/// `a` and `b` have the same coordinate along the model `axis`, which
/// both their views must show.
pub fn aligned(a: ViewPoint, b: ViewPoint, axis: ModelAxis) -> Link {
    Link(LinkKind::Aligned { a, b, axis })
}

impl Link {
    /// The views and points involved.
    pub fn points(&self) -> [ViewPoint; 2] {
        match self.0 {
            LinkKind::Aligned { a, b, .. } => [a, b],
        }
    }
}

/// A stored link: the relation and its provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredLink {
    link: Link,
    provenance: Provenance,
}

impl StoredLink {
    /// The relation.
    pub fn link(&self) -> &Link {
        &self.link
    }

    /// The provenance.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// What can go wrong with a system of views.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A view with this id already exists.
    DuplicateView(ViewId),
    /// No view has this id.
    NoSuchView(ViewId),
    /// A link with this id already exists.
    DuplicateLink(LinkId),
    /// No link has this id.
    NoSuchLink(LinkId),
    /// A link's two points are in the same view (that is a constraint).
    SameView(ViewId),
    /// The view does not show the link's axis.
    AxisNotShown {
        /// The view.
        view: ViewId,
        /// The axis.
        axis: ModelAxis,
    },
    /// A link's point does not exist in its view.
    NoSuchPoint(ViewPoint),
    /// The view is used by these links.
    ViewInUse {
        /// The view.
        view: ViewId,
        /// Its links.
        by: Vec<LinkId>,
    },
    /// A view error.
    View(view::Error),
    /// A sketch error.
    Sketch(sketch::Error),
    /// A geometry error.
    Geometry(geometry::Error),
    /// A provenance error.
    Provenance(provenance::Error),
    /// A solver error.
    Solver(eq::Error),
}

impl From<view::Error> for Error {
    fn from(e: view::Error) -> Self {
        Error::View(e)
    }
}

impl From<sketch::Error> for Error {
    fn from(e: sketch::Error) -> Self {
        Error::Sketch(e)
    }
}

impl From<geometry::Error> for Error {
    fn from(e: geometry::Error) -> Self {
        Error::Geometry(e)
    }
}

impl From<provenance::Error> for Error {
    fn from(e: provenance::Error) -> Self {
        Error::Provenance(e)
    }
}

impl From<eq::Error> for Error {
    fn from(e: eq::Error) -> Self {
        Error::Solver(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// A member of a system-level dependency group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Member {
    /// A constraint of a view.
    Constraint(ViewId, ConstraintId),
    /// A link.
    Link(LinkId),
}

/// Constraints and links that are not independent at the solution. For
/// a conflict, `culprits` are those whose removal alone would resolve it.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The members, constraints first by view then links, each once.
    pub members: Vec<Member>,
    /// The largest residual among them, mm.
    pub residual: f64,
    /// Members whose removal alone resolves the conflict (empty for a
    /// redundancy).
    pub culprits: Vec<Member>,
}

/// A driven (`REF`) dimension of a view checked against the system.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceCheck {
    /// The view.
    pub view: ViewId,
    /// The dimension.
    pub constraint: ConstraintId,
    /// The figure's value less the claimed one, per equation, in mm or
    /// rad; zero when the figure agrees with the sheet.
    pub off_by: Vec<f64>,
}

/// What a solve of the system found, for one view.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewAnalysis {
    /// What this view is free to do on its own.
    pub placement: Placement,
    /// How far each entity is determined.
    pub status: BTreeMap<EntityId, EntityStatus>,
}

/// What a solve or analysis of the system found. The solver's report is
/// translated into views' terms, since its own names are the merged
/// system's.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemAnalysis {
    /// Parameters in the merged system.
    pub parameters: usize,
    /// Equations in the merged system.
    pub equations: usize,
    /// Independent equations.
    pub rank: usize,
    /// Free parameters, placements included.
    pub free: usize,
    /// Dependent equations.
    pub redundant: usize,
    /// The largest residual at the solution, mm or rad.
    pub residual_max: f64,
    /// Why the solver stopped.
    pub termination: Termination,
    /// Freedoms other than the views' placements.
    pub internal_free: usize,
    /// Per view.
    pub views: BTreeMap<ViewId, ViewAnalysis>,
    /// Redundancies and conflicts, as constraints and links.
    pub groups: Vec<Group>,
    /// Every view's `REF` dimensions against the solved system.
    pub reference: Vec<ReferenceCheck>,
}

impl SystemAnalysis {
    /// One view's part of the analysis.
    pub fn view(&self, id: impl Into<ViewId>) -> Option<&ViewAnalysis> {
        self.views.get(&id.into())
    }
}

/// The merged system: the geometry, the equations, who owns each, and
/// the table from merged ids back to views' entities.
struct Lowered {
    geometry: Geometry<Model>,
    equations: Vec<Equation>,
    owners: Vec<Member>,
    bodies: Vec<Vec<EntityId>>,
    table: Table,
}

/// Merged ids are `m0, m1, …` in the order the views' entities are
/// visited; unique by construction, decoded only through this table.
#[derive(Default)]
struct Table {
    to_merged: BTreeMap<(ViewId, EntityId), EntityId>,
    from_merged: BTreeMap<EntityId, (ViewId, EntityId)>,
}

impl Table {
    fn insert(&mut self, view: ViewId, entity: EntityId) -> EntityId {
        let merged = EntityId::new(&format!("m{}", self.from_merged.len()));
        self.to_merged.insert((view, entity), merged);
        self.from_merged.insert(merged, (view, entity));
        merged
    }

    fn merged(&self, view: ViewId, entity: EntityId) -> EntityId {
        self.to_merged[&(view, entity)]
    }
}

/// Views and the links between them. See the module documentation.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct Views {
    views: BTreeMap<ViewId, View>,
    links: BTreeMap<LinkId, StoredLink>,
}

#[derive(Serialize, Deserialize)]
struct Raw {
    views: BTreeMap<ViewId, View>,
    links: BTreeMap<LinkId, StoredLink>,
}

impl From<Views> for Raw {
    fn from(v: Views) -> Self {
        Raw {
            views: v.views,
            links: v.links,
        }
    }
}

impl TryFrom<Raw> for Views {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        let mut v = Views::default();
        for (id, view) in r.views {
            v.add_view(id, view)?;
        }
        for (id, l) in r.links {
            v.add_link(id, l.link, l.provenance)?;
        }
        Ok(v)
    }
}

impl Views {
    /// No views.
    pub fn new() -> Self {
        Self::default()
    }

    /// A view.
    pub fn view(&self, id: impl Into<ViewId>) -> Option<&View> {
        let id = &id.into();
        self.views.get(id)
    }

    /// All views, in id order.
    pub fn views(&self) -> impl Iterator<Item = (&ViewId, &View)> {
        self.views.iter()
    }

    /// A link.
    pub fn link(&self, id: impl Into<LinkId>) -> Option<&StoredLink> {
        let id = &id.into();
        self.links.get(id)
    }

    /// All links, in id order.
    pub fn links(&self) -> impl Iterator<Item = (&LinkId, &StoredLink)> {
        self.links.iter()
    }

    /// Add a view.
    pub fn add_view(&mut self, id: ViewId, view: View) -> Result<(), Error> {
        if self.views.contains_key(&id) {
            return Err(Error::DuplicateView(id));
        }
        view.check()?;
        self.views.insert(id, view);
        Ok(())
    }

    /// The links that touch a view.
    pub fn links_of(&self, id: impl Into<ViewId>) -> Vec<LinkId> {
        let id = &id.into();
        self.links
            .iter()
            .filter(|(_, l)| l.link.points().iter().any(|p| p.view == *id))
            .map(|(lid, _)| *lid)
            .collect()
    }

    /// Remove a view. Refused while a link uses it.
    pub fn remove_view(&mut self, id: impl Into<ViewId>) -> Result<View, Error> {
        let id = &id.into();
        if !self.views.contains_key(id) {
            return Err(Error::NoSuchView(*id));
        }
        let by = self.links_of(id);
        if !by.is_empty() {
            return Err(Error::ViewInUse { view: *id, by });
        }
        Ok(self.views.remove(id).expect("checked"))
    }

    /// Edit a view's sketch through `f`. The links touching the view are
    /// re-checked afterwards; if one would dangle, the edit is rolled back
    /// and the error returned.
    pub fn edit<T>(
        &mut self,
        id: impl Into<ViewId>,
        f: impl FnOnce(&mut Sketch<Model>) -> Result<T, sketch::Error>,
    ) -> Result<T, Error> {
        let id = &id.into();
        let view = self.views.get_mut(id).ok_or(Error::NoSuchView(*id))?;
        let before = view.sketch.clone();
        let out = match f(&mut view.sketch) {
            Ok(t) => t,
            Err(e) => {
                view.sketch = before;
                return Err(e.into());
            }
        };
        for lid in self.links_of(id) {
            let link = self.links[&lid].link;
            if let Err(e) = self.check_link(&link) {
                self.views.get_mut(id).expect("exists").sketch = before;
                return Err(e);
            }
        }
        Ok(out)
    }

    /// Edit a view (its transform, region, frame claim, or sketch) through
    /// `f`, with the same re-check and rollback as [`edit`](Self::edit);
    /// the frame claim's provenance is checked too.
    pub fn edit_view<T>(
        &mut self,
        id: impl Into<ViewId>,
        f: impl FnOnce(&mut View) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let id = &id.into();
        let view = self.views.get_mut(id).ok_or(Error::NoSuchView(*id))?;
        let before = view.clone();
        let out = match f(view) {
            Ok(t) => t,
            Err(e) => {
                *view = before;
                return Err(e);
            }
        };
        let check = view.check().map_err(Error::from);
        let check = check.and_then(|()| {
            self.links_of(id)
                .into_iter()
                .try_for_each(|lid| self.check_link(&self.links[&lid].link))
        });
        if let Err(e) = check {
            *self.views.get_mut(id).expect("exists") = before;
            return Err(e);
        }
        Ok(out)
    }

    /// Everything a link must satisfy: two different existing views, both
    /// showing the axis, both points present in their sketches.
    fn check_link(&self, link: &Link) -> Result<(), Error> {
        let LinkKind::Aligned { a, b, axis } = link.0;
        if a.view == b.view {
            return Err(Error::SameView(a.view));
        }
        for vp in [a, b] {
            let view = self.views.get(&vp.view).ok_or(Error::NoSuchView(vp.view))?;
            if view.axis_index(axis).is_none() {
                return Err(Error::AxisNotShown {
                    view: vp.view,
                    axis,
                });
            }
            if view.sketch.geometry().position(&vp.point).is_err() {
                return Err(Error::NoSuchPoint(vp));
            }
        }
        Ok(())
    }

    /// Add a link, checked.
    pub fn add_link(
        &mut self,
        id: LinkId,
        link: Link,
        provenance: Provenance,
    ) -> Result<(), Error> {
        if self.links.contains_key(&id) {
            return Err(Error::DuplicateLink(id));
        }
        provenance.check()?;
        self.check_link(&link)?;
        self.links.insert(id, StoredLink { link, provenance });
        Ok(())
    }

    /// Remove a link.
    pub fn remove_link(&mut self, id: impl Into<LinkId>) -> Result<StoredLink, Error> {
        let id = &id.into();
        self.links.remove(id).ok_or(Error::NoSuchLink(*id))
    }

    /// The link's equation in the merged system: the two points' sketch
    /// coordinates that show the axis are equal.
    fn lower_link(&self, link: &Link, table: &Table) -> Equation {
        let LinkKind::Aligned { a, b, axis } = link.0;
        let sketch_axis = |vp: ViewPoint| match self.views[&vp.view].axis_index(axis) {
            Some(0) => Axis::X,
            _ => Axis::Y,
        };
        let pa = a.point.with_id(table.merged(a.view, a.point.id()));
        let pb = b.point.with_id(table.merged(b.view, b.point.id()));
        eq::same(pa, sketch_axis(a), pb, sketch_axis(b))
    }

    /// Merge the views into one geometry and gather every equation, the
    /// parameters resolved in the document's table.
    fn lower(&self, params: &Params) -> Result<Lowered, Error> {
        let mut geometry = Geometry::default();
        let mut equations = Vec::new();
        let mut owners = Vec::new();
        let mut bodies = Vec::new();
        let mut table = Table::default();
        for (vid, view) in &self.views {
            let mut body = Vec::new();
            for (eid, e) in view.sketch.geometry().iter() {
                let mid = table.insert(*vid, *eid);
                geometry.add(mid, e.clone())?;
                body.push(mid);
            }
            bodies.push(body);
            let rename = |id: EntityId| table.merged(*vid, id);
            let low = view.sketch.lower(params)?;
            for (e, owner) in low.equations.iter().zip(&low.owners) {
                equations.push(e.map_ids(&rename));
                owners.push(Member::Constraint(*vid, *owner));
            }
        }
        for (lid, l) in &self.links {
            if !l.provenance.active() {
                continue;
            }
            equations.push(self.lower_link(&l.link, &table).canonical()?);
            owners.push(Member::Link(*lid));
        }
        Ok(Lowered {
            geometry,
            equations,
            owners,
            bodies,
            table,
        })
    }

    /// Solve all views as one system in the environment `params` and
    /// apply the result to each.
    pub fn solve(&mut self, o: Options, params: &Params) -> Result<SystemAnalysis, Error> {
        let mut low = self.lower(params)?;
        let solved = solver::solve_bodies(&low.geometry, &low.equations, &low.bodies, o)?;
        // Each view takes back its share.
        for (vid, view) in &mut self.views {
            let mut g = Geometry::default();
            for (eid, _) in view.sketch.geometry().iter() {
                let e = solved
                    .geometry
                    .get(low.table.merged(*vid, *eid))
                    .expect("the solver returns every entity")
                    .clone();
                g.add(*eid, e)?;
            }
            view.sketch.set_geometry(g);
        }
        // The report is of the solution: the reference checks and the
        // culprits' trial solves start from there, as a sketch's do.
        low.geometry = solved.geometry;
        self.analysis(&low, solved.report, o, params)
    }

    /// The same report at the current state, without moving anything.
    pub fn analyse(&self, o: Options, params: &Params) -> Result<SystemAnalysis, Error> {
        let low = self.lower(params)?;
        let at = solver::analyse_bodies(&low.geometry, &low.equations, &low.bodies, o)?;
        self.analysis(&low, at.report, o, params)
    }

    /// Translate the report into views' terms. For a conflicting group,
    /// each member is removed in turn and the rest re-solved; those whose
    /// removal resolves it are the culprits. Driven dimensions are
    /// checked in the merged geometry, where links have had their say.
    fn analysis(
        &self,
        low: &Lowered,
        report: Report,
        o: Options,
        params: &Params,
    ) -> Result<SystemAnalysis, Error> {
        let mut views = BTreeMap::new();
        for (k, (vid, view)) in self.views.iter().enumerate() {
            let placement = report.bodies.get(k).copied().unwrap_or_default();
            let status = view
                .sketch
                .geometry()
                .iter()
                .map(|(eid, e)| {
                    let width = e.values();
                    let m = report.motion.get(&low.table.merged(*vid, *eid));
                    (*eid, sketch::entity_status(m, width))
                })
                .collect();
            views.insert(*vid, ViewAnalysis { placement, status });
        }
        let mut groups = Vec::new();
        for d in &report.dependencies {
            let mut members: Vec<Member> =
                d.equations.iter().map(|&i| low.owners[i].clone()).collect();
            members.sort_by_key(|m| match m {
                Member::Constraint(v, c) => (0, v.as_str().to_string(), c.as_str().to_string()),
                Member::Link(l) => (1, l.as_str().to_string(), String::new()),
            });
            members.dedup();
            let mut culprits = Vec::new();
            if d.residual > CONFLICT {
                for m in &members {
                    let rest: Vec<Equation> = low
                        .equations
                        .iter()
                        .zip(&low.owners)
                        .filter(|(_, owner)| *owner != m)
                        .map(|(e, _)| *e)
                        .collect();
                    let trial = solver::solve_bodies(&low.geometry, &rest, &low.bodies, o)?;
                    if trial.report.residual_max <= CONFLICT {
                        culprits.push(m.clone());
                    }
                }
            }
            let g = Group {
                members,
                residual: d.residual,
                culprits,
            };
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        let mut reference = Vec::new();
        for (vid, view) in &self.views {
            let rename = |id: EntityId| low.table.merged(*vid, id);
            let driven = view.sketch.lower_driven(params)?;
            let eqs: Vec<Equation> = driven
                .equations
                .iter()
                .map(|e| e.map_ids(&rename))
                .collect();
            let residuals = solver::residuals(&low.geometry, &eqs)?;
            for (owner, r) in driven.owners.iter().zip(residuals) {
                match reference.last_mut() {
                    Some(ReferenceCheck {
                        view,
                        constraint,
                        off_by,
                    }) if view == vid && constraint == owner => off_by.extend(r),
                    _ => reference.push(ReferenceCheck {
                        view: *vid,
                        constraint: *owner,
                        off_by: r,
                    }),
                }
            }
        }
        Ok(SystemAnalysis {
            parameters: report.parameters,
            equations: report.equations,
            rank: report.rank,
            free: report.free,
            redundant: report.redundant,
            residual_max: report.residual_max,
            termination: report.termination,
            internal_free: report.internal_free,
            views,
            groups,
            reference,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch;
    use crate::solver::equation::line;
    use crate::value::measure::Measure;
    use crate::value::provenance::{Agent, Date, Method};
    use crate::value::relation::{coincident, horizontal, length, position, vertical};
    use crate::value::view::Transform;

    fn ours() -> Provenance {
        Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 15).unwrap(),
        )
    }

    fn mm(v: f64) -> Measure {
        Measure::mm(v).unwrap()
    }

    /// A rectangle, optionally with a width, a height, and a fiducial.
    fn rect(width: Option<f64>, height: Option<f64>, origin: bool) -> Sketch<Model> {
        let mut s = sketch!(Model {
            entities(layer: "visible", provenance: ours()) {
                b: line (0.0, 0.0) -> (30.0, 0.0);
                r: line (30.0, 0.0) -> (30.0, 10.0);
                t: line (30.0, 10.0) -> (0.0, 10.0);
                l: line (0.0, 10.0) -> (0.0, 0.0);
            }
            constraints(provenance: ours()) {
                c1: coincident(line("b").end(), line("r").start());
                c2: coincident(line("r").end(), line("t").start());
                c3: coincident(line("t").end(), line("l").start());
                c4: coincident(line("l").end(), line("b").start());
                h1: horizontal(line("b"));
                h2: horizontal(line("t"));
                v1: vertical(line("r"));
                v2: vertical(line("l"));
            }
        });
        if let Some(w) = width {
            s.add_constraint("w".into(), length(line("b"), mm(w)), ours(), &Params::NONE)
                .unwrap();
        }
        if let Some(h) = height {
            s.add_constraint("h".into(), length(line("r"), mm(h)), ours(), &Params::NONE)
                .unwrap();
        }
        if origin {
            s.add_constraint(
                "o".into(),
                position(line("b").start(), mm(0.0), mm(0.0)),
                ours(),
                &Params::NONE,
            )
            .unwrap();
        }
        s
    }

    fn view(axes: (ModelAxis, ModelAxis), sketch: Sketch<Model>) -> View {
        View::new(axes, sketch, Transform::at(0.0, 0.0))
            .unwrap()
            .claim_model(ours())
            .unwrap()
    }

    #[test]
    fn links_are_checked() {
        let mut v = Views::new();
        v.add_view(
            "top".into(),
            view(
                (ModelAxis::X, ModelAxis::Y),
                rect(Some(30.0), Some(10.0), true),
            ),
        )
        .unwrap();
        v.add_view(
            "front".into(),
            view((ModelAxis::X, ModelAxis::Z), rect(None, None, false)),
        )
        .unwrap();
        assert_eq!(
            v.add_view(
                "top".into(),
                view((ModelAxis::X, ModelAxis::Y), rect(None, None, false))
            )
            .err(),
            Some(Error::DuplicateView("top".into()))
        );
        let b = line("b");
        // Same view twice is a constraint, not a link.
        assert_eq!(
            v.add_link(
                "l".into(),
                aligned(
                    in_view("top", b.start()),
                    in_view("top", b.end()),
                    ModelAxis::X
                ),
                ours()
            )
            .err(),
            Some(Error::SameView("top".into()))
        );
        // The axis must be shown by both views.
        assert_eq!(
            v.add_link(
                "l".into(),
                aligned(
                    in_view("top", b.start()),
                    in_view("front", b.start()),
                    ModelAxis::Y
                ),
                ours()
            )
            .err(),
            Some(Error::AxisNotShown {
                view: "front".into(),
                axis: ModelAxis::Y
            })
        );
        // Both points must exist.
        let nope = in_view("front", line("nope").start());
        assert_eq!(
            v.add_link(
                "l".into(),
                aligned(in_view("top", b.start()), nope, ModelAxis::X),
                ours()
            )
            .err(),
            Some(Error::NoSuchPoint(nope))
        );
        assert_eq!(
            v.add_link(
                "l".into(),
                aligned(
                    in_view("top", b.start()),
                    in_view("nowhere", b.start()),
                    ModelAxis::X
                ),
                ours()
            )
            .err(),
            Some(Error::NoSuchView("nowhere".into()))
        );
        // A good one, once.
        let good = aligned(
            in_view("front", b.start()),
            in_view("top", b.start()),
            ModelAxis::X,
        );
        v.add_link("l".into(), good, ours()).unwrap();
        assert_eq!(
            v.add_link("l".into(), good, ours()).err(),
            Some(Error::DuplicateLink("l".into()))
        );
        // A view in use cannot go.
        assert_eq!(
            v.remove_view("front").err(),
            Some(Error::ViewInUse {
                view: "front".into(),
                by: vec!["l".into()]
            })
        );
        v.remove_link("l").unwrap();
        v.remove_view("front").unwrap();
        assert_eq!(
            v.remove_link("l").err(),
            Some(Error::NoSuchLink("l".into()))
        );
    }

    #[test]
    fn edits_that_would_dangle_a_link_roll_back() {
        let mut v = Views::new();
        v.add_view(
            "top".into(),
            view(
                (ModelAxis::X, ModelAxis::Y),
                rect(Some(30.0), Some(10.0), true),
            ),
        )
        .unwrap();
        v.add_view(
            "front".into(),
            view((ModelAxis::X, ModelAxis::Z), rect(None, None, false)),
        )
        .unwrap();
        let b = line("b");
        v.add_link(
            "l".into(),
            aligned(
                in_view("front", b.end()),
                in_view("top", b.end()),
                ModelAxis::X,
            ),
            ours(),
        )
        .unwrap();
        // Removing the linked line (after its constraints) would dangle
        // the link: refused, and the sketch is as before.
        let before = v.view("front").unwrap().sketch.clone();
        let r = v.edit("front", |s| {
            for c in ["c1", "c4", "h1", "h2"] {
                let _ = s.remove_constraint(c);
            }
            s.remove_entity("b").map(|_| ())
        });
        assert!(matches!(
            r,
            Err(Error::NoSuchPoint(_)) | Err(Error::Sketch(_))
        ));
        assert_eq!(v.view("front").unwrap().sketch, before);
        // An edit that keeps the link is kept.
        v.edit("front", |s| {
            s.add_constraint(
                "hh".into(),
                length(line("r"), mm(10.0)),
                ours(),
                &Params::NONE,
            )
        })
        .unwrap();
        assert!(v.view("front").unwrap().sketch.constraint("hh").is_some());
    }

    #[test]
    fn linked_views_solve_as_one_and_report_shared_placement() {
        let mut v = Views::new();
        v.add_view(
            "top".into(),
            view(
                (ModelAxis::X, ModelAxis::Y),
                rect(Some(30.0), Some(10.0), true),
            ),
        )
        .unwrap();
        // The front view starts the wrong width; the links fix it.
        v.add_view(
            "front".into(),
            view((ModelAxis::X, ModelAxis::Z), rect(None, None, false)),
        )
        .unwrap();
        let b = line("b");
        v.add_link(
            "x0".into(),
            aligned(
                in_view("front", b.start()),
                in_view("top", b.start()),
                ModelAxis::X,
            ),
            ours(),
        )
        .unwrap();
        v.add_link(
            "x1".into(),
            aligned(
                in_view("front", b.end()),
                in_view("top", b.end()),
                ModelAxis::X,
            ),
            ours(),
        )
        .unwrap();
        let a = v.solve(Options::default(), &Params::NONE).unwrap();
        assert!(a.residual_max < 1e-9, "{a:?}");
        let fg = v.view("front").unwrap().sketch.geometry();
        assert!((fg.position(&b.end()).unwrap().x - 30.0).abs() < 1e-9);
        // Top: fixed. Front: its Z position and height are free; width is
        // not. That is one placement (Z) and one internal freedom (height).
        let top = a.view("top").unwrap();
        let front = a.view("front").unwrap();
        assert!(top.status.values().all(|s| *s == EntityStatus::Fixed));
        assert_eq!(top.placement, solver::Placement::default());
        assert!(front.placement.y && !front.placement.x && !front.placement.rotation);
        assert_eq!(a.internal_free, 1);
        assert_eq!(a.free, 2);
        assert!(a.groups.is_empty());
        // Analysis without solving agrees; the file round trip holds.
        let b2 = v.analyse(Options::default(), &Params::NONE).unwrap();
        assert_eq!(b2.views, a.views);
        let json = serde_json::to_string(&v).unwrap();
        let back: Views = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
        let bad = json.replace(r#""view":"front""#, r#""view":"nowhere""#);
        assert!(serde_json::from_str::<Views>(&bad).is_err());
    }

    #[test]
    fn a_conflicting_link_is_a_culprit_and_refs_are_checked() {
        let mut v = Views::new();
        v.add_view(
            "top".into(),
            view(
                (ModelAxis::X, ModelAxis::Y),
                rect(Some(30.0), Some(10.0), true),
            ),
        )
        .unwrap();
        // The front claims 31 wide, driving; and carries a REF height.
        let mut front = rect(Some(31.0), None, false);
        front
            .add_constraint(
                "h_ref".into(),
                length(line("r"), mm(12.0)).driven().unwrap(),
                ours(),
                &Params::NONE,
            )
            .unwrap();
        v.add_view("front".into(), view((ModelAxis::X, ModelAxis::Z), front))
            .unwrap();
        let b = line("b");
        let x0 = aligned(
            in_view("front", b.start()),
            in_view("top", b.start()),
            ModelAxis::X,
        );
        let x1 = aligned(
            in_view("front", b.end()),
            in_view("top", b.end()),
            ModelAxis::X,
        );
        v.add_link("x0".into(), x0, ours()).unwrap();
        v.add_link("x1".into(), x1, ours()).unwrap();
        let a = v.solve(Options::default(), &Params::NONE).unwrap();
        // 30 by the top view through the links, 31 by the front's own
        // width: a conflict. Removing either link, the top's width or the
        // front's width resolves it; the group names all four.
        assert_eq!(a.groups.len(), 1, "{:?}", a.groups);
        let g = &a.groups[0];
        assert!(g.residual > CONFLICT);
        for m in [
            Member::Link("x0".into()),
            Member::Link("x1".into()),
            Member::Constraint("top".into(), "w".into()),
            Member::Constraint("front".into(), "w".into()),
        ] {
            assert!(g.culprits.contains(&m), "{m:?} not in {:?}", g.culprits);
        }
        assert_eq!(g.members, g.culprits);
        // The REF height: the front's height is free in this system, so
        // the check is against wherever it sits; 10 as drawn, claims 12.
        assert_eq!(a.reference.len(), 1);
        let r = &a.reference[0];
        assert_eq!((r.view, r.constraint), ("front".into(), "h_ref".into()));
        assert_eq!(r.off_by.len(), 1);
        assert!((r.off_by[0] - (10.0 - 12.0)).abs() < 1e-6, "{:?}", r.off_by);
    }

    #[test]
    fn a_redundant_link_is_named() {
        let mut v = Views::new();
        v.add_view(
            "top".into(),
            view(
                (ModelAxis::X, ModelAxis::Y),
                rect(Some(30.0), Some(10.0), true),
            ),
        )
        .unwrap();
        v.add_view(
            "front".into(),
            view((ModelAxis::X, ModelAxis::Z), rect(Some(30.0), None, false)),
        )
        .unwrap();
        let b = line("b");
        v.add_link(
            "x0".into(),
            aligned(
                in_view("front", b.start()),
                in_view("top", b.start()),
                ModelAxis::X,
            ),
            ours(),
        )
        .unwrap();
        // The front already has its width; aligning the far end repeats it.
        v.add_link(
            "x1".into(),
            aligned(
                in_view("front", b.end()),
                in_view("top", b.end()),
                ModelAxis::X,
            ),
            ours(),
        )
        .unwrap();
        let a = v.solve(Options::default(), &Params::NONE).unwrap();
        assert_eq!(a.groups.len(), 1);
        let g = &a.groups[0];
        assert!(g.residual < 1e-9);
        assert!(g.members.contains(&Member::Link("x1".into())));
        assert!(g
            .members
            .contains(&Member::Constraint("front".into(), "w".into())));
        assert!(g.culprits.is_empty());
    }
}
