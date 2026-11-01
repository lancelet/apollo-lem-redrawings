//! lmsheets-font: the single-stroke lettering of the drawings, read from
//! the LMF1 byte format that `scripts/font/pack_font.py` writes from the
//! font's JSON source (`fonts/lm-lettering.json`, see `fonts/README.md`).
//!
//! A [`Font`] is a set of [`Glyph`]s, each an advance and a list of
//! [`Stroke`]s: pen centre lines in font units, where the ink cap height
//! is 1000, y is up and the origin is the glyph origin with its side
//! bearings. A stroke is a polyline, a cubic Bézier, an arc or a full
//! circle; the renderer draws each with a round pen of [`Font::pen`]
//! units (or the lettering standard's weight) and nothing is flattened.
//!
//! The parse is strict: every byte of the file is accounted for, every
//! tag known, every count non-trivial, every code point once; a file that
//! fails any of this is refused as a whole. [`Font::lm_lettering`] is the
//! font built into the crate.
//!
//! # The LMF1 format
//!
//! One global 256-grid, little endian: coordinates are bytes, `units =
//! origin + byte × step`; advances, radii and the pen are `byte × step`.
//!
//! ```text
//! header  "LMF1", step f32, origin x i16, origin y i16, pen u8,
//!         glyph count u16
//! glyph   code point u16, advance u8, stroke count u8
//! line    tag 1, n u8 (n ≥ 2), n × (x u8, y u8)
//! cubic   tag 2, 4 × (x u8, y u8)
//! arc     tag 3, start (x, y), end (x, y), radius u8, flags u8
//!         flags: bit 0 large (over 180°), bit 1 counter-clockwise,
//!         bit 2 full circle (start and end diametrically opposite)
//! ```
//!
//! Arcs are stored by their endpoints, so a large-radius arc's distant
//! centre never is; the reader recovers it.

#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// A point in font units (cap height 1000, y up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct P {
    /// Units.
    pub x: f64,
    /// Units.
    pub y: f64,
}

/// One pen centre line of a glyph, font units.
#[derive(Clone, Debug, PartialEq)]
pub enum Stroke {
    /// A polyline of at least two points.
    Line(Vec<P>),
    /// A cubic Bézier: start, two controls, end.
    Cubic([P; 4]),
    /// An arc from `start` angle through `sweep` radians (positive
    /// counter-clockwise, y up), `0 < |sweep| < 2π`.
    Arc {
        /// The centre.
        centre: P,
        /// The radius, units.
        radius: f64,
        /// The start angle, radians from +x.
        start: f64,
        /// The signed sweep, radians.
        sweep: f64,
    },
    /// A full circle.
    Circle {
        /// The centre.
        centre: P,
        /// The radius, units.
        radius: f64,
    },
}

/// An axis-aligned box in font units: `min` at or below `max`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    /// The lower-left corner (y up).
    pub min: P,
    /// The upper-right corner.
    pub max: P,
}

impl Extent {
    fn of(p: P) -> Self {
        Self { min: p, max: p }
    }

    fn take(self, p: P) -> Self {
        Self {
            min: P {
                x: self.min.x.min(p.x),
                y: self.min.y.min(p.y),
            },
            max: P {
                x: self.max.x.max(p.x),
                y: self.max.y.max(p.y),
            },
        }
    }

    /// The box around both.
    pub fn union(self, o: Self) -> Self {
        self.take(o.min).take(o.max)
    }

    /// Grown by `m` units on every side (a pen's half width).
    pub fn grow(self, m: f64) -> Self {
        Self {
            min: P {
                x: self.min.x - m,
                y: self.min.y - m,
            },
            max: P {
                x: self.max.x + m,
                y: self.max.y + m,
            },
        }
    }

    /// Moved along x by `dx` units.
    pub fn shifted(self, dx: f64) -> Self {
        Self {
            min: P {
                x: self.min.x + dx,
                y: self.min.y,
            },
            max: P {
                x: self.max.x + dx,
                y: self.max.y,
            },
        }
    }
}

