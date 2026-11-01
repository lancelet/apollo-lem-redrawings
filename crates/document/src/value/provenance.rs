//! Provenance: where a claim came from, how, and where it stands.
//!
//! Every entity, constraint and parameter in a sketch is a claim about the
//! drawing, and each carries a `Provenance` record: the evidence it cites
//! (documents in the document's source registry), the method by which it
//! was obtained, its status, who recorded it and when. The three display
//! modes in use (on sheet, inferred, reference) are functions of the record
//! ([`Provenance::mode`]), never stored tags, so a new kind of source is a
//! registry row and a new colour is a function, not a data change. So is a
//! reading's trust ([`Provenance::trust`]): how far its checks have taken
//! it.
//!
//! Status moves forward only: provisional, accepted, verified, and from any
//! of those to disputed or superseded. A superseded claim is kept, not
//! deleted; the claim that replaces it is a new item, and this one points
//! to it. Nothing here knows about geometry or equations.

use crate::value::geometry::EntityId;
use crate::value::measure::Measure;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// A document in the source registry: this sheet, another sheet, a report,
/// a photograph, the model. The registry itself is document-level; a
/// `DocId` is the row's name, an interned `Copy` handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DocId(Ustr);

impl DocId {
    /// A registry name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&DocId> for DocId {
    fn from(id: &DocId) -> Self {
        *id
    }
}

impl From<&str> for DocId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// The name of a constraint in a sketch, an interned `Copy` handle. Defined
/// here because a provenance may point to the constraint that supersedes
/// its claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ConstraintId(Ustr);

impl ConstraintId {
    /// A name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&ConstraintId> for ConstraintId {
    fn from(id: &ConstraintId) -> Self {
        *id
    }
}

impl From<&str> for ConstraintId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for ConstraintId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A claim within the same sketch that another claim refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ItemRef {
    /// An entity.
    Entity(EntityId),
    /// A constraint.
    Constraint(ConstraintId),
}

/// Where in a document something is: a zone, a page, a scan position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Citation {
    /// The document, by registry name.
    pub doc: DocId,
    /// Where in it, in words: "zone C4", "p. 12", "scan px (4472, 2731)".
    pub at: String,
}

impl Citation {
    /// A citation.
    pub fn new(doc: impl Into<DocId>, at: &str) -> Self {
        Self {
            doc: doc.into(),
            at: at.to_string(),
        }
    }
}

/// How a claim was obtained.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Method {
    /// Stated in the cited document: a printed dimension, a note, a table.
    Read,
    /// Scaled off the cited document (ink, a photograph), with its
    /// uncertainty.
    Measured {
        /// One standard deviation, mm.
        sigma_mm: f64,
    },
    /// Computed from other claims: symmetry, arithmetic, a rule.
    Derived {
        /// The claims it follows from, in this sketch.
        from: Vec<ItemRef>,
    },
    /// Our judgement from the cited evidence, without a measurement.
    Estimated,
    /// A placeholder with no evidence, to be replaced.
    Assumed,
    /// A drawing element of ours (construction, a note, an added
    /// annotation), not a claim about the artefact. Needs no evidence.
    Authored,
    /// An assumption that completes an under-dimensioned drawing, with
    /// the basis it rests on (docs/redraw-plan.md §8.2, K10). Cites the
    /// evidence the basis comes from (the scan it was scaled from, the
    /// other sheet). The lowest trust of all.
    Inferred {
        /// What it rests on.
        basis: Basis,
    },
}

/// What an inferred claim rests on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Basis {
    /// The part looks symmetric from its genuine dimensions.
    Symmetry,
    /// Scaled from the ink: the value as measured, and the precision the
    /// drawing supports (what the stated value is rounded to), each with
    /// its unit.
    Scaled {
        /// The measured value.
        measured: Measure,
        /// The precision it is rounded to.
        precision: Measure,
    },
    /// What drawing practice does here.
    StandardPractice,
    /// Another sheet says so (cited as the evidence).
    OtherSheet,
}

