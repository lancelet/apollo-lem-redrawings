//! The drafting standard: the sizes every leader, dimension and (later)
//! section indicator is drawn to, the terminators they share, and the
//! units policy a value of ours prints by.
//!
//! Like the lettering table, this is one document-wide value in paper
//! millimetres: a leader names nothing of it and simply takes the gap,
//! the shoulder and the arrowhead from here, so changing the table
//! changes the sheet at once and our output is consistent. The
//! standard values are the medians of the 2026-10-15 survey of the
//! Grumman drawings (lmsheets/docs/annotation-requirements.md §1.2,
//! §2.2).

use crate::value::measure::{Quantity, Unit};
use serde::{Deserialize, Serialize};

/// What can be wrong with the drafting table.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A size that is not finite or not positive.
    BadSize(f64),
    /// A units policy whose unit is not a length.
    BadUnit(Unit),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// What marks the end of a leader's tip or a dimension line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminator {
    /// A filled arrowhead, its tip on the target.
    Arrow,
    /// A filled dot centred on the target.
    Dot,
    /// The line simply ends on the target.
    None,
}

/// An arrowhead's size: a filled triangle, `length` from tip to base
/// and `width` across the base, mm; the line it ends runs `inset` into
/// the head past the base, so no seam shows between them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Head {
    /// Tip to base, mm.
    pub length: f64,
    /// Across the base, mm.
    pub width: f64,
    /// How far the line runs into the head past its base, mm; zero to
    /// stop at the base, at most the length.
    pub inset: f64,
}

impl Head {
    /// A head; the sizes finite and positive, the inset finite, not
    /// negative and no more than the length.
    pub fn new(length: f64, width: f64, inset: f64) -> Result<Self, Error> {
        let h = Self {
            length,
            width,
            inset,
        };
        h.check()?;
        Ok(h)
    }

    /// Finite positive sizes; a finite inset within `0..=length`.
    pub fn check(&self) -> Result<(), Error> {
        for v in [self.length, self.width] {
            if !(v.is_finite() && v > 0.0) {
                return Err(Error::BadSize(v));
            }
        }
        if !(self.inset.is_finite() && (0.0..=self.length).contains(&self.inset)) {
            return Err(Error::BadSize(self.inset));
        }
        Ok(())
    }

    /// Where the line a head ends stops: this far short of the tip.
    pub fn shaft_short(&self) -> f64 {
        self.length - self.inset
    }
}

/// How a value of ours prints: a printed measure prints verbatim (it is
/// evidence); a value or a resolved parameter prints by this.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Units {
    /// The length unit shown (no unit mark is written, as on the sheets).
    pub unit: Unit,
    /// Decimal places for lengths and angles.
    pub decimals: u8,
    /// Whether a value below one keeps its leading zero (".188" without).
    pub leading_zero: bool,
}

impl Units {
    /// The Grumman policy: inches, three places, no leading zero.
    pub const GRUMMAN: Units = Units {
        unit: Unit::In,
        decimals: 3,
        leading_zero: false,
    };

    /// A length unit only.
    pub fn check(&self) -> Result<(), Error> {
        if self.unit.quantity() == Quantity::Length {
            Ok(())
        } else {
            Err(Error::BadUnit(self.unit))
        }
    }

    /// A value in its base unit (mm or rad) printed by the policy: a
    /// length in the policy's unit, an angle in degrees with the sign.
    pub fn print(&self, base: f64, q: Quantity) -> String {
        let (v, mark) = match q {
            Quantity::Length => (base / self.unit.to_base(), ""),
            Quantity::Angle => (base.to_degrees(), "°"),
        };
        let mut s = format!("{v:.*}", self.decimals as usize);
        // A value that rounds to zero is zero, never "-0".
        if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
            s.remove(0);
        }
        // An angle drops its trailing zeros: "30°", "22.5°".
        if q == Quantity::Angle && s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        if !self.leading_zero {
            if let Some(rest) = s.strip_prefix("0.") {
                s = format!(".{rest}");
            } else if let Some(rest) = s.strip_prefix("-0.") {
                s = format!("-.{rest}");
            }
        }
        s + mark
    }
}

