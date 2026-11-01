//! Typesetting: a text block's derived box, its anchors, and its glyph
//! strokes in paper millimetres.
//!
//! The block is laid out in a local frame, x right and y up in
//! millimetres, the last line's baseline at y = 0 and the first line's
//! cap line at the top; lines are set regularly from the font's
//! advances at the role's cap height and width factor, one pitch apart,
//! aligned left, centred or right within the block's width. A line's
//! spans sit side by side: a run on the baseline; a stack as two rows
//! about the line's middle (cap / 2), each centred on the other, at a
//! share of the cap, with a bar between them at the middle if asked.
//! The box is from the left of the widest line to its right and from
//! the last baseline (or the lowest stack's bottom) to the first cap
//! line (or the tallest stack's top). An [`Anchor`] names a point of
//! that frame; the text's placement puts that point on the paper,
//! turned by the text's angle, and every stroke follows.
//!
//! Glyph arcs are circular in the font; under a width factor other than
//! one they are ellipses, and are kept as such.

use crate::scene::Path;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::bounds::Bounded;
use lmsheets_document::value::geometry::Rect;
use lmsheets_document::value::geometry::{Paper, P2};
use lmsheets_document::value::lettering::Lettering;
use lmsheets_document::value::style::Pen;
use lmsheets_document::value::text::{
    Align, Anchor, BoxPoint, Enclosure, Line, Side, Size, Span, StackAlign, StackStyle, Text,
};
use lmsheets_font::{Font, Stroke};
use std::f64::consts::TAU;

/// A point in the block's local frame, mm, y up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Local {
    /// Millimetres right of the block's left edge.
    pub x: f64,
    /// Millimetres above the last baseline.
    pub y: f64,
}

/// One span as set: its width, and its stack's style if it is one.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanLayout {
    /// The span's width, mm.
    pub width: f64,
    /// The style a stack is set in, from the lettering table.
    pub stack: Option<StackStyle>,
}

/// One line as set.
#[derive(Clone, Debug, PartialEq)]
pub struct LineLayout {
    /// The line's left end on its baseline, local.
    pub origin: Local,
    /// Its width, mm.
    pub width: f64,
    /// Each span as set, in order.
    pub spans: Vec<SpanLayout>,
}

/// A text block as set, before placement.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Cap height, mm.
    pub cap: f64,
    /// Width factor applied to the font's advances.
    pub width_factor: f64,
    /// Pen width, mm.
    pub weight: f64,
    /// Baseline to baseline, mm.
    pub pitch: f64,
    /// The block's width, mm.
    pub width: f64,
    /// The box's top above the last baseline, mm: the first line's cap
    /// line, or higher if its stacks reach higher.
    pub height: f64,
    /// How far the box reaches below the last baseline, mm: zero unless
    /// the last line's stacks reach lower.
    pub descent: f64,
    /// The lines, first first.
    pub lines: Vec<LineLayout>,
    /// Whether the lines are underlined, and how far below the baseline.
    pub underline: Option<f64>,
}

/// A row's leading sign, if it has one, and the rest.
fn split_sign(s: &str) -> (Option<char>, &str) {
    match s.chars().next() {
        Some(c @ ('+' | '-' | '±')) => (Some(c), &s[c.len_utf8()..]),
        _ => (None, s),
    }
}

/// Where a stack's two rows start within it, and its width, for rows
/// whose glyph units are scaled by `kx`: centred on each other, or on
/// their decimal points with any leading signs set in a common field.
/// Each start is (the sign's offset, the body's offset), local mm.
fn stack_columns(
    font: &Font,
    rows: [&str; 2],
    kx: f64,
    align: StackAlign,
) -> ([(f64, f64); 2], f64) {
    let measure = |s: &str| font.width(s).expect("every character has a glyph") * kx;
    let widths = rows.map(measure);
    match align {
        StackAlign::Centre => {
            let w = widths[0].max(widths[1]);
            (
                [(0.0, (w - widths[0]) / 2.0), (0.0, (w - widths[1]) / 2.0)],
                w,
            )
        }
        StackAlign::Point => {
            let parts = rows.map(split_sign);
            let signs = parts.map(|(c, _)| c.map_or(0.0, |c| measure(&c.to_string())));
            let field = signs[0].max(signs[1]);
            let points = parts.map(|(_, body)| body.find('.').map(|i| measure(&body[..i])));
            let point = points[0].unwrap_or(0.0).max(points[1].unwrap_or(0.0));
            let starts = [0, 1].map(|i| {
                let shift = points[i].map_or(0.0, |p| point - p);
                ((field - signs[i]) / 2.0, field + shift)
            });
            let w = [0, 1]
                .map(|i| starts[i].1 + measure(parts[i].1))
                .into_iter()
                .fold(0.0, f64::max);
            (starts, w)
        }
    }
}

