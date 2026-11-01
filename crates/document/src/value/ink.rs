//! Ink as drawn: how the sheet's own hand drew an item, as strokes.
//!
//! An item whose shape the format does not reproduce, a text above all
//! (set in our regular lettering, never glyph for glyph), may carry the
//! strokes the drafter drew for it: centre-line polylines on the paper,
//! each vertex with the stroke's width there. They are a record of the
//! ink, not what the item is: the text is still its reading, placement
//! and size, and prints in our lettering. They let a scan-faithful
//! rendering reproduce the sheet (the coverage check's subtraction, its
//! labels, training data for reading hands), and they carry their own
//! provenance (measured from the scan), apart from the item's.

use crate::value::geometry::{Paper, P2};
use crate::value::provenance::Provenance;
use serde::{Deserialize, Serialize};

/// What makes ink as drawn invalid.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A stroke with no points.
    Empty,
    /// A stroke whose widths do not match its points one for one.
    Widths {
        /// Points.
        points: usize,
        /// Widths.
        widths: usize,
    },
    /// A point or width that is not finite, or a width that is negative.
    Bad,
    /// Its provenance is not valid.
    Provenance(crate::value::provenance::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "a stroke with no points"),
            Error::Widths { points, widths } => {
                write!(f, "a stroke of {points} points with {widths} widths")
            }
            Error::Bad => write!(f, "a point or width not finite, or a width below zero"),
            Error::Provenance(e) => write!(f, "provenance: {e:?}"),
        }
    }
}

impl std::error::Error for Error {}

/// One stroke: a centre-line polyline and its width at each point, mm.
/// A single point is a dot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// The centre-line, in order.
    pub points: Vec<P2<Paper>>,
    /// The width at each point.
    pub widths: Vec<f64>,
}

impl Stroke {
    fn check(&self) -> Result<(), Error> {
        if self.points.is_empty() {
            return Err(Error::Empty);
        }
        if self.widths.len() != self.points.len() {
            return Err(Error::Widths {
                points: self.points.len(),
                widths: self.widths.len(),
            });
        }
        let finite = self
            .points
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite());
        if !finite || self.widths.iter().any(|w| !(w.is_finite() && *w >= 0.0)) {
            return Err(Error::Bad);
        }
        Ok(())
    }
}

/// An item's ink as drawn: its strokes and where they come from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct AsDrawn {
    strokes: Vec<Stroke>,
    provenance: Provenance,
}

#[derive(Serialize, Deserialize)]
struct Raw {
    strokes: Vec<Stroke>,
    provenance: Provenance,
}

impl From<AsDrawn> for Raw {
    fn from(a: AsDrawn) -> Self {
        Raw {
            strokes: a.strokes,
            provenance: a.provenance,
        }
    }
}

impl TryFrom<Raw> for AsDrawn {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        AsDrawn::new(r.strokes, r.provenance)
    }
}

impl AsDrawn {
    /// Ink of these strokes.
    pub fn new(strokes: Vec<Stroke>, provenance: Provenance) -> Result<Self, Error> {
        let a = AsDrawn {
            strokes,
            provenance,
        };
        a.check()?;
        Ok(a)
    }

    /// Everything it must satisfy.
    pub fn check(&self) -> Result<(), Error> {
        self.provenance.check().map_err(Error::Provenance)?;
        self.strokes.iter().try_for_each(Stroke::check)
    }

    /// The strokes.
    pub fn strokes(&self) -> &[Stroke] {
        &self.strokes
    }

    /// Where they come from.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::provenance::{Agent, Citation, Date, Method};

    fn prov() -> Provenance {
        Provenance::new(
            Method::Measured { sigma_mm: 0.1 },
            Agent::User,
            Date::new(2026, 10, 21).unwrap(),
        )
        .cite(Citation::new("scan", "ink of segment s1"))
    }

    /// A stroke needs a width per point, finite and not negative; ink
    /// survives its file form.
    #[test]
    fn strokes_check_and_round_trip() {
        let s = |n: usize, w: usize| Stroke {
            points: (0..n).map(|k| P2::new(k as f64, 0.0)).collect(),
            widths: vec![0.3; w],
        };
        assert_eq!(AsDrawn::new(vec![s(0, 0)], prov()), Err(Error::Empty));
        assert_eq!(
            AsDrawn::new(vec![s(2, 1)], prov()),
            Err(Error::Widths {
                points: 2,
                widths: 1
            })
        );
        let mut bad = s(2, 2);
        bad.widths[1] = -1.0;
        assert_eq!(AsDrawn::new(vec![bad], prov()), Err(Error::Bad));
        let a = AsDrawn::new(vec![s(3, 3), s(1, 1)], prov()).unwrap();
        let back: AsDrawn = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back, a);
    }
}
