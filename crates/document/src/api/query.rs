//! The read side: questions a client asks of the model, and the answers.
//!
//! A client, a panel in the viewer or an agent over the API, never holds
//! the document. It sends a [`Query`] and gets an [`Answer`], both
//! serialisable and both deterministic over the model's state, so the
//! wire surface is symmetric with the messages: `update(Msg)` in,
//! `query(Query)` out. Queries are joins: [`Query::Item`] gathers what
//! six accessors and a walk would, once, here, where it is tested.
//!
//! Queries read the shown document (the preview during a drag, else the
//! settled one), as the screen does.

use crate::api::model::{Model as Elm, Msg};
use crate::solver::solve::{Options, Placement};
use crate::state::document::{Document, Error, Source, Title};
use crate::state::views::{LinkId, Member, StoredLink};
use crate::value::dimension::{Dimension, DimensionId};
use crate::value::drafting::Drafting;
use crate::value::frame::{Frame, FrameId};
use crate::value::geometry::{Entity, EntityId, Model, Paper, Rect, P2};
use crate::value::leader::{Leader, LeaderId};
use crate::value::lettering::Lettering;
use crate::value::measure::{Measure, ParamName};
use crate::value::provenance::{ConstraintId, DocId, ItemRef, Provenance};
use crate::value::relation::Relation;
use crate::value::sketch::{EntityStatus, LayerId, Sketch};
use crate::value::style::{Appearance, Layer, PaletteId};
use crate::value::text::{Text, TextId};
use crate::value::view::{ModelAxis, Transform, ViewId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where an item lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "in", rename_all = "snake_case")]
pub enum Target {
    /// The sheet sketch.
    Sheet,
    /// A view's sketch.
    View {
        /// The view.
        view: ViewId,
    },
}

/// A question.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum Query {
    /// The title, the counts, the history depth.
    Info,
    /// Everything about one entity.
    Item {
        /// Where.
        target: Target,
        /// Which.
        id: EntityId,
    },
    /// Everything about one constraint.
    Constraint {
        /// Where.
        target: Target,
        /// Which.
        id: ConstraintId,
    },
    /// The analysis, in the sketches' terms.
    Status,
    /// The layer table.
    Layers,
    /// The palette.
    Palette,
    /// The lettering table.
    Lettering,
    /// The texts.
    Texts,
    /// The drafting table.
    Drafting,
    /// The leaders.
    Leaders,
    /// The dimensions.
    Dimensions,
    /// The source registry.
    Sources,
    /// The views and links.
    Views,
    /// The messages since an index.
    Journal {
        /// The first index wanted.
        from: usize,
    },
    /// The whole snapshot as a `.lmsheet`.
    Document,
}

/// The counts and depths.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Info {
    /// The title block.
    pub title: Title,
    /// The frames the sheet was read from.
    pub frames: BTreeMap<FrameId, Frame>,
    /// Entities on the sheet.
    pub sheet_entities: usize,
    /// Views.
    pub views: usize,
    /// Links.
    pub links: usize,
    /// Notes.
    pub notes: usize,
    /// Steps back available.
    pub undo_depth: usize,
    /// Steps forward available.
    pub redo_depth: usize,
    /// Whether a preview is in progress.
    pub previewing: bool,
    /// Messages handled so far.
    pub journal_len: usize,
}

/// One entity, joined with what refers to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Where.
    pub target: Target,
    /// Which.
    pub id: EntityId,
    /// Its values in the sketch's coordinates, mm.
    pub entity: Entity<Model>,
    /// Its layer.
    pub layer: LayerId,
    /// Its provenance.
    pub provenance: Provenance,
    /// Constraints that mention it.
    pub constraints: Vec<ConstraintId>,
    /// Links that mention it (a view's entity only).
    pub links: Vec<LinkId>,
    /// How far it is determined, from the last analysis.
    pub status: EntityStatus,
}

/// One constraint, joined with its standing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConstraintInfo {
    /// Where.
    pub target: Target,
    /// Which.
    pub id: ConstraintId,
    /// The relation.
    pub relation: Relation,
    /// Its values, in the relation's order.
    pub values: Vec<Measure>,
    /// Its provenance.
    pub provenance: Provenance,
    /// Whether it drives the geometry (else it is checked against it).
    pub driving: bool,
    /// The dependency groups it belongs to, by index into
    /// [`Status::groups`].
    pub groups: Vec<usize>,
}

