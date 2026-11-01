//! The solve's residuals, stored in the document so a viewer reads them
//! instead of recomputing (docs/redraw-plan.md §2, chunk 22c).
//!
//! When a view's design is solved against the ink it was drawn from, the
//! solve says how far each read dimension, each relation of the design
//! and each entity is from the ink, and how hard each constraint pulls
//! the design off it. This section keeps that, keyed by the document's
//! own ids: the dimensions by [`DimensionId`], the relations and the
//! pulling constraints by [`ConstraintId`] of the view, the entities by
//! [`EntityId`] of the view.
//!
//! Units. A `_px` value is scan pixels at the **measured** scale (the
//! scale the dimensions best give, [`Scale::measured`]; the view is drawn
//! at [`Scale::used`], usually a standard scale a little smaller). A
//! `_in` value is part inches. A `_mm` value is part mm (the solver's own
//! residual). Scales are paper mm per part mm. A `pull` is the solver's
//! multiplier for the constraint, cost per part mm: how hard it holds the
//! design away from the ink.

use crate::value::dimension::DimensionId;
use crate::value::geometry::EntityId;
use crate::value::provenance::{Agent, ConstraintId, Date};
use crate::value::view::ViewId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The residuals of one view's solve ([module](self)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Residuals {
    /// The view the solve designed; its entities and constraints are the
    /// ids below.
    pub view: ViewId,
    /// What measured them.
    pub by: Agent,
    /// When.
    pub when: Date,
    /// How, in words.
    pub how: String,
    /// The scale.
    pub scale: Scale,
    /// The solver's summary.
    pub solver: Solver,
    /// Each read dimension against the ink.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dimensions: BTreeMap<DimensionId, DimensionResidual>,
    /// Each relation of the design (designed or inferred) against the ink.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub relations: BTreeMap<ConstraintId, RelationResidual>,
    /// Each entity against its ink.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub entities: BTreeMap<EntityId, EntityResidual>,
}

/// The scale the dimensions give, paper mm per part mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scale {
    /// The dimensions' median (where the search started).
    pub median: f64,
    /// Their best fit under the constraints.
    pub measured: f64,
    /// The scale the view is drawn at.
    pub used: f64,
    /// How `used` is written (`2:1`), if it is a standard scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
}

/// The solver's summary of the view's constraints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solver {
    /// Whether every constraint holds.
    pub converged: bool,
    /// Parameters solved for.
    pub parameters: usize,
    /// Equations.
    pub equations: usize,
    /// The equations' rank.
    pub rank: usize,
    /// The largest constraint residual, part mm.
    pub residual_max_mm: f64,
}

/// A read dimension against the solve. The "ink" is the drawing's own
/// line work at the measured scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DimensionResidual {
    /// As read, part inches.
    pub read_in: f64,
    /// The ink's, part inches.
    pub ink_in: f64,
    /// Read less ink, inches.
    pub delta_in: f64,
    /// Read less ink, scan px.
    pub delta_px: f64,
    /// The scale this dimension alone gives.
    pub scale_sample: f64,
    /// The scale the other dimensions give without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_without: Option<f64>,
    /// How hard it pulls the design off the ink.
    pub pull: f64,
}

/// A relation of the design against the ink.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelationResidual {
    /// The ink's largest residual of it, scan px.
    pub ink_px: f64,
    /// How hard it pulls the design off the ink.
    pub pull: f64,
}

/// An entity's design against its ink.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityResidual {
    /// RMS departure along it, scan px.
    pub rms_px: f64,
    /// Largest departure, scan px.
    pub max_px: f64,
    /// The constraint whose removal lets it back the most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_by: Option<MovedBy>,
}

/// The constraint that moves an entity off its ink the most.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MovedBy {
    /// The constraint, of the view.
    pub constraint: ConstraintId,
    /// How much of the entity's RMS it accounts for, scan px.
    pub rms_px: f64,
}

/// What can be wrong with the residuals.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A number is not finite.
    NotFinite,
    /// The view is not the document's.
    NoView(ViewId),
    /// A dimension the document does not have.
    NoDimension(DimensionId),
    /// A constraint the view does not have.
    NoConstraint(ConstraintId),
    /// An entity the view does not have.
    NoEntity(EntityId),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

impl Residuals {
    /// Valid on its own: every number finite.
    pub fn check(&self) -> Result<(), Error> {
        let s = &self.scale;
        let mut v = vec![s.median, s.measured, s.used, self.solver.residual_max_mm];
        for d in self.dimensions.values() {
            v.extend([
                d.read_in,
                d.ink_in,
                d.delta_in,
                d.delta_px,
                d.scale_sample,
                d.pull,
            ]);
            v.extend(d.scale_without);
        }
        for r in self.relations.values() {
            v.extend([r.ink_px, r.pull]);
        }
        for e in self.entities.values() {
            v.extend([e.rms_px, e.max_px]);
            v.extend(e.moved_by.iter().map(|m| m.rms_px));
        }
        if v.iter().all(|x| x.is_finite()) {
            Ok(())
        } else {
            Err(Error::NotFinite)
        }
    }

    /// The residual of the dimension `id`.
    pub fn dimension(&self, id: impl Into<DimensionId>) -> Option<&DimensionResidual> {
        self.dimensions.get(&id.into())
    }

    /// The residual of the design relation `id`.
    pub fn relation(&self, id: impl Into<ConstraintId>) -> Option<&RelationResidual> {
        self.relations.get(&id.into())
    }

    /// The departure of the entity `id` from its ink.
    pub fn entity(&self, id: impl Into<EntityId>) -> Option<&EntityResidual> {
        self.entities.get(&id.into())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn residuals() -> Residuals {
        Residuals {
            view: ViewId::new("part"),
            by: Agent::Script {
                name: "test".into(),
            },
            when: Date::new(2026, 11, 1).unwrap(),
            how: "measured".into(),
            scale: Scale {
                median: 2.0167,
                measured: 2.0135,
                used: 2.0,
                standard: Some("2:1".into()),
            },
            solver: Solver {
                converged: true,
                parameters: 66,
                equations: 60,
                rank: 60,
                residual_max_mm: 7.1e-15,
            },
            dimensions: BTreeMap::new(),
            relations: BTreeMap::new(),
            entities: BTreeMap::new(),
        }
    }

    #[test]
    fn residuals_round_trip_through_their_text() {
        let mut r = residuals();
        r.entities.insert(
            EntityId::new("e1"),
            EntityResidual {
                rms_px: 1.5,
                max_px: 2.5,
                moved_by: Some(MovedBy {
                    constraint: ConstraintId::new("D1"),
                    rms_px: 0.3,
                }),
            },
        );
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Residuals>(&text).unwrap(), r);
        assert_eq!(r.check(), Ok(()));
        assert_eq!(r.entity("e1").unwrap().max_px, 2.5);
    }

    #[test]
    fn a_number_that_is_not_finite_is_refused() {
        let mut r = residuals();
        r.scale.used = f64::NAN;
        assert_eq!(r.check(), Err(Error::NotFinite));
    }
}
