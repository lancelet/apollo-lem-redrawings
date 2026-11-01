//! A leader: the line from a note to what it names.
//!
//! A leader leaves a text at one of its anchors (an end of a line at mid
//! cap height, as on the drawings) and runs the standard gap, or leaves
//! a balloon's ring at its left or right point with no gap; then it
//! runs a horizontal shoulder away from the text, bends at any knees,
//! and ends in one or more tips, each on a target of the drawing with a
//! terminator (a filled arrowhead, a dot, or nothing). The knees are
//! offsets from the attachment point (the anchor, or the ring's point)
//! along the paper's axes, so moving the text carries its leader and
//! only the final runs stretch; each tip may have knees of its own
//! after the shared ones. Targets are
//! points and curves of the sketches, so a re-solve keeps every tip on
//! its feature, or free points on the paper or in a view for a surface
//! or an area.
//!
//! The sizes (gap, shoulder, arrowhead, dot) come from the document's
//! [`Drafting`](crate::value::drafting::Drafting) table; a leader may
//! state its shoulder's length or have none, which is how the straight
//! station callouts are drawn.

use crate::value::drafting::{Head, Terminator};
use crate::value::geometry::{EntityId, Model, Paper, P2};
use crate::value::provenance::Provenance;
use crate::value::sketch::LayerId;
use crate::value::text::{Anchor, DrawingPoint, Side, TextId};
use crate::value::view::ViewId;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// The name of a leader, an interned `Copy` handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LeaderId(Ustr);

impl LeaderId {
    /// A name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&str> for LeaderId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<&LeaderId> for LeaderId {
    fn from(id: &LeaderId) -> Self {
        *id
    }
}

impl std::fmt::Display for LeaderId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a leader leaves from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum Origin {
    /// A text's anchor, on the text's left or right.
    Text {
        /// The text.
        text: TextId,
        /// Its anchor.
        anchor: Anchor,
    },
    /// The ring of an enclosed text (a balloon): the leader leaves the
    /// ring at its left or right point on the paper, whatever the
    /// text's angle (balloons stand upright), at the text's centre
    /// height, with no gap, as the drawings do. The survey's one exit
    /// from the top of a ring is not modelled.
    Enclosure {
        /// The enclosed text.
        text: TextId,
        /// Which side of the ring it leaves from.
        side: Side,
    },
    /// A point of the drawing, with no text: an arrow standing for
    /// itself, as a section's cutting-plane arrow leaves the plane's end
    /// or a target's offset arrow its arm. It leaves the point with no
    /// gap and no shoulder, straight for its first knee or its tip. The
    /// point is a point target ([`Target::Point`], [`Target::Paper`] or
    /// [`Target::InView`]).
    Point {
        /// The point.
        at: Target,
    },
}

impl Origin {
    /// The text the leader leaves from; none for a leader from a point.
    pub fn text(&self) -> Option<TextId> {
        match *self {
            Origin::Text { text, .. } | Origin::Enclosure { text, .. } => Some(text),
            Origin::Point { .. } => None,
        }
    }
}

/// The horizontal run before the first knee.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shoulder", rename_all = "snake_case")]
pub enum Shoulder {
    /// The drafting table's length.
    Standard,
    /// A stated length, mm.
    Length {
        /// Millimetres.
        mm: f64,
    },
    /// None: the first run leaves the gap point directly.
    None,
}

/// A route from an attachment: the horizontal shoulder, then the knees
/// shared by every tip, as offsets from the attachment in paper mm.
/// Leaders own one, and so do the notes that draw as leaders (radius,
/// diameter and station callouts), so there is one representation and
/// one check.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    /// The horizontal run before the first knee.
    #[serde(flatten)]
    pub shoulder: Shoulder,
    /// The shared knees after the shoulder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knees: Vec<P2<Paper>>,
}

impl Route {
    /// The standard shoulder and no knees.
    pub fn standard() -> Self {
        Route {
            shoulder: Shoulder::Standard,
            knees: Vec::new(),
        }
    }

    /// The same route with this shoulder.
    pub fn shouldered(mut self, shoulder: Shoulder) -> Self {
        self.shoulder = shoulder;
        self
    }

    /// The same route with these knees.
    pub fn via(mut self, knees: Vec<P2<Paper>>) -> Self {
        self.knees = knees;
        self
    }

    /// Finite knees; a stated shoulder finite and positive.
    pub fn check(&self) -> Result<(), Error> {
        if !self
            .knees
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite())
        {
            return Err(Error::NotFinite);
        }
        match self.shoulder {
            Shoulder::Length { mm } if !mm.is_finite() => Err(Error::NotFinite),
            Shoulder::Length { mm } if mm <= 0.0 => Err(Error::BadShoulder(mm)),
            _ => Ok(()),
        }
    }
}

