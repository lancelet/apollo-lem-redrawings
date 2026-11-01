//! The lettering standard: how text is sized and drawn, by role.
//!
//! A text names a role (`text`, `view_title`, …) and takes its cap
//! height and width factor from the document's lettering table, the way
//! an entity names a layer; the table changes the sheet at once. Its
//! stroke is its layer's [`Pen`](crate::value::style::Pen), a share of
//! the cap height on the standard layers. A text may instead state a
//! cap height outright, and may override the width factor or the
//! stroke: legitimate, visible exceptions, as a measured dash pattern is
//! for a line. Sizes are paper millimetres, so lettering is the same
//! size in every view whatever the view's scale, as on a drawing.
//!
//! The standard table comes from the sizes measured on LDW280-23631 sh3
//! (apollo-lunar-module/docs/lettering/sizes.json): cap heights by role
//! in inches and a width factor of 0.8.

use crate::value::text::{StackAlign, StackKind, StackStyle};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ustr::Ustr;

/// The name of a lettering role, an interned `Copy` handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RoleId(Ustr);

impl RoleId {
    /// A name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&str> for RoleId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<&RoleId> for RoleId {
    fn from(id: &RoleId) -> Self {
        *id
    }
}

impl std::fmt::Display for RoleId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What can be wrong with lettering.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A cap height that is not finite or not positive.
    BadCap(f64),
    /// A width factor that is not finite or not positive.
    BadWidth(f64),
    /// An underline offset that is not finite or not positive.
    BadUnderline(f64),
    /// A line pitch below one cap height.
    BadPitch(f64),
    /// A stack style with a scale that is not positive or a gap that is
    /// negative.
    BadStack(f64, f64),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// How one role is lettered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Role {
    /// Cap height, mm on the paper.
    pub cap: f64,
    /// Whether the text is underlined (view titles are).
    pub underline: bool,
}

impl Role {
    /// A role; the cap must be finite and positive.
    pub fn new(cap: f64, underline: bool) -> Result<Self, Error> {
        let r = Self { cap, underline };
        r.check()?;
        Ok(r)
    }

    /// Finite positive cap.
    pub fn check(&self) -> Result<(), Error> {
        if self.cap.is_finite() && self.cap > 0.0 {
            Ok(())
        } else {
            Err(Error::BadCap(self.cap))
        }
    }
}

/// The lettering table: the roles and what every role shares.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct Lettering {
    roles: BTreeMap<RoleId, Role>,
    width_factor: f64,
    pitch_of_cap: f64,
    underline_below_cap: f64,
    fraction: StackStyle,
    sheet_fraction: Option<SheetFraction>,
    limits: StackStyle,
    tolerance: StackStyle,
}

/// A sheet's one stacked-fraction digit size, which replaces the
/// table's share of each text's cap: on a drawing the digits of a
/// stacked value are one size whatever the role of the text around it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetFraction {
    /// The rows' cap height, paper mm: finite and positive.
    pub cap: f64,
    /// Each row's distance from the line's middle, a share of this cap
    /// (the clear space between digits and bar goes with the digits'
    /// size and pen, not with the text beside the fraction): finite and
    /// not negative.
    pub gap_of_cap: f64,
}

#[derive(Serialize, Deserialize)]
struct Raw {
    roles: BTreeMap<RoleId, Role>,
    width_factor: f64,
    pitch_of_cap: f64,
    underline_below_cap: f64,
    fraction: StackStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sheet_fraction: Option<SheetFraction>,
    limits: StackStyle,
    tolerance: StackStyle,
}

impl From<Lettering> for Raw {
    fn from(l: Lettering) -> Self {
        Raw {
            roles: l.roles,
            width_factor: l.width_factor,
            pitch_of_cap: l.pitch_of_cap,
            underline_below_cap: l.underline_below_cap,
            fraction: l.fraction,
            sheet_fraction: l.sheet_fraction,
            limits: l.limits,
            tolerance: l.tolerance,
        }
    }
}

impl TryFrom<Raw> for Lettering {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        let l = Lettering {
            roles: r.roles,
            width_factor: r.width_factor,
            pitch_of_cap: r.pitch_of_cap,
            underline_below_cap: r.underline_below_cap,
            fraction: r.fraction,
            sheet_fraction: r.sheet_fraction,
            limits: r.limits,
            tolerance: r.tolerance,
        };
        l.check()?;
        Ok(l)
    }
}

