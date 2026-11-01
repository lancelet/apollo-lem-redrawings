//! An unmodelled mark: something on the sheet the format has no type
//! for yet (a weld symbol, a joggle mark), named in words and bounded
//! by a region of the paper.
//!
//! It is the escape hatch that keeps a sheet's account complete: the
//! scan's ink inside the region is its ink, so nothing on the sheet is
//! silently left out, and every one is a feature still to be modelled.
//! It draws nothing on paper (the screen shows its outline); it goes on
//! the working `unmodelled` layer.

use crate::value::geometry::{Paper, P2};
use crate::value::provenance::Provenance;
use crate::value::sketch::LayerId;
use serde::{Deserialize, Serialize};

id!(
    /// The name of an unmodelled mark.
    UnmodelledId
);

/// What makes an unmodelled mark invalid on its own.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// It says nothing of what it looks like.
    NoDescription,
    /// Its region has fewer than three corners.
    TooFewCorners(usize),
    /// A corner is not finite.
    NotFinite,
    /// Its region encloses no area.
    NoArea,
    /// Its provenance is not valid.
    Provenance(crate::value::provenance::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NoDescription => write!(f, "an unmodelled mark says nothing of itself"),
            Error::TooFewCorners(n) => write!(f, "a region of {n} corner(s), not three"),
            Error::NotFinite => write!(f, "a corner is not finite"),
            Error::NoArea => write!(f, "a region with no area"),
            Error::Provenance(e) => write!(f, "provenance: {e:?}"),
        }
    }
}

impl std::error::Error for Error {}

/// An unmodelled mark.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct Unmodelled {
    what: String,
    region: Vec<P2<Paper>>,
    layer: LayerId,
    provenance: Provenance,
    symbol: Option<Symbol>,
}

#[derive(Serialize, Deserialize)]
struct Raw {
    what: String,
    region: Vec<P2<Paper>>,
    layer: LayerId,
    provenance: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    symbol: Option<Symbol>,
}

/// The symbol an unmodelled mark is, as the markup named it: its name
/// on the sheet and its kind, the same for every symbol of that kind on
/// any sheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    /// Its name on the sheet, `S12`.
    pub name: String,
    /// What kind of symbol, `tape-strip`.
    pub kind: String,
}

impl From<Unmodelled> for Raw {
    fn from(u: Unmodelled) -> Self {
        Raw {
            what: u.what,
            region: u.region,
            layer: u.layer,
            provenance: u.provenance,
            symbol: u.symbol,
        }
    }
}

impl TryFrom<Raw> for Unmodelled {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        let u = Unmodelled {
            what: r.what,
            region: r.region,
            layer: r.layer,
            provenance: r.provenance,
            symbol: r.symbol,
        };
        u.check()?;
        Ok(u)
    }
}

impl Unmodelled {
    /// A mark described by `what`, inside `region` (a polygon on the
    /// paper, mm), on the `unmodelled` layer.
    pub fn new(what: &str, region: Vec<P2<Paper>>, provenance: Provenance) -> Result<Self, Error> {
        let u = Unmodelled {
            what: what.trim().to_string(),
            region,
            layer: LayerId::new("unmodelled"),
            provenance,
            symbol: None,
        };
        u.check()?;
        Ok(u)
    }

    /// The same mark, said to be `symbol`.
    pub fn of_symbol(self, symbol: Symbol) -> Self {
        Self {
            symbol: Some(symbol),
            ..self
        }
    }

    /// The symbol it is, when the markup named one.
    pub fn symbol(&self) -> Option<&Symbol> {
        self.symbol.as_ref()
    }

    /// Everything it must satisfy on its own.
    pub fn check(&self) -> Result<(), Error> {
        self.provenance.check().map_err(Error::Provenance)?;
        let unnamed = |y: &Symbol| y.name.trim().is_empty() || y.kind.trim().is_empty();
        if self.what.trim().is_empty() || self.symbol.as_ref().is_some_and(unnamed) {
            return Err(Error::NoDescription);
        }
        if self.region.len() < 3 {
            return Err(Error::TooFewCorners(self.region.len()));
        }
        if self
            .region
            .iter()
            .any(|p| !(p.x.is_finite() && p.y.is_finite()))
        {
            return Err(Error::NotFinite);
        }
        if self.area() <= 0.0 {
            return Err(Error::NoArea);
        }
        Ok(())
    }

    /// The region's area, mm² (either winding).
    pub fn area(&self) -> f64 {
        let n = self.region.len();
        let twice: f64 = (0..n)
            .map(|i| {
                let (a, b) = (self.region[i], self.region[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum();
        twice.abs() / 2.0
    }

    /// Whether a point of the paper is inside the region.
    pub fn contains(&self, p: P2<Paper>) -> bool {
        let n = self.region.len();
        let mut inside = false;
        for i in 0..n {
            let (a, b) = (self.region[i], self.region[(i + n - 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        }
        inside
    }

    /// What it looks like, in words.
    pub fn what(&self) -> &str {
        &self.what
    }

    /// Its region's corners, mm.
    pub fn region(&self) -> &[P2<Paper>] {
        &self.region
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::provenance::{Agent, Date, Method};

    fn prov() -> Provenance {
        Provenance::new(
            Method::Authored,
            Agent::User,
            Date::new(2026, 10, 21).unwrap(),
        )
    }

    /// It needs words and an area; it knows its inside; it survives its
    /// file form.
    #[test]
    fn regions_check_contain_and_round_trip() {
        let sq = vec![
            P2::new(0.0, 0.0),
            P2::new(4.0, 0.0),
            P2::new(4.0, 2.0),
            P2::new(0.0, 2.0),
        ];
        assert_eq!(
            Unmodelled::new(" ", sq.clone(), prov()),
            Err(Error::NoDescription)
        );
        assert_eq!(
            Unmodelled::new("weld", sq[..2].to_vec(), prov()),
            Err(Error::TooFewCorners(2))
        );
        let flat = vec![P2::new(0.0, 0.0), P2::new(1.0, 0.0), P2::new(2.0, 0.0)];
        assert_eq!(Unmodelled::new("weld", flat, prov()), Err(Error::NoArea));
        let u = Unmodelled::new("weld symbol", sq, prov())
            .unwrap()
            .of_symbol(Symbol {
                name: "S1".into(),
                kind: "weld".into(),
            });
        assert_eq!(u.check(), Ok(()));
        assert_eq!(u.area(), 8.0);
        assert!(u.contains(P2::new(1.0, 1.0)));
        assert!(!u.contains(P2::new(5.0, 1.0)));
        let back: Unmodelled = serde_json::from_str(&serde_json::to_string(&u).unwrap()).unwrap();
        assert_eq!(back, u);
    }
}