/// A dependency group, in document terms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupInfo {
    /// The members.
    pub members: Vec<GroupMember>,
    /// The largest residual among them, mm.
    pub residual: f64,
    /// Whether it is a conflict (else a redundancy).
    pub conflict: bool,
    /// Members whose removal alone resolves a conflict.
    pub culprits: Vec<GroupMember>,
}

/// A member of a group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroupMember {
    /// A constraint.
    Constraint {
        /// Where.
        target: Target,
        /// Which.
        id: ConstraintId,
    },
    /// A link.
    Link {
        /// Which.
        id: LinkId,
    },
}

/// A `REF` dimension's check against the figure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReferenceInfo {
    /// Where.
    pub target: Target,
    /// Which.
    pub constraint: ConstraintId,
    /// The figure's value less the claimed one, per equation.
    pub off_by: Vec<f64>,
}

/// One view's standing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewStatus {
    /// What the view is free to do on its own.
    pub placement: Placement,
    /// Each entity's status.
    pub entities: BTreeMap<EntityId, EntityStatus>,
}

/// The analysis of the whole document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// The sheet's free parameters beyond placement.
    pub sheet_internal_free: usize,
    /// Each sheet entity's status.
    pub sheet: BTreeMap<EntityId, EntityStatus>,
    /// The views' free parameters beyond their placements.
    pub views_internal_free: usize,
    /// Each view's standing.
    pub views: BTreeMap<ViewId, ViewStatus>,
    /// Redundancies and conflicts, sheet first then the system.
    pub groups: Vec<GroupInfo>,
    /// `REF` checks.
    pub reference: Vec<ReferenceInfo>,
    /// The largest residual anywhere, mm or rad.
    pub residual_max: f64,
}

/// One view, without its sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewInfo {
    /// The model axes.
    pub axes: (ModelAxis, ModelAxis),
    /// The claim to model coordinates, if made.
    pub model_frame: Option<Provenance>,
    /// Where it sits on the paper.
    pub transform: Transform,
    /// Its region, if laid out.
    pub region: Option<Rect<Paper>>,
    /// Entities in it.
    pub entities: usize,
    /// Constraints in it.
    pub constraints: usize,
}

/// The views and the links.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewsInfo {
    /// The views.
    pub views: BTreeMap<ViewId, ViewInfo>,
    /// The links.
    pub links: BTreeMap<LinkId, StoredLink>,
}

/// An answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Answer {
    /// To [`Query::Info`].
    Info(Info),
    /// To [`Query::Item`].
    Item(Item),
    /// To [`Query::Constraint`].
    Constraint(ConstraintInfo),
    /// To [`Query::Status`].
    Status(Status),
    /// To [`Query::Layers`].
    Layers(BTreeMap<LayerId, Layer>),
    /// To [`Query::Palette`].
    Palette(BTreeMap<PaletteId, Appearance>),
    /// To [`Query::Lettering`].
    Lettering(Lettering),
    /// To [`Query::Texts`].
    Texts(BTreeMap<TextId, Text>),
    /// To [`Query::Drafting`].
    Drafting(Drafting),
    /// To [`Query::Leaders`].
    Leaders(BTreeMap<LeaderId, Leader>),
    /// To [`Query::Dimensions`].
    Dimensions(BTreeMap<DimensionId, Dimension>),
    /// To [`Query::Sources`].
    Sources(BTreeMap<DocId, Source>),
    /// To [`Query::Views`].
    Views(ViewsInfo),
    /// To [`Query::Journal`].
    Journal {
        /// The index of the first message returned.
        from: usize,
        /// The messages.
        messages: Vec<Msg>,
    },
    /// To [`Query::Document`]: the `.lmsheet` text.
    Document(String),
}

/// What can go wrong answering.
#[derive(Clone, Debug, PartialEq)]
pub enum QueryError {
    /// No such view.
    NoSuchView(ViewId),
    /// No such entity there.
    NoSuchEntity(Target, EntityId),
    /// No such constraint there.
    NoSuchConstraint(Target, ConstraintId),
    /// The analysis or the file form failed.
    Document(Error),
    /// The file form failed.
    Json(String),
}

