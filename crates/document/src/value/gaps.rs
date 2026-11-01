//! The sheet's gaps: what the document does not yet say of its sheet
//! (docs/redraw-plan.md §2.9).
//!
//! A gap is one of three kinds: a node kind the format lacks (the ⊠
//! marking region, drawn as plain lines until it is a node), a reading
//! not to be relied on, or a limit of the format (a leader routed from a
//! typeset anchor). It names the nodes it is about, or a box of a frame's
//! pixels when no node holds it, or both; who saw it and when; and where
//! it stands.
//!
//! An unmodelled mark is a gap by itself, and the aggregator
//! (`tools/gaps.py`) counts it with the listed gaps, unless a gap names
//! it: then that gap is its row (the marking region and its symbol).
//!
//! A gap's id names the gap, not its place: the same gap seen on two
//! sheets has the same id on both, so the aggregator can count the
//! sheets it is seen on.

use crate::value::claims::Node;
use crate::value::frame::FrameId;
use crate::value::provenance::{Agent, Date};
use serde::{Deserialize, Serialize};

id! {
    /// A gap's name, the same on every sheet it is seen on.
    GapId
}

/// What kind of gap ([module](self)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A node kind the format lacks: what it would be is drawn some
    /// other way meanwhile.
    MissingNodeKind,
    /// A reading not to be relied on, or not made.
    UnreliableReading,
    /// Something the format cannot yet say.
    FormatLimitation,
}

/// A box of a frame's pixels, `[x0, y0, x1, y1]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// The frame.
    pub frame: FrameId,
    /// The box, in its pixels.
    pub px: [f64; 4],
}

/// Who saw a gap, and when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    /// Who.
    pub by: Agent,
    /// When.
    pub when: Date,
}

/// Where a gap stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Status {
    /// Nothing planned.
    Open,
    /// To be closed by the plan named (`redraw plan chunk 14`).
    Planned {
        /// The plan.
        plan: String,
    },
    /// Closed, and how; kept for the record.
    Closed {
        /// How.
        how: String,
    },
}

/// A gap ([module](self)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    /// What is missing, in words.
    pub what: String,
    /// Its kind.
    pub kind: Kind,
    /// The nodes it is about.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<Node>,
    /// Where it is, when no node holds it (or to show it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Region>,
    /// Who saw it, and when.
    pub seen: Seen,
    /// Where it stands.
    pub status: Status,
}

impl Gap {
    /// Valid on its own: it says what it is, it is somewhere (nodes or a
    /// region), and its region is a finite box with area.
    pub fn check(&self) -> Result<(), Error> {
        if self.what.trim().is_empty() {
            return Err(Error::NoDescription);
        }
        if self.nodes.is_empty() && self.region.is_none() {
            return Err(Error::Nowhere);
        }
        if let Some(Region { px, .. }) = &self.region {
            let [x0, y0, x1, y1] = *px;
            if !(px.iter().all(|v| v.is_finite()) && x1 > x0 && y1 > y0) {
                return Err(Error::BadRegion(*px));
            }
        }
        Ok(())
    }
}

/// What can be wrong with a gap.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// It says nothing of what is missing.
    NoDescription,
    /// It names no node and no region.
    Nowhere,
    /// Its region is not a finite box with area.
    BadRegion([f64; 4]),
    /// Its region cites a frame the document does not have.
    NoFrame(FrameId),
    /// It names a node the document does not have.
    NoNode(Node),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::value::claims;

    pub(crate) fn gap(nodes: &[(claims::Kind, &str)]) -> Gap {
        Gap {
            what: "the ⊠ marking region, drawn as plain lines".into(),
            kind: Kind::MissingNodeKind,
            nodes: nodes
                .iter()
                .map(|(kind, name)| Node {
                    kind: *kind,
                    name: (*name).into(),
                })
                .collect(),
            region: None,
            seen: Seen {
                by: Agent::Claude,
                when: Date::new(2026, 10, 27).unwrap(),
            },
            status: Status::Planned {
                plan: "redraw plan chunk 14".into(),
            },
        }
    }

    #[test]
    fn a_gap_round_trips_through_its_text() {
        let mut g = gap(&[(claims::Kind::Text, "T1")]);
        g.region = Some(Region {
            frame: "scan".into(),
            px: [1.0, 2.0, 30.0, 40.0],
        });
        let text = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<Gap>(&text).unwrap(), g);
        assert_eq!(g.check(), Ok(()));
    }

    #[test]
    fn bad_gaps_are_refused() {
        let g = gap(&[]);
        assert_eq!(g.check(), Err(Error::Nowhere));
        let blank = Gap {
            what: " ".into(),
            ..gap(&[(claims::Kind::Text, "T1")])
        };
        assert_eq!(blank.check(), Err(Error::NoDescription));
        let named = gap(&[(claims::Kind::Unmodelled, "u1")]);
        assert_eq!(named.check(), Ok(()));
        let flat = Gap {
            region: Some(Region {
                frame: "scan".into(),
                px: [1.0, 2.0, 1.0, 40.0],
            }),
            ..g
        };
        assert!(matches!(flat.check(), Err(Error::BadRegion(_))));
    }
}
