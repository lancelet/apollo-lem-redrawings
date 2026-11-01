//! Every edit to a document as a message.
//!
//! An [`Op`] names one change: to a table of the document, to the sheet
//! sketch, to a view's sketch, to a view, or to a link. [`Document::apply`]
//! takes a document and an op and gives the changed document or an error,
//! leaving the original as it was; the document is a value. The UI builds
//! ops, the API carries them, the journal stores them, and nothing else
//! changes a document. An op is serialisable, so a journal replays.
//!
//! Sketch ops are generic over the frame: a [`SketchOp<Paper>`] can only be
//! aimed at the sheet and a [`SketchOp<Model>`] only at a view, so an
//! entity cannot land in the wrong coordinates.

use crate::state::document::{Document, Error, Note, Source, Title};
use crate::state::views::{Link, LinkId};
use crate::value::centre::{CentreLine, CentreLineId, CentreMark, CentreMarkId};
use crate::value::dimension::{Dimension, DimensionId};
use crate::value::drafting::Drafting;
use crate::value::frame::{self, FrameId};
use crate::value::geometry::{Entity, EntityId, Frame, Model, Paper, Rect};
use crate::value::leader::{Leader, LeaderId};
use crate::value::lettering::{Role, RoleId};
use crate::value::measure::{Measure, ParamName, Tolerance};
use crate::value::params::{Param, Params};
use crate::value::provenance::{Check, ConstraintId, DocId, ItemRef, Provenance};
use crate::value::relation::Relation;
use crate::value::sketch::{EntityMeta, LayerId, Sketch};
use crate::value::style::{Appearance, Layer, PaletteId, Visibility};
use crate::value::text::{Text, TextId};
use crate::value::unmodelled::{Unmodelled, UnmodelledId};
use crate::value::view::{Transform, View, ViewId};
use crate::value::weights::{LineKind, Measured};
use serde::{Deserialize, Serialize};

/// A change to one sketch, the sheet's or a view's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "", tag = "edit", rename_all = "snake_case")]
pub enum SketchOp<F: Frame> {
    /// Add an entity with its layer and provenance.
    AddEntity {
        /// The id.
        id: EntityId,
        /// The values.
        entity: Entity<F>,
        /// Layer and provenance.
        meta: EntityMeta,
    },
    /// Replace an entity's values (not its kind).
    SetEntity {
        /// The id.
        id: EntityId,
        /// The values.
        entity: Entity<F>,
    },
    /// Remove an entity that nothing refers to.
    RemoveEntity {
        /// The id.
        id: EntityId,
    },
    /// Replace an entity's provenance: how its values came to be (the
    /// ink fit, or the solve that moved it).
    SetEntityProvenance {
        /// The id.
        id: EntityId,
        /// The provenance.
        provenance: Provenance,
    },
    /// Move an entity to a layer.
    SetEntityLayer {
        /// The id.
        id: EntityId,
        /// The layer.
        layer: LayerId,
    },
    /// Add a constraint.
    AddConstraint {
        /// The id.
        id: ConstraintId,
        /// The relation.
        relation: Relation,
        /// The provenance.
        provenance: Provenance,
    },
    /// Set a constraint's values.
    SetValues {
        /// The id.
        id: ConstraintId,
        /// The values, in the relation's order.
        values: Vec<Measure>,
    },
    /// Set a dimension's tolerance.
    SetTolerance {
        /// The id.
        id: ConstraintId,
        /// The tolerance.
        tolerance: Tolerance,
    },
    /// Remove a constraint that nothing cites.
    RemoveConstraint {
        /// The id.
        id: ConstraintId,
    },
    /// Accept a provisional claim.
    Accept {
        /// The item.
        item: ItemRef,
    },
    /// Verify a claim with a check.
    Verify {
        /// The item.
        item: ItemRef,
        /// The check.
        check: Check,
    },
    /// Dispute a claim against others.
    Dispute {
        /// The item.
        item: ItemRef,
        /// What it conflicts with.
        against: Vec<ItemRef>,
    },
    /// Supersede a claim by another.
    Supersede {
        /// The item.
        item: ItemRef,
        /// Its replacement.
        by: ItemRef,
    },
}

