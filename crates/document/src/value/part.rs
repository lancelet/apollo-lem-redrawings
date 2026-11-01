//! The reading of a sheet as an engineering drawing: what part it
//! depicts, its features, and what still does not make sense.
//!
//! A sheet's geometry and lettering say what was drawn; the reading
//! says what it means: the part ("a formed sheet-metal stiffener, its
//! flanges bent down and up"), how it is made, and each feature (a
//! bend, a joggle, a relief notch, a tooling hole) with where on the
//! paper it is drawn. It is how a sheet is checked as a whole: every
//! line on the part should belong to a feature, and a feature should be
//! drawn as such a feature is. What the reader could not explain is kept
//! as questions, never dropped.

use crate::value::geometry::{Paper, Rect};
use crate::value::provenance::{Agent, Date};
use serde::{Deserialize, Serialize};

/// What makes a reading invalid on its own.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// It does not say what the sheet depicts.
    NoSummary,
    /// A feature has no name.
    UnnamedFeature(usize),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NoSummary => write!(f, "a reading says nothing of what the sheet depicts"),
            Error::UnnamedFeature(k) => write!(f, "feature {k} has no name"),
        }
    }
}

impl std::error::Error for Error {}

/// The sheet read as an engineering drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartReading {
    /// What the sheet depicts, in a line: "LDW280-17149-13, a formed
    /// sheet-metal stiffener".
    pub summary: String,
    /// The reading in full: the views and how they relate, the material
    /// and how the part is made, what it is for.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Its features, in reading order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<Feature>,
    /// What still does not make sense, each in a sentence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<String>,
    /// Who read it.
    pub by: Agent,
    /// When.
    pub when: Date,
}

/// A feature of the part, and where on the paper it is drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    /// Its name: "joggle", "IML BDN", "left end".
    pub name: String,
    /// What kind of feature, in a word: outline, edge, bend, joggle,
    /// jog, flange, hole, notch, chamfer, fillet, marking, symbol,
    /// dimension, note.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// What it is and how it is drawn ("a bend line, phantom, the top
    /// flange bent down along it").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Where it is drawn: boxes on the paper.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub at: Vec<Rect<Paper>>,
}

impl PartReading {
    /// A reading says what the sheet depicts, and names its features.
    pub fn check(&self) -> Result<(), Error> {
        if self.summary.trim().is_empty() {
            return Err(Error::NoSummary);
        }
        if let Some(k) = self.features.iter().position(|f| f.name.trim().is_empty()) {
            return Err(Error::UnnamedFeature(k));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::geometry::P2;

    fn reading() -> PartReading {
        PartReading {
            summary: "a formed strip".into(),
            description: String::new(),
            features: vec![Feature {
                name: "joggle".into(),
                kind: "joggle".into(),
                description: "two solid lines across the web".into(),
                at: vec![Rect::new(P2::new(1.0, 2.0), P2::new(3.0, 4.0)).unwrap()],
            }],
            questions: vec!["why is the top edge phantom?".into()],
            by: Agent::Claude,
            when: Date::new(2026, 10, 22).unwrap(),
        }
    }

    /// A reading round-trips through its file form, and says what the
    /// sheet depicts and names its features.
    #[test]
    fn a_reading_names_the_part_and_its_features() {
        let r = reading();
        assert_eq!(r.check(), Ok(()));
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<PartReading>(&text).unwrap(), r);
        let mut blank = r.clone();
        blank.summary = " ".into();
        assert_eq!(blank.check(), Err(Error::NoSummary));
        let mut unnamed = r;
        unnamed.features[0].name.clear();
        assert_eq!(unnamed.check(), Err(Error::UnnamedFeature(0)));
    }
}