impl From<Error> for QueryError {
    fn from(e: Error) -> Self {
        QueryError::Document(e)
    }
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for QueryError {}

fn member(m: &Member) -> GroupMember {
    match m {
        Member::Constraint(v, c) => GroupMember::Constraint {
            target: Target::View { view: *v },
            id: *c,
        },
        Member::Link(l) => GroupMember::Link { id: *l },
    }
}

impl Elm {
    /// Answer a query against the shown document.
    pub fn query(&self, q: &Query) -> Result<Answer, QueryError> {
        let d = self.shown();
        Ok(match q {
            Query::Info => Answer::Info(Info {
                title: d.title().clone(),
                frames: d.frames().map(|(id, f)| (*id, f.clone())).collect(),
                sheet_entities: d.sheet().geometry().len(),
                views: d.views().views().count(),
                links: d.views().links().count(),
                notes: d.notes().len(),
                undo_depth: self.undo_depth(),
                redo_depth: self.redo_depth(),
                previewing: self.preview().is_some(),
                journal_len: self.journal().len(),
            }),
            Query::Item { target, id } => Answer::Item(item(d, *target, *id)?),
            Query::Constraint { target, id } => Answer::Constraint(constraint(d, *target, *id)?),
            Query::Status => Answer::Status(status(d)?),
            Query::Layers => Answer::Layers(d.layers().map(|(k, v)| (*k, v.clone())).collect()),
            Query::Palette => Answer::Palette(d.palette().map(|(k, v)| (*k, *v)).collect()),
            Query::Lettering => Answer::Lettering(d.lettering().clone()),
            Query::Texts => Answer::Texts(d.texts().map(|(k, v)| (*k, v.clone())).collect()),
            Query::Drafting => Answer::Drafting(d.drafting().clone()),
            Query::Leaders => Answer::Leaders(d.leaders().map(|(k, v)| (*k, v.clone())).collect()),
            Query::Dimensions => {
                Answer::Dimensions(d.dimensions().map(|(k, v)| (*k, v.clone())).collect())
            }
            Query::Sources => Answer::Sources(d.sources().map(|(k, v)| (*k, v.clone())).collect()),
            Query::Views => Answer::Views(ViewsInfo {
                views: d
                    .views()
                    .views()
                    .map(|(id, v)| {
                        (
                            *id,
                            ViewInfo {
                                axes: v.axes,
                                model_frame: v.model_frame.clone(),
                                transform: v.transform,
                                region: v.region,
                                entities: v.sketch.geometry().len(),
                                constraints: v.sketch.constraints().count(),
                            },
                        )
                    })
                    .collect(),
                links: d.views().links().map(|(k, v)| (*k, v.clone())).collect(),
            }),
            Query::Journal { from } => {
                let from = (*from).min(self.journal().len());
                Answer::Journal {
                    from,
                    messages: self.journal()[from..].to_vec(),
                }
            }
            Query::Document => {
                Answer::Document(d.to_json().map_err(|e| QueryError::Json(e.to_string()))?)
            }
        })
    }
}

/// The sketch a target names, with its entity values lifted to model
/// coordinates (the sheet's paper coordinates are reported as they are;
/// both are mm).
fn sketch_of(d: &Document, target: Target) -> Result<SketchRef<'_>, QueryError> {
    match target {
        Target::Sheet => Ok(SketchRef::Sheet(d.sheet())),
        Target::View { view } => d
            .views()
            .view(view)
            .map(|v| SketchRef::View(&v.sketch))
            .ok_or(QueryError::NoSuchView(view)),
    }
}