/// A verification pass that a claim has been through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    /// Our drawing laid over the scan.
    Overlay,
    /// Read again independently.
    SecondRead,
    /// Agrees with another sheet.
    CrossSheet,
    /// Agrees with the model.
    Model,
    /// The definition is right: what the dimension measures between
    /// (which features it names) is what the drafter means, per the
    /// engineering intent (docs/redraw-plan.md K4, "Dimensioned to
    /// intent"). Not on the [`Trust`] ladder: it says nothing about the
    /// value, only about what the value is a value of. The cite's text
    /// holds the verdict and the reason.
    Intent,
}

/// One verification of a claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// The kind of check.
    pub kind: CheckKind,
    /// What it was checked against.
    pub cite: Citation,
}

/// Where a claim stands. Moves forward only; see the methods on
/// [`Provenance`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Status {
    /// Recorded, not yet accepted.
    Provisional,
    /// Taken as true.
    Accepted,
    /// Taken as true and checked.
    Verified {
        /// The checks it passed.
        checks: Vec<Check>,
    },
    /// Believed wrong, against other claims.
    Disputed {
        /// The claims it disagrees with.
        against: Vec<ItemRef>,
    },
    /// Replaced by another claim; kept for the record.
    Superseded {
        /// The replacement.
        by: ItemRef,
    },
}

/// Who recorded a claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Agent {
    /// The user.
    User,
    /// Claude.
    Claude,
    /// A script, by name.
    Script {
        /// The script.
        name: String,
    },
}

/// A calendar date, `YYYY-MM-DD`. The core does not read the clock (not
/// available on every target); the caller supplies the date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// A date, checked for a real calendar day in the years the form
    /// `YYYY-MM-DD` can write (1900 to 9999).
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, Error> {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 0,
        };
        if !(1900..=9999).contains(&year) || day == 0 || day > days {
            return Err(Error::BadDate(format!("{year:04}-{month:02}-{day:02}")));
        }
        Ok(Self { year, month, day })
    }

    /// Parse `YYYY-MM-DD`.
    pub fn parse(s: &str) -> Result<Self, Error> {
        let bad = || Error::BadDate(s.to_string());
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
            return Err(bad());
        }
        // Digits only: `u16::from_str` would take a sign too.
        let n = |p: &str| match p.chars().all(|c| c.is_ascii_digit()) {
            true => p.parse::<u16>().map_err(|_| bad()),
            false => Err(bad()),
        };
        Self::new(n(parts[0])?, n(parts[1])? as u8, n(parts[2])? as u8)
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl TryFrom<String> for Date {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Error> {
        Self::parse(&s)
    }
}

impl From<Date> for String {
    fn from(d: Date) -> Self {
        d.to_string()
    }
}

/// What can be wrong with a provenance.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A method that needs evidence was given none.
    NoEvidence,
    /// A derived claim with nothing to derive from.
    DerivedFromNothing,
    /// A measurement uncertainty that is negative or not finite.
    BadSigma(f64),
    /// A status change that is not allowed from the current status.
    BadTransition {
        /// The status it was in.
        from: Status,
        /// What was attempted.
        attempted: &'static str,
    },
    /// A disputed status with nothing to dispute against.
    DisputedAgainstNothing,
    /// Not a calendar date in `YYYY-MM-DD`.
    BadDate(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// How a claim reads in the viewer and in exports: a function of its
/// record, never a stored tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Read or measured from this sheet: part of the drawing.
    OnSheet,
    /// Read from another document: official, but not on this sheet.
    Reference,
    /// Derived, estimated, assumed or authored by us.
    Inferred,
}

/// How far a reading has been checked, weakest first: each level
/// includes the ones before (docs/transcripts/FORMAT.md in
/// apollo-lunar-module, "Trust"). A function of the record
/// ([`Provenance::trust`]), never a stored tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Trust {
    /// Inferred to complete the drawing (K10): no observation at all.
    Inferred,
    /// Read once from the cited document.
    Read,
    /// And laid over the scan, seen to sit on the ink.
    Overlay,
    /// And read again independently, with the same result.
    SecondRead,
    /// And agrees with another sheet.
    CrossSheet,
    /// And agrees with the model.
    Model,
}

