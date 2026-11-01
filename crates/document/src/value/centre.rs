//! Centre marks and centre lines: the drafting marks of a centre and of
//! an axis, which name the features they mark.
//!
//! A **centre mark** is a cross on the common centre of one or more
//! circles or arcs (concentric ones share one mark): its arms are the
//! drafting table's length unless it states its own, along the paper's
//! axes unless turned (bolt circles turn theirs to the radial). A
//! **centre line** is an axis through two or more points of the drawing
//! (the centres of a row of holes, of a link's two ends), or through one
//! point in a stated direction (the lines crossing at a lone hole's
//! centre), drawn long-short-long with a short dash on each point,
//! running on past the outermost by the drafting table's overshoot
//! unless it states its own.
//! Both are drawn from the features they name, so a re-solve moves them
//! with the geometry; both go on the `centre` layer.
//!
//! The drafting survey's figures: arms 0.12–0.2 in (0.15 standard, 0.09
//! inside small holes); centre lines run 0.2–0.75 in past the features
//! (`docs/annotation-requirements.md`, section 3).

use crate::value::geometry::{Paper, P2};
use crate::value::leader::DrawingEntity;
use crate::value::provenance::Provenance;
use crate::value::sketch::LayerId;
use crate::value::text::DrawingPoint;
use serde::{Deserialize, Serialize};

id!(
    /// The name of a centre mark.
    CentreMarkId
);
id!(
    /// The name of a centre line.
    CentreLineId
);

/// What makes a centre mark or a centre line invalid on its own.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A centre mark names no feature.
    NoFeatures,
    /// A centre line names no point.
    NoPoints,
    /// A centre line through one point states no direction, or one
    /// through several states one (their line is its direction).
    Direction(usize),
    /// A length is not finite and positive, mm.
    BadLength(f64),
    /// An angle is not finite.
    NotFinite,
    /// Its provenance is not valid.
    Provenance(crate::value::provenance::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NoFeatures => write!(f, "a centre mark names no circle or arc"),
            Error::NoPoints => write!(f, "a centre line through no point"),
            Error::Direction(n) => write!(
                f,
                "a centre line through {n} point(s) needs a direction exactly when through one"
            ),
            Error::BadLength(v) => write!(f, "length {v} mm is not finite and positive"),
            Error::NotFinite => write!(f, "an angle is not finite"),
            Error::Provenance(e) => write!(f, "provenance: {e:?}"),
        }
    }
}

impl std::error::Error for Error {}

fn length(v: Option<f64>) -> Result<(), Error> {
    match v {
        Some(v) if !(v.is_finite() && v > 0.0) => Err(Error::BadLength(v)),
        _ => Ok(()),
    }
}

/// A centre mark.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MarkRaw", into = "MarkRaw")]
pub struct CentreMark {
    of: Vec<DrawingEntity>,
    arm: Option<f64>,
    angle: f64,
    layer: LayerId,
    provenance: Provenance,
}

#[derive(Serialize, Deserialize)]
struct MarkRaw {
    of: Vec<DrawingEntity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    arm: Option<f64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    angle: f64,
    layer: LayerId,
    provenance: Provenance,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl From<CentreMark> for MarkRaw {
    fn from(m: CentreMark) -> Self {
        MarkRaw {
            of: m.of,
            arm: m.arm,
            angle: m.angle,
            layer: m.layer,
            provenance: m.provenance,
        }
    }
}

impl TryFrom<MarkRaw> for CentreMark {
    type Error = Error;

    fn try_from(r: MarkRaw) -> Result<Self, Error> {
        let m = CentreMark {
            of: r.of,
            arm: r.arm,
            angle: r.angle,
            layer: r.layer,
            provenance: r.provenance,
        };
        m.check()?;
        Ok(m)
    }
}

impl CentreMark {
    /// A mark on the centre of these circles or arcs, the table's arms,
    /// along the paper's axes, on the `centre` layer.
    pub fn new(of: Vec<DrawingEntity>, provenance: Provenance) -> Result<Self, Error> {
        let m = CentreMark {
            of,
            arm: None,
            angle: 0.0,
            layer: LayerId::new("centre"),
            provenance,
        };
        m.check()?;
        Ok(m)
    }

