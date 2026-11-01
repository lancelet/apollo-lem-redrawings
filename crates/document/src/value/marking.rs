//! The marking region: the boxed X of a part (docs/redraw-plan.md K5,
//! chunk 14), where the part number and the other markings are stamped.
//!
//! On a drawing it is a rectangle with both its diagonals, and the form's
//! operations note points at it ("... IN REGION MARKED" and the same
//! boxed X, printed). The lines are ordinary solved geometry of a view,
//! with all their constraints and their layer (the drawing's convention
//! stays: visible lines); the region is the node that says what they
//! *are*: it owns the four sides and the two diagonals, so the ink they
//! were fitted from is its ink, and it holds the typed link to the form's
//! printed note ([`NamedBy`]: the region *is named by* the note's line and
//! its box-X, which *name* it) and the region's meaning.

use crate::value::geometry::EntityId;
use crate::value::provenance::Provenance;
use crate::value::view::ViewId;
use serde::{Deserialize, Serialize};

id!(
    /// The name of a marking region.
    MarkingRegionId
);

/// What the region is for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum Meaning {
    /// The part number and all the other markings are stamped in it.
    PartNumberAndMarkings,
    /// Anything else, in words.
    Other(String),
}

impl Meaning {
    /// The words the command takes: `part-number-and-markings`,
    /// `other(TEXT)`.
    pub fn words(&self) -> String {
        match self {
            Meaning::PartNumberAndMarkings => "part-number-and-markings".into(),
            Meaning::Other(t) => format!("other({t})"),
        }
    }

    /// The meaning the words name, or None.
    pub fn parse(words: &str) -> Option<Meaning> {
        match words.trim() {
            "part-number-and-markings" => Some(Meaning::PartNumberAndMarkings),
            w => {
                let t = w.strip_prefix("other(")?.strip_suffix(')')?.trim();
                (!t.is_empty()).then(|| Meaning::Other(t.into()))
            }
        }
    }
}

/// The printed form's side of the link: the items of the placed form
/// that name the region, by their names
/// ([`FormItem::name`](crate::value::form::FormItem::name)).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedBy {
    /// The operations note's printed line that points at the region
    /// ("IN REGION MARKED").
    pub note: String,
    /// The rules of the box-X printed beside it (its sides and
    /// diagonals).
    pub box_x: Vec<String>,
}

/// A marking region ([module](self)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarkingRegion {
    /// The view whose lines they are.
    pub view: ViewId,
    /// The rectangle's four sides.
    pub rectangle: [EntityId; 4],
    /// The two diagonals.
    pub diagonals: [EntityId; 2],
    /// What the region is for.
    pub meaning: Meaning,
    /// The form's printed note and box-X that name it, when the sheet's
    /// form has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_by: Option<NamedBy>,
    /// Who stated it, and on what evidence.
    pub provenance: Provenance,
}

/// What can be wrong with a marking region.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// One line is named twice among its six.
    Repeated(EntityId),
    /// A meaning of `other` with no words.
    NoMeaning,
    /// A link to the form with no box-X, or a repeated item.
    BadLink,
    /// Its provenance is not valid.
    Provenance(crate::value::provenance::Error),
    /// It names a view the document does not have.
    NoView(ViewId),
    /// A line of it is not a line of the view.
    NoLine(EntityId),
    /// It is named by form items, and the document has no form that has
    /// them (the item's name).
    NoFormItem(String),
    /// A line of it is already another region's.
    Shared(EntityId),
}

impl MarkingRegion {
    /// Its six lines: the sides, then the diagonals.
    pub fn lines(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.rectangle.iter().chain(&self.diagonals).copied()
    }

    /// Whether `line` is one of its six.
    pub fn has(&self, line: EntityId) -> bool {
        self.lines().any(|l| l == line)
    }

    /// Valid on its own: six different lines, a meaning that says
    /// something, a link with a note and a box-X, valid provenance.
    pub fn check(&self) -> Result<(), Error> {
        let lines: Vec<EntityId> = self.lines().collect();
        for (k, l) in lines.iter().enumerate() {
            if lines[..k].contains(l) {
                return Err(Error::Repeated(*l));
            }
        }
        if matches!(&self.meaning, Meaning::Other(t) if t.trim().is_empty()) {
            return Err(Error::NoMeaning);
        }
        if let Some(n) = &self.named_by {
            let mut items: Vec<&String> = n.box_x.iter().collect();
            items.push(&n.note);
            let distinct = items
                .iter()
                .enumerate()
                .all(|(k, i)| !items[..k].contains(i));
            if n.box_x.is_empty() || !distinct {
                return Err(Error::BadLink);
            }
        }
        self.provenance.check().map_err(Error::Provenance)
    }
}