/// An entity of the drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "in", rename_all = "snake_case")]
pub enum DrawingEntity {
    /// An entity of the sheet sketch.
    Sheet {
        /// The entity.
        id: EntityId,
    },
    /// An entity of a view.
    View {
        /// The view.
        view: ViewId,
        /// The entity.
        id: EntityId,
    },
}

/// Where along a curve a tip lands.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Along {
    /// A fraction of the way along, `0..=1`.
    Fraction {
        /// The fraction.
        t: f64,
    },
    /// Wherever the last run meets the curve: the point of the curve
    /// nearest the run's start.
    Nearest,
}

/// What a tip lands on.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// A point of a sketch: a cross centre, a line's end, a circle's
    /// centre.
    Point {
        /// The point.
        point: DrawingPoint,
    },
    /// A place on a line, arc or circle of a sketch.
    Along {
        /// The curve.
        entity: DrawingEntity,
        /// Where on it.
        at: Along,
    },
    /// Another text's anchor.
    Text {
        /// The text.
        text: TextId,
        /// Its anchor.
        anchor: Anchor,
    },
    /// A free point on the paper.
    Paper {
        /// The point, mm.
        point: P2<Paper>,
    },
    /// A free point in a view's frame: a surface or an area, moving
    /// with the view.
    InView {
        /// The view.
        view: ViewId,
        /// The point, model mm.
        point: P2<Model>,
    },
}

/// One tip: its own knees after the shared ones, then the target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tip {
    /// Knees of this branch, offsets from the attachment point, mm.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knees: Vec<P2<Paper>>,
    /// What it lands on.
    pub target: Target,
    /// What marks the end.
    pub terminator: Terminator,
    /// How far short of the target the tip stops, mm, along the arrival;
    /// the drafting table's when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_back: Option<f64>,
    /// Its own arrowhead's size; the drafting table's when none (a
    /// target's large offset arrows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<Head>,
}

impl Tip {
    fn with(target: Target, terminator: Terminator) -> Self {
        Self {
            knees: Vec::new(),
            target,
            terminator,
            pull_back: None,
            head: None,
        }
    }

    /// A tip with an arrowhead on the target.
    pub fn arrow(target: Target) -> Self {
        Self::with(target, Terminator::Arrow)
    }

    /// A tip with a dot on the target.
    pub fn dot(target: Target) -> Self {
        Self::with(target, Terminator::Dot)
    }

    /// A tip ending plainly on the target.
    pub fn plain(target: Target) -> Self {
        Self::with(target, Terminator::None)
    }

    /// The same tip with these knees of its own.
    pub fn via(mut self, knees: Vec<P2<Paper>>) -> Self {
        self.knees = knees;
        self
    }

    /// The same tip stopping this far short of its target, mm.
    pub fn pulled_back(mut self, mm: f64) -> Self {
        self.pull_back = Some(mm);
        self
    }

    /// The same tip with an arrowhead of its own size.
    pub fn with_head(mut self, head: Head) -> Self {
        self.head = Some(head);
        self
    }
}

/// What can be wrong with a leader on its own.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// No tips.
    NoTips,
    /// A knee, length or fraction that is not finite.
    NotFinite,
    /// A stated shoulder that is not positive.
    BadShoulder(f64),
    /// A fraction outside `0..=1`.
    BadFraction(f64),
    /// A pull-back that is negative.
    BadPullBack(f64),
    /// An origin anchor on no side of the text: a leader leaves from
    /// the left or the right.
    OriginNeedsSide,
    /// A tip landing on the text the leader leaves from.
    SelfTarget,
    /// A leader from a point whose point is not one (a place along a
    /// curve, a text's anchor).
    OriginNotAPoint,
    /// A tip's own arrowhead with a size that is not one.
    BadHead,
    /// A leader from a point with a shoulder: it leaves the point
    /// straight for its first knee or tip.
    ShoulderAtPoint,
    /// Its provenance fails the record's checks.
    Provenance(crate::value::provenance::Error),
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

/// A leader.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct Leader {
    from: Origin,
    route: Route,
    tips: Vec<Tip>,
    refers_to: Vec<DrawingEntity>,
    layer: LayerId,
    provenance: Provenance,
}

#[derive(Serialize, Deserialize)]
struct Raw {
    #[serde(flatten)]
    from: Origin,
    #[serde(flatten)]
    route: Route,
    tips: Vec<Tip>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    refers_to: Vec<DrawingEntity>,
    layer: LayerId,
    provenance: Provenance,
}