/// The parameters in (0, 1) where one coordinate of a cubic turns.
fn cubic_turns(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let scale = a.abs().max(b.abs()).max(c.abs());
    let mut roots = Vec::new();
    if scale == 0.0 {
        return roots;
    }
    if a.abs() <= 1e-12 * scale {
        if b.abs() > 1e-12 * scale {
            roots.push(-c / b);
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let q = -0.5 * (b + b.signum() * disc.sqrt());
            roots.push(q / a);
            if q != 0.0 {
                roots.push(c / q);
            }
        }
    }
    roots.retain(|t| *t > 0.0 && *t < 1.0);
    roots
}

impl Stroke {
    /// The exact box of the pen's centre line: a polyline's points, a
    /// cubic's ends and turning points, an arc's ends and the axis
    /// points it passes, a circle's square.
    pub fn bounds(&self) -> Extent {
        match self {
            Stroke::Line(points) => points[1..]
                .iter()
                .fold(Extent::of(points[0]), |e, p| e.take(*p)),
            Stroke::Cubic(c) => {
                let at = |t: f64| {
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    P {
                        x: (0..4).map(|i| w[i] * c[i].x).sum(),
                        y: (0..4).map(|i| w[i] * c[i].y).sum(),
                    }
                };
                cubic_turns(c[0].x, c[1].x, c[2].x, c[3].x)
                    .into_iter()
                    .chain(cubic_turns(c[0].y, c[1].y, c[2].y, c[3].y))
                    .fold(Extent::of(c[0]).take(c[3]), |e, t| e.take(at(t)))
            }
            Stroke::Arc {
                centre,
                radius,
                start,
                sweep,
            } => {
                let at = |a: f64| P {
                    x: centre.x + radius * a.cos(),
                    y: centre.y + radius * a.sin(),
                };
                let mut e = Extent::of(at(*start)).take(at(start + sweep));
                let (lo, hi) = if *sweep >= 0.0 {
                    (*start, start + sweep)
                } else {
                    (start + sweep, *start)
                };
                let quarter = std::f64::consts::FRAC_PI_2;
                let mut k = (lo / quarter).ceil();
                while k * quarter <= hi {
                    e = e.take(at(k * quarter));
                    k += 1.0;
                }
                e
            }
            Stroke::Circle { centre, radius } => Extent::of(*centre).grow(*radius),
        }
    }
}

/// One glyph.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    /// The advance to the next glyph's origin, units.
    pub advance: f64,
    /// The pen centre lines; empty only for the space.
    pub strokes: Vec<Stroke>,
}

impl Glyph {
    /// The box of the ink a pen `pen` units wide lays along the glyph's
    /// strokes, about its own origin; `None` for a glyph with no
    /// strokes (the space).
    pub fn ink_bounds(&self, pen: f64) -> Option<Extent> {
        self.strokes
            .iter()
            .map(Stroke::bounds)
            .reduce(Extent::union)
            .map(|e| e.grow(pen / 2.0))
    }
}

/// A font.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    /// The grid step the file was stored at, units: its precision.
    pub step: f64,
    /// The pen's width, units: the stroke width the glyphs were drawn
    /// for; a lettering standard may draw them with another.
    pub pen: f64,
    glyphs: BTreeMap<char, Glyph>,
}