enum SketchRef<'a> {
    Sheet(&'a Sketch<Paper>),
    View(&'a Sketch<Model>),
}

fn item(d: &Document, target: Target, id: EntityId) -> Result<Item, QueryError> {
    let (entity, meta, users) = match sketch_of(d, target)? {
        SketchRef::Sheet(s) => {
            let e = s
                .geometry()
                .get(id)
                .ok_or(QueryError::NoSuchEntity(target, id))?;
            (relabel(e), s.meta(id).cloned(), s.users_of_entity(id))
        }
        SketchRef::View(s) => {
            let e = s
                .geometry()
                .get(id)
                .ok_or(QueryError::NoSuchEntity(target, id))?;
            (e.clone(), s.meta(id).cloned(), s.users_of_entity(id))
        }
    };
    let meta = meta.expect("meta accompanies every entity");
    let constraints = users
        .into_iter()
        .filter_map(|u| match u {
            ItemRef::Constraint(c) => Some(c),
            ItemRef::Entity(_) => None,
        })
        .collect();
    let links = match target {
        Target::Sheet => Vec::new(),
        Target::View { view } => d
            .views()
            .links()
            .filter(|(_, l)| {
                l.link()
                    .points()
                    .iter()
                    .any(|p| p.view == view && p.point.id() == id)
            })
            .map(|(lid, _)| *lid)
            .collect(),
    };
    let status = status(d)?;
    let st = match target {
        Target::Sheet => status.sheet.get(&id).cloned(),
        Target::View { view } => status
            .views
            .get(&view)
            .and_then(|v| v.entities.get(&id).cloned()),
    };
    Ok(Item {
        target,
        id,
        entity,
        layer: meta.layer,
        provenance: meta.provenance,
        constraints,
        links,
        status: st.unwrap_or(EntityStatus::Free),
    })
}

/// A paper entity's values as a model entity: the same millimetres, for
/// one answer type.
fn relabel(e: &Entity<Paper>) -> Entity<Model> {
    let p = |q: P2<Paper>| P2::<Model>::new(q.x, q.y);
    match *e {
        Entity::Point { at } => Entity::Point { at: p(at) },
        Entity::Line { start, end } => Entity::Line {
            start: p(start),
            end: p(end),
        },
        Entity::Arc {
            centre,
            radius,
            start_angle,
            sweep,
        } => Entity::Arc {
            centre: p(centre),
            radius,
            start_angle,
            sweep,
        },
        Entity::Circle { centre, radius } => Entity::Circle {
            centre: p(centre),
            radius,
        },
        Entity::Spline { ref points } => Entity::Spline {
            points: points.iter().map(|q| p(*q)).collect(),
        },
    }
}

fn constraint(
    d: &Document,
    target: Target,
    id: ConstraintId,
) -> Result<ConstraintInfo, QueryError> {
    let c = match sketch_of(d, target)? {
        SketchRef::Sheet(s) => s.constraint(id).cloned(),
        SketchRef::View(s) => s.constraint(id).cloned(),
    }
    .ok_or(QueryError::NoSuchConstraint(target, id))?;
    let status = status(d)?;
    let me = GroupMember::Constraint { target, id };
    let groups = status
        .groups
        .iter()
        .enumerate()
        .filter(|(_, g)| g.members.contains(&me))
        .map(|(i, _)| i)
        .collect();
    Ok(ConstraintInfo {
        target,
        id,
        relation: c.relation().clone(),
        values: c.relation().values().into_iter().cloned().collect(),
        provenance: c.provenance().clone(),
        driving: c.relation().driving(),
        groups,
    })
}

fn status(d: &Document) -> Result<Status, QueryError> {
    let (sheet, system) = d.analyse(Options::default())?;
    let mut groups: Vec<GroupInfo> = sheet
        .groups
        .iter()
        .map(|g| GroupInfo {
            members: g
                .constraints
                .iter()
                .map(|c| GroupMember::Constraint {
                    target: Target::Sheet,
                    id: *c,
                })
                .collect(),
            residual: g.residual,
            conflict: !g.culprits.is_empty() || g.residual > crate::value::sketch::CONFLICT,
            culprits: g
                .culprits
                .iter()
                .map(|c| GroupMember::Constraint {
                    target: Target::Sheet,
                    id: *c,
                })
                .collect(),
        })
        .collect();
    groups.extend(system.groups.iter().map(|g| GroupInfo {
        members: g.members.iter().map(member).collect(),
        residual: g.residual,
        conflict: g.residual > crate::value::sketch::CONFLICT,
        culprits: g.culprits.iter().map(member).collect(),
    }));
    let mut reference: Vec<ReferenceInfo> = sheet
        .reference
        .iter()
        .map(|r| ReferenceInfo {
            target: Target::Sheet,
            constraint: r.constraint,
            off_by: r.off_by.clone(),
        })
        .collect();
    reference.extend(system.reference.iter().map(|r| ReferenceInfo {
        target: Target::View { view: r.view },
        constraint: r.constraint,
        off_by: r.off_by.clone(),
    }));
    Ok(Status {
        sheet_internal_free: sheet.internal_free,
        sheet: sheet.status.clone(),
        views_internal_free: system.internal_free,
        views: system
            .views
            .iter()
            .map(|(id, v)| {
                (
                    *id,
                    ViewStatus {
                        placement: v.placement,
                        entities: v.status.clone(),
                    },
                )
            })
            .collect(),
        groups,
        reference,
        residual_max: sheet.report.residual_max.max(system.residual_max),
    })
}

/// Parameter names appear in answers through [`Measure::Param`].
#[allow(dead_code)]
fn _param_names_are_measures(_: ParamName) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::msg::{Op, SketchOp};
    use crate::state::document::{SourceKind, Title};
    use crate::state::views::{aligned, in_view};
    use crate::value::provenance::{Agent, Date, Method};
    use crate::value::relation::{horizontal, length};
    use crate::value::view::{ModelAxis, Transform, View};

    fn ours() -> Provenance {
        Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 15).unwrap(),
        )
    }

    #[test]
    fn queries_join_and_serialise() {
        let line = Entity::Line {
            start: P2::<Model>::new(0.0, 0.0),
            end: P2::new(10.0, 0.0),
        };
        let view = |axes| View::new(axes, Sketch::new(), Transform::at(0.0, 0.0)).unwrap();
        let mut m = Elm::new(Document::new(Title::default()));
        let ops = vec![
            Op::add_source("sheet", Source::new(SourceKind::Sheet, "this")),
            Op::add_view("top", view((ModelAxis::X, ModelAxis::Y))),
            Op::add_view("front", view((ModelAxis::X, ModelAxis::Z))),
            SketchOp::add_entity("a", line.clone(), "visible", ours()).in_view("top"),
            SketchOp::add_entity("b", line, "visible", ours()).in_view("front"),
            SketchOp::add_constraint(
                "w",
                length(
                    crate::solver::equation::line("a"),
                    Measure::mm(10.0).unwrap(),
                ),
                ours(),
            )
            .in_view("top"),
            SketchOp::add_constraint("h", horizontal(crate::solver::equation::line("a")), ours())
                .in_view("top"),
            Op::add_link(
                "x0",
                aligned(
                    in_view("front", crate::solver::equation::line("b").start()),
                    in_view("top", crate::solver::equation::line("a").start()),
                    ModelAxis::X,
                ),
                ours(),
            ),
        ];
        m.update(Msg::Batch { ops }).unwrap();
        let Answer::Info(info) = m.query(&Query::Info).unwrap() else {
            panic!()
        };
        assert_eq!(
            (info.views, info.links, info.undo_depth, info.journal_len),
            (2, 1, 1, 1)
        );
        let Answer::Item(item) = m
            .query(&Query::Item {
                target: Target::View { view: "top".into() },
                id: "a".into(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(item.constraints, vec!["h".into(), "w".into()]);
        assert_eq!(item.links, vec!["x0".into()]);
        assert_eq!(item.layer, "visible".into());
        assert!(matches!(item.status, EntityStatus::FixedUpToPlacement));
        let Answer::Constraint(c) = m
            .query(&Query::Constraint {
                target: Target::View { view: "top".into() },
                id: "w".into(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(c.driving && c.groups.is_empty());
        assert_eq!(c.values, vec![Measure::mm(10.0).unwrap()]);
        let Answer::Status(st) = m.query(&Query::Status).unwrap() else {
            panic!()
        };
        assert!(st.groups.is_empty());
        assert!(st.views[&"top".into()].placement.x);
        // Errors name the target.
        assert_eq!(
            m.query(&Query::Item {
                target: Target::View {
                    view: "nope".into()
                },
                id: "a".into(),
            })
            .err(),
            Some(QueryError::NoSuchView("nope".into()))
        );
        // Everything on the wire round-trips.
        for q in [
            Query::Info,
            Query::Status,
            Query::Views,
            Query::Layers,
            Query::Palette,
            Query::Sources,
            Query::Lettering,
            Query::Texts,
            Query::Drafting,
            Query::Leaders,
            Query::Dimensions,
        ] {
            let a = m.query(&q).unwrap();
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Answer>(&json).unwrap(), a);
        }
        let Answer::Document(text) = m.query(&Query::Document).unwrap() else {
            panic!()
        };
        assert_eq!(&Document::parse(&text).unwrap(), m.doc());
        let Answer::Journal { from, messages } = m.query(&Query::Journal { from: 0 }).unwrap()
        else {
            panic!()
        };
        assert_eq!((from, messages.len()), (0, 1));
    }
}
