//! Observed features: seen on the sheet but not intended
//! (docs/redraw-plan.md §8.3, K11).
//!
//! Some ink shows something the drawing never states: on LDW280-17619
//! the junctions of the flats with the tapers are slightly rounded, with
//! no radius called out. An observed feature records what is seen, how
//! well it fits the ink against the intended geometry, and what ignoring
//! it would lose. It is **not part of the design**: it never enters the
//! solver (no equation, no freedom), and the design and its dimensions
//! stay sharp. It lives on its own `observed` layer, so it can be shown
//! (a drawn rounding replaces its sharp corner on the part), hidden and
//! inspected apart from the design.
//!
//! - A **rounding** belongs to a junction: two lines of a view. Its
//!   radius is the radius drawn (the fit to the ink's centre line less
//!   what the pen alone explains), in the view's units; the redraw trims
//!   the two lines back to where the arc, tangent to both, touches them,
//!   and draws the arc. Roundings that ought to match share a `shared`
//!   group, fitted jointly to one radius, each corner with its own
//!   centre.
//! - **Other** observed features (an overshoot, an off-centre line, …)
//!   are recorded the same way, and are not drawn by default.
//! - **Graduation:** an observed rounding becomes a real fillet when a
//!   justification is found (a note, a standard, another sheet): the
//!   record stays as the evidence and says what it became
//!   (`graduated_to`).

use crate::value::geometry::{EntityId, Model, P2};
use crate::value::provenance::{ItemRef, Provenance};
use crate::value::sketch::LayerId;
use crate::value::view::ViewId;
use serde::{Deserialize, Serialize};

id! {
    /// An observed feature's name.
    ObservedId
}

/// The layer observed features are on.
pub const LAYER: &str = "observed";

/// How a rounding fits the ink (every length paper mm, as the drawing
/// shows it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fit {
    /// The radius the ink's centre line fits, before the pen's share is
    /// taken off.
    pub ink_radius_mm: f64,
    /// What the pen alone explains and is taken off the radius: half the
    /// stroke width, at the junction's two lines' median.
    pub pen_mm: f64,
    /// RMS departure of the ink's centre line from the sharp corner of the
    /// ink's own two lines.
    pub rms_sharp_mm: f64,
    /// RMS departure of the ink's centre line from the same two lines
    /// rounded at the drawn radius.
    pub rms_rounded_mm: f64,
    /// RMS departure from them rounded at the ink radius: the best this
    /// family does (equal to `rms_rounded_mm` when no pen is taken off).
    pub rms_best_mm: f64,
    /// What is lost by ignoring it: the largest departure of the design's
    /// own sharp corner from the ink's centre line there.
    pub loss_mm: f64,
    /// The ink's centre-line points the fit used.
    pub points: u32,
    /// The standard error of the radius fitted (`ink_radius_mm`), paper
    /// mm; for a shared group the group's, from all its points.
    #[serde(default)]
    pub sigma_mm: f64,
    /// How this corner fares fitted alone, kept so that a shared radius's
    /// disagreement with it stays on the record. None for records made
    /// before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<OwnFit>,
}

/// A corner fitted by itself, on the same points as the (shared) fit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwnFit {
    /// Its own best radius (the ink's centre line, pen not taken off),
    /// paper mm.
    pub ink_radius_mm: f64,
    /// Its standard error, paper mm.
    pub sigma_mm: f64,
    /// RMS departure of its points from its two lines rounded at that
    /// radius; `Fit::rms_best_mm` is the same at the fitted (shared)
    /// radius, `rms_sharp_mm` at none.
    pub rms_mm: f64,
}

impl OwnFit {
    /// How many of its own standard errors its radius is from the fitted
    /// one in `fit`.
    pub fn off_sigmas(&self, fit: &Fit) -> f64 {
        (self.ink_radius_mm - fit.ink_radius_mm).abs() / self.sigma_mm.max(1e-12)
    }

    /// Whether the fitted (shared) radius is more than [`FLAG_SIGMAS`]
    /// of its standard errors from its own.
    pub fn disagrees(&self, fit: &Fit) -> bool {
        self.off_sigmas(fit) > FLAG_SIGMAS
    }
}

/// How many of a corner's own standard errors its radius may be from
/// the shared one before the sharing is flagged.
pub const FLAG_SIGMAS: f64 = 2.0;

/// Why corners that share a radius ought to (what a shared group
/// rests on; as an inferred constraint states its basis, §8.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum Basis {
    /// The part is symmetric (the corners mirror each other).
    Symmetry,
    /// The corners are one feature, made alike.
    SameFeature,
    /// This drafter rounds such corners alike.
    DrafterHabit,
    /// Anything else, in words.
    Other(String),
}