impl SheetFraction {
    /// A finite positive cap and a finite non-negative gap.
    pub fn check(&self) -> Result<(), Error> {
        let ok = self.cap.is_finite() && self.cap > 0.0;
        if ok && self.gap_of_cap.is_finite() && self.gap_of_cap >= 0.0 {
            Ok(())
        } else {
            Err(Error::BadStack(self.cap, self.gap_of_cap))
        }
    }
}

const IN: f64 = 25.4;

impl Lettering {
    /// The standard table: the roles and sizes measured on the sheets,
    /// width factor 0.8, the font's line pitch (1.65 cap heights),
    /// underlines 0.3 cap below the baseline; fractions at 0.7 of the cap
    /// with a bar, the rows 0.26 cap from it, centred; limits at full
    /// size with a bar, 0.2 from it, on their points; a plus over a
    /// minus at 0.6 with no bar, 0.2 from the middle, on their points.
    pub fn standard() -> Self {
        let role = |cap_in: f64, underline| Role {
            cap: cap_in * IN,
            underline,
        };
        Self {
            roles: [
                ("text", role(0.205, false)),
                ("general_note", role(0.205, false)),
                ("dimension", role(0.205, false)),
                ("view_title", role(0.3, true)),
                ("section_arrow", role(0.45, false)),
                ("zone_label", role(0.25, false)),
                ("border_drawing_number", role(0.25, false)),
                ("form_heading", role(0.17, false)),
                ("form_print", role(0.135, false)),
                ("title_fill", role(0.27, false)),
                ("title_number", role(0.27, false)),
                ("title_label", role(0.12, false)),
                ("title_company", role(0.15, false)),
            ]
            .into_iter()
            .map(|(id, r)| (RoleId::new(id), r))
            .collect(),
            width_factor: 0.8,
            pitch_of_cap: 1.65,
            underline_below_cap: 0.3,
            fraction: StackStyle {
                bar: true,
                scale: 0.7,
                gap: 0.26,
                align: StackAlign::Centre,
            },
            sheet_fraction: None,
            limits: StackStyle {
                bar: true,
                scale: 1.0,
                gap: 0.2,
                align: StackAlign::Point,
            },
            tolerance: StackStyle {
                bar: false,
                scale: 0.6,
                gap: 0.2,
                align: StackAlign::Point,
            },
        }
    }

    /// How a stack of a kind is set in a text of cap `cap` mm: as
    /// [`stack`](Self::stack), except that a fraction takes the sheet's
    /// own digit size where it has one, whatever the text's cap.
    pub fn stack_at(&self, kind: &StackKind, cap: f64) -> StackStyle {
        match (kind, self.sheet_fraction) {
            (StackKind::Fraction, Some(f)) => StackStyle {
                scale: f.cap / cap,
                gap: f.gap_of_cap * f.cap / cap,
                ..self.fraction
            },
            _ => self.stack(kind),
        }
    }

    /// The sheet's one fraction digit size, if it has one.
    pub fn sheet_fraction(&self) -> Option<SheetFraction> {
        self.sheet_fraction
    }

    /// The table's style of a kind of stack (a share of the text's cap;
    /// a fraction's is replaced by the sheet's, see
    /// [`stack_at`](Self::stack_at)), or the stated one.
    pub fn stack(&self, kind: &StackKind) -> StackStyle {
        match kind {
            StackKind::Fraction => self.fraction,
            StackKind::Limits => self.limits,
            StackKind::PlusMinus => self.tolerance,
            StackKind::Custom { style } => *style,
        }
    }

    /// Every value finite and positive; the pitch at least one cap.
    pub fn check(&self) -> Result<(), Error> {
        for r in self.roles.values() {
            r.check()?;
        }
        if !(self.width_factor.is_finite() && self.width_factor > 0.0) {
            return Err(Error::BadWidth(self.width_factor));
        }
        if !(self.pitch_of_cap.is_finite() && self.pitch_of_cap >= 1.0) {
            return Err(Error::BadPitch(self.pitch_of_cap));
        }
        if !(self.underline_below_cap.is_finite() && self.underline_below_cap > 0.0) {
            return Err(Error::BadUnderline(self.underline_below_cap));
        }
        for s in [&self.fraction, &self.limits, &self.tolerance] {
            s.check().map_err(|_| Error::BadStack(s.scale, s.gap))?;
        }
        if let Some(f) = &self.sheet_fraction {
            f.check()?;
        }
        Ok(())
    }