/// A stack's rows: the upper's baseline, the lower's baseline, the top
/// of the upper and the bottom of the lower, above the line's baseline.
fn stack_rows(cap: f64, scale: f64, gap: f64) -> (f64, f64, f64, f64) {
    let mid = cap / 2.0;
    let upper = mid + gap * cap;
    let lower = mid - gap * cap - scale * cap;
    (upper, lower, upper + scale * cap, lower)
}

/// Lay a text out from the lettering table, its layer's pen and the
/// font. Every character is known to the font (the document's
/// invariant).
pub fn layout(text: &Text, lettering: &Lettering, pen: Pen, font: &Font) -> Layout {
    let (cap, underline) = match text.size() {
        Size::Role { role } => {
            let r = lettering
                .role(*role)
                .expect("the document keeps every text's role in its table");
            (r.cap, r.underline)
        }
        Size::Cap { mm } => (*mm, false),
    };
    let width_factor = text.width().unwrap_or(lettering.width_factor());
    let weight = text.weight_mm().unwrap_or(pen.width(cap));
    let pitch = lettering.pitch_of_cap() * cap;
    let k = cap / 1000.0 * width_factor;
    let measure = |s: &str| font.width(s).expect("every character has a glyph") * k;
    let set_span = |s: &Span| match s {
        Span::Run { text } => SpanLayout {
            width: measure(text),
            stack: None,
        },
        Span::Stack {
            upper,
            lower,
            stack,
        } => {
            let style = lettering.stack_at(stack, cap);
            SpanLayout {
                width: stack_columns(font, [upper, lower], k * style.scale, style.align).1,
                stack: Some(style),
            }
        }
    };
    let widths: Vec<Vec<SpanLayout>> = text
        .lines()
        .iter()
        .map(|l| l.spans.iter().map(set_span).collect())
        .collect();
    let line_widths: Vec<f64> = widths
        .iter()
        .map(|w| w.iter().map(|s| s.width).sum())
        .collect();
    // A numbered item's lines are indented, those after its first
    // hanging under its body; only where the lines share a left edge.
    let mut offsets = vec![0.0; line_widths.len()];
    if text.align() == Align::Left {
        for (k, it) in text.items().iter().enumerate() {
            let end = text.items().get(k + 1).map_or(offsets.len(), |n| n.first);
            let hang = measure(&format!("{} ", it.number));
            for (i, o) in offsets.iter_mut().enumerate().take(end).skip(it.first) {
                *o = it.indent * cap + if i > it.first { hang } else { 0.0 };
            }
        }
    }
    let width = line_widths
        .iter()
        .zip(&offsets)
        .map(|(w, o)| w + o)
        .fold(0.0, f64::max);
    let n = text.lines().len();
    let reach = |spans: &[SpanLayout]| {
        spans
            .iter()
            .fold((cap, 0.0f64), |(top, bottom), s| match s.stack {
                None => (top, bottom),
                Some(style) => {
                    let (_, _, t, b) = stack_rows(cap, style.scale, style.gap);
                    (top.max(t), bottom.max(-b))
                }
            })
    };
    let (first_top, _) = reach(&widths[0]);
    let (_, last_bottom) = reach(&widths[n - 1]);
    let lines = line_widths
        .iter()
        .zip(widths)
        .enumerate()
        .map(|(i, (w, spans))| {
            let x = match text.align() {
                Align::Left => offsets[i],
                Align::Centre => (width - w) / 2.0,
                Align::Right => width - w,
            };
            LineLayout {
                origin: Local {
                    x,
                    y: (n - 1 - i) as f64 * pitch,
                },
                width: *w,
                spans,
            }
        })
        .collect();
    Layout {
        cap,
        width_factor,
        weight,
        pitch,
        width,
        height: (n - 1) as f64 * pitch + first_top,
        descent: last_bottom,
        lines,
        underline: underline.then_some(lettering.underline_below_cap() * cap),
    }
}