impl Basis {
    /// The words the command takes: `symmetry`, `same-feature`,
    /// `drafter-habit`, `other(TEXT)`.
    pub fn words(&self) -> String {
        match self {
            Basis::Symmetry => "symmetry".into(),
            Basis::SameFeature => "same-feature".into(),
            Basis::DrafterHabit => "drafter-habit".into(),
            Basis::Other(t) => format!("other({t})"),
        }
    }

    /// The basis the words name, or None.
    pub fn parse(words: &str) -> Option<Basis> {
        match words.trim() {
            "symmetry" => Some(Basis::Symmetry),
            "same-feature" => Some(Basis::SameFeature),
            "drafter-habit" => Some(Basis::DrafterHabit),
            w => {
                let t = w.strip_prefix("other(")?.strip_suffix(')')?.trim();
                (!t.is_empty()).then(|| Basis::Other(t.into()))
            }
        }
    }
}

/// What was seen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    /// A corner of two lines seen rounded.
    Rounding {
        /// The two lines of the junction, in the view.
        junction: [EntityId; 2],
        /// The radius drawn, in the view's units (mm of the part).
        radius: f64,
        /// The centre of the arc drawn, tangent to both lines, in the
        /// view's units.
        centre: P2<Model>,
        /// How it fits the ink.
        fit: Fit,
    },
    /// Anything else seen, in words; never drawn by default.
    Other {
        /// What it is.
        what: String,
    },
}

/// An observed feature ([module](self)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observed {
    /// The view it is in.
    pub view: ViewId,
    /// Its layer, [`LAYER`] by the standard table.
    pub layer: LayerId,
    /// What was seen.
    pub kind: Kind,
    /// Whether the redraw draws it in place. Corner roundings are drawn
    /// by default, other kinds not.
    pub drawn: bool,
    /// Observations that share a fitted value (a radius) name the same
    /// group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<String>,
    /// What the group rests on: required when `shared`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<Basis>,
    /// What it became when a justification was found (the real fillet's
    /// entity, or the constraint that states it). Unused until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graduated_to: Option<ItemRef>,
    /// Who saw it, how and on what evidence (measured off the scan).
    pub provenance: Provenance,
}

impl Observed {
    /// A rounding, drawn.
    pub fn rounding(
        view: ViewId,
        junction: [EntityId; 2],
        radius: f64,
        centre: P2<Model>,
        fit: Fit,
        provenance: Provenance,
    ) -> Observed {
        Observed {
            view,
            layer: LayerId::new(LAYER),
            kind: Kind::Rounding {
                junction,
                radius,
                centre,
                fit,
            },
            drawn: true,
            shared: None,
            basis: None,
            graduated_to: None,
            provenance,
        }
    }

    /// Another kind of observation, not drawn.
    pub fn other(view: ViewId, what: &str, provenance: Provenance) -> Observed {
        Observed {
            view,
            layer: LayerId::new(LAYER),
            kind: Kind::Other { what: what.into() },
            drawn: false,
            shared: None,
            basis: None,
            graduated_to: None,
            provenance,
        }
    }

    /// The lines it is a junction of, for a rounding.
    pub fn junction(&self) -> Option<[EntityId; 2]> {
        match &self.kind {
            Kind::Rounding { junction, .. } => Some(*junction),
            Kind::Other { .. } => None,
        }
    }

    /// Valid on its own: a shared group states its basis, its numbers are finite, a radius is positive,
    /// the junction is two lines, `Other` says what it is, its
    /// provenance is valid.
    pub fn check(&self) -> Result<(), Error> {
        if self.shared.is_some() && self.basis.is_none() {
            return Err(Error::NoBasis);
        }
        if matches!(&self.basis, Some(Basis::Other(t)) if t.trim().is_empty()) {
            return Err(Error::NoBasis);
        }
        match &self.kind {
            Kind::Rounding {
                junction,
                radius,
                centre,
                fit,
            } => {
                if junction[0] == junction[1] {
                    return Err(Error::SameLine);
                }
                if !(radius.is_finite() && *radius > 0.0) {
                    return Err(Error::BadRadius(*radius));
                }
                if !(centre.x.is_finite() && centre.y.is_finite()) {
                    return Err(Error::NotFinite);
                }
                let numbers = [
                    fit.ink_radius_mm,
                    fit.pen_mm,
                    fit.rms_sharp_mm,
                    fit.rms_rounded_mm,
                    fit.rms_best_mm,
                    fit.loss_mm,
                    fit.sigma_mm,
                ];
                let own = fit
                    .own
                    .iter()
                    .flat_map(|o| [o.ink_radius_mm, o.sigma_mm, o.rms_mm]);
                if !numbers
                    .iter()
                    .copied()
                    .chain(own)
                    .all(|v| v.is_finite() && v >= 0.0)
                {
                    return Err(Error::NotFinite);
                }
            }
            Kind::Other { what } => {
                if what.trim().is_empty() {
                    return Err(Error::NoDescription);
                }
            }
        }
        self.provenance.check().map_err(Error::Provenance)
    }
}