    /// A role.
    pub fn role(&self, id: impl Into<RoleId>) -> Option<&Role> {
        self.roles.get(&id.into())
    }

    /// The roles, in id order.
    pub fn roles(&self) -> impl Iterator<Item = (&RoleId, &Role)> {
        self.roles.iter()
    }

    /// Add or replace a role.
    pub fn set_role(&mut self, id: impl Into<RoleId>, role: Role) -> Result<Option<Role>, Error> {
        role.check()?;
        Ok(self.roles.insert(id.into(), role))
    }

    /// Remove a role (the document refuses while a text uses it).
    pub(crate) fn remove_role(&mut self, id: impl Into<RoleId>) -> Option<Role> {
        self.roles.remove(&id.into())
    }

    /// Set the width factor every role shares (a sheet's measured one);
    /// finite and positive.
    pub fn set_width_factor(&mut self, width_factor: f64) -> Result<(), Error> {
        if !(width_factor.is_finite() && width_factor > 0.0) {
            return Err(Error::BadWidth(width_factor));
        }
        self.width_factor = width_factor;
        Ok(())
    }

    /// Set the line pitch, baseline to baseline, as a share of the cap
    /// height (a sheet's measured one); at least one cap.
    pub fn set_pitch_of_cap(&mut self, pitch_of_cap: f64) -> Result<(), Error> {
        if !(pitch_of_cap.is_finite() && pitch_of_cap >= 1.0) {
            return Err(Error::BadPitch(pitch_of_cap));
        }
        self.pitch_of_cap = pitch_of_cap;
        Ok(())
    }

    /// Set the sheet's one fraction digit size (paper mm) and each
    /// row's distance from the middle (a share of that cap).
    pub fn set_fraction(&mut self, cap: f64, gap_of_cap: f64) -> Result<(), Error> {
        let f = SheetFraction { cap, gap_of_cap };
        f.check()?;
        self.sheet_fraction = Some(f);
        Ok(())
    }

    /// Glyph width as a share of the natural width: the standard's 0.8.
    pub fn width_factor(&self) -> f64 {
        self.width_factor
    }

    /// Baseline to baseline, as a share of the cap height.
    pub fn pitch_of_cap(&self) -> f64 {
        self.pitch_of_cap
    }

    /// How far below the baseline an underline sits, as a share of the
    /// cap height.
    pub fn underline_below_cap(&self) -> f64 {
        self.underline_below_cap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_is_the_measured_one_and_files_are_checked() {
        let l = Lettering::standard();
        assert!((l.role("text").unwrap().cap - 5.207).abs() < 1e-9);
        assert!(l.role("view_title").unwrap().underline);
        assert_eq!(l.role("nope"), None);
        let json = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<Lettering>(&json).unwrap(), l);
        let bad = json.replace("\"pitch_of_cap\":1.65", "\"pitch_of_cap\":0.5");
        assert_ne!(bad, json);
        assert!(serde_json::from_str::<Lettering>(&bad).is_err());
        assert_eq!(Role::new(0.0, false), Err(Error::BadCap(0.0)));
        // A sheet's measured width factor and pitch replace the table's.
        let mut m = Lettering::standard();
        m.set_width_factor(1.07).unwrap();
        m.set_pitch_of_cap(1.86).unwrap();
        assert_eq!((m.width_factor(), m.pitch_of_cap()), (1.07, 1.86));
        assert_eq!(m.set_width_factor(0.0), Err(Error::BadWidth(0.0)));
        assert_eq!(m.set_pitch_of_cap(0.9), Err(Error::BadPitch(0.9)));
        // The sheet's one fraction cap: absolute, the same in any text.
        m.set_fraction(4.0, 0.1).unwrap();
        let (a, b) = (
            m.stack_at(&StackKind::Fraction, 5.0),
            m.stack_at(&StackKind::Fraction, 8.0),
        );
        assert!((a.scale * 5.0 - 4.0).abs() < 1e-12 && (b.scale * 8.0 - 4.0).abs() < 1e-12);
        assert!((a.gap * 5.0 - 0.4).abs() < 1e-12 && (b.gap * 8.0 - 0.4).abs() < 1e-12);
        assert_eq!(
            m.stack_at(&StackKind::Limits, 5.0),
            m.stack(&StackKind::Limits)
        );
        assert!(m.set_fraction(0.0, 0.1).is_err());
        assert!(m.set_fraction(4.0, -0.1).is_err());
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Lettering>(&json).unwrap(), m);
    }
}
