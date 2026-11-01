//! The evidence a frame was read with, and the claims on it: which of
//! its ink each node was made from (docs/redraw-plan.md §2.4).
//!
//! **The evidence** belongs to a frame, since it is a reading of the
//! frame's pixels: the pixelnet run@step that read the ink and the
//! strokenet run@step that grouped it into segments, each with the hash
//! of its output, how many ink pixels pixelnet read, and the ids of
//! strokenet's segments (stable hashes, `t7eff15e54e8`).
//!
//! **A claim** is a node's: the ink it was made from, which part of it
//! that ink is (a line, lettering, an arrowhead, or what a mark made it)
//! and, for ink a mark took, whose mark it was. Its ink is the segments
//! it takes whole, by id, and the rest as runs along pixelnet's ink
//! (the evidence's ink pixels in reading order): a segment the claim
//! shares with another node, by nearest shape, and ink in no segment (a
//! head's wedge, a mark's region). Every claim cites the frame whose
//! evidence its segments and runs index. The pixels follow from the
//! segments and runs with the evidence; they are not stored.
//!
//! Marks of no drawing node (noise, foreign marks, pencil, ignored ink)
//! claim ink as the sheet's non-drawing nodes, named by what they are.

use crate::value::frame::FrameId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

id! {
    /// A strokenet segment's id.
    SegmentId
}

/// A model's output a frame was read with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    /// The model, `run@step`.
    pub model: String,
    /// The output file's hash, `algorithm:hex`.
    pub hash: String,
}

/// What a frame was read with ([module](self)).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// The ink read per pixel.
    pub pixelnet: Output,
    /// The ink grouped into segments; none when the segments were made
    /// from pixelnet's output alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strokenet: Option<Output>,
    /// How many ink pixels pixelnet read: a run indexes `0..ink_px`.
    pub ink_px: u32,
    /// The segments, by id.
    pub segments: Vec<SegmentId>,
}

impl Evidence {
    /// Its models named and its segments each once.
    pub fn check(&self) -> Result<(), Error> {
        let outputs = std::iter::once(&self.pixelnet).chain(&self.strokenet);
        if outputs
            .into_iter()
            .any(|o| o.model.is_empty() || o.hash.is_empty())
        {
            return Err(Error::Unnamed);
        }
        let mut seen = HashSet::with_capacity(self.segments.len());
        match self.segments.iter().find(|s| !seen.insert(**s)) {
            Some(s) => Err(Error::Twice(*s)),
            None => Ok(()),
        }
    }
}

/// The kinds of node a claim names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An entity of the sheet (its id) or of a view (`view/id`).
    Entity,
    /// A text.
    Text,
    /// A leader.
    Leader,
    /// A dimension: its lines and its value.
    Dimension,
    /// The witness lines of the dimensions named (comma-separated).
    Witness,
    /// A centre mark.
    CentreMark,
    /// A centre line.
    CentreLine,
    /// An unmodelled mark.
    Unmodelled,
    /// An observed feature: a rounding drawn in place of a sharp corner
    /// (docs/redraw-plan.md §8.3).
    Observed,
    /// A marking region: its rectangle and diagonals' ink
    /// (docs/redraw-plan.md K5).
    MarkingRegion,
    /// A printed item of the placed form, by its name
    /// ([`FormItem::name`](crate::value::form::FormItem::name)): its
    /// printed ink.
    Form,
    /// A field of the placed form, by its name: its entry's ink.
    Field,
    /// A non-drawing node, named by what it is: no mark (dirt, a speck).
    Noise,
    /// A non-drawing node: a mark not of the drawing (a stamp, the
    /// film's edge).
    Foreign,
    /// A non-drawing node: pencil.
    Pencil,
    /// A non-drawing node: ink not known (no label).
    Ignored,
}

/// A node a claim names.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Node {
    /// Its kind.
    pub kind: Kind,
    /// Its document id; a non-drawing node's, what it is.
    pub name: String,
}

/// Which part of its node the ink is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// Line work.
    Line,
    /// Lettering: a text, a dimension's value.
    Lettering,
    /// An arrowhead.
    Arrowhead,
    /// What a mark of no drawing node made it: no mark.
    Noise,
    /// A mark not of the drawing.
    Foreign,
    /// Pencil.
    Pencil,
    /// Not known.
    Ignored,
}

/// Whose mark took the ink.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum By {
    /// A person, in the window.
    User,
    /// A rule's guess, accepted.
    Rule,
    /// An agent, from looking at the scan.
    Agent,
}

/// A node's claim on ink of a frame ([module](self)).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// The node.
    pub node: Node,
    /// Which part of it the ink is.
    pub part: Part,
    /// Whose mark took it; none for the sketch's own assignments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<By>,
    /// The frame whose evidence it cites.
    pub evidence: FrameId,
    /// The segments it takes whole.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<SegmentId>,
    /// The rest of its ink, as runs along the evidence's ink pixels:
    /// pixels skipped, pixels taken, in turn, from the first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<u32>,
}