impl Layout {
    /// Where an anchor is in the local frame.
    pub fn anchor(&self, a: Anchor) -> Local {
        match a {
            Anchor::Box { at } => {
                let (w, top, bottom) = (self.width, self.height, -self.descent);
                let mid = (top + bottom) / 2.0;
                let (x, y) = match at {
                    BoxPoint::TopLeft => (0.0, top),
                    BoxPoint::TopCentre => (w / 2.0, top),
                    BoxPoint::TopRight => (w, top),
                    BoxPoint::MidLeft => (0.0, mid),
                    BoxPoint::Centre => (w / 2.0, mid),
                    BoxPoint::MidRight => (w, mid),
                    BoxPoint::BottomLeft => (0.0, bottom),
                    BoxPoint::BottomCentre => (w / 2.0, bottom),
                    BoxPoint::BottomRight => (w, bottom),
                };
                Local { x, y }
            }
            Anchor::Baseline { side } => Local {
                x: if side == Side::Left { 0.0 } else { self.width },
                y: 0.0,
            },
            Anchor::BaselineCentre => Local {
                x: self.width / 2.0,
                y: 0.0,
            },
            Anchor::Line { index, side } => {
                let l = &self.lines[index];
                Local {
                    x: l.origin.x + if side == Side::Left { 0.0 } else { l.width },
                    y: l.origin.y + self.cap / 2.0,
                }
            }
        }
    }
}

/// The local frame placed on the paper: the anchor at `at`, turned by
/// `angle`, local y up becoming paper y up the page.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    at: P2<Paper>,
    anchor: Local,
    cos: f64,
    sin: f64,
}

impl Frame {
    /// A frame.
    pub fn new(at: P2<Paper>, anchor: Local, angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self {
            at,
            anchor,
            cos,
            sin,
        }
    }

    /// A local point on the paper.
    pub fn to_paper(&self, p: Local) -> P2<Paper> {
        let (dx, dy) = (p.x - self.anchor.x, -(p.y - self.anchor.y));
        P2::new(
            self.at.x + self.cos * dx - self.sin * dy,
            self.at.y + self.sin * dx + self.cos * dy,
        )
    }

    /// The paper angle of a local angle (local y up, paper y down).
    fn angle(&self, a: f64) -> f64 {
        self.sin.atan2(self.cos) - a
    }
}

/// The strokes of one row of characters starting at local (`x`, `y`),
/// glyph units scaled by `kx` across and `ky` up; its width in local mm.
#[allow(clippy::too_many_arguments)]
fn row(
    font: &Font,
    chars: &str,
    x: f64,
    y: f64,
    kx: f64,
    ky: f64,
    frame: &Frame,
    out: &mut Vec<Path>,
) -> f64 {
    let mut x = x;
    let start = x;
    for ch in chars.chars() {
        let g = font.glyph(ch).expect("every character has a glyph");
        let at = |p: lmsheets_font::P| {
            frame.to_paper(Local {
                x: x + p.x * kx,
                y: y + p.y * ky,
            })
        };
        for s in &g.strokes {
            match s {
                Stroke::Line(pts) => {
                    for w in pts.windows(2) {
                        out.push(Path::Line {
                            a: at(w[0]),
                            b: at(w[1]),
                        });
                    }
                }
                Stroke::Cubic([a, c1, c2, b]) => out.push(Path::Cubic {
                    a: at(*a),
                    c1: at(*c1),
                    c2: at(*c2),
                    b: at(*b),
                }),
                Stroke::Arc {
                    centre,
                    radius,
                    start,
                    sweep,
                } => out.push(ellipse(
                    frame,
                    at(*centre),
                    radius * kx,
                    radius * ky,
                    *start,
                    *sweep,
                )),
                Stroke::Circle { centre, radius } => out.push(ellipse(
                    frame,
                    at(*centre),
                    radius * kx,
                    radius * ky,
                    0.0,
                    TAU,
                )),
            }
        }
        x += g.advance * kx;
    }
    x - start
}

/// The strokes of a laid-out text on the paper: the glyphs of every
/// span of every line, the stacks' bars, and any underlines, as paths.
pub fn strokes(text: &Text, layout: &Layout, frame: &Frame, font: &Font) -> Vec<Path> {
    weighted_strokes(text, layout, frame, font)
        .into_iter()
        .map(|(path, _)| path)
        .collect()
}

/// A stack's bar is drawn at this share of the pen its digits take (a
/// share of the cap, so the stack's scale of the text's): on 17619 the
/// scan's bars measure 5 to 7 px where ours at the digits' pen measured
/// 7 to 9 (mean run widths, the same estimator on both; chunk 13).
const STACK_BAR_PEN: f64 = 0.8;

