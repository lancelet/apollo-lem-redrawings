//! The sheet's measured line weights: the pen each kind of line was drawn
//! with on the original, with the evidence it was measured from.
//!
//! A layer's weight is the standard pen of its convention (0.7 mm thick,
//! 0.35 mm thin). A drawing's own pens differ by sheet and, on one sheet,
//! by kind of line (the boxed X of the marking region is lighter than the
//! outline). The table holds what was measured, per [`LineKind`], as a
//! [`Measured`] width with its sample count and spread; the renderer
//! draws a line of a kind that has an entry at that width, and any other
//! at its layer's standard pen. Which kind a line is follows from what it
//! is in the document (its convention, a dimension, a marking region's
//! side), never from its layer's name. Lettering's stroke is not here: it
//! is a share of the cap ([`Pen`](crate::value::style::Pen)).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A kind of line, as far as its pen goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    /// An edge of the part in view (visible convention).
    Outline,
    /// An edge behind the surface.
    Hidden,
    /// An axis or hole's centre line, and centre marks.
    Centre,
    /// The sheet's format: border and title block lines.
    Format,
    /// The six lines of a marking region (its sides and diagonals: on
    /// 17619 they measure the same, within a pixel).
    MarkingRegion,
    /// A dimension line, its tails and heads.
    Dimension,
    /// An extension (witness) line.
    Extension,
    /// A leader.
    Leader,
}

/// What can be wrong with a line weight.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A width that is not finite or not positive.
    BadWidth(f64),
    /// No samples, or a spread that does not hold the width.
    BadEvidence,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// A width measured on a sheet, and what it came from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    /// The pen's width, paper mm: the mean of the samples kept.
    pub mm: f64,
    /// How many places along the kind's lines it was measured at.
    pub samples: u32,
    /// The samples' spread, paper mm: their lower and upper quartiles.
    pub spread: [f64; 2],
}

impl Measured {
    /// Finite positive width, at least one sample, an ordered spread.
    pub fn check(&self) -> Result<(), Error> {
        if !(self.mm.is_finite() && self.mm > 0.0) {
            return Err(Error::BadWidth(self.mm));
        }
        let [lo, hi] = self.spread;
        if self.samples == 0 || !(lo.is_finite() && hi.is_finite() && 0.0 < lo && lo <= hi) {
            return Err(Error::BadEvidence);
        }
        Ok(())
    }
}

/// The sheet's measured pens, by kind of line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LineWeights {
    kinds: BTreeMap<LineKind, Measured>,
}

impl LineWeights {
    /// No kind measured: every line at its layer's standard pen.
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// A kind's measured weight, if the sheet has one.
    pub fn get(&self, kind: LineKind) -> Option<&Measured> {
        self.kinds.get(&kind)
    }

    /// The width a line of `kind` is drawn at: the sheet's measured one,
    /// else `standard` (its layer's).
    pub fn width(&self, kind: Option<LineKind>, standard: f64) -> f64 {
        kind.and_then(|k| self.kinds.get(&k))
            .map_or(standard, |m| m.mm)
    }

    /// The measured kinds, in order.
    pub fn iter(&self) -> impl Iterator<Item = (LineKind, &Measured)> {
        self.kinds.iter().map(|(k, m)| (*k, m))
    }

    /// Set or, with `None`, clear a kind's weight.
    pub fn set(&mut self, kind: LineKind, weight: Option<Measured>) -> Result<(), Error> {
        match weight {
            Some(m) => {
                m.check()?;
                self.kinds.insert(kind, m);
            }
            None => {
                self.kinds.remove(&kind);
            }
        }
        Ok(())
    }

    /// Every entry valid (for a file).
    pub fn check(&self) -> Result<(), Error> {
        self.kinds.values().try_for_each(Measured::check)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_takes_its_measured_pen_else_the_standard() {
        let mut w = LineWeights::default();
        assert!(w.is_empty());
        assert_eq!(w.width(Some(LineKind::Outline), 0.7), 0.7);
        let m = Measured {
            mm: 0.55,
            samples: 40,
            spread: [0.5, 0.6],
        };
        w.set(LineKind::Outline, Some(m)).unwrap();
        assert_eq!(w.width(Some(LineKind::Outline), 0.7), 0.55);
        assert_eq!(w.width(Some(LineKind::Centre), 0.35), 0.35);
        assert_eq!(w.width(None, 0.35), 0.35);
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(serde_json::from_str::<LineWeights>(&json).unwrap(), w);
        assert!(w
            .set(LineKind::Hidden, Some(Measured { mm: 0.0, ..m }))
            .is_err());
        assert!(w
            .set(LineKind::Hidden, Some(Measured { samples: 0, ..m }))
            .is_err());
        assert!(w
            .set(
                LineKind::Hidden,
                Some(Measured {
                    spread: [0.6, 0.5],
                    ..m
                })
            )
            .is_err());
        w.set(LineKind::Outline, None).unwrap();
        assert!(w.is_empty());
    }
}