impl<F: Frame> SketchOp<F> {
    /// Add an entity on a layer with its provenance.
    pub fn add_entity(
        id: impl Into<EntityId>,
        entity: Entity<F>,
        layer: impl Into<LayerId>,
        provenance: Provenance,
    ) -> Self {
        SketchOp::AddEntity {
            id: id.into(),
            entity,
            meta: EntityMeta::new(layer, provenance),
        }
    }

    /// Replace an entity's values.
    pub fn set_entity(id: impl Into<EntityId>, entity: Entity<F>) -> Self {
        SketchOp::SetEntity {
            id: id.into(),
            entity,
        }
    }

    /// Remove an entity.
    pub fn remove_entity(id: impl Into<EntityId>) -> Self {
        SketchOp::RemoveEntity { id: id.into() }
    }

    /// Add a constraint.
    pub fn add_constraint(
        id: impl Into<ConstraintId>,
        relation: Relation,
        provenance: Provenance,
    ) -> Self {
        SketchOp::AddConstraint {
            id: id.into(),
            relation,
            provenance,
        }
    }

    /// Remove a constraint.
    pub fn remove_constraint(id: impl Into<ConstraintId>) -> Self {
        SketchOp::RemoveConstraint { id: id.into() }
    }

    /// Replace a constraint's values.
    pub fn set_values(id: impl Into<ConstraintId>, values: Vec<Measure>) -> Self {
        SketchOp::SetValues {
            id: id.into(),
            values,
        }
    }

    /// Replace a dimension's tolerance.
    pub fn set_tolerance(id: impl Into<ConstraintId>, tolerance: Tolerance) -> Self {
        SketchOp::SetTolerance {
            id: id.into(),
            tolerance,
        }
    }

    /// This change, aimed at a view's sketch.
    pub fn in_view(self, view: impl Into<ViewId>) -> Op
    where
        Self: IntoViewOp,
    {
        self.into_view_op(view.into())
    }

    /// This change, aimed at the sheet.
    pub fn on_sheet(self) -> Op
    where
        Self: IntoSheetOp,
    {
        self.into_sheet_op()
    }

    /// Apply to a sketch, in the document's parameter table.
    fn apply(&self, s: &mut Sketch<F>, params: &Params) -> Result<(), crate::value::sketch::Error> {
        use SketchOp::*;
        match self.clone() {
            AddEntity { id, entity, meta } => s.add_entity(id, entity, meta),
            SetEntity { id, entity } => s.set_entity(id, entity).map(|_| ()),
            RemoveEntity { id } => s.remove_entity(id).map(|_| ()),
            SetEntityLayer { id, layer } => s.set_layer(id, layer).map(|_| ()),
            SetEntityProvenance { id, provenance } => {
                s.set_entity_provenance(id, provenance).map(|_| ())
            }
            AddConstraint {
                id,
                relation,
                provenance,
            } => s.add_constraint(id, relation, provenance, params),
            SetValues { id, values } => s.set_values(id, values, params).map(|_| ()),
            SetTolerance { id, tolerance } => s.set_tolerance(id, tolerance, params).map(|_| ()),
            RemoveConstraint { id } => s.remove_constraint(id).map(|_| ()),
            Accept { item } => s.accept(item),
            Verify { item, check } => s.verify(item, check),
            Dispute { item, against } => s.dispute(item, against),
            Supersede { item, by } => s.supersede(item, by),
        }
    }
}

/// A sketch op that can be aimed at a view (one in model coordinates).
pub trait IntoViewOp {
    /// Aim at the view.
    fn into_view_op(self, view: ViewId) -> Op;
}

impl IntoViewOp for SketchOp<Model> {
    fn into_view_op(self, view: ViewId) -> Op {
        Op::View { view, edit: self }
    }
}

/// A sketch op that can be aimed at the sheet (one in paper coordinates).
pub trait IntoSheetOp {
    /// Aim at the sheet.
    fn into_sheet_op(self) -> Op;
}

impl IntoSheetOp for SketchOp<Paper> {
    fn into_sheet_op(self) -> Op {
        Op::Sheet { edit: self }
    }
}

