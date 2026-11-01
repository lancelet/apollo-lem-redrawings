//! Terminators: the arrowhead and the dot that end a leader's tip, a
//! dimension line, or a section indicator, as primitives.
//!
//! One convention, stated once: the tip is exactly on the target and
//! the head extends back along the direction of arrival. A dimension's
//! outside arrows, whose tips are on the extension lines, arrive from
//! outside and need no flag.

use crate::scene::{Fill, Primitive};
use lmsheets_document::value::drafting::{Drafting, Head, Terminator};
use lmsheets_document::value::geometry::{Paper, P2};

/// A filled arrowhead with its tip at `at`, arriving along `towards`
/// (any length; the head points the way it points), of this size,
/// outlined at `outline` if given. None if `towards` has no direction.
pub fn arrowhead(
    at: P2<Paper>,
    towards: P2<Paper>,
    head: Head,
    outline: Option<f64>,
) -> Option<Primitive> {
    let len = (towards.x * towards.x + towards.y * towards.y).sqrt();
    if len.is_nan() || len <= 0.0 {
        return None;
    }
    let (ux, uy) = (towards.x / len, towards.y / len);
    let (l, w) = (head.length, head.width / 2.0);
    let (bx, by) = (at.x - ux * l, at.y - uy * l);
    Some(Primitive::Fill {
        fill: Fill::Polygon {
            points: vec![
                at,
                P2::new(bx - uy * w, by + ux * w),
                P2::new(bx + uy * w, by - ux * w),
            ],
        },
        outline,
    })
}

/// A filled dot centred at `at`, `diameter` across.
pub fn dot(at: P2<Paper>, diameter: f64) -> Primitive {
    Primitive::Fill {
        fill: Fill::Disc {
            centre: at,
            radius: diameter / 2.0,
        },
        outline: None,
    }
}

/// The terminator of a kind at `at`, arriving along `towards`, at the
/// drafting table's sizes; the arrowhead is outlined at `weight`, the
/// line's. None for no terminator, or for an arrow with no direction.
pub fn terminator(
    kind: Terminator,
    at: P2<Paper>,
    towards: P2<Paper>,
    drafting: &Drafting,
    weight: f64,
) -> Option<Primitive> {
    match kind {
        Terminator::Arrow => arrowhead(at, towards, drafting.arrow(), Some(weight)),
        Terminator::Dot => Some(dot(at, drafting.dot())),
        Terminator::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_point_the_way_they_arrive() {
        let head = Head::new(4.0, 2.0, 0.5).unwrap();
        // Arriving along +x: the base is 4 back, corners 1 either side.
        let Some(Primitive::Fill {
            fill: Fill::Polygon { points },
            outline,
        }) = arrowhead(P2::new(10.0, 0.0), P2::new(3.0, 0.0), head, Some(0.5))
        else {
            panic!()
        };
        assert_eq!(outline, Some(0.5));
        assert_eq!(points[0], P2::new(10.0, 0.0));
        assert_eq!(points[1], P2::new(6.0, 1.0));
        assert_eq!(points[2], P2::new(6.0, -1.0));
        assert_eq!(
            arrowhead(P2::new(0.0, 0.0), P2::new(0.0, 0.0), head, None),
            None
        );
        let d = Drafting::standard();
        assert_eq!(
            terminator(
                Terminator::None,
                P2::new(0.0, 0.0),
                P2::new(1.0, 0.0),
                &d,
                0.3
            ),
            None
        );
        assert!(matches!(
            terminator(
                Terminator::Dot,
                P2::new(0.0, 0.0),
                P2::new(0.0, 0.0),
                &d,
                0.3
            ),
            Some(Primitive::Fill {
                fill: Fill::Disc { .. },
                outline: None
            })
        ));
    }
}