impl From<Leader> for Raw {
    fn from(l: Leader) -> Self {
        Raw {
            from: l.from,
            route: l.route,
            tips: l.tips,
            refers_to: l.refers_to,
            layer: l.layer,
            provenance: l.provenance,
        }
    }
}

impl TryFrom<Raw> for Leader {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        let l = Leader {
            from: r.from,
            route: r.route,
            tips: r.tips,
            refers_to: r.refers_to,
            layer: r.layer,
            provenance: r.provenance,
        };
        l.check()?;
        Ok(l)
    }
}

impl Leader {
    /// A leader from a text's anchor with the standard shoulder, one
    /// tip, on the `leader` layer. The builders below change the rest.
    pub fn new(
        text: impl Into<TextId>,
        anchor: Anchor,
        tip: Tip,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        let l = Self {
            from: Origin::Text {
                text: text.into(),
                anchor,
            },
            route: Route::standard(),
            tips: vec![tip],
            refers_to: Vec::new(),
            layer: LayerId::new("leader"),
            provenance,
        };
        l.check()?;
        Ok(l)
    }

    /// A leader from a side of a balloon's ring (an enclosed text) with
    /// the standard shoulder, one tip, on the `leader` layer: the
    /// balloon callouts of the drawings. The builders below change the
    /// rest.
    pub fn balloon(
        text: impl Into<TextId>,
        side: Side,
        tip: Tip,
        provenance: Provenance,
    ) -> Result<Self, Error> {
        let l = Self {
            from: Origin::Enclosure {
                text: text.into(),
                side,
            },
            route: Route::standard(),
            tips: vec![tip],
            refers_to: Vec::new(),
            layer: LayerId::new("leader"),
            provenance,
        };
        l.check()?;
        Ok(l)
    }

    /// A leader from a point of the drawing (see [`Origin::Point`]),
    /// one tip, no shoulder, on the `leader` layer. The builders below
    /// change the rest.
    pub fn from_point(at: Target, tip: Tip, provenance: Provenance) -> Result<Self, Error> {
        let l = Self {
            from: Origin::Point { at },
            route: Route {
                shoulder: Shoulder::None,
                knees: Vec::new(),
            },
            tips: vec![tip],
            refers_to: Vec::new(),
            layer: LayerId::new("leader"),
            provenance,
        };
        l.check()?;
        Ok(l)
    }

    /// The same leader with this shoulder.
    pub fn shouldered(mut self, shoulder: Shoulder) -> Result<Self, Error> {
        self.route.shoulder = shoulder;
        self.check()?;
        Ok(self)
    }

    /// The same leader with these shared knees, offsets from the
    /// attachment point in paper mm.
    pub fn via(mut self, knees: Vec<P2<Paper>>) -> Result<Self, Error> {
        self.route.knees = knees;
        self.check()?;
        Ok(self)
    }

    /// The same leader with another tip from the last shared knee.
    pub fn also_to(mut self, tip: Tip) -> Result<Self, Error> {
        self.tips.push(tip);
        self.check()?;
        Ok(self)
    }

    /// The same leader standing for these entities as well ("4 PLACES").
    pub fn standing_for(mut self, entities: Vec<DrawingEntity>) -> Self {
        self.refers_to = entities;
        self
    }

    /// The same leader on another layer.
    pub fn on_layer(mut self, layer: impl Into<LayerId>) -> Self {
        self.layer = layer.into();
        self
    }