    /// The same with its own arm length (centre to tip), mm.
    pub fn with_arm(mut self, mm: f64) -> Result<Self, Error> {
        self.arm = Some(mm);
        self.check()?;
        Ok(self)
    }

    /// The same turned by `angle` radians on the paper.
    pub fn turned(mut self, angle: f64) -> Result<Self, Error> {
        self.angle = angle;
        self.check()?;
        Ok(self)
    }

    /// The same on another layer.
    pub fn on_layer(mut self, layer: impl Into<LayerId>) -> Self {
        self.layer = layer.into();
        self
    }

    /// Everything it must satisfy on its own; the document checks that
    /// what it names exists and is a circle or an arc.
    pub fn check(&self) -> Result<(), Error> {
        self.provenance.check().map_err(Error::Provenance)?;
        if self.of.is_empty() {
            return Err(Error::NoFeatures);
        }
        length(self.arm)?;
        if !self.angle.is_finite() {
            return Err(Error::NotFinite);
        }
        Ok(())
    }

    /// The circles and arcs it marks (their centre is the first's).
    pub fn of(&self) -> &[DrawingEntity] {
        &self.of
    }

    /// Its own arm length, mm, if it states one.
    pub fn arm(&self) -> Option<f64> {
        self.arm
    }

    /// Its turn on the paper, radians.
    pub fn angle(&self) -> f64 {
        self.angle
    }

    /// Its layer.
    pub fn layer(&self) -> LayerId {
        self.layer
    }

    /// Where it comes from.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// A centre line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "LineRaw", into = "LineRaw")]
pub struct CentreLine {
    through: Vec<DrawingPoint>,
    angle: Option<f64>,
    overshoot: Option<f64>,
    /// How far it runs past its outermost point at its start and at
    /// its end ([`CentreLine::run`]), mm, where it states it (its
    /// ink's run); else its overshoot.
    before: Option<f64>,
    after: Option<f64>,
    layer: LayerId,
    provenance: Provenance,
}

#[derive(Serialize, Deserialize)]
struct LineRaw {
    through: Vec<DrawingPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    overshoot: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after: Option<f64>,
    layer: LayerId,
    provenance: Provenance,
}

impl From<CentreLine> for LineRaw {
    fn from(l: CentreLine) -> Self {
        LineRaw {
            through: l.through,
            angle: l.angle,
            overshoot: l.overshoot,
            before: l.before,
            after: l.after,
            layer: l.layer,
            provenance: l.provenance,
        }
    }
}

impl TryFrom<LineRaw> for CentreLine {
    type Error = Error;

    fn try_from(r: LineRaw) -> Result<Self, Error> {
        let l = CentreLine {
            through: r.through,
            angle: r.angle,
            overshoot: r.overshoot,
            before: r.before,
            after: r.after,
            layer: r.layer,
            provenance: r.provenance,
        };
        l.check()?;
        Ok(l)
    }
}

impl CentreLine {
    /// An axis through these points (two or more), running past the
    /// outermost by the table's overshoot, on the `centre` layer.
    pub fn new(through: Vec<DrawingPoint>, provenance: Provenance) -> Result<Self, Error> {
        let l = CentreLine {
            through,
            angle: None,
            overshoot: None,
            before: None,
            after: None,
            layer: LayerId::new("centre"),
            provenance,
        };
        l.check()?;
        Ok(l)
    }

    /// An axis through one point, `angle` radians on the paper from +x,
    /// running the table's overshoot either side, on the `centre` layer.
    pub fn across(at: DrawingPoint, angle: f64, provenance: Provenance) -> Result<Self, Error> {
        let l = CentreLine {
            through: vec![at],
            angle: Some(angle),
            overshoot: None,
            before: None,
            after: None,
            layer: LayerId::new("centre"),
            provenance,
        };
        l.check()?;
        Ok(l)
    }

    /// The same running past its outermost points by this much, mm.
    pub fn with_overshoot(mut self, mm: f64) -> Result<Self, Error> {
        self.overshoot = Some(mm);
        self.check()?;
        Ok(self)
    }