/// A change to a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Set the title block.
    SetTitle {
        /// The title.
        title: Title,
    },
    /// Set a frame, or remove it.
    SetFrame {
        /// The id.
        id: FrameId,
        /// The frame, or none.
        frame: Option<Box<frame::Frame>>,
    },
    /// Set the claims on the frames' ink, all of them.
    SetClaims {
        /// The claims.
        claims: Vec<crate::value::claims::Claim>,
    },
    /// Set the observed features (docs/redraw-plan.md §8.3), all of them.
    SetObserved {
        /// The observed features, by id.
        observed: std::collections::BTreeMap<
            crate::value::observed::ObservedId,
            crate::value::observed::Observed,
        >,
    },
    /// Set the marking regions (docs/redraw-plan.md K5), all of them.
    SetMarkingRegions {
        /// The marking regions, by id.
        regions: std::collections::BTreeMap<
            crate::value::marking::MarkingRegionId,
            crate::value::marking::MarkingRegion,
        >,
    },
    /// Place the printed form, with its fields, or remove it.
    SetForm {
        /// The form; none removes it.
        form: Option<Box<crate::value::form::Form>>,
    },
    /// Set the sheet's gaps, all of them.
    SetGaps {
        /// The gaps, by id.
        gaps: std::collections::BTreeMap<crate::value::gaps::GapId, crate::value::gaps::Gap>,
    },
    /// Set the solve's residuals, or remove them.
    SetResiduals {
        /// The residuals; none removes them.
        residuals: Option<Box<crate::value::residuals::Residuals>>,
    },
    /// Add a layer.
    AddLayer {
        /// The id.
        id: LayerId,
        /// The row.
        layer: Layer,
    },
    /// Change a layer.
    SetLayer {
        /// The id.
        id: LayerId,
        /// The row.
        layer: Layer,
    },
    /// Set where a layer is visible.
    SetLayerVisibility {
        /// The id.
        id: LayerId,
        /// Where.
        visible: Visibility,
    },
    /// Remove an unused layer.
    RemoveLayer {
        /// The id.
        id: LayerId,
    },
    /// Add a palette entry.
    AddPaletteEntry {
        /// The id.
        id: PaletteId,
        /// What it does.
        appearance: Appearance,
    },
    /// Change a palette entry.
    SetPaletteEntry {
        /// The id.
        id: PaletteId,
        /// What it does.
        appearance: Appearance,
    },
    /// Remove a palette entry no layer uses.
    RemovePaletteEntry {
        /// The id.
        id: PaletteId,
    },
    /// Add a source.
    AddSource {
        /// The id.
        id: DocId,
        /// The row.
        source: Source,
    },
    /// Change a source.
    SetSource {
        /// The id.
        id: DocId,
        /// The row.
        source: Source,
    },
    /// Remove an uncited source.
    RemoveSource {
        /// The id.
        id: DocId,
    },
    /// Add or replace a lettering role.
    SetRole {
        /// The id.
        id: RoleId,
        /// The role.
        role: Role,
    },
    /// Set the width factor the lettering table's roles share.
    SetWidthFactor {
        /// The factor, finite and positive.
        width_factor: f64,
    },
    /// Set the line pitch of the lettering, a share of the cap height.
    SetPitch {
        /// The pitch, at least one cap.
        pitch_of_cap: f64,
    },
    /// Set the sheet's one fraction digit size, which every stacked
    /// fraction takes whatever its text's role.
    SetFraction {
        /// The rows' cap height, paper mm, finite and positive.
        cap: f64,
        /// Each row's distance from the line's middle, a share of that
        /// cap, finite and not negative.
        gap_of_cap: f64,
    },
    /// Set, or with `None` clear, the measured pen of a kind of line.
    SetLineWeight {
        /// The kind.
        kind: LineKind,
        /// The pen with its evidence.
        weight: Option<Measured>,
    },
    /// Remove a lettering role no text uses.
    RemoveRole {
        /// The id.
        id: RoleId,
    },
    /// Add a text.
    AddText {
        /// The id.
        id: TextId,
        /// The text.
        text: Text,
    },
    /// Replace a text.
    SetText {
        /// The id.
        id: TextId,
        /// The text.
        text: Text,
    },
    /// Remove a text no other is aligned to and no leader touches.
    RemoveText {
        /// The id.
        id: TextId,
    },
    /// Replace the drafting table.
    SetDrafting {
        /// The table.
        drafting: Drafting,
    },
    /// Add a leader.
    AddLeader {
        /// The id.
        id: LeaderId,
        /// The leader.
        leader: Leader,
    },
    /// Replace a leader.
    SetLeader {
        /// The id.
        id: LeaderId,
        /// The leader.
        leader: Leader,
    },
    /// Remove a leader.
    RemoveLeader {
        /// The id.
        id: LeaderId,
    },
    /// Add a centre mark.
    AddCentreMark {
        /// The id.
        id: CentreMarkId,
        /// The mark.
        mark: CentreMark,
    },
    /// Replace a centre mark.
    SetCentreMark {
        /// The id.
        id: CentreMarkId,
        /// The mark.
        mark: CentreMark,
    },
    /// Remove a centre mark.
    RemoveCentreMark {
        /// The id.
        id: CentreMarkId,
    },
    /// Add a centre line.
    AddCentreLine {
        /// The id.
        id: CentreLineId,
        /// The line.
        line: CentreLine,
    },
    /// Replace a centre line.
    SetCentreLine {
        /// The id.
        id: CentreLineId,
        /// The line.
        line: CentreLine,
    },
    /// Remove a centre line.
    RemoveCentreLine {
        /// The id.
        id: CentreLineId,
    },
    /// Add an unmodelled mark.
    AddUnmodelled {
        /// The id.
        id: UnmodelledId,
        /// The mark.
        mark: Unmodelled,
    },
    /// Replace an unmodelled mark.
    SetUnmodelled {
        /// The id.
        id: UnmodelledId,
        /// The mark.
        mark: Unmodelled,
    },
    /// Remove an unmodelled mark.
    RemoveUnmodelled {
        /// The id.
        id: UnmodelledId,
    },
    /// Add a dimension.
    AddDimension {
        /// The id.
        id: DimensionId,
        /// The dimension.
        dimension: Dimension,
    },
    /// Replace a dimension.
    SetDimension {
        /// The id.
        id: DimensionId,
        /// The dimension.
        dimension: Dimension,
    },
    /// Remove a dimension.
    RemoveDimension {
        /// The id.
        id: DimensionId,
    },
    /// Add a note.
    AddNote {
        /// The note.
        note: Note,
    },
    /// Remove a note by position.
    RemoveNote {
        /// The position.
        index: usize,
    },
    /// Set the sheet's reading as an engineering drawing, or take it
    /// away.
    SetPartReading {
        /// The reading.
        reading: Option<crate::value::part::PartReading>,
    },
    /// Set or add a parameter of the document's table. A replacement
    /// must still fit every constraint, in any sketch, that names it.
    SetParam {
        /// The name.
        name: ParamName,
        /// The value and provenance.
        param: Param,
    },
    /// Remove a parameter that no constraint names.
    RemoveParam {
        /// The name.
        name: ParamName,
    },
    /// A change to the sheet sketch.
    Sheet {
        /// The change.
        #[serde(flatten)]
        edit: SketchOp<Paper>,
    },
    /// A change to a view's sketch.
    View {
        /// The view.
        view: ViewId,
        /// The change.
        #[serde(flatten)]
        edit: SketchOp<Model>,
    },
    /// Add a view.
    AddView {
        /// The id.
        id: ViewId,
        /// The view.
        view: View,
    },
    /// Remove a view no link uses.
    RemoveView {
        /// The id.
        id: ViewId,
    },
    /// Place a view on the paper.
    SetTransform {
        /// The view.
        view: ViewId,
        /// Its transform.
        transform: Transform,
    },
    /// Set or clear a view's region.
    SetRegion {
        /// The view.
        view: ViewId,
        /// Its region, or none.
        region: Option<Rect<Paper>>,
    },
    /// Make or withdraw a view's claim to model coordinates.
    SetModelFrame {
        /// The view.
        view: ViewId,
        /// The claim's provenance, or none to withdraw it.
        provenance: Option<Provenance>,
    },
    /// Add a link.
    AddLink {
        /// The id.
        id: LinkId,
        /// The link.
        link: Link,
        /// The provenance.
        provenance: Provenance,
    },
    /// Remove a link.
    RemoveLink {
        /// The id.
        id: LinkId,
    },
    /// Solve the sheet and the views as a system with default options,
    /// moving geometry to the solution.
    Solve,
}