impl Trust {
    /// The levels above `Read`, in order, each with the check that
    /// reaches it.
    const LADDER: [(CheckKind, Trust); 4] = [
        (CheckKind::Overlay, Trust::Overlay),
        (CheckKind::SecondRead, Trust::SecondRead),
        (CheckKind::CrossSheet, Trust::CrossSheet),
        (CheckKind::Model, Trust::Model),
    ];
}

/// The provenance record. Built with [`Provenance::new`] and the `cite`
/// and `note` builders; checked by [`Provenance::check`] at construction
/// and on load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct Provenance {
    evidence: Vec<Citation>,
    method: Method,
    status: Status,
    by: Agent,
    when: Date,
    note: Option<String>,
}

/// The file form: the fields, checked on the way in.
#[derive(Serialize, Deserialize)]
struct Raw {
    evidence: Vec<Citation>,
    method: Method,
    status: Status,
    by: Agent,
    when: Date,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

impl From<Provenance> for Raw {
    fn from(p: Provenance) -> Self {
        Raw {
            evidence: p.evidence,
            method: p.method,
            status: p.status,
            by: p.by,
            when: p.when,
            note: p.note,
        }
    }
}

impl TryFrom<Raw> for Provenance {
    type Error = Error;

    fn try_from(r: Raw) -> Result<Self, Error> {
        let p = Provenance {
            evidence: r.evidence,
            method: r.method,
            status: r.status,
            by: r.by,
            when: r.when,
            note: r.note,
        };
        p.check()?;
        Ok(p)
    }
}

impl Provenance {
    /// A provisional claim with no evidence yet. Add citations with
    /// [`cite`](Self::cite); `Read`, `Measured`, `Derived` and `Estimated`
    /// claims must have at least one before they pass [`check`](Self::check).
    pub fn new(method: Method, by: Agent, when: Date) -> Self {
        Self {
            evidence: Vec::new(),
            method,
            status: Status::Provisional,
            by,
            when,
            note: None,
        }
    }

    /// Add a citation.
    pub fn cite(mut self, c: Citation) -> Self {
        self.evidence.push(c);
        self
    }

    /// The same claim in this status (a replacement that keeps where the
    /// old one stood).
    pub(crate) fn with_status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// Add the reason, in words.
    pub fn note(mut self, note: &str) -> Self {
        self.note = Some(note.to_string());
        self
    }

    /// The rules a record must satisfy: evidence unless `Authored` or
    /// `Assumed`; a finite, non-negative `sigma_mm`; a derived claim names
    /// what it derives from; a disputed claim names what it disputes.
    pub fn check(&self) -> Result<(), Error> {
        match &self.method {
            Method::Authored | Method::Assumed => {}
            Method::Derived { from } if from.is_empty() => return Err(Error::DerivedFromNothing),
            _ if self.evidence.is_empty() => return Err(Error::NoEvidence),
            Method::Measured { sigma_mm } if !(sigma_mm.is_finite() && *sigma_mm >= 0.0) => {
                return Err(Error::BadSigma(*sigma_mm))
            }
            _ => {}
        }
        if let Status::Disputed { against } = &self.status {
            if against.is_empty() {
                return Err(Error::DisputedAgainstNothing);
            }
        }
        Ok(())
    }

    /// The citations.
    pub fn evidence(&self) -> &[Citation] {
        &self.evidence
    }

    /// The method.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// What it rests on, if it is inferred (K10): the filter for the
    /// `inferred` layer, which constraints (having no layer of their
    /// own) take from their provenance.
    pub fn basis(&self) -> Option<&Basis> {
        match &self.method {
            Method::Inferred { basis } => Some(basis),
            _ => None,
        }
    }

    /// The status.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Who recorded it.
    pub fn by(&self) -> &Agent {
        &self.by
    }

    /// When.
    pub fn when(&self) -> Date {
        self.when
    }

