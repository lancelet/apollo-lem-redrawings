//! The paper trail of a node: its claims, each through the frame it cites
//! (and the evidence of that frame) and the frame's hops back to the
//! archive. One walk, for the acceptance check A8 (every claim resolves)
//! and for the viewer's detail panel (a node's trail), so the two cannot
//! disagree (docs/redraw-plan.md §5, chunk 22).

use crate::state::document::Document;
use crate::value::claims::{Claim, Evidence, Node};
use crate::value::frame::{Frame, FrameId};
use crate::value::hops::{self, Upload};

/// One claim followed back: the claim, the frame it cites, that frame's
/// evidence, and the upload its hops lead to.
#[derive(Debug)]
pub struct ClaimTrail<'a> {
    /// The claim's place among the document's.
    pub index: usize,
    /// The claim.
    pub claim: &'a Claim,
    /// The frame it cites, if the document has it.
    pub frame: Option<(&'a FrameId, &'a Frame)>,
    /// The evidence of that frame, if it has any.
    pub evidence: Option<&'a Evidence>,
    /// The walk back from the frame's hash to the upload in the archive.
    pub upload: Result<&'a Upload, hops::Error>,
}

impl ClaimTrail<'_> {
    /// The archive URL of the original the frame came from, when the hops
    /// hold.
    pub fn archive_url(&self) -> Option<&str> {
        self.upload.as_ref().ok().map(|u| u.original.url.as_str())
    }

    /// The ink the claim takes as runs along the evidence's pixels: how
    /// many pixels, and how many whole segments besides.
    pub fn ink(&self) -> (u64, usize) {
        let pixels = self.claim.run_pixels().count() as u64;
        (pixels, self.claim.segments.len())
    }
}

impl Document {
    /// The claim `index`, followed back to the archive; none past the end.
    pub fn claim_trail(&self, index: usize) -> Option<ClaimTrail<'_>> {
        let claim = self.claims().get(index)?;
        let frame = self.frames().find(|(id, _)| **id == claim.evidence);
        let evidence = frame.and_then(|(_, f)| f.evidence.as_ref());
        let upload = match frame {
            Some((_, f)) => hops::walk(&f.hops, f.hash.as_deref()),
            None => Err(hops::Error::Empty),
        };
        Some(ClaimTrail {
            index,
            claim,
            frame,
            evidence,
            upload,
        })
    }

    /// Every claim followed back, in the document's order.
    pub fn claim_trails(&self) -> impl Iterator<Item = ClaimTrail<'_>> {
        (0..self.claims().len()).filter_map(|k| self.claim_trail(k))
    }

    /// The trails of the claims on `node`.
    pub fn trail_of<'a>(&'a self, node: &'a Node) -> impl Iterator<Item = ClaimTrail<'a>> {
        self.claim_trails().filter(move |t| &t.claim.node == node)
    }
}