/// The same strokes, each with its share of the text's pen: one for a
/// run, a stack's scale for its rows and its bar (the pen is a share of
/// the cap, and a stack's digits are smaller caps).
pub fn weighted_strokes(
    text: &Text,
    layout: &Layout,
    frame: &Frame,
    font: &Font,
) -> Vec<(Path, f64)> {
    let k = layout.cap / 1000.0;
    let kx = k * layout.width_factor;
    let mut out: Vec<(Path, f64)> = Vec::new();
    for (line, l) in text.lines().iter().zip(&layout.lines) {
        let mut x = l.origin.x;
        for (span, sl) in line.spans.iter().zip(&l.spans) {
            let w = sl.width;
            match span {
                Span::Run { text } => {
                    let mut paths = Vec::new();
                    row(font, text, x, l.origin.y, kx, k, frame, &mut paths);
                    out.extend(paths.into_iter().map(|p| (p, 1.0)));
                }
                Span::Stack { upper, lower, .. } => {
                    let StackStyle {
                        bar,
                        scale,
                        gap,
                        align,
                    } = sl.stack.expect("a stack is set with a style");
                    let (up, low, _, _) = stack_rows(layout.cap, scale, gap);
                    let (starts, _) = stack_columns(font, [upper, lower], kx * scale, align);
                    let rows = [(upper, up), (lower, low)].into_iter().zip(starts);
                    for ((s, y), (sign_at, body_at)) in rows {
                        let (sign, body) = match align {
                            StackAlign::Point => split_sign(s),
                            StackAlign::Centre => (None, s.as_str()),
                        };
                        let (sx, sy) = (kx * scale, k * scale);
                        if let Some(c) = sign {
                            let c = c.to_string();
                            let mut paths = Vec::new();
                            row(
                                font,
                                &c,
                                x + sign_at,
                                l.origin.y + y,
                                sx,
                                sy,
                                frame,
                                &mut paths,
                            );
                            out.extend(paths.into_iter().map(|p| (p, scale)));
                        }
                        let mut paths = Vec::new();
                        row(
                            font,
                            body,
                            x + body_at,
                            l.origin.y + y,
                            sx,
                            sy,
                            frame,
                            &mut paths,
                        );
                        out.extend(paths.into_iter().map(|p| (p, scale)));
                    }
                    if bar {
                        let y = l.origin.y + layout.cap / 2.0;
                        out.push((
                            Path::Line {
                                a: frame.to_paper(Local { x, y }),
                                b: frame.to_paper(Local { x: x + w, y }),
                            },
                            scale * STACK_BAR_PEN,
                        ));
                    }
                }
            }
            x += w;
        }
        if let Some(below) = layout.underline {
            out.push((
                Path::Line {
                    a: frame.to_paper(Local {
                        x: l.origin.x,
                        y: l.origin.y - below,
                    }),
                    b: frame.to_paper(Local {
                        x: l.origin.x + l.width,
                        y: l.origin.y - below,
                    }),
                },
                1.0,
            ));
        }
    }
    out
}

/// A glyph arc on the paper: circular when the width factor is one,
/// else an ellipse with its x semi-axis along the text's baseline. The
/// local frame's y points up, the paper's down, so angles turn over.
fn ellipse(frame: &Frame, centre: P2<Paper>, rx: f64, ry: f64, start: f64, sweep: f64) -> Path {
    if (rx - ry).abs() < 1e-9 && sweep.abs() >= TAU {
        return Path::Circle { centre, radius: rx };
    }
    if (rx - ry).abs() < 1e-9 {
        return Path::Arc {
            centre,
            radius: rx,
            start: frame.angle(start),
            sweep: -sweep,
        };
    }
    Path::EllipticalArc {
        centre,
        rx,
        ry,
        rotation: frame.angle(0.0),
        start: -start,
        sweep: -sweep,
    }
}

/// One line of a laid-out text as set on the paper: the line itself
/// (its spans, so a stack stays a stack), its baseline's left end, the
/// cap height, its width and the anchor's fraction along it.
#[derive(Clone, Debug, PartialEq)]
pub struct SetLine {
    /// The line as written in the document.
    pub line: Line,
    /// The left end of its baseline on the paper, mm.
    pub origin: P2<Paper>,
    /// Cap height, mm.
    pub cap: f64,
    /// Its width as set, mm.
    pub width: f64,
    /// The anchor's fraction along it, 0 at its left end to 1 at its
    /// right.
    pub anchor: f64,
}