    /// Everything a leader must satisfy on its own; the document checks
    /// what it refers to.
    pub fn check(&self) -> Result<(), Error> {
        self.provenance.check()?;
        if self.tips.is_empty() {
            return Err(Error::NoTips);
        }
        let own = self.from.text();
        let finite = |p: &P2<Paper>| p.x.is_finite() && p.y.is_finite();
        match self.from {
            Origin::Text { anchor, .. } if anchor.side().is_none() => {
                return Err(Error::OriginNeedsSide);
            }
            Origin::Point { at } => {
                match at {
                    Target::Point { .. } => {}
                    Target::Paper { point } if !finite(&point) => return Err(Error::NotFinite),
                    Target::InView { point, .. }
                        if !(point.x.is_finite() && point.y.is_finite()) =>
                    {
                        return Err(Error::NotFinite)
                    }
                    Target::Paper { .. } | Target::InView { .. } => {}
                    Target::Along { .. } | Target::Text { .. } => {
                        return Err(Error::OriginNotAPoint)
                    }
                }
                if self.route.shoulder != Shoulder::None {
                    return Err(Error::ShoulderAtPoint);
                }
            }
            _ => {}
        }
        self.route.check()?;
        for tip in &self.tips {
            if !tip.knees.iter().all(finite) {
                return Err(Error::NotFinite);
            }
            if let Some(h) = &tip.head {
                h.check().map_err(|_| Error::BadHead)?;
            }
            match tip.pull_back {
                Some(p) if !p.is_finite() => return Err(Error::NotFinite),
                Some(p) if p < 0.0 => return Err(Error::BadPullBack(p)),
                _ => {}
            }
            match tip.target {
                Target::Along {
                    at: Along::Fraction { t },
                    ..
                } => {
                    if !t.is_finite() {
                        return Err(Error::NotFinite);
                    }
                    if !(0.0..=1.0).contains(&t) {
                        return Err(Error::BadFraction(t));
                    }
                }
                Target::Text { text, .. } if Some(text) == own => return Err(Error::SelfTarget),
                Target::Paper { point } if !(point.x.is_finite() && point.y.is_finite()) => {
                    return Err(Error::NotFinite)
                }
                Target::InView { point, .. } if !(point.x.is_finite() && point.y.is_finite()) => {
                    return Err(Error::NotFinite)
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Where it leaves from.
    pub fn from(&self) -> Origin {
        self.from
    }

    /// Its shoulder.
    pub fn shoulder(&self) -> Shoulder {
        self.route.shoulder
    }

    /// The shared knees.
    pub fn knees(&self) -> &[P2<Paper>] {
        &self.route.knees
    }

    /// The route: the shoulder and the shared knees.
    pub fn route(&self) -> &Route {
        &self.route
    }

    /// The tips.
    pub fn tips(&self) -> &[Tip] {
        &self.tips
    }

    /// The entities it stands for besides its targets.
    pub fn refers_to(&self) -> &[DrawingEntity] {
        &self.refers_to
    }

    /// The layer.
    pub fn layer(&self) -> LayerId {
        self.layer
    }

    /// The provenance.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Every entity a tip lands on or the leader stands for.
    pub fn entities(&self) -> impl Iterator<Item = DrawingEntity> + '_ {
        let targets = self.tips.iter().filter_map(|t| match t.target {
            Target::Point { point } => Some(match point {
                DrawingPoint::Sheet { point } => DrawingEntity::Sheet { id: point.id() },
                DrawingPoint::View { view, point } => DrawingEntity::View {
                    view,
                    id: point.id(),
                },
            }),
            Target::Along { entity, .. } => Some(entity),
            _ => None,
        });
        targets.chain(self.refers_to.iter().copied())
    }

    /// Every text it leaves from or lands on.
    pub fn texts(&self) -> impl Iterator<Item = TextId> + '_ {
        self.from
            .text()
            .into_iter()
            .chain(self.tips.iter().filter_map(|t| match t.target {
                Target::Text { text, .. } => Some(text),
                _ => None,
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::provenance::{Agent, Date, Method};
    use crate::value::text::{BoxPoint, Side};

    fn ours() -> Provenance {
        Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 15).unwrap(),
        )
    }

    #[test]
    fn leaders_are_checked_and_round_trip() {
        let line0 = Anchor::Line {
            index: 0,
            side: Side::Left,
        };
        let hole = Target::Along {
            entity: DrawingEntity::View {
                view: "top".into(),
                id: "c0".into(),
            },
            at: Along::Nearest,
        };
        let l = Leader::new("note", line0, Tip::arrow(hole), ours())
            .unwrap()
            .via(vec![P2::new(-10.0, 5.0)])
            .unwrap()
            .also_to(Tip::dot(Target::Paper {
                point: P2::new(1.0, 2.0),
            }))
            .unwrap();
        let json = serde_json::to_string(&l).unwrap();
        assert!(json.contains("\"from\":\"text\""), "{json}");
        assert!(json.contains("\"shoulder\":\"standard\""), "{json}");
        assert!(json.contains("\"target\":{\"kind\":\"along\""), "{json}");
        assert!(json.contains("\"at\":{\"kind\":\"nearest\"}"), "{json}");
        assert_eq!(serde_json::from_str::<Leader>(&json).unwrap(), l);
        assert_eq!(l.entities().count(), 1);
        assert_eq!(l.texts().collect::<Vec<_>>(), vec!["note".into()]);
        // A leader from a balloon's ring leaves a side of it with the
        // standard shoulder; it may not land on its own balloon.
        let b = Leader::balloon("b1", Side::Right, Tip::arrow(hole), ours()).unwrap();
        assert_eq!(
            b.from(),
            Origin::Enclosure {
                text: "b1".into(),
                side: Side::Right
            }
        );
        assert_eq!(b.shoulder(), Shoulder::Standard);
        assert_eq!(b.texts().collect::<Vec<_>>(), vec!["b1".into()]);
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"from\":\"enclosure\""), "{json}");
        assert!(json.contains("\"side\":\"right\""), "{json}");
        assert_eq!(serde_json::from_str::<Leader>(&json).unwrap(), b);
        assert_eq!(
            b.clone()
                .also_to(Tip::arrow(Target::Text {
                    text: "b1".into(),
                    anchor: line0,
                }))
                .err(),
            Some(Error::SelfTarget)
        );
        // A leader leaves from a side of its text, whatever its shoulder.
        for at in [
            BoxPoint::BottomCentre,
            BoxPoint::TopCentre,
            BoxPoint::Centre,
        ] {
            assert_eq!(
                Leader::new("note", Anchor::Box { at }, Tip::arrow(hole), ours()).err(),
                Some(Error::OriginNeedsSide)
            );
        }
        let right = Anchor::Box {
            at: BoxPoint::MidRight,
        };
        Leader::new("note", right, Tip::arrow(hole), ours())
            .unwrap()
            .shouldered(Shoulder::None)
            .unwrap();
        assert_eq!(
            l.clone().shouldered(Shoulder::Length { mm: 0.0 }).err(),
            Some(Error::BadShoulder(0.0))
        );
        assert_eq!(
            l.clone()
                .also_to(Tip::arrow(Target::Along {
                    entity: DrawingEntity::Sheet { id: "x".into() },
                    at: Along::Fraction { t: 1.5 },
                }))
                .err(),
            Some(Error::BadFraction(1.5))
        );
        assert_eq!(
            l.clone()
                .also_to(Tip::arrow(Target::Text {
                    text: "note".into(),
                    anchor: line0,
                }))
                .err(),
            Some(Error::SelfTarget)
        );
        assert_eq!(
            l.clone().via(vec![P2::new(f64::NAN, 0.0)]).err(),
            Some(Error::NotFinite)
        );
        assert_eq!(
            l.clone().also_to(Tip::arrow(hole).pulled_back(-1.0)).err(),
            Some(Error::BadPullBack(-1.0))
        );
        // A file with no tips is refused (the moved list is ignored).
        let bad = json.replacen("\"tips\":[", "\"tips\":[],\"was\":[", 1);
        assert_ne!(bad, json);
        let e = serde_json::from_str::<Leader>(&bad).unwrap_err();
        assert!(e.to_string().contains("NoTips"), "{e}");
    }

    #[test]
    fn a_leader_from_a_point_has_no_text_and_no_shoulder() {
        let at = Target::Paper {
            point: P2::new(10.0, 20.0),
        };
        let tip = Tip::arrow(Target::Paper {
            point: P2::new(30.0, 20.0),
        })
        .with_head(Head::new(8.0, 2.5, 1.0).unwrap());
        let l = Leader::from_point(at, tip, ours())
            .unwrap()
            .via(vec![P2::new(0.0, -6.0)])
            .unwrap();
        assert_eq!(l.from(), Origin::Point { at });
        assert_eq!(l.from().text(), None);
        assert_eq!(l.texts().count(), 0);
        assert_eq!(l.shoulder(), Shoulder::None);
        let json = serde_json::to_string(&l).unwrap();
        assert!(json.contains("\"from\":\"point\""), "{json}");
        assert!(json.contains("\"head\":{"), "{json}");
        assert_eq!(serde_json::from_str::<Leader>(&json).unwrap(), l);
        // It leaves straight from its point, so takes no shoulder; and
        // it leaves a point, not a curve or a text.
        assert_eq!(
            l.clone().shouldered(Shoulder::Standard).err(),
            Some(Error::ShoulderAtPoint)
        );
        let along = Target::Along {
            entity: DrawingEntity::Sheet { id: "x".into() },
            at: Along::Nearest,
        };
        assert_eq!(
            Leader::from_point(along, Tip::arrow(at), ours()).err(),
            Some(Error::OriginNotAPoint)
        );
        let nan = Target::Paper {
            point: P2::new(f64::NAN, 0.0),
        };
        assert_eq!(
            Leader::from_point(nan, Tip::arrow(at), ours()).err(),
            Some(Error::NotFinite)
        );
        // A tip's own head must be a head.
        let mut bad = Tip::arrow(at);
        bad.head = Some(Head {
            length: -1.0,
            width: 1.0,
            inset: 0.0,
        });
        assert_eq!(
            Leader::from_point(at, bad, ours()).err(),
            Some(Error::BadHead)
        );
    }
}