    /// The reason, if given.
    pub fn note_text(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// Whether the claim is in force: not disputed, not superseded.
    pub fn active(&self) -> bool {
        !matches!(
            self.status,
            Status::Disputed { .. } | Status::Superseded { .. }
        )
    }

    /// How the claim reads, given which registry document is this sheet.
    pub fn mode(&self, this: DocId) -> Mode {
        let cites_this = self.evidence.iter().any(|c| c.doc == this);
        let cites_other = self.evidence.iter().any(|c| c.doc != this);
        match self.method {
            Method::Read | Method::Measured { .. } if cites_this => Mode::OnSheet,
            Method::Read if cites_other => Mode::Reference,
            _ => Mode::Inferred,
        }
    }

    /// How far the claim is trusted, if it is an observation of the cited
    /// document: read off it or measured (scaled) from it, as
    /// docs/transcripts/FORMAT.md gives trust to scaled geometry too.
    /// An inferred claim has the lowest trust, [`Trust::Inferred`], below
    /// a single read, whatever checks it has. Derived, estimated, assumed
    /// and authored claims observe nothing, and have no trust. An observation is `Read`, raised
    /// one level by each check of the ladder passed in order, so a level
    /// counts only when every level below it has been reached too (a model
    /// check on a reading never laid over the scan leaves it at `Read`).
    /// Only a verified claim has checks: a disputed or superseded one is
    /// back at `Read`.
    pub fn trust(&self) -> Option<Trust> {
        if matches!(self.method, Method::Inferred { .. }) {
            return Some(Trust::Inferred);
        }
        if !matches!(self.method, Method::Read | Method::Measured { .. }) {
            return None;
        }
        let checks: &[Check] = match &self.status {
            Status::Verified { checks } => checks,
            _ => &[],
        };
        let passed = |kind: CheckKind| checks.iter().any(|c| c.kind == kind);
        Some(
            Trust::LADDER
                .iter()
                .take_while(|(kind, _)| passed(*kind))
                .last()
                .map_or(Trust::Read, |(_, level)| *level),
        )
    }

    fn transition(&self, attempted: &'static str) -> Error {
        Error::BadTransition {
            from: self.status.clone(),
            attempted,
        }
    }

    /// Provisional → accepted.
    pub fn accept(&mut self) -> Result<(), Error> {
        match self.status {
            Status::Provisional => {
                self.status = Status::Accepted;
                Ok(())
            }
            _ => Err(self.transition("accept")),
        }
    }

    /// Provisional, accepted or verified → verified, with one more check.
    pub fn verify(&mut self, check: Check) -> Result<(), Error> {
        match &mut self.status {
            Status::Provisional | Status::Accepted => {
                self.status = Status::Verified {
                    checks: vec![check],
                };
                Ok(())
            }
            Status::Verified { checks } => {
                checks.push(check);
                Ok(())
            }
            _ => Err(self.transition("verify")),
        }
    }

    /// Provisional, accepted or verified → disputed, against named claims.
    pub fn dispute(&mut self, against: Vec<ItemRef>) -> Result<(), Error> {
        if against.is_empty() {
            return Err(Error::DisputedAgainstNothing);
        }
        match self.status {
            Status::Provisional | Status::Accepted | Status::Verified { .. } => {
                self.status = Status::Disputed { against };
                Ok(())
            }
            _ => Err(self.transition("dispute")),
        }
    }

    /// Anything but superseded → superseded by a named claim.
    pub fn supersede(&mut self, by: ItemRef) -> Result<(), Error> {
        match self.status {
            Status::Superseded { .. } => Err(self.transition("supersede")),
            _ => {
                self.status = Status::Superseded { by };
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> Date {
        Date::new(2026, 10, 15).unwrap()
    }

    fn this() -> DocId {
        "LDW280-23631 sh3".into()
    }

    fn read_here() -> Provenance {
        Provenance::new(Method::Read, Agent::Claude, day()).cite(Citation::new(this(), "zone C4"))
    }

    #[test]
    fn evidence_rules() {
        assert_eq!(
            Provenance::new(Method::Read, Agent::User, day()).check(),
            Err(Error::NoEvidence)
        );
        assert!(read_here().check().is_ok());
        assert!(Provenance::new(Method::Authored, Agent::Claude, day())
            .check()
            .is_ok());
        assert!(Provenance::new(Method::Assumed, Agent::Claude, day())
            .note("placeholder until sheet 2 is read")
            .check()
            .is_ok());
        assert_eq!(
            Provenance::new(Method::Derived { from: vec![] }, Agent::Claude, day()).check(),
            Err(Error::DerivedFromNothing)
        );
        let derived = Method::Derived {
            from: vec![ItemRef::Constraint("c1".into())],
        };
        assert_eq!(
            Provenance::new(derived, Agent::Claude, day()).check(),
            Err(Error::NoEvidence)
        );
        let measured = |s| {
            Provenance::new(Method::Measured { sigma_mm: s }, Agent::Claude, day())
                .cite(Citation::new(this(), "scan px (4472, 2731)"))
        };
        assert!(measured(0.3).check().is_ok());
        assert_eq!(measured(-0.1).check(), Err(Error::BadSigma(-0.1)));
        assert!(matches!(
            measured(f64::NAN).check(),
            Err(Error::BadSigma(_))
        ));
    }

    #[test]
    fn status_moves_forward_only() {
        let mut p = read_here();
        assert_eq!(p.status(), &Status::Provisional);
        assert!(p.active());
        p.accept().unwrap();
        assert_eq!(p.status(), &Status::Accepted);
        assert!(matches!(
            p.accept(),
            Err(Error::BadTransition {
                attempted: "accept",
                ..
            })
        ));
        let check = Check {
            kind: CheckKind::Overlay,
            cite: Citation::new(this(), "overlay 2026-10-15"),
        };
        p.verify(check.clone()).unwrap();
        p.verify(Check {
            kind: CheckKind::CrossSheet,
            cite: Citation::new("LDW280-23631 sh2", "zone B2"),
        })
        .unwrap();
        match p.status() {
            Status::Verified { checks } => assert_eq!(checks.len(), 2),
            s => panic!("{s:?}"),
        }
        // Verified can still be disputed; disputed cannot be verified.
        p.dispute(vec![ItemRef::Constraint("c9".into())]).unwrap();
        assert!(!p.active());
        assert!(p.verify(check).is_err());
        assert!(p.accept().is_err());
        // Anything can be superseded once; superseded is final.
        p.supersede(ItemRef::Constraint("c10".into())).unwrap();
        assert!(!p.active());
        assert!(p.supersede(ItemRef::Constraint("c11".into())).is_err());
        assert!(p.dispute(vec![ItemRef::Constraint("c9".into())]).is_err());
        // A provisional claim can be verified directly (verifying accepts).
        let mut q = read_here();
        q.verify(Check {
            kind: CheckKind::SecondRead,
            cite: Citation::new(this(), "zone C4"),
        })
        .unwrap();
        assert!(matches!(q.status(), Status::Verified { .. }));
        assert_eq!(q.dispute(vec![]), Err(Error::DisputedAgainstNothing));
    }

    /// K10: an inferred claim has the lowest trust, below a read, with
    /// its basis recorded; it needs evidence; checks do not raise it; a
    /// read raised by a second read reaches `SecondRead`.
    #[test]
    fn inferred_is_below_a_read_and_a_second_read_is_above_it() {
        let inferred = |basis: Basis| {
            Provenance::new(Method::Inferred { basis }, Agent::User, day())
                .cite(Citation::new(this(), "scan"))
        };
        let mut p = inferred(Basis::Scaled {
            measured: Measure::inch(0.5078).unwrap(),
            precision: Measure::inch(1.0 / 64.0).unwrap(),
        });
        assert_eq!(p.trust(), Some(Trust::Inferred));
        assert!(Trust::Inferred < Trust::Read);
        assert_eq!(p.mode(this()), Mode::Inferred);
        assert!(
            matches!(p.basis(), Some(Basis::Scaled { precision, .. }) if precision.base().is_some_and(|p| p > 0.0))
        );
        p.verify(Check {
            kind: CheckKind::Overlay,
            cite: Citation::new(this(), "overlay"),
        })
        .unwrap();
        assert_eq!(p.trust(), Some(Trust::Inferred));
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Provenance>(&json).unwrap(), p);
        assert_eq!(
            Provenance::new(
                Method::Inferred {
                    basis: Basis::Symmetry
                },
                Agent::User,
                day()
            )
            .check(),
            Err(Error::NoEvidence)
        );
        assert_eq!(read_here().basis(), None);
        let mut r = read_here();
        r.verify(Check {
            kind: CheckKind::SecondRead,
            cite: Citation::new(this(), "again"),
        })
        .unwrap();
        // The ladder counts a level only with every level below it: a
        // second read with no overlay under it leaves the reading at
        // `Read`; with the overlay it reaches `SecondRead`.
        assert_eq!(r.trust(), Some(Trust::Read));
        r.verify(Check {
            kind: CheckKind::Overlay,
            cite: Citation::new(this(), "overlay"),
        })
        .unwrap();
        assert_eq!(r.trust(), Some(Trust::SecondRead));
    }

    #[test]
    fn mode_is_a_function_of_the_record() {
        assert_eq!(read_here().mode(this()), Mode::OnSheet);
        let measured = Provenance::new(Method::Measured { sigma_mm: 0.3 }, Agent::Claude, day())
            .cite(Citation::new(this(), "scan"));
        assert_eq!(measured.mode(this()), Mode::OnSheet);
        let other = Provenance::new(Method::Read, Agent::User, day())
            .cite(Citation::new("NASA TM-X-70000", "p. 12"));
        assert_eq!(other.mode(this()), Mode::Reference);
        // Estimated from this sheet's ink is still ours.
        let est = Provenance::new(Method::Estimated, Agent::Claude, day())
            .cite(Citation::new(this(), "zone C4"));
        assert_eq!(est.mode(this()), Mode::Inferred);
        assert_eq!(
            Provenance::new(Method::Authored, Agent::Claude, day()).mode(this()),
            Mode::Inferred
        );
        // Mode does not depend on status.
        let mut sup = read_here();
        sup.supersede(ItemRef::Constraint("c2".into())).unwrap();
        assert_eq!(sup.mode(this()), Mode::OnSheet);
    }

    #[test]
    fn an_intent_check_round_trips_with_its_verdict_and_reason() {
        let mut p = read_here();
        p.verify(Check {
            kind: CheckKind::Intent,
            cite: Citation::new(this(), "line-line E45.5 E45.1: the width across the flats"),
        })
        .unwrap();
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"kind\":\"intent\""), "{json}");
        let back: Provenance = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        let Status::Verified { checks } = back.status() else {
            panic!("verified");
        };
        assert_eq!(checks[0].kind, CheckKind::Intent);
        assert!(checks[0].cite.at.contains("width across the flats"));
    }

    #[test]
    fn trust_is_a_function_of_the_checks() {
        let check = |kind| Check {
            kind,
            cite: Citation::new(this(), "zone C4"),
        };
        let with = |kinds: &[CheckKind]| {
            let mut p = read_here();
            for k in kinds {
                p.verify(check(*k)).unwrap();
            }
            p.trust()
        };
        use CheckKind::*;
        // Every level, each reached by the checks below it in order.
        assert_eq!(with(&[]), Some(Trust::Read));
        assert_eq!(with(&[Overlay]), Some(Trust::Overlay));
        assert_eq!(with(&[Overlay, SecondRead]), Some(Trust::SecondRead));
        assert_eq!(
            with(&[Overlay, SecondRead, CrossSheet]),
            Some(Trust::CrossSheet)
        );
        assert_eq!(
            with(&[Overlay, SecondRead, CrossSheet, Model]),
            Some(Trust::Model)
        );
        // The order the checks were made in does not matter; a gap does.
        assert_eq!(
            with(&[Model, CrossSheet, SecondRead, Overlay]),
            Some(Trust::Model)
        );
        assert_eq!(with(&[Model]), Some(Trust::Read));
        assert_eq!(with(&[Overlay, CrossSheet, Model]), Some(Trust::Overlay));
        assert_eq!(with(&[Overlay, Overlay]), Some(Trust::Overlay));
        // An intent check says the definition is right, not the value:
        // it is no rung of the ladder, alone or among the others.
        assert_eq!(with(&[Intent]), Some(Trust::Read));
        assert_eq!(with(&[Intent, Overlay]), Some(Trust::Overlay));
        assert_eq!(
            with(&[Overlay, Intent, SecondRead]),
            Some(Trust::SecondRead)
        );
        // The levels are ordered weakest first.
        assert!(
            Trust::Read < Trust::Overlay
                && Trust::Overlay < Trust::SecondRead
                && Trust::SecondRead < Trust::CrossSheet
                && Trust::CrossSheet < Trust::Model
        );
        // Accepted is still a single read; a dispute drops the checks.
        let mut p = read_here();
        p.accept().unwrap();
        assert_eq!(p.trust(), Some(Trust::Read));
        p.verify(check(Overlay)).unwrap();
        p.dispute(vec![ItemRef::Constraint("c9".into())]).unwrap();
        assert_eq!(p.trust(), Some(Trust::Read));
        // A measurement is an observation too: overlay is its first check.
        let mut m = Provenance::new(Method::Measured { sigma_mm: 0.3 }, Agent::Claude, day())
            .cite(Citation::new(this(), "scan"));
        assert_eq!(m.trust(), Some(Trust::Read));
        m.verify(check(Overlay)).unwrap();
        assert_eq!(m.trust(), Some(Trust::Overlay));
        // What observes nothing has no trust, checked or not.
        let derived = Method::Derived {
            from: vec![ItemRef::Constraint("c1".into())],
        };
        for method in [
            derived,
            Method::Estimated,
            Method::Assumed,
            Method::Authored,
        ] {
            let mut p =
                Provenance::new(method, Agent::Claude, day()).cite(Citation::new(this(), "x"));
            p.verify(check(Overlay)).unwrap();
            assert_eq!(p.trust(), None, "{:?}", p.method());
        }
    }

    #[test]
    fn dates() {
        assert_eq!(Date::parse("2026-10-15").unwrap().to_string(), "2026-10-15");
        assert!(Date::new(2024, 2, 29).is_ok());
        assert!(Date::new(2026, 2, 29).is_err());
        assert!(Date::new(2026, 13, 1).is_err());
        assert!(Date::new(1899, 1, 1).is_err());
        for bad in ["2026-1-5", "26-10-15", "2026/10/15", "2026-10-15x", ""] {
            assert!(Date::parse(bad).is_err(), "{bad}");
        }
        assert!(Date::parse("2026-10-15").unwrap() < Date::parse("2026-10-16").unwrap());
    }

    #[test]
    fn file_form_is_checked_on_load() {
        let mut p = read_here().note("3.200 printed between the clips");
        p.accept().unwrap();
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains(r#""method":{"kind":"read"}"#));
        assert!(s.contains(r#""when":"2026-10-15""#));
        assert_eq!(serde_json::from_str::<Provenance>(&s).unwrap(), p);
        // A hand-edited file that drops the evidence is refused.
        let bad = s.replace(
            r#""evidence":[{"doc":"LDW280-23631 sh3","at":"zone C4"}]"#,
            r#""evidence":[]"#,
        );
        assert_ne!(bad, s);
        assert!(serde_json::from_str::<Provenance>(&bad)
            .unwrap_err()
            .to_string()
            .contains("NoEvidence"));
        // The note is optional in the file.
        let q = Provenance::new(Method::Authored, Agent::User, day());
        let s = serde_json::to_string(&q).unwrap();
        assert!(!s.contains("note"));
        assert_eq!(serde_json::from_str::<Provenance>(&s).unwrap(), q);
    }
}