/// The lines of a laid-out text as runs on the paper.
pub fn runs(text: &Text, layout: &Layout, frame: &Frame) -> Vec<SetLine> {
    let anchor = layout.anchor(text.anchor());
    text.lines()
        .iter()
        .zip(&layout.lines)
        .map(|(line, set)| SetLine {
            line: line.clone(),
            origin: frame.to_paper(set.origin),
            cap: layout.cap,
            width: set.width,
            anchor: if set.width > 0.0 {
                ((anchor.x - set.origin.x) / set.width).clamp(0.0, 1.0)
            } else {
                0.0
            },
        })
        .collect()
}

/// The enclosure of a text on the paper: the centre of its box and
/// half its size (a ring's radius, a square's half side): the text's own
/// enclosure size or the drafting table's balloon diameter, halved, or
/// more when the box's corners (a square: its sides) would come within
/// the balloon clearance of it. `None` for a text without an enclosure.
pub fn enclosure(
    doc: &Document,
    text: &Text,
    layout: &Layout,
    frame: &Frame,
) -> Option<(P2<Paper>, f64)> {
    let kind = text.enclosure()?;
    let centre = frame.to_paper(layout.anchor(Anchor::Box {
        at: BoxPoint::Centre,
    }));
    let (w, h) = (layout.width, layout.height + layout.descent);
    let d = doc.drafting();
    let size = text.enclosure_mm().unwrap_or(d.balloon_diameter());
    let reach = match kind {
        Enclosure::Circle => (w * w + h * h).sqrt() / 2.0,
        Enclosure::Square | Enclosure::DashedSquare => w.max(h) / 2.0,
    };
    Some((centre, (size / 2.0).max(reach + d.balloon_clearance())))
}

/// The curves that draw an enclosure about `centre`, `half` its size
/// (as [`enclosure`] gives them), upright on the paper: a circle; a
/// square's four sides; or a dashed square's dashes, each corner one
/// dash turning round a rounded corner (a line, a quarter arc, a line,
/// tangent: one pen stroke) and one dash in the middle of each side,
/// in the proportions of LDW280-18065 sh3's flag box (corner radius and
/// 0.12 of the side, each corner leg 0.1, gaps 0.14).
pub fn enclosure_paths(kind: Enclosure, centre: P2<Paper>, half: f64) -> Vec<Path> {
    let p = |x: f64, y: f64| P2::new(centre.x + x, centre.y + y);
    match kind {
        Enclosure::Circle => vec![Path::Circle {
            centre,
            radius: half,
        }],
        Enclosure::Square => {
            let c = [(-half, -half), (half, -half), (half, half), (-half, half)];
            (0..4)
                .map(|k| {
                    let (a, b) = (c[k], c[(k + 1) % 4]);
                    Path::Line {
                        a: p(a.0, a.1),
                        b: p(b.0, b.1),
                    }
                })
                .collect()
        }
        Enclosure::DashedSquare => {
            let side = 2.0 * half;
            let (rc, leg, gap) = (0.12 * side, 0.1 * side, 0.14 * side);
            let mid = side - 2.0 * rc - 2.0 * (leg + gap);
            let mut out = Vec::new();
            // Each corner's dash: in along one side, round, out along
            // the next; corners in turn clockwise on the page (y down),
            // from the top left.
            for k in 0..4 {
                // The corner's arc centre and the two sides' directions
                // away from it.
                let (sx, sy) = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)][k];
                let c = p(sx * (half - rc), sy * (half - rc));
                // The side before the corner runs along x on corners 0
                // and 2, along y on 1 and 3 (clockwise).
                let along_x = k % 2 == 0;
                let tangent_a = if along_x {
                    P2::new(c.x, c.y + sy * rc)
                } else {
                    P2::new(c.x + sx * rc, c.y)
                };
                let tangent_b = if along_x {
                    P2::new(c.x + sx * rc, c.y)
                } else {
                    P2::new(c.x, c.y + sy * rc)
                };
                let back = |t: P2<Paper>| {
                    if along_x {
                        // Back along the side before: along x, away
                        // from the corner.
                        P2::new(t.x - sx * leg, t.y)
                    } else {
                        P2::new(t.x, t.y - sy * leg)
                    }
                };
                let on = |t: P2<Paper>| {
                    if along_x {
                        P2::new(t.x, t.y - sy * leg)
                    } else {
                        P2::new(t.x - sx * leg, t.y)
                    }
                };
                let start = (tangent_a.y - c.y).atan2(tangent_a.x - c.x);
                let end = (tangent_b.y - c.y).atan2(tangent_b.x - c.x);
                let mut sweep = end - start;
                while sweep > std::f64::consts::PI {
                    sweep -= std::f64::consts::TAU;
                }
                while sweep < -std::f64::consts::PI {
                    sweep += std::f64::consts::TAU;
                }
                out.push(Path::Line {
                    a: back(tangent_a),
                    b: tangent_a,
                });
                out.push(Path::Arc {
                    centre: c,
                    radius: rc,
                    start,
                    sweep,
                });
                out.push(Path::Line {
                    a: tangent_b,
                    b: on(tangent_b),
                });
            }
            // The middle dash of each side.
            for (ax, ay, bx, by) in [
                (-mid / 2.0, -half, mid / 2.0, -half),
                (half, -mid / 2.0, half, mid / 2.0),
                (mid / 2.0, half, -mid / 2.0, half),
                (-half, mid / 2.0, -half, -mid / 2.0),
            ] {
                out.push(Path::Line {
                    a: p(ax, ay),
                    b: p(bx, by),
                });
            }
            out
        }
    }
}