impl Op {
    /// Add a view.
    pub fn add_view(id: impl Into<ViewId>, view: View) -> Self {
        Op::AddView {
            id: id.into(),
            view,
        }
    }

    /// Add a link.
    pub fn add_link(id: impl Into<LinkId>, link: Link, provenance: Provenance) -> Self {
        Op::AddLink {
            id: id.into(),
            link,
            provenance,
        }
    }

    /// Add a source.
    pub fn add_source(id: impl Into<DocId>, source: Source) -> Self {
        Op::AddSource {
            id: id.into(),
            source,
        }
    }

    /// Add a layer.
    pub fn add_layer(id: impl Into<LayerId>, layer: Layer) -> Self {
        Op::AddLayer {
            id: id.into(),
            layer,
        }
    }

    /// Add a text.
    pub fn add_text(id: impl Into<TextId>, text: Text) -> Self {
        Op::AddText {
            id: id.into(),
            text,
        }
    }

    /// Add a leader.
    pub fn add_leader(id: impl Into<LeaderId>, leader: Leader) -> Self {
        Op::AddLeader {
            id: id.into(),
            leader,
        }
    }

    /// Add a centre mark.
    pub fn add_centre_mark(id: impl Into<CentreMarkId>, mark: CentreMark) -> Self {
        Op::AddCentreMark {
            id: id.into(),
            mark,
        }
    }