/// What can be wrong with a font file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not an LMF1 file.
    BadMagic,
    /// The file ends inside a record.
    Truncated {
        /// Where.
        at: usize,
    },
    /// Bytes after the last glyph.
    TrailingBytes {
        /// How many.
        count: usize,
    },
    /// A step or pen that is not finite and positive.
    BadScale,
    /// A code point that is not a character.
    BadCodePoint(u16),
    /// A character twice.
    Duplicate(char),
    /// An unknown stroke tag.
    BadTag(u8),
    /// A polyline of fewer than two points.
    ShortLine,
    /// An arc with no length or no radius.
    DegenerateArc,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// A cursor over the file.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.at + n;
        if end > self.bytes.len() {
            return Err(Error::Truncated { at: self.at });
        }
        let s = &self.bytes[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn i16(&mut self) -> Result<i16, Error> {
        let b = self.take(2)?;
        Ok(i16::from_le_bytes([b[0], b[1]]))
    }

    fn f32(&mut self) -> Result<f32, Error> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// The centre, start angle and sweep of an arc given by its endpoints,
/// radius and flags. A radius shorter than half the chord (quantisation)
/// is raised to it, as SVG does.
fn arc_from_ends(a: P, b: P, radius: f64, large: bool, ccw: bool) -> Result<Stroke, Error> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let d = (dx * dx + dy * dy).sqrt();
    if d == 0.0 || radius <= 0.0 {
        return Err(Error::DegenerateArc);
    }
    let r = radius.max(d / 2.0);
    let h = (r * r - d * d / 4.0).max(0.0).sqrt();
    let (mx, my) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    // The left normal of the chord; a short counter-clockwise arc has its
    // centre on the left, a long one on the right, and clockwise swaps.
    let (nx, ny) = (-dy / d, dx / d);
    let side = if ccw != large { 1.0 } else { -1.0 };
    let centre = P {
        x: mx + side * h * nx,
        y: my + side * h * ny,
    };
    let start = (a.y - centre.y).atan2(a.x - centre.x);
    let half = (d / (2.0 * r)).clamp(-1.0, 1.0).asin();
    let magnitude = if large { TAU - 2.0 * half } else { 2.0 * half };
    let sweep = if ccw { magnitude } else { -magnitude };
    Ok(Stroke::Arc {
        centre,
        radius: r,
        start,
        sweep,
    })
}

impl Font {
    /// The font built into the crate: LM Lettering.
    pub fn lm_lettering() -> Font {
        Font::parse(include_bytes!("../../../fonts/lm-lettering.lmf8"))
            .expect("the built-in font is valid")
    }

