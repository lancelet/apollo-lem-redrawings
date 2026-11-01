//! Measures: the values of dimensions.
//!
//! A dimension's value is one of three things. What the sheet printed, kept
//! as the printed text and its unit, so the precision is evidence and the
//! number is derived from it rather than stored beside it. A value of ours,
//! as a number in a unit. Or a named parameter, resolved by the sketch that
//! holds the parameters. Lengths convert to millimetres and angles to
//! radians exactly when a value is needed; nothing is rounded on the way in.
//!
//! Printed text is restricted to a plain decimal number (`3.200`, `.188`,
//! `-0.5`, `45`). Diameter signs, `REF`, `TYP` and tolerances are not part
//! of the value; they belong to the kind of constraint or to the annotation.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use ustr::Ustr;

/// What a unit measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantity {
    /// A length; the base unit is the millimetre.
    Length,
    /// An angle; the base unit is the radian.
    Angle,
}

/// A unit a value can be given in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Inches, as on the sheets.
    In,
    /// Millimetres.
    Mm,
    /// Degrees.
    Deg,
}

impl Unit {
    /// What this unit measures.
    pub fn quantity(self) -> Quantity {
        match self {
            Unit::In | Unit::Mm => Quantity::Length,
            Unit::Deg => Quantity::Angle,
        }
    }

    /// Factor to the base unit (mm or rad).
    pub fn to_base(self) -> f64 {
        match self {
            Unit::In => 25.4,
            Unit::Mm => 1.0,
            Unit::Deg => PI / 180.0,
        }
    }
}

/// The name of a parameter: a non-empty string without surrounding space,
/// unique within a sketch. A `Copy` handle, like `EntityId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ParamName(Ustr);

impl ParamName {
    /// A name; checked by [`Measure::param`].
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn validate(&self) -> Result<(), Error> {
        let n = self.as_str();
        if n.is_empty() || n.trim() != n {
            return Err(Error::BadName(*self));
        }
        Ok(())
    }
}

impl From<&ParamName> for ParamName {
    fn from(id: &ParamName) -> Self {
        *id
    }
}

impl From<&str> for ParamName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for ParamName {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What can be wrong with a measure.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// Printed text that is not a decimal number, a fraction or a mixed
    /// number.
    BadText(String),
    /// A value that is NaN or infinite.
    NotFinite(f64),
    /// A parameter name that is empty or has surrounding space.
    BadName(ParamName),
    /// A unit of one quantity where another was required.
    WrongQuantity {
        /// What was required.
        expected: Quantity,
        /// What the unit measures.
        found: Quantity,
    },
    /// A negative tolerance amount, or limits the wrong way round.
    BadTolerance(Tolerance),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// The value of a dimension. Built through the constructors, a printed
/// text always parses and a value is always finite; one built from the
/// variants is held to the same by [`Measure::valid`] where it enters a
/// document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    try_from = "Raw",
    into = "Raw"
)]
pub enum Measure {
    /// What the sheet printed, verbatim, with its unit.
    Printed {
        /// The number as printed, e.g. `3.200` or `.188`.
        text: String,
        /// The unit it is in.
        unit: Unit,
    },
    /// A number of ours.
    Value {
        /// The number.
        value: f64,
        /// Its unit.
        unit: Unit,
    },
    /// A named parameter of the document.
    Param {
        /// The parameter.
        name: ParamName,
    },
}

/// The file form of `Measure`; identical, but checked on the way in.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Raw {
    Printed { text: String, unit: Unit },
    Value { value: f64, unit: Unit },
    Param { name: ParamName },
}

impl From<Measure> for Raw {
    fn from(m: Measure) -> Self {
        match m {
            Measure::Printed { text, unit } => Raw::Printed { text, unit },
            Measure::Value { value, unit } => Raw::Value { value, unit },
            Measure::Param { name } => Raw::Param { name },
        }
    }
}

impl TryFrom<Raw> for Measure {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        match r {
            Raw::Printed { text, unit } => Measure::printed(&text, unit),
            Raw::Value { value, unit } => Measure::value(value, unit),
            Raw::Param { name } => Measure::param(name),
        }
    }
}

/// Parse a plain decimal number: an optional sign, digits with at most one
/// point, at least one digit. No exponent, no thousands separator, no
/// surrounding space.
fn parse_decimal(text: &str) -> Option<f64> {
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    let mut points = 0;
    let mut ndigits = 0;
    for c in digits.chars() {
        match c {
            '0'..='9' => ndigits += 1,
            '.' => points += 1,
            _ => return None,
        }
    }
    if ndigits == 0 || points > 1 {
        return None;
    }
    text.parse().ok()
}