    /// Add a centre line.
    pub fn add_centre_line(id: impl Into<CentreLineId>, line: CentreLine) -> Self {
        Op::AddCentreLine {
            id: id.into(),
            line,
        }
    }

    /// Add an unmodelled mark.
    pub fn add_unmodelled(id: impl Into<UnmodelledId>, mark: Unmodelled) -> Self {
        Op::AddUnmodelled {
            id: id.into(),
            mark,
        }
    }

    /// Add a dimension.
    pub fn add_dimension(id: impl Into<DimensionId>, dimension: Dimension) -> Self {
        Op::AddDimension {
            id: id.into(),
            dimension,
        }
    }

    /// Replace a dimension.
    pub fn set_dimension(id: impl Into<DimensionId>, dimension: Dimension) -> Self {
        Op::SetDimension {
            id: id.into(),
            dimension,
        }
    }

    /// Remove a dimension.
    pub fn remove_dimension(id: impl Into<DimensionId>) -> Self {
        Op::RemoveDimension { id: id.into() }
    }

    /// Set or add a parameter.
    pub fn set_param(name: impl Into<ParamName>, param: Param) -> Self {
        Op::SetParam {
            name: name.into(),
            param,
        }
    }

    /// Remove a parameter.
    pub fn remove_param(name: impl Into<ParamName>) -> Self {
        Op::RemoveParam { name: name.into() }
    }

    /// Add a palette entry.
    pub fn add_palette_entry(id: impl Into<PaletteId>, appearance: Appearance) -> Self {
        Op::AddPaletteEntry {
            id: id.into(),
            appearance,
        }
    }

    /// Lay a view out on the paper.
    pub fn set_region(view: impl Into<ViewId>, region: Rect<Paper>) -> Self {
        Op::SetRegion {
            view: view.into(),
            region: Some(region),
        }
    }
}

impl Document {
    /// The document with `op` applied, or the error; `self` is untouched.
    pub fn apply(&self, op: &Op) -> Result<Document, Error> {
        let mut d = self.clone();
        d.apply_in_place(op)?;
        Ok(d)
    }