/// The drafting table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct Drafting {
    leader_gap: f64,
    shoulder: f64,
    arrow: Head,
    dot: f64,
    pull_back: f64,
    balloon_diameter: f64,
    balloon_clearance: f64,
    centre_arm: f64,
    centre_overshoot: f64,
    ext_gap: f64,
    ext_overshoot: f64,
    outside_tail: f64,
    text_gap: f64,
    shelf_run: f64,
    units: Units,
}

#[derive(Serialize, Deserialize)]
struct Raw {
    leader_gap: f64,
    shoulder: f64,
    arrow: Head,
    dot: f64,
    #[serde(default)]
    pull_back: f64,
    #[serde(default = "default_balloon_diameter")]
    balloon_diameter: f64,
    #[serde(default = "default_balloon_clearance")]
    balloon_clearance: f64,
    #[serde(default = "default_centre_arm")]
    centre_arm: f64,
    #[serde(default = "default_centre_overshoot")]
    centre_overshoot: f64,
    ext_gap: f64,
    ext_overshoot: f64,
    outside_tail: f64,
    text_gap: f64,
    shelf_run: f64,
    units: Units,
}

/// The standard balloon diameter, for documents written before it was
/// a field.
fn default_balloon_diameter() -> f64 {
    Drafting::standard().balloon_diameter
}

/// The standard balloon clearance, likewise.
fn default_balloon_clearance() -> f64 {
    Drafting::standard().balloon_clearance
}

/// The standard centre mark arm and centre line overshoot, likewise.
fn default_centre_arm() -> f64 {
    Drafting::standard().centre_arm
}

fn default_centre_overshoot() -> f64 {
    Drafting::standard().centre_overshoot
}

impl From<Drafting> for Raw {
    fn from(d: Drafting) -> Self {
        Raw {
            leader_gap: d.leader_gap,
            shoulder: d.shoulder,
            arrow: d.arrow,
            dot: d.dot,
            pull_back: d.pull_back,
            balloon_diameter: d.balloon_diameter,
            balloon_clearance: d.balloon_clearance,
            centre_arm: d.centre_arm,
            centre_overshoot: d.centre_overshoot,
            ext_gap: d.ext_gap,
            ext_overshoot: d.ext_overshoot,
            outside_tail: d.outside_tail,
            text_gap: d.text_gap,
            shelf_run: d.shelf_run,
            units: d.units,
        }
    }
}

impl TryFrom<Raw> for Drafting {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        let d = Drafting {
            leader_gap: r.leader_gap,
            shoulder: r.shoulder,
            arrow: r.arrow,
            dot: r.dot,
            pull_back: r.pull_back,
            balloon_diameter: r.balloon_diameter,
            balloon_clearance: r.balloon_clearance,
            centre_arm: r.centre_arm,
            centre_overshoot: r.centre_overshoot,
            ext_gap: r.ext_gap,
            ext_overshoot: r.ext_overshoot,
            outside_tail: r.outside_tail,
            text_gap: r.text_gap,
            shelf_run: r.shelf_run,
            units: r.units,
        };
        d.check()?;
        Ok(d)
    }
}

const IN: f64 = 25.4;

impl Drafting {
    /// The standard table: the survey medians. Text to leader 0.075 in,
    /// shoulder 0.3 in, arrowhead 0.2 × 0.075 in with the line run 0.3 mm
    /// into it, dot 0.05 in across, tips exactly on their targets;
    /// extension lines 0.05 in from the feature and 0.2 in past the
    /// dimension line, outside-arrow tails 0.3 in, text 0.075 in from a
    /// line, shelves 0.3 in; balloons 0.6 in across, clearing their text
    /// by at least 0.06 in; Grumman units.
    pub fn standard() -> Self {
        Self {
            leader_gap: 0.075 * IN,
            shoulder: 0.3 * IN,
            arrow: Head {
                length: 0.2 * IN,
                width: 0.075 * IN,
                inset: 0.3,
            },
            dot: 0.05 * IN,
            pull_back: 0.0,
            balloon_diameter: 0.6 * IN,
            balloon_clearance: 0.06 * IN,
            // The survey: arms 0.15 in, lines 0.2–0.75 in past the
            // features.
            centre_arm: 0.15 * IN,
            centre_overshoot: 0.25 * IN,
            ext_gap: 0.05 * IN,
            ext_overshoot: 0.2 * IN,
            outside_tail: 0.3 * IN,
            text_gap: 0.075 * IN,
            shelf_run: 0.3 * IN,
            units: Units::GRUMMAN,
        }
    }

