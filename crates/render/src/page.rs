//! How paper millimetres land on an output page: its size, where the
//! paper's origin sits on it, the scale, and whether line weights scale
//! with the drawing.

use lmsheets_document::value::geometry::Rect;
use lmsheets_document::value::geometry::{Paper, P2};

/// A page size, mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSize {
    /// Width, mm.
    pub width: f64,
    /// Height, mm.
    pub height: f64,
}

impl PageSize {
    /// ISO A4, portrait.
    pub const A4: PageSize = PageSize {
        width: 210.0,
        height: 297.0,
    };
    /// ISO A3, portrait.
    pub const A3: PageSize = PageSize {
        width: 297.0,
        height: 420.0,
    };
    /// US Letter, portrait.
    pub const LETTER: PageSize = PageSize {
        width: 215.9,
        height: 279.4,
    };

    /// The size turned so the long side runs across.
    pub fn landscape(self) -> Self {
        Self {
            width: self.width.max(self.height),
            height: self.width.min(self.height),
        }
    }

    /// The size turned so the long side runs down.
    pub fn portrait(self) -> Self {
        Self {
            width: self.width.min(self.height),
            height: self.width.max(self.height),
        }
    }
}

/// Whether line weights (and point markers, which follow them) scale with
/// the drawing when it is scaled to a page, or stay at their paper
/// millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineWidths {
    /// Weights scale with the drawing: a reduced print has finer lines.
    Scaled,
    /// Weights stay as the layer table says, mm on the output.
    Fixed,
}

/// How to lay the drawing on a page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// A page just large enough for the drawing plus a margin all round,
    /// at full scale.
    Tight {
        /// The margin, mm.
        margin: f64,
    },
    /// A page of this size with the drawing scaled to fill it inside the
    /// margin, centred. A drawing with no extent (a single point) is
    /// placed at full scale.
    Page {
        /// The page.
        size: PageSize,
        /// The margin, mm.
        margin: f64,
    },
    /// A page of this size with the drawing at this scale, centred;
    /// what does not fit runs off the page.
    Scale {
        /// The page.
        size: PageSize,
        /// Page millimetres per paper millimetre.
        scale: f64,
    },
}

/// An output page, mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page {
    /// Width, mm.
    pub width: f64,
    /// Height, mm.
    pub height: f64,
    /// Where the paper's origin lands on the page, mm.
    pub origin: P2<Paper>,
    /// Page millimetres per paper millimetre.
    pub scale: f64,
    /// Whether weights scale with the drawing.
    pub line_widths: LineWidths,
}

impl Page {
    /// A page of this size showing the paper at full scale, origin at the
    /// page's top-left corner.
    pub fn sized(size: PageSize) -> Self {
        Self {
            width: size.width,
            height: size.height,
            origin: P2::new(0.0, 0.0),
            scale: 1.0,
            line_widths: LineWidths::Scaled,
        }
    }

    /// The page for a drawing's bounds under a fit.
    pub fn layout(bounds: Rect<Paper>, fit: Fit, line_widths: LineWidths) -> Self {
        let (w, h) = (bounds.width(), bounds.height());
        let mut page = match fit {
            Fit::Tight { margin } => Self {
                width: w + 2.0 * margin,
                height: h + 2.0 * margin,
                origin: P2::new(margin - bounds.min().x, margin - bounds.min().y),
                scale: 1.0,
                line_widths,
            },
            Fit::Page { size, margin } => {
                let avail = (size.width - 2.0 * margin, size.height - 2.0 * margin);
                let scale = (avail.0 / w).min(avail.1 / h);
                let scale = if scale.is_finite() && scale > 0.0 {
                    scale
                } else {
                    1.0
                };
                Self::centred(bounds, size, scale, line_widths)
            }
            Fit::Scale { size, scale } => Self::centred(bounds, size, scale, line_widths),
        };
        page.line_widths = line_widths;
        page
    }

    fn centred(bounds: Rect<Paper>, size: PageSize, scale: f64, line_widths: LineWidths) -> Self {
        let (w, h) = (bounds.width(), bounds.height());
        Self {
            width: size.width,
            height: size.height,
            origin: P2::new(
                (size.width - w * scale) / 2.0 - bounds.min().x * scale,
                (size.height - h * scale) / 2.0 - bounds.min().y * scale,
            ),
            scale,
            line_widths,
        }
    }

    /// A paper point on the page.
    pub fn to_page(&self, p: P2<Paper>) -> (f64, f64) {
        (
            self.origin.x + p.x * self.scale,
            self.origin.y + p.y * self.scale,
        )
    }

    /// A paper length on the page.
    pub fn length(&self, mm: f64) -> f64 {
        mm * self.scale
    }

    /// A line weight on the page.
    pub fn weight(&self, mm: f64) -> f64 {
        match self.line_widths {
            LineWidths::Scaled => mm * self.scale,
            LineWidths::Fixed => mm,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_place_the_paper() {
        let b = Rect::new(P2::new(10.0, 20.0), P2::new(110.0, 70.0)).unwrap();
        let p = Page::layout(b, Fit::Tight { margin: 5.0 }, LineWidths::Scaled);
        assert_eq!((p.width, p.height), (110.0, 60.0));
        assert_eq!(p.to_page(P2::new(10.0, 20.0)), (5.0, 5.0));
        // Fit 100 × 50 into A4 landscape with 10 mm margins: the width
        // limits, scale 2.77, centred vertically; fixed weights stay.
        let f = Page::layout(
            b,
            Fit::Page {
                size: PageSize::A4.landscape(),
                margin: 10.0,
            },
            LineWidths::Fixed,
        );
        assert_eq!((f.width, f.height), (297.0, 210.0));
        assert!((f.scale - 2.77).abs() < 1e-12);
        let (x, y) = f.to_page(P2::new(10.0, 20.0));
        assert!((x - 10.0).abs() < 1e-9);
        assert!((y - (210.0 - 50.0 * 2.77) / 2.0).abs() < 1e-9);
        assert_eq!(f.weight(0.7), 0.7);
        assert!((f.length(1.0) - 2.77).abs() < 1e-12);
        // Full scale on A4 portrait: centred, scale 1.
        let s = Page::layout(
            b,
            Fit::Scale {
                size: PageSize::A4,
                scale: 1.0,
            },
            LineWidths::Scaled,
        );
        assert_eq!(s.scale, 1.0);
        assert_eq!(s.to_page(P2::new(60.0, 45.0)), (105.0, 148.5));
    }
}