/// The box of a laid-out text on the paper as set: its block from the
/// lowest stack's bottom to the first cap line, the full width, turned
/// with the text. The ring of an enclosed text is outside it.
pub fn bounds(layout: &Layout, frame: &Frame) -> Option<Rect<Paper>> {
    let (w, top, bottom) = (layout.width, layout.height, -layout.descent);
    Rect::of_points(
        [(0.0, bottom), (w, bottom), (0.0, top), (w, top)]
            .into_iter()
            .map(|(x, y)| frame.to_paper(Local { x, y })),
    )
}

/// The box of a laid-out text's ink on the paper: every glyph stroke,
/// bar and underline grown by half the pen; `None` for a text with no
/// ink (only spaces).
pub fn ink_bounds(text: &Text, layout: &Layout, frame: &Frame, font: &Font) -> Option<Rect<Paper>> {
    Rect::union_all(
        strokes(text, layout, frame, font)
            .iter()
            .filter_map(Bounded::bounds),
    )
    .map(|r| r.grow(layout.weight / 2.0))
}

/// A text laid out with its layer's pen and framed on the paper, given
/// its placement resolved to `at`.
pub fn place(doc: &Document, text: &Text, at: P2<Paper>) -> (Layout, Frame) {
    let pen = doc
        .layer(text.layer())
        .expect("the document keeps every text's layer in its table")
        .style
        .pen;
    let layout = layout(text, doc.lettering(), pen, Document::font());
    let anchor = layout.anchor(text.anchor());
    (layout.clone(), Frame::new(at, anchor, text.angle()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmsheets_document::value::provenance::{Agent, Date, Method, Provenance};
    use lmsheets_document::value::text::Line;

    #[test]
    fn a_long_balloon_label_grows_its_ring() {
        use lmsheets_document::examples::block;
        use lmsheets_document::value::text::Enclosure;
        let ours = Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 17).unwrap(),
        );
        let doc = block();
        let d = doc.drafting();
        let at = P2::new(50.0, 50.0);
        let short = Text::balloon("-15", at, ours.clone()).unwrap();
        let (layout, frame) = place(&doc, &short, at);
        let (_, r) = enclosure(&doc, &short, &layout, &frame).unwrap();
        assert!((r - d.balloon_diameter() / 2.0).abs() < 1e-9);
        let long = Text::balloon("-157 (REF)", at, ours.clone()).unwrap();
        let (layout, frame) = place(&doc, &long, at);
        let (c, r) = enclosure(&doc, &long, &layout, &frame).unwrap();
        let half_diagonal =
            (layout.width.powi(2) + (layout.height + layout.descent).powi(2)).sqrt() / 2.0;
        assert!((r - half_diagonal - d.balloon_clearance()).abs() < 1e-9);
        assert!(r > d.balloon_diameter() / 2.0);
        assert!((c.x - at.x).abs() < 1e-9 && (c.y - at.y).abs() < 1e-9);
        assert_eq!(long.enclosure(), Some(Enclosure::Circle));
        assert_eq!(enclosure(&doc, &long.unenclosed(), &layout, &frame), None);
    }

    #[test]
    fn a_texts_ink_lies_in_its_set_box_and_turns_with_it() {
        use lmsheets_document::examples::block;
        let ours = Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 17).unwrap(),
        );
        let doc = block();
        let font = Document::font();
        let at = P2::new(100.0, 50.0);
        for angle in [0.0, 0.5, std::f64::consts::FRAC_PI_2, 2.0] {
            let t = Text::new(&["PLATE 1/4", "NOTES"], "text", at, ours.clone())
                .unwrap()
                .turned(angle)
                .unwrap();
            let (layout, frame) = place(&doc, &t, at);
            let set = bounds(&layout, &frame).unwrap();
            let ink = ink_bounds(&t, &layout, &frame, font).unwrap();
            // The ink is inside the set box but for half a pen and the
            // round letters' overshoot.
            let slack = layout.weight / 2.0 + layout.cap * 0.05;
            assert!(
                set.grow(slack).contains_rect(&ink),
                "{angle}: {ink:?} not in {set:?}"
            );
            // Upright, the ink fills most of the box (turned, a short
            // second line leaves a corner of the turned box empty), and
            // the box is the block's width by its height.
            if angle == 0.0 {
                assert!(ink.width() > set.width() * 0.8 && ink.height() > set.height() * 0.8);
                assert!((set.width() - layout.width).abs() < 1e-9);
                assert!((set.height() - (layout.height + layout.descent)).abs() < 1e-9);
            }
            // A quarter turn swaps them.
            if angle == std::f64::consts::FRAC_PI_2 {
                assert!((set.height() - layout.width).abs() < 1e-9);
            }
        }
        // Spaces lay no ink.
        let blank = Text::new(&["   "], "text", at, ours).unwrap();
        let (layout, frame) = place(&doc, &blank, at);
        assert_eq!(ink_bounds(&blank, &layout, &frame, font), None);
        assert!(bounds(&layout, &frame).is_some());
    }

    #[test]
    fn a_numbered_item_hangs_under_its_body() {
        use lmsheets_document::value::text::Item;
        let ours = Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 28).unwrap(),
        );
        let font = Document::font();
        let lettering = Lettering::standard();
        let at = P2::new(0.0, 0.0);
        let lines = ["NOTES", "1. NO TOOLING HOLES", "IN PARTS."];
        let plain = Text::new(&lines, "text", at, ours.clone()).unwrap();
        let item = |indent| Item {
            first: 1,
            number: "1.".into(),
            indent,
        };
        let listed = plain.clone().itemised(vec![item(1.0)]).unwrap();
        let (p, l) = (
            layout(&plain, &lettering, Pen::STANDARD, font),
            layout(&listed, &lettering, Pen::STANDARD, font),
        );
        let (cap, hang) = (
            l.cap,
            font.width("1. ").unwrap() * l.cap / 1000.0 * l.width_factor,
        );
        assert_eq!(l.lines[0].origin.x, 0.0);
        assert!((l.lines[1].origin.x - cap).abs() < 1e-9);
        assert!((l.lines[2].origin.x - (cap + hang)).abs() < 1e-9);
        assert!(l.width > p.width);
        // An item must begin where its number says and not run backwards.
        assert!(plain
            .clone()
            .itemised(vec![Item {
                first: 0,
                ..item(0.0)
            }])
            .is_err());
        assert!(plain.itemised(vec![item(-1.0)]).is_err());
    }

    #[test]
    fn stacks_widen_the_box_and_reach_past_the_cap() {
        let ours = Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 15).unwrap(),
        );
        let font = Document::font();
        let lettering = Lettering::standard();
        let plain = Text::new(&["1.250"], "dimension", P2::new(0.0, 0.0), ours.clone()).unwrap();
        let limits = Text::with_lines(
            vec![Line::of(vec![Span::limits("1.266", "1.234")])],
            "dimension",
            P2::new(0.0, 0.0),
            ours.clone(),
        )
        .unwrap();
        let p = layout(&plain, &lettering, Pen::STANDARD, font);
        let l = layout(&limits, &lettering, Pen::STANDARD, font);
        let cap = p.cap;
        assert!(p.descent == 0.0 && (p.height - cap).abs() < 1e-9);
        // Full-size limits reach 0.7 cap below the baseline and 1.7 above.
        assert!((l.descent - 0.7 * cap).abs() < 1e-9, "{}", l.descent);
        assert!((l.height - 1.7 * cap).abs() < 1e-9, "{}", l.height);
        assert!((l.width - p.width).abs() < 0.5);
        // The box's centre is the bar: the line's middle.
        let c = l.anchor(Anchor::Box {
            at: BoxPoint::Centre,
        });
        assert!((c.y - cap / 2.0).abs() < 1e-9);
        // A mixed fraction sets at 0.7, with a bar.
        let frac = Text::with_lines(
            vec![Line::of(vec![Span::run("2 "), Span::fraction("5", "16")])],
            "dimension",
            P2::new(0.0, 0.0),
            ours,
        )
        .unwrap();
        let f = layout(&frac, &lettering, Pen::STANDARD, font);
        let frame = Frame::new(P2::new(0.0, 0.0), Local { x: 0.0, y: 0.0 }, 0.0);
        let paths = strokes(&frac, &f, &frame, font);
        // The bar is the one horizontal line at mid cap height.
        let bars: Vec<&Path> = paths
            .iter()
            .filter(|p| matches!(p, Path::Line { a, b } if (a.y - b.y).abs() < 1e-9 && (a.y + cap / 2.0).abs() < 1e-9))
            .collect();
        assert_eq!(bars.len(), 1);
    }

    #[test]
    fn point_aligned_stacks_line_up_signs_and_points() {
        let font = Document::font();
        // "+0.1" over "-0.0": the signs share a field, the bodies start
        // together, so the points align; the narrower sign is centred.
        let (starts, w) = stack_columns(font, ["+0.1", "-0.0"], 1.0, StackAlign::Point);
        assert!((starts[0].1 - starts[1].1).abs() < 1e-9);
        let field = font.width("+").unwrap().max(font.width("-").unwrap());
        assert!((starts[0].1 - field).abs() < 1e-9);
        assert!(starts[1].0 > starts[0].0);
        assert!(w > starts[0].1);
        // "20.05" over "9.95": the points align, the shorter row shifts.
        let (starts, _) = stack_columns(font, ["20.05", "9.95"], 1.0, StackAlign::Point);
        let before = |s: &str| font.width(&s[..s.find('.').unwrap()]).unwrap();
        let (u, l) = (starts[0].1 + before("20.05"), starts[1].1 + before("9.95"));
        assert!((u - l).abs() < 1e-9);
        // Centred rows are centred.
        let (starts, w) = stack_columns(font, ["5", "16"], 1.0, StackAlign::Centre);
        assert!((starts[0].1 + font.width("5").unwrap() / 2.0 - w / 2.0).abs() < 1e-9);
        assert!(starts[1].1.abs() < 1e-9);
    }

    #[test]
    fn squares_enclose_a_flag_note() {
        use lmsheets_document::examples::block;
        let ours = Provenance::new(
            Method::Authored,
            Agent::Claude,
            Date::new(2026, 10, 23).unwrap(),
        );
        let doc = block();
        let at = P2::new(50.0, 50.0);
        let flag = Text::new(&["1"], "text", at, ours)
            .unwrap()
            .enclosed(Enclosure::DashedSquare)
            .unwrap()
            .enclosure_sized(12.0)
            .unwrap();
        let (layout, frame) = place(&doc, &flag, at);
        let (c, half) = enclosure(&doc, &flag, &layout, &frame).unwrap();
        assert!((half - 6.0).abs() < 1e-9, "its own size: {half}");
        // A square: four sides; a dashed one: four corner dashes (a
        // line, a quarter arc, a line) and four middle dashes, all on
        // the square's outline.
        assert_eq!(enclosure_paths(Enclosure::Square, c, half).len(), 4);
        let dashed = enclosure_paths(Enclosure::DashedSquare, c, half);
        assert_eq!(dashed.len(), 16);
        let mut ink = 0.0;
        for path in &dashed {
            ink += path.length();
            for t in [0.0, 0.5, 1.0] {
                let p = path.at(t);
                let (dx, dy) = ((p.x - c.x).abs(), (p.y - c.y).abs());
                assert!(dx <= half + 1e-9 && dy <= half + 1e-9, "{p:?}");
                assert!(dx.max(dy) > half * 0.7, "near the outline: {p:?}");
            }
            if let Path::Arc { sweep, .. } = path {
                assert!((sweep.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
            }
        }
        // Dashes with gaps: less ink than the whole outline.
        assert!(ink < 8.0 * half && ink > 5.0 * half, "{ink}");
    }
}