impl Claim {
    /// Its ink not in its whole segments: indices into the evidence's
    /// ink pixels, ascending.
    pub fn run_pixels(&self) -> impl Iterator<Item = u32> + '_ {
        let mut at = 0u32;
        self.runs.chunks(2).flat_map(move |r| {
            let from = at + r[0];
            at = from + r.get(1).copied().unwrap_or(0);
            from..at
        })
    }

    /// Valid against `evidence`: its segments known, its runs in pairs,
    /// each run taking ink and each after the first skipping some, all
    /// within the ink.
    pub fn check(&self, evidence: &Evidence, segments: &HashSet<SegmentId>) -> Result<(), Error> {
        if let Some(s) = self.segments.iter().find(|s| !segments.contains(s)) {
            return Err(Error::UnknownSegment(*s));
        }
        let paired = self.runs.len() % 2 == 0;
        let gaps = self.runs.iter().step_by(2).skip(1).all(|&n| n > 0);
        let takes = self.runs.iter().skip(1).step_by(2).all(|&n| n > 0);
        let end = self.runs.iter().try_fold(0u32, |a, &n| a.checked_add(n));
        if !(paired && gaps && takes && end.is_some_and(|e| e <= evidence.ink_px)) {
            return Err(Error::BadRuns);
        }
        Ok(())
    }
}

/// Runs ([`Claim::runs`]) of ascending ink pixel indices.
pub fn runs(pixels: impl IntoIterator<Item = u32>) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut end = 0u32;
    for p in pixels {
        match out.last_mut() {
            Some(take) if p == end => *take += 1,
            _ => out.extend([p - end, 1]),
        }
        end = p + 1;
    }
    out
}

/// What can be wrong with evidence or a claim.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A model or a hash left empty.
    Unnamed,
    /// A segment listed twice.
    Twice(SegmentId),
    /// A claim citing a segment its evidence does not have.
    UnknownSegment(SegmentId),
    /// A claim's runs not as [`Claim::runs`] says, or past the ink.
    BadRuns,
    /// A claim citing a frame that has no evidence, or no such frame.
    NoEvidence(FrameId),
    /// A claim naming a node the document does not have.
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

    pub(crate) fn evidence() -> Evidence {
        Evidence {
            pixelnet: Output {
                model: "r3b@110000".into(),
                hash: "fnv1a64:0123456789abcdef".into(),
            },
            strokenet: Some(Output {
                model: "r3b@48000".into(),
                hash: "fnv1a64:fedcba9876543210".into(),
            }),
            ink_px: 100,
            segments: vec!["t7eff15e54e8".into(), "t0a1b2c3d4e5".into()],
        }
    }

    pub(crate) fn claim(kind: Kind, name: &str) -> Claim {
        Claim {
            node: Node {
                kind,
                name: name.into(),
            },
            part: Part::Line,
            by: None,
            evidence: "scan".into(),
            segments: vec!["t7eff15e54e8".into()],
            runs: runs([3, 4, 5, 9, 99]),
        }
    }

    #[test]
    fn runs_round_trip() {
        let px = [0, 1, 2, 7, 9, 10, 40];
        let c = Claim {
            runs: runs(px),
            ..claim(Kind::Text, "T1")
        };
        assert_eq!(c.runs, [0, 3, 4, 1, 1, 2, 29, 1]);
        assert_eq!(c.run_pixels().collect::<Vec<_>>(), px);
        assert!(runs([]).is_empty());
    }

    #[test]
    fn a_claim_round_trips_through_its_text() {
        let mut c = claim(Kind::Foreign, "NAS 9-1100 stamp");
        c.part = Part::Foreign;
        c.by = Some(By::Agent);
        let text = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Claim>(&text).unwrap(), c);
        let e = evidence();
        let text = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Evidence>(&text).unwrap(), e);
    }

    #[test]
    fn bad_evidence_and_claims_are_refused() {
        let e = evidence();
        let known: HashSet<SegmentId> = e.segments.iter().copied().collect();
        assert_eq!(e.check(), Ok(()));
        let c = claim(Kind::Text, "T1");
        assert_eq!(c.check(&e, &known), Ok(()));
        let unknown = Claim {
            segments: vec!["tffffffffff0".into()],
            ..c.clone()
        };
        assert_eq!(
            unknown.check(&e, &known),
            Err(Error::UnknownSegment("tffffffffff0".into()))
        );
        for bad in [vec![3], vec![3, 0], vec![0, 2, 0, 1], vec![99, 2]] {
            let b = Claim {
                runs: bad.clone(),
                ..c.clone()
            };
            assert_eq!(b.check(&e, &known), Err(Error::BadRuns), "{bad:?}");
        }
        let mut twice = e.clone();
        twice.segments.push("t7eff15e54e8".into());
        assert_eq!(twice.check(), Err(Error::Twice("t7eff15e54e8".into())));
        let mut unnamed = e;
        unnamed.pixelnet.hash.clear();
        assert_eq!(unnamed.check(), Err(Error::Unnamed));
    }
}