    /// Every size finite and positive; the pull-back finite and not
    /// negative; the units a length.
    pub fn check(&self) -> Result<(), Error> {
        for v in [
            self.leader_gap,
            self.shoulder,
            self.balloon_diameter,
            self.balloon_clearance,
            self.centre_arm,
            self.centre_overshoot,
            self.dot,
            self.ext_gap,
            self.ext_overshoot,
            self.outside_tail,
            self.text_gap,
            self.shelf_run,
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(Error::BadSize(v));
            }
        }
        if !(self.pull_back.is_finite() && self.pull_back >= 0.0) {
            return Err(Error::BadSize(self.pull_back));
        }
        self.arrow.check()?;
        self.units.check()
    }

    /// The same table with another arrowhead.
    pub fn with_arrow(mut self, arrow: Head) -> Result<Self, Error> {
        self.arrow = arrow;
        self.check()?;
        Ok(self)
    }

    /// The same table with another pull-back.
    pub fn with_pull_back(mut self, mm: f64) -> Result<Self, Error> {
        self.pull_back = mm;
        self.check()?;
        Ok(self)
    }

    /// The same table with another shoulder, mm.
    pub fn with_shoulder(mut self, mm: f64) -> Result<Self, Error> {
        self.shoulder = mm;
        self.check()?;
        Ok(self)
    }

    /// The same table with extension lines starting `gap` mm from the
    /// feature and running `overshoot` mm past the dimension line.
    pub fn with_extension(mut self, gap: f64, overshoot: f64) -> Result<Self, Error> {
        self.ext_gap = gap;
        self.ext_overshoot = overshoot;
        self.check()?;
        Ok(self)
    }

    /// The same table with another tail behind an outside arrow, mm.
    pub fn with_outside_tail(mut self, mm: f64) -> Result<Self, Error> {
        self.outside_tail = mm;
        self.check()?;
        Ok(self)
    }

    /// The same table with another space between a dimension line and
    /// its text, mm.
    pub fn with_text_gap(mut self, mm: f64) -> Result<Self, Error> {
        self.text_gap = mm;
        self.check()?;
        Ok(self)
    }

    /// The same table with another units policy.
    pub fn with_units(mut self, units: Units) -> Result<Self, Error> {
        self.units = units;
        self.check()?;
        Ok(self)
    }

    /// From the text's anchor to where the leader starts, mm.
    pub fn leader_gap(&self) -> f64 {
        self.leader_gap
    }

    /// The standard shoulder, mm: the horizontal run before the knee.
    pub fn shoulder(&self) -> f64 {
        self.shoulder
    }

    /// The arrowhead on leaders and dimensions.
    pub fn arrow(&self) -> Head {
        self.arrow
    }

    /// The dot terminator's diameter, mm.
    pub fn dot(&self) -> f64 {
        self.dot
    }

    /// How far short of its target a tip stops, mm, along its arrival:
    /// drawing space between the feature and the arrow. Zero as the
    /// Grumman sheets have it.
    pub fn pull_back(&self) -> f64 {
        self.pull_back
    }

    /// A balloon's diameter, mm: the ring round an enclosed text, unless
    /// the text needs a larger one.
    pub fn balloon_diameter(&self) -> f64 {
        self.balloon_diameter
    }

    /// The least clearance between an enclosed text's box and its ring,
    /// mm: a ring grows past the standard diameter to keep it.
    pub fn balloon_clearance(&self) -> f64 {
        self.balloon_clearance
    }

    /// A centre mark's arm, centre to tip, mm.
    pub fn centre_arm(&self) -> f64 {
        self.centre_arm
    }

    /// How far a centre line runs past its outermost points, mm.
    pub fn centre_overshoot(&self) -> f64 {
        self.centre_overshoot
    }

    /// The same table with this balloon diameter, mm.
    pub fn with_balloon_diameter(mut self, mm: f64) -> Result<Self, Error> {
        self.balloon_diameter = mm;
        self.check()?;
        Ok(self)
    }

    /// The same table with this balloon clearance, mm.
    pub fn with_balloon_clearance(mut self, mm: f64) -> Result<Self, Error> {
        self.balloon_clearance = mm;
        self.check()?;
        Ok(self)
    }

    /// From the feature to where its extension line starts, mm.
    pub fn ext_gap(&self) -> f64 {
        self.ext_gap
    }

    /// How far an extension line runs past the dimension line, mm.
    pub fn ext_overshoot(&self) -> f64 {
        self.ext_overshoot
    }

    /// The tail behind an outside arrow, mm.
    pub fn outside_tail(&self) -> f64 {
        self.outside_tail
    }

    /// From a dimension line to its text, mm: the clear space either side
    /// of inline text, and the space before text on a shelf.
    pub fn text_gap(&self) -> f64 {
        self.text_gap
    }

    /// The standard run of a shelf past the extension line, mm.
    pub fn shelf_run(&self) -> f64 {
        self.shelf_run
    }

    /// How a value of ours prints.
    pub fn units(&self) -> Units {
        self.units
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_is_the_survey_and_files_are_checked() {
        let d = Drafting::standard();
        assert!((d.shoulder() - 7.62).abs() < 1e-9);
        assert!((d.arrow().length - 5.08).abs() < 1e-9);
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Drafting>(&json).unwrap(), d);
        let bad = json.replace("\"dot\":1.27", "\"dot\":0.0");
        assert_ne!(bad, json);
        assert!(serde_json::from_str::<Drafting>(&bad).is_err());
        assert_eq!(Head::new(1.0, -1.0, 0.0), Err(Error::BadSize(-1.0)));
        assert_eq!(Head::new(1.0, 1.0, 1.5), Err(Error::BadSize(1.5)));
        assert_eq!(d.clone().with_pull_back(-0.5), Err(Error::BadSize(-0.5)));
        // The sheet's own sizes (LDW280-17619 sh3: extension lines
        // nearly touching, short overshoots, long outside tails) are
        // set and checked like the rest.
        let own = d
            .clone()
            .with_shoulder(5.0)
            .and_then(|d| d.with_extension(0.3, 2.5))
            .and_then(|d| d.with_outside_tail(11.4))
            .and_then(|d| d.with_text_gap(2.5))
            .unwrap();
        assert_eq!(
            (own.shoulder(), own.ext_gap(), own.ext_overshoot()),
            (5.0, 0.3, 2.5)
        );
        assert_eq!((own.outside_tail(), own.text_gap()), (11.4, 2.5));
        assert_eq!(d.clone().with_extension(0.0, 2.0), Err(Error::BadSize(0.0)));
        assert_eq!(d.clone().with_shoulder(-1.0), Err(Error::BadSize(-1.0)));
        assert!(d.clone().with_outside_tail(f64::NAN).is_err());
        // The balloon sizes are checked, and a file from before they
        // were fields gets the standard ones.
        let d = Drafting::standard();
        assert_eq!(
            d.clone().with_balloon_diameter(0.0),
            Err(Error::BadSize(0.0))
        );
        assert_eq!(d.with_balloon_clearance(-1.0), Err(Error::BadSize(-1.0)));
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v.as_object_mut().unwrap().remove("balloon_diameter");
        v.as_object_mut().unwrap().remove("balloon_clearance");
        let old: Drafting = serde_json::from_value(v).unwrap();
        assert!((old.balloon_diameter() - 0.6 * IN).abs() < 1e-9);
        assert!((old.balloon_clearance() - 0.06 * IN).abs() < 1e-9);
        assert_eq!(
            Drafting::standard().with_units(Units {
                unit: Unit::Deg,
                decimals: 1,
                leading_zero: true
            }),
            Err(Error::BadUnit(Unit::Deg))
        );
    }

    #[test]
    fn values_print_by_the_policy() {
        let g = Units::GRUMMAN;
        assert_eq!(g.print(4.7752, Quantity::Length), ".188");
        assert_eq!(g.print(-4.7752, Quantity::Length), "-.188");
        assert_eq!(g.print(81.28, Quantity::Length), "3.200");
        assert_eq!(g.print(std::f64::consts::FRAC_PI_4, Quantity::Angle), "45°");
        assert_eq!(g.print(22.5f64.to_radians(), Quantity::Angle), "22.5°");
        let mm = Units {
            unit: Unit::Mm,
            decimals: 1,
            leading_zero: true,
        };
        assert_eq!(mm.print(0.25, Quantity::Length), "0.2");
        assert_eq!(mm.print(60.0, Quantity::Length), "60.0");
    }
}