/// What can be wrong with an observed feature.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A rounding's junction names one line twice.
    SameLine,
    /// A radius that is not positive and finite.
    BadRadius(f64),
    /// A number that is not finite (or is negative where it is a size).
    NotFinite,
    /// A shared group that does not say what it rests on.
    NoBasis,
    /// An `Other` that says nothing of what it is.
    NoDescription,
    /// Its provenance is not valid.
    Provenance(crate::value::provenance::Error),
    /// It names a view the document does not have.
    NoView(ViewId),
    /// A line of its junction is not a line of the view.
    NoLine(EntityId),
    /// It names a layer the document does not have.
    NoLayer(LayerId),
    /// It says it graduated to something the view does not have.
    NoGraduate(ItemRef),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::provenance::{Agent, Citation, Date, Method};

    fn prov() -> Provenance {
        Provenance::new(
            Method::Measured { sigma_mm: 0.1 },
            Agent::Script {
                name: "test".into(),
            },
            Date::new(2026, 10, 28).unwrap(),
        )
        .cite(Citation::new("scan", "px (1, 2)"))
    }

    fn fit() -> Fit {
        Fit {
            ink_radius_mm: 3.0,
            pen_mm: 0.3,
            rms_sharp_mm: 0.2,
            rms_rounded_mm: 0.05,
            rms_best_mm: 0.04,
            loss_mm: 0.5,
            points: 40,
            sigma_mm: 0.4,
            own: Some(OwnFit {
                ink_radius_mm: 3.5,
                sigma_mm: 0.3,
                rms_mm: 0.04,
            }),
        }
    }

    fn rounding() -> Observed {
        Observed::rounding(
            "part".into(),
            ["a".into(), "b".into()],
            1.2,
            P2::new(1.0, 2.0),
            fit(),
            prov(),
        )
    }

    /// A rounding is drawn by default, on the observed layer, and has
    /// no graduate; other kinds are not drawn.
    #[test]
    fn a_rounding_is_drawn_and_another_kind_is_not() {
        let r = rounding();
        assert!(r.drawn && r.shared.is_none() && r.graduated_to.is_none());
        assert_eq!(r.layer, LAYER.into());
        assert_eq!(r.junction(), Some(["a".into(), "b".into()]));
        r.check().unwrap();
        let o = Observed::other("part".into(), "the top line overshoots", prov());
        assert!(!o.drawn);
        assert_eq!(o.junction(), None);
        o.check().unwrap();
    }

    /// The basis words round trip; the own fit says when sharing is doubtful.
    #[test]
    fn basis_words_and_the_disagreement_flag() {
        for b in [
            Basis::Symmetry,
            Basis::SameFeature,
            Basis::DrafterHabit,
            Basis::Other("one template".into()),
        ] {
            assert_eq!(Basis::parse(&b.words()), Some(b));
        }
        assert_eq!(Basis::parse("whim"), None);
        assert_eq!(Basis::parse("other()"), None);
        let f = fit();
        let own = f.own.clone().unwrap();
        // 0.5 mm apart at 0.3 mm each: 1.7 sigma, not flagged.
        assert!(!own.disagrees(&f));
        let far = OwnFit {
            ink_radius_mm: 4.0,
            ..own
        };
        assert!(far.disagrees(&f));
    }

    /// The invalid ones, and the file form round trip.
    #[test]
    fn invalid_records_are_refused_and_the_form_round_trips() {
        let mut r = rounding();
        r.shared = Some("top".into());
        assert_eq!(r.check(), Err(Error::NoBasis));
        r.basis = Some(Basis::Other("same tool".into()));
        r.check().unwrap();
        r.basis = Some(Basis::Other(" ".into()));
        assert_eq!(r.check(), Err(Error::NoBasis));
        r.basis = Some(Basis::Symmetry);
        r.graduated_to = Some(ItemRef::Entity("fillet".into()));
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Observed>(&json).unwrap(), r);
        let mut bad = rounding();
        if let Kind::Rounding { radius, .. } = &mut bad.kind {
            *radius = 0.0;
        }
        assert_eq!(bad.check(), Err(Error::BadRadius(0.0)));
        let mut same = rounding();
        if let Kind::Rounding { junction, .. } = &mut same.kind {
            *junction = ["a".into(), "a".into()];
        }
        assert_eq!(same.check(), Err(Error::SameLine));
        assert_eq!(
            Observed::other("part".into(), " ", prov()).check(),
            Err(Error::NoDescription)
        );
        let unevidenced = Observed::other(
            "part".into(),
            "x",
            Provenance::new(
                Method::Measured { sigma_mm: 0.1 },
                Agent::Claude,
                Date::new(2026, 10, 28).unwrap(),
            ),
        );
        assert!(matches!(unevidenced.check(), Err(Error::Provenance(_))));
    }
}
