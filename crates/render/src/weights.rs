//! Which kind of line a drawn item is, and the sheet's measured pen for
//! it ([`LineWeights`](lmsheets_document::value::weights::LineWeights)).
//!
//! A line's kind follows from what it is in the document: an entity's
//! convention (and, for a marking region's six lines, the region that
//! owns them), a dimension's line, a
//! witness, a leader, a centre line. Never its layer's name. The drawing
//! plane applies the pens once, after every item is made
//! ([`apply`]), so the layout (clearances, arrowheads, dashes) works from
//! the standard pens and only the stroke widths change.

use crate::scene::{Part, Plane, Primitive, Source};
use lmsheets_document::state::document::Document;
use lmsheets_document::value::sketch::EntityMeta;
use lmsheets_document::value::style::Convention;
use lmsheets_document::value::weights::LineKind;

/// The kind of line a drawn item is, if it has a pen of its own kind.
pub fn kind_of(doc: &Document, source: Source) -> Option<LineKind> {
    match source {
        Source::Sheet(e) => entity_kind(doc, doc.sheet().meta(e)?),
        Source::View(v, e) => {
            if doc.marking_region_of(v.as_str(), e.as_str()).is_some() {
                return Some(LineKind::MarkingRegion);
            }
            let meta = doc.views().view(v)?.sketch.meta(e)?;
            entity_kind(doc, meta)
        }
        Source::Observed(_) => Some(LineKind::Outline),
        Source::CentreLine(_) | Source::CentreMark(_) => Some(LineKind::Centre),
        Source::Dimension(_, Part::Line) => Some(LineKind::Dimension),
        Source::Witness(_) => Some(LineKind::Extension),
        Source::Leader(_) => Some(LineKind::Leader),
        _ => None,
    }
}

/// An entity's kind from its convention on its layer.
fn entity_kind(doc: &Document, meta: &EntityMeta) -> Option<LineKind> {
    match meta.convention_on(doc.layer(meta.layer)?)? {
        Convention::Visible => Some(LineKind::Outline),
        Convention::Hidden => Some(LineKind::Hidden),
        Convention::Centre => Some(LineKind::Centre),
        Convention::Format => Some(LineKind::Format),
        _ => None,
    }
}

/// Set the stroke widths (and a head's outline) of every item whose kind
/// the sheet has measured to the measured pen.
pub fn apply(doc: &Document, plane: &mut Plane) {
    let weights = doc.weights();
    if weights.is_empty() {
        return;
    }
    for item in &mut plane.items {
        let Some(kind) = kind_of(doc, item.source) else {
            continue;
        };
        let Some(m) = weights.get(kind) else {
            continue;
        };
        match &mut item.primitive {
            Primitive::Stroke { weight, .. } | Primitive::Marker { weight, .. } => *weight = m.mm,
            Primitive::Fill { outline, .. } => {
                if outline.is_some() {
                    *outline = Some(m.mm);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::draw;
    use lmsheets_document::api::msg::Op;
    use lmsheets_document::examples::block;
    use lmsheets_document::value::style::Purpose;
    use lmsheets_document::value::weights::Measured;

    /// A sheet's measured pen replaces the layer's for its kind alone.
    #[test]
    fn a_measured_kind_draws_at_its_pen_and_the_rest_at_the_layers() {
        let mut d = block();
        let widths = |d: &Document| -> Vec<(Option<LineKind>, f64)> {
            draw(d, Purpose::Export)
                .items
                .iter()
                .filter_map(|i| match i.primitive {
                    Primitive::Stroke { weight, .. } => Some((kind_of(d, i.source), weight)),
                    _ => None,
                })
                .collect()
        };
        let before = widths(&d);
        assert!(before
            .iter()
            .any(|(k, w)| *k == Some(LineKind::Outline) && *w == 0.7));
        let weight = Some(Measured {
            mm: 0.55,
            samples: 10,
            spread: [0.5, 0.6],
        });
        d.apply_in_place(&Op::SetLineWeight {
            kind: LineKind::Outline,
            weight,
        })
        .unwrap();
        let after = widths(&d);
        assert_eq!(before.len(), after.len());
        for ((k, w0), (_, w1)) in before.iter().zip(&after) {
            assert_eq!(
                *w1,
                if *k == Some(LineKind::Outline) {
                    0.55
                } else {
                    *w0
                }
            );
        }
    }
}