    /// The same running past its outermost points by these, mm, at
    /// its start and its end ([`CentreLine::run`]); none at an end:
    /// its overshoot there.
    pub fn with_ends(mut self, before: Option<f64>, after: Option<f64>) -> Result<Self, Error> {
        self.before = before;
        self.after = after;
        self.check()?;
        Ok(self)
    }

    /// The same on another layer.
    pub fn on_layer(mut self, layer: impl Into<LayerId>) -> Self {
        self.layer = layer.into();
        self
    }

    /// Everything it must satisfy on its own; the document checks that
    /// its points exist.
    pub fn check(&self) -> Result<(), Error> {
        self.provenance.check().map_err(Error::Provenance)?;
        if self.through.is_empty() {
            return Err(Error::NoPoints);
        }
        if (self.through.len() == 1) != self.angle.is_some() {
            return Err(Error::Direction(self.through.len()));
        }
        if self.angle.is_some_and(|a| !a.is_finite()) {
            return Err(Error::NotFinite);
        }
        length(self.overshoot)?;
        length(self.before)?;
        length(self.after)
    }

    /// The points it runs through.
    pub fn through(&self) -> &[DrawingPoint] {
        &self.through
    }

    /// Its direction on the paper, radians from +x, when it runs through
    /// one point.
    pub fn angle(&self) -> Option<f64> {
        self.angle
    }

    /// Its own overshoot, mm, if it states one.
    pub fn overshoot(&self) -> Option<f64> {
        self.overshoot
    }

    /// How far it runs past its outermost point at its start, mm, if it
    /// states it.
    pub fn before(&self) -> Option<f64> {
        self.before
    }

    /// How far it runs past its outermost point at its end, mm, if it
    /// states it.
    pub fn after(&self) -> Option<f64> {
        self.after
    }

    /// Its layer.
    pub fn layer(&self) -> LayerId {
        self.layer
    }

    /// Where it comes from.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Where it runs on the paper, from the paper points it runs
    /// through (in `through`'s order) and the overshoot it falls back
    /// on (the table's, when it states none). Its start is its point
    /// (through one) or the earlier listed of its two farthest points,
    /// and it runs toward its angle or the later; none when its
    /// farthest points coincide.
    pub fn run(&self, points: &[P2<Paper>], overshoot: f64) -> Option<Run> {
        let (from, dir, far) = match self.angle {
            Some(angle) => (
                *points.first()?,
                P2::<Paper>::new(angle.cos(), angle.sin()),
                0.0,
            ),
            None => {
                let (mut a, mut b, mut far) = (*points.first()?, *points.get(1)?, -1.0);
                for (i, p) in points.iter().enumerate() {
                    for q in &points[i + 1..] {
                        let d = (q.x - p.x).hypot(q.y - p.y);
                        if d > far {
                            (a, b, far) = (*p, *q, d);
                        }
                    }
                }
                if far <= 0.0 {
                    return None;
                }
                (a, P2::new((b.x - a.x) / far, (b.y - a.y) / far), far)
            }
        };
        let over = self.overshoot.unwrap_or(overshoot);
        Some(Run {
            from,
            dir,
            far,
            lo: -self.before.unwrap_or(over),
            hi: far + self.after.unwrap_or(over),
        })
    }
}

/// A centre line on the paper ([`CentreLine::run`]): from `from`
/// along the unit `dir`, its far point `far` mm on; it is drawn from
/// `lo` (at most 0) to `hi` (at least `far`), mm along it from `from`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    /// Where it starts: its point, or the earlier of its farthest two.
    pub from: P2<Paper>,
    /// The unit vector it runs along.
    pub dir: P2<Paper>,
    /// How far on its far point is, mm.
    pub far: f64,
    /// Where it is drawn from, mm along it from `from` (at most 0).
    pub lo: f64,
    /// Where it is drawn to, mm along it from `from` (at least `far`).
    pub hi: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::geometry::{ArcPos, EntityId, PointRef};
    use crate::value::provenance::{Agent, Date, Method};

    fn prov() -> Provenance {
        Provenance::new(
            Method::Authored,
            Agent::User,
            Date::new(2026, 10, 21).unwrap(),
        )
    }