    /// A font from an LMF1 file.
    pub fn parse(bytes: &[u8]) -> Result<Font, Error> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != b"LMF1" {
            return Err(Error::BadMagic);
        }
        let step = r.f32()? as f64;
        let ox = r.i16()? as f64;
        let oy = r.i16()? as f64;
        let pen = r.u8()? as f64 * step;
        if !(step.is_finite() && step > 0.0 && pen > 0.0) {
            return Err(Error::BadScale);
        }
        let count = r.u16()? as usize;
        let mut glyphs = BTreeMap::new();
        let point = |r: &mut Reader| -> Result<P, Error> {
            let x = r.u8()? as f64;
            let y = r.u8()? as f64;
            Ok(P {
                x: ox + x * step,
                y: oy + y * step,
            })
        };
        for _ in 0..count {
            let cp = r.u16()?;
            let ch = char::from_u32(cp as u32).ok_or(Error::BadCodePoint(cp))?;
            let advance = r.u8()? as f64 * step;
            let n = r.u8()? as usize;
            let mut strokes = Vec::with_capacity(n);
            for _ in 0..n {
                let stroke = match r.u8()? {
                    1 => {
                        let n = r.u8()? as usize;
                        if n < 2 {
                            return Err(Error::ShortLine);
                        }
                        let mut pts = Vec::with_capacity(n);
                        for _ in 0..n {
                            pts.push(point(&mut r)?);
                        }
                        Stroke::Line(pts)
                    }
                    2 => Stroke::Cubic([
                        point(&mut r)?,
                        point(&mut r)?,
                        point(&mut r)?,
                        point(&mut r)?,
                    ]),
                    3 => {
                        let a = point(&mut r)?;
                        let b = point(&mut r)?;
                        let radius = r.u8()? as f64 * step;
                        let flags = r.u8()?;
                        let (large, ccw, full) = (flags & 1 != 0, flags & 2 != 0, flags & 4 != 0);
                        if full {
                            let centre = P {
                                x: (a.x + b.x) / 2.0,
                                y: (a.y + b.y) / 2.0,
                            };
                            let radius = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt() / 2.0;
                            if radius <= 0.0 {
                                return Err(Error::DegenerateArc);
                            }
                            Stroke::Circle { centre, radius }
                        } else {
                            arc_from_ends(a, b, radius, large, ccw)?
                        }
                    }
                    t => return Err(Error::BadTag(t)),
                };
                strokes.push(stroke);
            }
            if glyphs.insert(ch, Glyph { advance, strokes }).is_some() {
                return Err(Error::Duplicate(ch));
            }
        }
        if r.at != bytes.len() {
            return Err(Error::TrailingBytes {
                count: bytes.len() - r.at,
            });
        }
        Ok(Font { step, pen, glyphs })
    }

    /// A glyph.
    pub fn glyph(&self, ch: char) -> Option<&Glyph> {
        self.glyphs.get(&ch)
    }

    /// Every glyph, by character.
    pub fn glyphs(&self) -> impl Iterator<Item = (char, &Glyph)> {
        self.glyphs.iter().map(|(c, g)| (*c, g))
    }

    /// Whether every character of `text` has a glyph; the first that
    /// does not, otherwise.
    pub fn missing(&self, text: &str) -> Option<char> {
        text.chars().find(|c| !self.glyphs.contains_key(c))
    }

    /// The box of the ink `text` lays down with a pen `pen` units wide,
    /// set from origin 0 with each glyph at the sum of the advances
    /// before it; `None` if it lays no ink (empty, or only spaces), and
    /// the first character without a glyph as the error.
    pub fn ink_bounds(&self, text: &str, pen: f64) -> Result<Option<Extent>, char> {
        let mut x = 0.0;
        let mut out: Option<Extent> = None;
        for c in text.chars() {
            let g = self.glyphs.get(&c).ok_or(c)?;
            if let Some(e) = g.ink_bounds(pen) {
                let e = e.shifted(x);
                out = Some(match out {
                    Some(o) => o.union(e),
                    None => e,
                });
            }
            x += g.advance;
        }
        Ok(out)
    }

    /// The width of `text` in units: the sum of its advances, which carry
    /// the side bearings.
    pub fn width(&self, text: &str) -> Result<f64, char> {
        text.chars()
            .map(|c| self.glyphs.get(&c).map(|g| g.advance).ok_or(c))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_font_parses_and_is_what_the_source_says() {
        let f = Font::lm_lettering();
        assert_eq!(f.glyphs().count(), 116);
        // The centre-line symbol, the flag triangle, the check mark and
        // the marking-region symbol the drawings use.
        assert!(f.glyph('℄').is_some() && f.glyph('▲').is_some() && f.glyph('✓').is_some());
        assert!(f.glyph('⊠').is_some());
        assert!((f.step - 5.5).abs() < 1e-6);
        assert!((f.pen - 24.0 * 5.5).abs() < 1e-9);
        // The I is one stroke, a vertical line the cap height long.
        let i = f.glyph('I').unwrap();
        assert_eq!(i.strokes.len(), 1);
        let Stroke::Line(p) = &i.strokes[0] else {
            panic!()
        };
        assert_eq!(p.len(), 2);
        assert!((p[0].x - p[1].x).abs() < 1e-9);
        assert!(((p[0].y - p[1].y).abs() - 870.0).abs() < 6.0);
        // The degree sign is a circle; the 7 is there; space has no ink.
        assert!(matches!(
            f.glyph('°').unwrap().strokes[0],
            Stroke::Circle { .. }
        ));
        assert!(f.glyph('7').is_some());
        assert!(f.glyph(' ').unwrap().strokes.is_empty());
        assert_eq!(f.missing("LDW280-23631"), None);
        assert_eq!(f.missing("½"), Some('½'));
        assert!(f.width("X131.140").unwrap() > 3000.0);
    }

    #[test]
    fn every_point_is_within_the_grid_step_of_the_json_source() {
        let f = Font::lm_lettering();
        let json: serde_json::Value =
            serde_json::from_str(include_str!("../../../fonts/lm-lettering.json")).unwrap();
        let tol = f.step / 2.0 + 1e-6;
        let mut points = 0;
        for (ch, g) in json["glyphs"].as_object().unwrap() {
            let ch = ch.chars().next().unwrap();
            let ours = f.glyph(ch).unwrap();
            let src = g["strokes"].as_array().unwrap();
            assert_eq!(ours.strokes.len(), src.len(), "{ch}");
            for (a, b) in ours.strokes.iter().zip(src) {
                let pts = |v: &serde_json::Value| -> Vec<P> {
                    v.as_array()
                        .unwrap()
                        .iter()
                        .map(|p| P {
                            x: p[0].as_f64().unwrap(),
                            y: p[1].as_f64().unwrap(),
                        })
                        .collect()
                };
                match (a, b["type"].as_str().unwrap()) {
                    (Stroke::Line(_), "line") | (Stroke::Cubic(_), "bezier") => {
                        let q = pts(&b["p"]);
                        let p: Vec<P> = match a {
                            Stroke::Line(p) => p.clone(),
                            Stroke::Cubic(c) => c.to_vec(),
                            _ => unreachable!(),
                        };
                        assert_eq!(p.len(), q.len(), "{ch}");
                        for (x, y) in p.iter().zip(&q) {
                            assert!((x.x - y.x).abs() <= tol && (x.y - y.y).abs() <= tol, "{ch}");
                            points += 1;
                        }
                    }
                    (
                        Stroke::Arc { centre, radius, .. } | Stroke::Circle { centre, radius },
                        "arc",
                    ) => {
                        // The centre is recovered from quantised ends and
                        // radius: allow a few steps, more for a large radius.
                        let cx = b["c"][0].as_f64().unwrap();
                        let cy = b["c"][1].as_f64().unwrap();
                        let r = b["r"].as_f64().unwrap();
                        let slack = 3.0 * f.step + r * 0.02;
                        assert!(
                            (centre.x - cx).abs() <= slack,
                            "{ch} cx {} vs {cx}",
                            centre.x
                        );
                        assert!(
                            (centre.y - cy).abs() <= slack,
                            "{ch} cy {} vs {cy}",
                            centre.y
                        );
                        assert!((radius - r).abs() <= slack, "{ch} r {radius} vs {r}");
                        points += 1;
                    }
                    (a, t) => panic!("{ch}: {a:?} vs {t}"),
                }
            }
        }
        assert!(points > 1000);
    }

    /// Points along a stroke, densely.
    fn samples(s: &Stroke) -> Vec<P> {
        let n = 4000;
        match s {
            Stroke::Line(points) => points.clone(),
            Stroke::Cubic(c) => (0..=n)
                .map(|i| {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    P {
                        x: (0..4).map(|k| w[k] * c[k].x).sum(),
                        y: (0..4).map(|k| w[k] * c[k].y).sum(),
                    }
                })
                .collect(),
            Stroke::Arc {
                centre,
                radius,
                start,
                sweep,
            } => (0..=n)
                .map(|i| {
                    let a = start + sweep * i as f64 / n as f64;
                    P {
                        x: centre.x + radius * a.cos(),
                        y: centre.y + radius * a.sin(),
                    }
                })
                .collect(),
            Stroke::Circle { centre, radius } => (0..=n)
                .map(|i| {
                    let a = TAU * i as f64 / n as f64;
                    P {
                        x: centre.x + radius * a.cos(),
                        y: centre.y + radius * a.sin(),
                    }
                })
                .collect(),
        }
    }

    #[test]
    fn every_glyphs_stroke_bounds_agree_with_dense_sampling() {
        // Exact boxes contain every sample and are no looser than the
        // samples by more than a hair, for every stroke of the font.
        let f = Font::lm_lettering();
        let mut kinds = [0usize; 4];
        for (ch, g) in f.glyphs() {
            for s in &g.strokes {
                kinds[match s {
                    Stroke::Line(_) => 0,
                    Stroke::Cubic(_) => 1,
                    Stroke::Arc { .. } => 2,
                    Stroke::Circle { .. } => 3,
                }] += 1;
                let e = s.bounds();
                let pts = samples(s);
                let tight = pts[1..].iter().fold(Extent::of(pts[0]), |e, p| e.take(*p));
                let eps = 1e-6;
                assert!(
                    tight.min.x >= e.min.x - eps
                        && tight.min.y >= e.min.y - eps
                        && tight.max.x <= e.max.x + eps
                        && tight.max.y <= e.max.y + eps,
                    "{ch:?}: {s:?} samples {tight:?} outside {e:?}"
                );
                let slack = 0.05;
                assert!(
                    e.min.x >= tight.min.x - slack
                        && e.min.y >= tight.min.y - slack
                        && e.max.x <= tight.max.x + slack
                        && e.max.y <= tight.max.y + slack,
                    "{ch:?}: {s:?} box {e:?} looser than {tight:?}"
                );
            }
        }
        // The font exercises every stroke kind but perhaps circles.
        assert!(kinds[0] > 0 && kinds[1] + kinds[2] > 0, "{kinds:?}");
    }

    #[test]
    fn glyph_and_text_ink_bounds() {
        let f = Font::lm_lettering();
        // The I is one vertical stroke whose ink is the cap height (its
        // centre line stops half a pen short at each end): the line
        // grown by half the pen all round, 1000 units tall from 0.
        let i = f.glyph('I').unwrap();
        let e = i.ink_bounds(f.pen).unwrap();
        let line = i.strokes[0].bounds();
        assert!((e.max.y - e.min.y - 1000.0).abs() < f.step, "{e:?}");
        assert!(e.min.y.abs() < f.step, "{e:?}");
        assert!((e.max.x - e.min.x - f.pen).abs() < 1e-9);
        assert_eq!(e, line.grow(f.pen / 2.0));
        // A space lays no ink; nor does a string of spaces or nothing.
        assert_eq!(f.glyph(' ').unwrap().ink_bounds(f.pen), None);
        assert_eq!(f.ink_bounds("   ", f.pen), Ok(None));
        assert_eq!(f.ink_bounds("", f.pen), Ok(None));
        // "II" is the first I and the second shifted by an advance.
        let ii = f.ink_bounds("II", f.pen).unwrap().unwrap();
        assert!((ii.min.x - e.min.x).abs() < 1e-9);
        assert!((ii.max.x - (e.max.x + i.advance)).abs() < 1e-9);
        // A space between moves the second by both advances.
        let i_i = f.ink_bounds("I I", f.pen).unwrap().unwrap();
        let space = f.glyph(' ').unwrap().advance;
        assert!((i_i.max.x - (e.max.x + i.advance + space)).abs() < 1e-9);
        // Digits and capitals stand on the baseline and reach the cap
        // line: their ink spans 0..1000, round ones overshooting a
        // little, as lettering does. (The Q's tail descends.)
        for ch in "0123456789ABCDEFGHIJKLMNOPRSTUVWXYZ".chars() {
            let e = f.glyph(ch).unwrap().ink_bounds(f.pen).unwrap();
            assert!(e.min.y.abs() < 40.0, "{ch}: {e:?}");
            assert!((e.max.y - 1000.0).abs() < 40.0, "{ch}: {e:?}");
        }
        let q = f.glyph('Q').unwrap().ink_bounds(f.pen).unwrap();
        assert!(q.min.y < -40.0 && (q.max.y - 1000.0).abs() < 40.0, "{q:?}");
        // A string's ink is within its advance width, side bearings
        // included, give or take the pen.
        let w = f.width("PLATE").unwrap();
        let ink = f.ink_bounds("PLATE", f.pen).unwrap().unwrap();
        assert!(ink.min.x > -f.pen && ink.max.x < w + f.pen);
        // A missing glyph is named.
        assert_eq!(f.ink_bounds("A\u{2603}", f.pen), Err('\u{2603}'));
    }

    #[test]
    fn bad_files_are_refused_as_a_whole() {
        let good = include_bytes!("../../../fonts/lm-lettering.lmf8");
        assert_eq!(Font::parse(b"LMF9").err(), Some(Error::BadMagic));
        assert!(matches!(
            Font::parse(&good[..good.len() - 1]).err(),
            Some(Error::Truncated { .. })
        ));
        let mut extra = good.to_vec();
        extra.push(0);
        assert_eq!(
            Font::parse(&extra).err(),
            Some(Error::TrailingBytes { count: 1 })
        );
    }
}