/// Parse a printed number: a plain decimal, or a fraction of whole
/// numbers (`5/16`), or a mixed number (`2 5/16`, one space), with an
/// optional leading sign.
fn parse_plain(text: &str) -> Option<f64> {
    let (sign, body) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text.strip_prefix('+').unwrap_or(text)),
    };
    if body.starts_with(['-', '+']) {
        return None;
    }
    match split_fraction(body) {
        None => parse_decimal(text),
        Some((whole, num, den)) => {
            let int = |t: &str| {
                (!t.is_empty() && t.chars().all(|c| c.is_ascii_digit()))
                    .then(|| t.parse::<f64>().ok())
                    .flatten()
            };
            let w = if whole.is_empty() { 0.0 } else { int(whole)? };
            let (n, d) = (int(num)?, int(den)?);
            (d > 0.0).then_some(sign * (w + n / d))
        }
    }
}

/// A printed number's fraction, if it has one: the whole part (empty
/// for a bare fraction), the numerator and the denominator, as written.
pub fn split_fraction(text: &str) -> Option<(&str, &str, &str)> {
    let (num, den) = text.split_once('/')?;
    let (whole, num) = match num.split_once(' ') {
        Some((w, n)) => (w, n),
        None => ("", num),
    };
    // A whole part, when the space is there, is at least one digit.
    (whole.chars().all(|c| c.is_ascii_digit())
        && (whole.len() == 1 || !num.contains(' ') && !text.starts_with(' '))
        && !num.is_empty()
        && num.chars().all(|c| c.is_ascii_digit())
        && !den.is_empty()
        && den.chars().all(|c| c.is_ascii_digit()))
    .then_some((whole, num, den))
}

/// A dimension's tolerance: none, or symmetric about the nominal, or a
/// plus and a minus, or the two limits themselves. Each part is a
/// [`Measure`] of the value's quantity, so printed tolerances stay as
/// printed.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Tolerance {
    /// No tolerance stated.
    #[default]
    None,
    /// Plus and minus the same amount.
    Symmetric {
        /// The amount.
        by: Measure,
    },
    /// A plus and a minus amount (both given as magnitudes).
    Asymmetric {
        /// Above the nominal.
        plus: Measure,
        /// Below the nominal.
        minus: Measure,
    },
    /// The upper and lower limits.
    Limits {
        /// The upper limit.
        upper: Measure,
        /// The lower limit.
        lower: Measure,
    },
}

impl Tolerance {
    /// Plus and minus `by`.
    pub fn symmetric(by: Measure) -> Self {
        Tolerance::Symmetric { by }
    }

    /// `plus` above and `minus` below, both magnitudes.
    pub fn asymmetric(plus: Measure, minus: Measure) -> Self {
        Tolerance::Asymmetric { plus, minus }
    }

    /// The limits themselves.
    pub fn limits(upper: Measure, lower: Measure) -> Self {
        Tolerance::Limits { upper, lower }
    }

    /// Whether none is stated.
    pub fn is_none(&self) -> bool {
        matches!(self, Tolerance::None)
    }

    /// Every part measures `q`; amounts are not negative and the upper
    /// limit is not below the lower, where the numbers are known (a
    /// parameter is checked when resolved).
    pub fn check(&self, q: Quantity) -> Result<(), Error> {
        for m in self.parts() {
            m.check(q)?;
        }
        let negative = |m: &Measure| m.base().is_some_and(|v| v < 0.0);
        match self {
            Tolerance::None => {}
            Tolerance::Symmetric { by } => {
                if negative(by) {
                    return Err(Error::BadTolerance(self.clone()));
                }
            }
            Tolerance::Asymmetric { plus, minus } => {
                if negative(plus) || negative(minus) {
                    return Err(Error::BadTolerance(self.clone()));
                }
            }
            Tolerance::Limits { upper, lower } => {
                if let (Some(u), Some(l)) = (upper.base(), lower.base()) {
                    if u < l {
                        return Err(Error::BadTolerance(self.clone()));
                    }
                }
            }
        }
        Ok(())
    }

    /// The measures in it.
    pub fn parts(&self) -> Vec<&Measure> {
        match self {
            Tolerance::None => Vec::new(),
            Tolerance::Symmetric { by } => vec![by],
            Tolerance::Asymmetric { plus, minus } => vec![plus, minus],
            Tolerance::Limits { upper, lower } => vec![upper, lower],
        }
    }
}