    /// A mark needs a feature and a sane arm; a line two points; both
    /// survive their file form.
    #[test]
    fn marks_and_lines_check_and_round_trip() {
        assert_eq!(CentreMark::new(vec![], prov()), Err(Error::NoFeatures));
        let arc = DrawingEntity::Sheet {
            id: EntityId::new("a1"),
        };
        let m = CentreMark::new(vec![arc], prov()).unwrap();
        assert!(m.clone().with_arm(-1.0).is_err());
        let m = m.with_arm(3.8).unwrap().turned(0.5).unwrap();
        let back: CentreMark = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        let p = |id: &str| DrawingPoint::Sheet {
            point: PointRef::arc(EntityId::new(id), ArcPos::Centre),
        };
        assert!(matches!(
            CentreLine::new(vec![p("a1")], prov()),
            Err(Error::Direction(1))
        ));
        assert!(matches!(
            CentreLine::new(vec![], prov()),
            Err(Error::NoPoints)
        ));
        let one = CentreLine::across(p("a1"), std::f64::consts::FRAC_PI_2, prov()).unwrap();
        let back: CentreLine = serde_json::from_str(&serde_json::to_string(&one).unwrap()).unwrap();
        assert_eq!(back, one);
        let l = CentreLine::new(vec![p("a1"), p("a2")], prov())
            .unwrap()
            .with_overshoot(6.0)
            .unwrap();
        let back: CentreLine = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(back, l);
    }

    /// A centre line runs its overshoot past its outermost points, or what
    /// it states at either end; its start is its point, or the earlier
    /// listed of its two farthest; its own ends survive its file form.
    #[test]
    fn a_centre_line_runs_past_its_points() {
        let p = |id: &str| DrawingPoint::Sheet {
            point: PointRef::arc(EntityId::new(id), ArcPos::Centre),
        };
        let at = |x: f64, y: f64| P2::<Paper>::new(x, y);
        let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-9, "{a} against {b}");
        let one = CentreLine::across(p("a1"), 0.0, prov()).unwrap();
        let r = one.run(&[at(10.0, 20.0)], 12.7).unwrap();
        assert_eq!((r.from, r.dir), (at(10.0, 20.0), at(1.0, 0.0)));
        near(r.far, 0.0);
        near(r.lo, -12.7);
        near(r.hi, 12.7);
        let r = one
            .with_ends(Some(30.0), None)
            .unwrap()
            .run(&[at(10.0, 20.0)], 12.7)
            .unwrap();
        near(r.lo, -30.0);
        near(r.hi, 12.7);

        let three = CentreLine::new(vec![p("a1"), p("a2"), p("a3")], prov()).unwrap();
        let r = three
            .clone()
            .with_ends(None, Some(5.0))
            .unwrap()
            .run(&[at(50.0, 0.0), at(0.0, 0.0), at(100.0, 0.0)], 12.7)
            .unwrap();
        assert_eq!((r.from, r.dir), (at(0.0, 0.0), at(1.0, 0.0)));
        near(r.far, 100.0);
        near(r.lo, -12.7);
        near(r.hi, 105.0);
        let r = three
            .clone()
            .run(&[at(100.0, 0.0), at(0.0, 0.0), at(50.0, 0.0)], 12.7)
            .unwrap();
        assert_eq!((r.from, r.dir), (at(100.0, 0.0), at(-1.0, 0.0)));
        let r = three
            .clone()
            .with_overshoot(6.0)
            .unwrap()
            .run(&[at(50.0, 0.0), at(0.0, 0.0), at(100.0, 0.0)], 12.7)
            .unwrap();
        near(r.lo, -6.0);
        near(r.hi, r.far + 6.0);
        let two = CentreLine::new(vec![p("a1"), p("a2")], prov()).unwrap();
        assert_eq!(two.run(&[at(1.0, 1.0), at(1.0, 1.0)], 12.7), None);
        assert_eq!(
            two.clone().with_ends(Some(-1.0), None),
            Err(Error::BadLength(-1.0))
        );

        let json = |l: &CentreLine| serde_json::to_string(l).unwrap();
        let ends = two.clone().with_ends(Some(4.0), Some(9.0)).unwrap();
        let back: CentreLine = serde_json::from_str(&json(&ends)).unwrap();
        assert_eq!(back, ends);
        assert!(!json(&two).contains("before") && !json(&two).contains("after"));
    }
}
