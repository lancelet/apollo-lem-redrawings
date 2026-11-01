//! Parameters: the document's table of named, shared values.
//!
//! A drawing's views are projections of one part, so a value a part has,
//! a hole's radius, a stock thickness, belongs to the document, not to any
//! one view. The document owns the one [`Params`] table, and creates,
//! replaces and removes entries through its own operations. A sketch, a
//! view's or the sheet's, only *names* a parameter, as the value of a
//! constraint (`Measure::Param`); it holds no values of its own and is
//! an open term, closed by the table it is given when it is checked,
//! lowered or solved.
//!
//! Invariants, kept on every path in, the file form included: a name is
//! a valid parameter name, a value is never itself a parameter, and the
//! provenance passes its checks. Whether the constraints that name a
//! parameter resolve against the table is the document's invariant.

use crate::value::measure::{self, Measure, ParamName};
use crate::value::provenance::{self, Provenance};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What can be wrong with a parameter or the table.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A name or a value that is not valid.
    Measure(measure::Error),
    /// The provenance fails its checks.
    Provenance(provenance::Error),
    /// A parameter whose value is a parameter.
    ParamOfParam(ParamName),
    /// No parameter has this name.
    NoSuchParam(ParamName),
}

impl From<measure::Error> for Error {
    fn from(e: measure::Error) -> Self {
        Error::Measure(e)
    }
}

impl From<provenance::Error> for Error {
    fn from(e: provenance::Error) -> Self {
        Error::Provenance(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// A shared value with its provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawParam", into = "RawParam")]
pub struct Param {
    value: Measure,
    provenance: Provenance,
}

/// The file form of a parameter, checked on the way in.
#[derive(Serialize, Deserialize)]
struct RawParam {
    value: Measure,
    provenance: Provenance,
}

impl From<Param> for RawParam {
    fn from(p: Param) -> Self {
        RawParam {
            value: p.value,
            provenance: p.provenance,
        }
    }
}

impl TryFrom<RawParam> for Param {
    type Error = Error;

    fn try_from(r: RawParam) -> Result<Self, Error> {
        Param::new(r.value, r.provenance)
    }
}

impl Param {
    /// A parameter. Its value must be valid and may not itself be a
    /// parameter.
    pub fn new(value: Measure, provenance: Provenance) -> Result<Self, Error> {
        if let Measure::Param { name } = value {
            return Err(Error::ParamOfParam(name));
        }
        value.valid()?;
        provenance.check()?;
        Ok(Self { value, provenance })
    }

    /// The value.
    pub fn value(&self) -> &Measure {
        &self.value
    }

    /// The provenance.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// The table: parameters by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "BTreeMap<ParamName, Param>",
    into = "BTreeMap<ParamName, Param>"
)]
pub struct Params(BTreeMap<ParamName, Param>);

impl From<Params> for BTreeMap<ParamName, Param> {
    fn from(p: Params) -> Self {
        p.0
    }
}

impl TryFrom<BTreeMap<ParamName, Param>> for Params {
    type Error = Error;

    fn try_from(m: BTreeMap<ParamName, Param>) -> Result<Self, Error> {
        let mut p = Params::new();
        for (name, param) in m {
            p.set(name, param)?;
        }
        Ok(p)
    }
}

impl Params {
    /// The empty table: the environment of a sketch that names nothing.
    pub const NONE: Params = Params(BTreeMap::new());

    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// A parameter.
    pub fn get(&self, name: impl Into<ParamName>) -> Option<&Param> {
        self.0.get(&name.into())
    }

    /// All parameters, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&ParamName, &Param)> {
        self.0.iter()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many parameters.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Set a parameter, adding or replacing it; returns the old one if
    /// any. The name is checked. Whether the constraints that use it
    /// still fit the new value is the document's check.
    pub fn set(&mut self, name: ParamName, param: Param) -> Result<Option<Param>, Error> {
        Measure::param(name)?;
        Ok(self.0.insert(name, param))
    }

    /// Remove a parameter. Whether anything still uses it is the
    /// document's check.
    pub fn remove(&mut self, name: impl Into<ParamName>) -> Result<Param, Error> {
        let name = name.into();
        self.0.remove(&name).ok_or(Error::NoSuchParam(name))
    }

    /// The table as a resolver, for lowering relations.
    pub fn lookup(&self) -> impl Fn(ParamName) -> Option<Measure> + '_ {
        move |n| self.0.get(&n).map(|p| p.value.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::provenance::{Agent, Date, Method};

    fn authored() -> Provenance {
        Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 16).unwrap(),
        )
    }

    fn mm(v: f64) -> Measure {
        Measure::mm(v).unwrap()
    }

    #[test]
    fn a_param_is_a_checked_value() {
        assert_eq!(
            Param::new(Measure::param("x".into()).unwrap(), authored()).err(),
            Some(Error::ParamOfParam("x".into()))
        );
        let unevidenced =
            Provenance::new(Method::Read, Agent::User, Date::new(2026, 10, 16).unwrap());
        assert!(matches!(
            Param::new(mm(1.0), unevidenced),
            Err(Error::Provenance(provenance::Error::NoEvidence))
        ));
        let p = Param::new(mm(4.0), authored()).unwrap();
        assert_eq!(p.value(), &mm(4.0));
    }

    #[test]
    fn the_table_checks_names_and_returns_what_it_replaced() {
        let mut t = Params::new();
        assert!(t.is_empty());
        assert!(matches!(
            t.set(" bad".into(), Param::new(mm(1.0), authored()).unwrap()),
            Err(Error::Measure(_))
        ));
        assert_eq!(
            t.set("rb".into(), Param::new(mm(4.0), authored()).unwrap()),
            Ok(None)
        );
        let old = t
            .set("rb".into(), Param::new(mm(4.5), authored()).unwrap())
            .unwrap();
        assert_eq!(old.unwrap().value(), &mm(4.0));
        assert_eq!(t.len(), 1);
        assert_eq!((t.lookup())("rb".into()), Some(mm(4.5)));
        assert_eq!((t.lookup())("nope".into()), None);
        assert_eq!(t.remove("nope"), Err(Error::NoSuchParam("nope".into())));
        assert_eq!(t.remove("rb").unwrap().value(), &mm(4.5));
        assert!(t.is_empty());
    }

    #[test]
    fn the_file_form_is_checked() {
        let mut t = Params::new();
        t.set("rb".into(), Param::new(mm(4.0), authored()).unwrap())
            .unwrap();
        let json = serde_json::to_string_pretty(&t).unwrap();
        let back: Params = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        // A value that is a parameter, and a bad name, are refused.
        let mut nested: serde_json::Value = serde_json::from_str(&json).unwrap();
        nested["rb"]["value"] = serde_json::to_value(Measure::param("x".into()).unwrap()).unwrap();
        assert!(serde_json::from_value::<Params>(nested)
            .unwrap_err()
            .to_string()
            .contains("ParamOfParam"));
        let bad = json.replacen("\"rb\"", "\" rb\"", 1);
        assert_ne!(bad, json);
        assert!(serde_json::from_str::<Params>(&bad)
            .unwrap_err()
            .to_string()
            .contains("BadName"));
    }
}