impl Measure {
    /// What the sheet printed. Fails unless `text` is a plain decimal
    /// number.
    pub fn printed(text: &str, unit: Unit) -> Result<Self, Error> {
        match parse_plain(text) {
            Some(_) => Ok(Measure::Printed {
                text: text.to_string(),
                unit,
            }),
            None => Err(Error::BadText(text.to_string())),
        }
    }

    /// A number of ours. Fails unless finite.
    pub fn value(value: f64, unit: Unit) -> Result<Self, Error> {
        if value.is_finite() {
            Ok(Measure::Value { value, unit })
        } else {
            Err(Error::NotFinite(value))
        }
    }

    /// A parameter reference.
    pub fn param(name: ParamName) -> Result<Self, Error> {
        name.validate()?;
        Ok(Measure::Param { name })
    }

    /// Inches, as printed on the sheet.
    pub fn inches(text: &str) -> Result<Self, Error> {
        Self::printed(text, Unit::In)
    }

    /// A value in millimetres.
    pub fn mm(v: f64) -> Result<Self, Error> {
        Self::value(v, Unit::Mm)
    }

    /// A value in inches.
    pub fn inch(v: f64) -> Result<Self, Error> {
        Self::value(v, Unit::In)
    }

    /// A value in degrees.
    pub fn deg(v: f64) -> Result<Self, Error> {
        Self::value(v, Unit::Deg)
    }

    /// The unit, if the measure carries one (a parameter does not).
    pub fn unit(&self) -> Option<Unit> {
        match self {
            Measure::Printed { unit, .. } | Measure::Value { unit, .. } => Some(*unit),
            Measure::Param { .. } => None,
        }
    }

    /// The number in the measure's own unit, if it carries one.
    pub fn number(&self) -> Option<f64> {
        match self {
            Measure::Printed { text, .. } => parse_plain(text),
            Measure::Value { value, .. } => Some(*value),
            Measure::Param { .. } => None,
        }
    }

    /// The value in the base unit of its quantity (mm or rad), if the
    /// measure carries one. A parameter is resolved by its sketch.
    pub fn base(&self) -> Option<f64> {
        Some(self.number()? * self.unit()?.to_base())
    }

    /// What the constructors promise: printed text that parses, a
    /// finite value, a valid name. The variants are public, so a measure
    /// built from them directly is held to this wherever it enters a
    /// document, as a file's is on load.
    pub fn valid(&self) -> Result<(), Error> {
        match self {
            Measure::Printed { text, unit } => Measure::printed(text, *unit).map(|_| ()),
            Measure::Value { value, unit } => Measure::value(*value, *unit).map(|_| ()),
            Measure::Param { name } => Measure::param(*name).map(|_| ()),
        }
    }