    /// Apply `op` to this document. On an error the document is as it was.
    pub fn apply_in_place(&mut self, op: &Op) -> Result<(), Error> {
        use Op::*;
        match op.clone() {
            SetTitle { title } => {
                self.set_title(title);
                Ok(())
            }
            SetFrame { id, frame } => self.set_frame(id, frame.map(|f| *f)),
            SetClaims { claims } => self.set_claims(claims),
            SetGaps { gaps } => self.set_gaps(gaps),
            SetResiduals { residuals } => self.set_residuals(residuals.map(|r| *r)),
            SetObserved { observed } => self.set_observed(observed),
            SetMarkingRegions { regions } => self.set_marking_regions(regions),
            SetForm { form } => self.set_form(form.map(|f| *f)),
            AddLayer { id, layer } => self.add_layer(id, layer),
            SetLayer { id, layer } => self.set_layer(id, layer).map(|_| ()),
            SetLayerVisibility { id, visible } => {
                let mut layer = self.layer(id).cloned().ok_or(Error::NoSuchLayer(id))?;
                layer.visible = visible;
                self.set_layer(id, layer).map(|_| ())
            }
            RemoveLayer { id } => self.remove_layer(id).map(|_| ()),
            AddPaletteEntry { id, appearance } => self.add_palette(id, appearance),
            SetPaletteEntry { id, appearance } => self.set_palette(id, appearance).map(|_| ()),
            RemovePaletteEntry { id } => self.remove_palette(id).map(|_| ()),
            AddSource { id, source } => self.add_source(id, source),
            SetSource { id, source } => self.set_source(id, source).map(|_| ()),
            RemoveSource { id } => self.remove_source(id).map(|_| ()),
            SetRole { id, role } => self.set_role(id, role).map(|_| ()),
            SetWidthFactor { width_factor } => self.set_width_factor(width_factor),
            SetPitch { pitch_of_cap } => self.set_pitch_of_cap(pitch_of_cap),
            SetFraction { cap, gap_of_cap } => self.set_fraction(cap, gap_of_cap),
            SetLineWeight { kind, weight } => self.set_line_weight(kind, weight),
            RemoveRole { id } => self.remove_role(id).map(|_| ()),
            AddText { id, text } => self.add_text(id, text),
            SetText { id, text } => self.set_text(id, text).map(|_| ()),
            RemoveText { id } => self.remove_text(id).map(|_| ()),
            SetDrafting { drafting } => self.set_drafting(drafting).map(|_| ()),
            AddLeader { id, leader } => self.add_leader(id, leader),
            SetLeader { id, leader } => self.set_leader(id, leader).map(|_| ()),
            RemoveLeader { id } => self.remove_leader(id).map(|_| ()),
            AddCentreMark { id, mark } => self.add_centre_mark(id, mark),
            SetCentreMark { id, mark } => self.set_centre_mark(id, mark).map(|_| ()),
            RemoveCentreMark { id } => self.remove_centre_mark(id).map(|_| ()),
            AddCentreLine { id, line } => self.add_centre_line(id, line),
            SetCentreLine { id, line } => self.set_centre_line(id, line).map(|_| ()),
            RemoveCentreLine { id } => self.remove_centre_line(id).map(|_| ()),
            AddUnmodelled { id, mark } => self.add_unmodelled(id, mark),
            SetUnmodelled { id, mark } => self.set_unmodelled(id, mark).map(|_| ()),
            RemoveUnmodelled { id } => self.remove_unmodelled(id).map(|_| ()),
            AddDimension { id, dimension } => self.add_dimension(id, dimension),
            SetDimension { id, dimension } => self.set_dimension(id, dimension).map(|_| ()),
            RemoveDimension { id } => self.remove_dimension(id).map(|_| ()),
            AddNote { note } => self.add_note(note),
            RemoveNote { index } => self
                .remove_note(index)
                .map(|_| ())
                .ok_or(Error::NoSuchNote(index)),
            SetPartReading { reading } => self.set_part_reading(reading).map(|_| ()),
            SetParam { name, param } => self.set_param(name, param).map(|_| ()),
            RemoveParam { name } => self.remove_param(name).map(|_| ()),
            Sheet { edit } => self.edit_sheet(|s, p| edit.apply(s, p)),
            View { view, edit } => self.edit_view(view, |s, p| edit.apply(s, p)),
            AddView { id, view } => self.edit_views(|v| v.add_view(id, view)),
            RemoveView { id } => self.edit_views(|v| v.remove_view(id).map(|_| ())),
            SetTransform { view, transform } => self.edit_views(|v| {
                v.edit_view(view, |w| {
                    w.transform = transform;
                    Ok(())
                })
            }),
            SetRegion { view, region } => self.edit_views(|v| {
                v.edit_view(view, |w| {
                    w.region = region;
                    Ok(())
                })
            }),
            SetModelFrame { view, provenance } => self.edit_views(|v| {
                v.edit_view(view, |w| {
                    w.model_frame = provenance;
                    Ok(())
                })
            }),
            AddLink {
                id,
                link,
                provenance,
            } => self.edit_views(|v| v.add_link(id, link, provenance)),
            RemoveLink { id } => self.edit_views(|v| v.remove_link(id).map(|_| ())),
            Solve => self.solve(Default::default()).map(|_| ()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::document::SourceKind;
    use crate::value::geometry::P2;
    use crate::value::provenance::{Agent, Date, Method};
    use crate::value::view::ModelAxis;

    fn ours() -> Provenance {
        Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 15).unwrap(),
        )
    }

    fn line<F: Frame>(x: f64) -> Entity<F> {
        Entity::Line {
            start: P2::new(x, 0.0),
            end: P2::new(x + 10.0, 0.0),
        }
    }

    #[test]
    fn ops_apply_as_values_and_errors_leave_the_original() {
        let d0 = Document::new(Title::default());
        let view = View::new(
            (ModelAxis::X, ModelAxis::Y),
            Sketch::new(),
            Transform::at(50.0, 50.0),
        )
        .unwrap();
        let ops = [
            Op::AddSource {
                id: "sheet".into(),
                source: Source::new(SourceKind::Sheet, "this"),
            },
            Op::Sheet {
                edit: SketchOp::AddEntity {
                    id: "border".into(),
                    entity: line(0.0),
                    meta: EntityMeta::new("format", ours()),
                },
            },
            Op::AddView {
                id: "front".into(),
                view,
            },
            Op::View {
                view: "front".into(),
                edit: SketchOp::AddEntity {
                    id: "a".into(),
                    entity: line(0.0),
                    meta: EntityMeta::new("visible", ours()),
                },
            },
            Op::SetRegion {
                view: "front".into(),
                region: Some(Rect::new(P2::new(40.0, 40.0), P2::new(80.0, 60.0)).unwrap()),
            },
            Op::Solve,
        ];
        let mut d = d0.clone();
        for op in &ops {
            d = d.apply(op).unwrap();
        }
        assert_eq!(d0, Document::new(Title::default()));
        assert_eq!(d.sheet().geometry().len(), 1);
        let front = d.views().view("front").unwrap();
        assert_eq!(front.sketch.geometry().len(), 1);
        assert!(front.region.is_some());
        // A failing op returns the error and the document is unchanged.
        let before = d.clone();
        for bad in [
            Op::RemoveLayer {
                id: "visible".into(),
            },
            Op::View {
                view: "front".into(),
                edit: SketchOp::SetEntityLayer {
                    id: "a".into(),
                    layer: "nope".into(),
                },
            },
            Op::RemoveView {
                id: "nowhere".into(),
            },
            Op::RemoveNote { index: 3 },
            Op::SetTransform {
                view: "front".into(),
                transform: Transform::at(f64::NAN, 0.0),
            },
        ] {
            assert!(d.apply(&bad).is_err(), "{bad:?}");
            let mut m = d.clone();
            assert!(m.apply_in_place(&bad).is_err());
            assert_eq!(m, before, "{bad:?}");
        }
        // Ops are serialisable and the sheet/view frames are told apart.
        let json = serde_json::to_string(&ops[1]).unwrap();
        assert!(json.contains("\"op\":\"sheet\""), "{json}");
        assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), ops[1]);
        let json = serde_json::to_string(&ops[3]).unwrap();
        assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), ops[3]);
    }

    #[test]
    fn balloon_ops_apply_and_journal() {
        use crate::value::leader::{Leader, Target, Tip};
        use crate::value::text::{Side, Text};
        let d = Document::new(Title::default());
        let add_text = Op::add_text(
            "b",
            Text::balloon("-15", P2::new(40.0, 30.0), ours()).unwrap(),
        );
        let add_leader = Op::add_leader(
            "bl",
            Leader::balloon(
                "b",
                Side::Right,
                Tip::arrow(Target::Paper {
                    point: P2::new(80.0, 60.0),
                }),
                ours(),
            )
            .unwrap(),
        );
        // The leader needs its balloon first; both ops journal.
        assert!(d.apply(&add_leader).is_err());
        let d = d.apply(&add_text).unwrap().apply(&add_leader).unwrap();
        assert!(d.text("b").unwrap().enclosure().is_some());
        for op in [&add_text, &add_leader] {
            let json = serde_json::to_string(op).unwrap();
            assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), *op, "{json}");
        }
        let json = serde_json::to_string(&add_text).unwrap();
        assert!(json.contains("\"enclosure\":\"circle\""), "{json}");
        let json = serde_json::to_string(&add_leader).unwrap();
        assert!(json.contains("\"from\":\"enclosure\""), "{json}");
        // Taking the enclosure away under the leader is refused through
        // the same op path.
        let plain = Text::balloon("-15", P2::new(40.0, 30.0), ours())
            .unwrap()
            .unenclosed();
        let set = Op::SetText {
            id: "b".into(),
            text: plain,
        };
        assert!(d.apply(&set).is_err());
    }
}