    /// Check that the measure is [`valid`](Self::valid) and its unit
    /// measures `q`. A parameter passes here and is checked by its sketch
    /// when resolved.
    pub fn check(&self, q: Quantity) -> Result<(), Error> {
        self.valid()?;
        match self.unit() {
            Some(u) if u.quantity() != q => Err(Error::WrongQuantity {
                expected: q,
                found: u.quantity(),
            }),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerances_are_magnitudes_and_limits_are_ordered() {
        let mm = |v| Measure::mm(v).unwrap();
        assert_eq!(
            Tolerance::symmetric(mm(-0.1)).check(Quantity::Length),
            Err(Error::BadTolerance(Tolerance::symmetric(mm(-0.1))))
        );
        assert!(Tolerance::asymmetric(mm(0.1), mm(-0.2))
            .check(Quantity::Length)
            .is_err());
        assert!(Tolerance::limits(mm(1.0), mm(2.0))
            .check(Quantity::Length)
            .is_err());
        assert!(Tolerance::limits(mm(2.0), mm(1.0))
            .check(Quantity::Length)
            .is_ok());
        // A parameter's number is not known here, so it passes.
        assert!(Tolerance::symmetric(Measure::param("tol".into()).unwrap())
            .check(Quantity::Length)
            .is_ok());
    }

    #[test]
    fn printed_numbers_may_be_fractions() {
        let m = Measure::inches("2 5/16").unwrap();
        assert!((m.number().unwrap() - 2.3125).abs() < 1e-12);
        assert!((Measure::inches("5/16").unwrap().number().unwrap() - 0.3125).abs() < 1e-12);
        assert!((Measure::inches("-1/2").unwrap().number().unwrap() + 0.5).abs() < 1e-12);
        assert_eq!(split_fraction("2 5/16"), Some(("2", "5", "16")));
        assert_eq!(split_fraction("5/16"), Some(("", "5", "16")));
        assert_eq!(split_fraction(" 5/16"), None);
        assert_eq!(split_fraction("5/16 "), None);
        assert_eq!(split_fraction("2  5/16"), None);
        assert_eq!(split_fraction(".188"), None);
        for bad in ["5/0", "2  5/16", "a/b", "1/", "/2", "2 5/16 "] {
            assert!(Measure::inches(bad).is_err(), "{bad}");
        }
        let t = Tolerance::Symmetric {
            by: Measure::mm(0.1).unwrap(),
        };
        assert!(t.check(Quantity::Length).is_ok());
        assert!(t.check(Quantity::Angle).is_err());
        assert!(Tolerance::None.is_none());
    }
    use approx::assert_abs_diff_eq;

    #[test]
    fn printed_text_is_a_plain_number() {
        for ok in ["3.200", ".188", "0", "45", "-0.5", "+2", "10."] {
            assert!(Measure::printed(ok, Unit::In).is_ok(), "{ok}");
        }
        for bad in [
            "",
            ".",
            "-",
            "Ø.188",
            "3.200 REF",
            " 3.2",
            "3.2 ",
            "1,5",
            "1e3",
            "1.2.3",
            "3  1/2",
        ] {
            assert_eq!(
                Measure::printed(bad, Unit::In),
                Err(Error::BadText(bad.to_string())),
                "{bad}"
            );
        }
    }

    #[test]
    fn conversion_is_exact() {
        let m = Measure::inches("3.200").unwrap();
        assert_eq!(m.number(), Some(3.2));
        assert_abs_diff_eq!(m.base().unwrap(), 81.28, epsilon = 1e-12);
        let m = Measure::printed(".188", Unit::In).unwrap();
        assert_abs_diff_eq!(m.base().unwrap(), 4.7752, epsilon = 1e-12);
        let m = Measure::mm(10.0).unwrap();
        assert_eq!(m.base(), Some(10.0));
        let m = Measure::deg(90.0).unwrap();
        assert_abs_diff_eq!(m.base().unwrap(), PI / 2.0, epsilon = 1e-15);
        assert_eq!(Measure::param("t".into()).unwrap().base(), None);
    }

    #[test]
    fn values_and_names_are_checked() {
        assert!(matches!(
            Measure::mm(f64::NAN),
            Err(Error::NotFinite(v)) if v.is_nan()
        ));
        assert!(matches!(
            Measure::deg(f64::INFINITY),
            Err(Error::NotFinite(_))
        ));
        for bad in ["", " t", "t "] {
            assert_eq!(
                Measure::param(bad.into()),
                Err(Error::BadName(bad.into())),
                "{bad:?}"
            );
        }
        assert!(Measure::param("rb_o".into()).is_ok());
    }

    #[test]
    fn units_are_checked_against_the_quantity() {
        let len = Measure::inches("3.200").unwrap();
        let ang = Measure::deg(45.0).unwrap();
        let par = Measure::param("t".into()).unwrap();
        assert!(len.check(Quantity::Length).is_ok());
        assert_eq!(
            len.check(Quantity::Angle),
            Err(Error::WrongQuantity {
                expected: Quantity::Angle,
                found: Quantity::Length
            })
        );
        assert!(ang.check(Quantity::Angle).is_ok());
        assert!(ang.check(Quantity::Length).is_err());
        // A parameter has no unit of its own; its sketch checks it.
        assert!(par.check(Quantity::Length).is_ok());
        assert!(par.check(Quantity::Angle).is_ok());
    }

    #[test]
    fn file_form_is_checked_on_load() {
        let m = Measure::inches(".750").unwrap();
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(s, r#"{"kind":"printed","text":".750","unit":"in"}"#);
        assert_eq!(serde_json::from_str::<Measure>(&s).unwrap(), m);
        let bad = r#"{"kind":"printed","text":"Ø.750","unit":"in"}"#;
        assert!(serde_json::from_str::<Measure>(bad)
            .unwrap_err()
            .to_string()
            .contains("BadText"));
        let bad = r#"{"kind":"param","name":""}"#;
        assert!(serde_json::from_str::<Measure>(bad).is_err());
        let p = Measure::param("t".into()).unwrap();
        let s = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Measure>(&s).unwrap(), p);
    }
}
