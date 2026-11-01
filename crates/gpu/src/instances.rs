//! A styled scene as GPU instances: every stroke a segment or a true
//! circular arc the shader measures distance to, every fill a triangle
//! or a disc. Dashed strokes are cut into their dashes here, so the
//! shader knows nothing of dashes; cubics and elliptical arcs (the
//! lettering) are flattened to short segments at a stated tolerance.

use lmsheets_document::value::geometry::{Paper, P2};
use lmsheets_document::value::style::{MarkerKind, Rgba};
use lmsheets_render::resolve::{Styled, StyledItem};
use lmsheets_render::scene::{Fill, Path, Primitive};

/// A segment instance: `a` to `b` of `weight` (the units of the view it is
/// drawn in), at least `floor_px` screen px wide at full opacity when
/// `floor_px` is above 0.
pub fn segment(
    a: (f64, f64),
    b: (f64, f64),
    weight: f64,
    rgba: [f32; 4],
    floor_px: f32,
) -> Instance {
    Instance {
        k_a: [SEGMENT, a.0 as f32, a.1 as f32, weight as f32],
        b: [b.0 as f32, b.1 as f32, 0.0, floor_px],
        rgba,
    }
}

/// An arc instance: centre, radius, start angle and signed sweep, in the
/// units of the view it is drawn in, with a stroke `weight`.
pub fn arc(
    centre: (f64, f64),
    radius: f64,
    start: f64,
    sweep: f64,
    weight: f64,
    rgba: [f32; 4],
) -> Instance {
    Instance {
        k_a: [ARC, centre.0 as f32, centre.1 as f32, weight as f32],
        b: [radius as f32, start as f32, sweep as f32, 0.0],
        rgba,
    }
}

/// A filled disc instance.
pub fn filled_disc(centre: (f64, f64), radius: f64, rgba: [f32; 4]) -> Instance {
    Instance {
        k_a: [DISC, centre.0 as f32, centre.1 as f32, 0.0],
        b: [radius as f32, 0.0, 0.0, 0.0],
        rgba,
    }
}

/// One instance, as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    /// kind (0 segment, 1 arc, 2 triangle, 3 disc), a.x, a.y, weight mm.
    pub k_a: [f32; 4],
    /// segment: b.x, b.y, 0, floor; arc: radius, start, sweep, floor;
    /// triangle: b, c; disc: radius. The floor is a minimum width in screen
    /// px for this instance, drawn at full opacity (0: none).
    pub b: [f32; 4],
    /// The colour.
    pub rgba: [f32; 4],
}

const SEGMENT: f32 = 0.0;
const ARC: f32 = 1.0;
const TRIANGLE: f32 = 2.0;
const DISC: f32 = 3.0;

/// How finely curves are flattened: the largest distance between a
/// chord and the curve, mm on the paper.
pub const FLATNESS: f64 = 0.01;

fn rgba(c: Rgba) -> [f32; 4] {
    [c.r as f32, c.g as f32, c.b as f32, c.a as f32]
}

fn p(v: P2<Paper>) -> (f32, f32) {
    (v.x as f32, v.y as f32)
}

/// The instances of a styled scene, in draw order. Each comes with the
/// index of the styled item it draws, for picking and highlighting.
pub fn build(styled: &Styled) -> (Vec<Instance>, Vec<usize>) {
    let mut out = Vec::new();
    let mut owners = Vec::new();
    for (index, item) in styled.items.iter().enumerate() {
        let before = out.len();
        item_instances(item, &mut out);
        owners.extend(std::iter::repeat_n(index, out.len() - before));
    }
    (out, owners)
}

fn item_instances(item: &StyledItem, out: &mut Vec<Instance>) {
    let colour = rgba(item.colour);
    match &item.primitive {
        Primitive::Stroke { path, weight, .. } => {
            let mut emit = Emit {
                weight: *weight as f32,
                colour,
                out,
            };
            if item.dashes.is_empty() {
                emit.path(path);
            } else {
                dashed(path, &item.dashes, &mut emit);
            }
        }
        Primitive::Marker { at, marker, weight } => {
            let h = marker.size / 2.0;
            match marker.kind {
                MarkerKind::Dot => out.push(disc(*at, h, colour)),
                MarkerKind::Cross => {
                    let mut emit = Emit {
                        weight: *weight as f32,
                        colour,
                        out,
                    };
                    emit.segment(P2::new(at.x - h, at.y - h), P2::new(at.x + h, at.y + h));
                    emit.segment(P2::new(at.x - h, at.y + h), P2::new(at.x + h, at.y - h));
                }
            }
        }
        Primitive::Fill { fill, outline } => {
            match fill {
                Fill::Polygon { points } => {
                    // A fan from the first corner: arrowheads are convex.
                    for i in 1..points.len().saturating_sub(1) {
                        out.push(triangle(points[0], points[i], points[i + 1], colour));
                    }
                }
                Fill::Disc { centre, radius } => out.push(disc(*centre, *radius, colour)),
            }
            if let Some(w) = outline {
                let mut emit = Emit {
                    weight: *w as f32,
                    colour,
                    out,
                };
                match fill {
                    Fill::Polygon { points } => {
                        for i in 0..points.len() {
                            emit.segment(points[i], points[(i + 1) % points.len()]);
                        }
                    }
                    Fill::Disc { centre, radius } => {
                        emit.arc(*centre, *radius, 0.0, std::f64::consts::TAU)
                    }
                }
            }
        }
    }
}

fn disc(centre: P2<Paper>, radius: f64, colour: [f32; 4]) -> Instance {
    let (x, y) = p(centre);
    Instance {
        k_a: [DISC, x, y, 0.0],
        b: [radius as f32, 0.0, 0.0, 0.0],
        rgba: colour,
    }
}

fn triangle(a: P2<Paper>, b: P2<Paper>, c: P2<Paper>, colour: [f32; 4]) -> Instance {
    let (ax, ay) = p(a);
    let (bx, by) = p(b);
    let (cx, cy) = p(c);
    Instance {
        k_a: [TRIANGLE, ax, ay, 0.0],
        b: [bx, by, cx, cy],
        rgba: colour,
    }
}

/// Emits the pieces of one stroke.
struct Emit<'a> {
    weight: f32,
    colour: [f32; 4],
    out: &'a mut Vec<Instance>,
}

impl Emit<'_> {
    fn segment(&mut self, a: P2<Paper>, b: P2<Paper>) {
        let (ax, ay) = p(a);
        let (bx, by) = p(b);
        self.out.push(Instance {
            k_a: [SEGMENT, ax, ay, self.weight],
            b: [bx, by, 0.0, 0.0],
            rgba: self.colour,
        });
    }

    fn arc(&mut self, centre: P2<Paper>, radius: f64, start: f64, sweep: f64) {
        let (cx, cy) = p(centre);
        self.out.push(Instance {
            k_a: [ARC, cx, cy, self.weight],
            b: [radius as f32, start as f32, sweep as f32, 0.0],
            rgba: self.colour,
        });
    }

    /// A whole path.
    fn path(&mut self, path: &Path) {
        match *path {
            Path::Line { a, b } => self.segment(a, b),
            Path::Arc {
                centre,
                radius,
                start,
                sweep,
            } => self.arc(centre, radius, start, sweep),
            Path::Circle { centre, radius } => self.arc(centre, radius, 0.0, std::f64::consts::TAU),
            Path::Cubic { .. } | Path::EllipticalArc { .. } => self.flattened(path, 0.0, 1.0),
        }
    }

    /// The part of a path between parameters `t0` and `t1`, for curves
    /// the shader has no exact form of, as chords within the flatness.
    fn flattened(&mut self, path: &Path, t0: f64, t1: f64) {
        let n = chords(path, t0, t1);
        let mut prev = path.at(t0);
        for i in 1..=n {
            let t = t0 + (t1 - t0) * i as f64 / n as f64;
            let next = path.at(t);
            self.segment(prev, next);
            prev = next;
        }
    }
}

/// How many chords flatten the part of a path between `t0` and `t1`
/// within the flatness: from the curve's bound on its second derivative
/// (a cubic's control polygon, an ellipse's larger semi-axis).
fn chords(path: &Path, t0: f64, t1: f64) -> usize {
    let span = (t1 - t0).abs();
    let second = match *path {
        Path::Cubic { a, c1, c2, b } => {
            // |B''| ≤ 6 · max(|c1 - 2 a + ... |): the usual bound.
            let d1 = ((c2.x - 2.0 * c1.x + a.x).powi(2) + (c2.y - 2.0 * c1.y + a.y).powi(2)).sqrt();
            let d2 = ((b.x - 2.0 * c2.x + c1.x).powi(2) + (b.y - 2.0 * c2.y + c1.y).powi(2)).sqrt();
            6.0 * d1.max(d2)
        }
        Path::EllipticalArc { rx, ry, sweep, .. } => rx.max(ry) * sweep * sweep,
        _ => 0.0,
    };
    // A chord over parameter step h deviates at most |f''| h² / 8.
    let n = (second * span * span / (8.0 * FLATNESS)).sqrt().ceil();
    (n as usize).clamp(1, 256)
}

/// A dashed path as its dashes: the fitted sequence (dash, gap, dash, …
/// summing to the path's length) cut from the path by arc length.
fn dashed(path: &Path, dashes: &[f64], emit: &mut Emit) {
    let length = path.length();
    if length <= 0.0 {
        return;
    }
    let mut s = 0.0;
    for (i, piece) in dashes.iter().enumerate() {
        let (from, to) = (s, (s + piece).min(length));
        s += piece;
        if i % 2 == 1 || to <= from {
            continue;
        }
        let (t0, t1) = (from / length, to / length);
        match *path {
            Path::Line { a, b } => {
                let at = |t: f64| P2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                emit.segment(at(t0), at(t1));
            }
            Path::Arc {
                centre,
                radius,
                start,
                sweep,
            } => emit.arc(centre, radius, start + sweep * t0, sweep * (t1 - t0)),
            Path::Circle { centre, radius } => {
                let tau = std::f64::consts::TAU;
                emit.arc(centre, radius, tau * t0, tau * (t1 - t0))
            }
            Path::Cubic { .. } | Path::EllipticalArc { .. } => emit.flattened(path, t0, t1),
        }
    }
}

/// How far a chord of a warped arc may lie from the true arc, in the
/// units of the source (paper mm): 0.0005 mm is a fiftieth of a scan pixel
/// at 200 px/in, so at the viewer's deepest zoom (64x) a quarter of a screen
/// pixel.
pub const WARP_SAGITTA: f64 = 0.0005;

/// The pieces a straight run is cut into so the warp's curvature cannot
/// bend it off the ink, and the most an arc is cut into.
const WARP_MAX_PIECES: usize = 4096;

/// The instances carried through a map `f` of the plane (the registration
/// chain, paper mm to crop px): the same drawing in the map's units, for an
/// overlay on the scan. The map is applied to points, so the drawing is the
/// map's image of the vectors, not of any bitmap.
///
/// - A segment is cut into pieces of at most `step` (source units), each
///   piece's ends mapped. An arc is cut so no chord is more than
///   [`WARP_SAGITTA`] from it (and no piece longer than `step`). A triangle
///   maps its corners, a disc its centre and (radially) its radius.
/// - A stroke's weight scales by the ratio of its piece's mapped length to
///   its source length, the map's local scale along the stroke.
/// - Pieces keep the instance's colour and floor, and each draws the
///   styled item `owners[i]` of its source instance.
pub fn warp(
    base: &[Instance],
    owners: &[usize],
    f: &dyn Fn((f64, f64)) -> (f64, f64),
    step: f64,
) -> (Vec<Instance>, Vec<usize>) {
    let mut out = Vec::with_capacity(base.len() * 2);
    let mut out_owners = Vec::with_capacity(base.len() * 2);
    let length = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).hypot(b.1 - a.1);
    for (i, inst) in base.iter().enumerate() {
        let owner = owners.get(i).copied().unwrap_or(0);
        let before = out.len();
        let a = (f64::from(inst.k_a[1]), f64::from(inst.k_a[2]));
        let weight = f64::from(inst.k_a[3]);
        // One stroke piece from source points `p` to `q`.
        let piece = |p: (f64, f64), q: (f64, f64), out: &mut Vec<Instance>| {
            let (fp, fq) = (f(p), f(q));
            let (src, dst) = (length(p, q), length(fp, fq));
            let w = if src > 1e-12 {
                weight * dst / src
            } else {
                weight
            };
            out.push(Instance {
                k_a: [SEGMENT, fp.0 as f32, fp.1 as f32, w as f32],
                b: [fq.0 as f32, fq.1 as f32, 0.0, inst.b[3]],
                rgba: inst.rgba,
            });
        };
        if inst.k_a[0] < 0.5 {
            let b = (f64::from(inst.b[0]), f64::from(inst.b[1]));
            let n = ((length(a, b) / step).ceil() as usize).clamp(1, WARP_MAX_PIECES);
            let at = |j: usize| {
                let t = j as f64 / n as f64;
                (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
            };
            for j in 0..n {
                piece(at(j), at(j + 1), &mut out);
            }
        } else if inst.k_a[0] < 1.5 {
            let (r, start, sweep) = (
                f64::from(inst.b[0]),
                f64::from(inst.b[1]),
                f64::from(inst.b[2]),
            );
            // The angle one chord may span within the sagitta, and within the step.
            let by_sagitta = if r > WARP_SAGITTA {
                2.0 * (1.0 - WARP_SAGITTA / r).acos()
            } else {
                std::f64::consts::PI
            };
            let by_step = step / r.max(1e-9);
            let n =
                ((sweep.abs() / by_sagitta.min(by_step)).ceil() as usize).clamp(1, WARP_MAX_PIECES);
            let at = |j: usize| {
                let t = start + sweep * j as f64 / n as f64;
                (a.0 + r * t.cos(), a.1 + r * t.sin())
            };
            for j in 0..n {
                piece(at(j), at(j + 1), &mut out);
            }
        } else if inst.k_a[0] < 2.5 {
            let map = |x: f32, y: f32| f((f64::from(x), f64::from(y)));
            let (fa, fb, fc) = (
                map(inst.k_a[1], inst.k_a[2]),
                map(inst.b[0], inst.b[1]),
                map(inst.b[2], inst.b[3]),
            );
            out.push(Instance {
                k_a: [TRIANGLE, fa.0 as f32, fa.1 as f32, 0.0],
                b: [fb.0 as f32, fb.1 as f32, fc.0 as f32, fc.1 as f32],
                rgba: inst.rgba,
            });
        } else {
            let r = f64::from(inst.b[0]);
            let (fc, fr) = (f(a), f((a.0 + r, a.1)));
            out.push(Instance {
                k_a: [DISC, fc.0 as f32, fc.1 as f32, 0.0],
                b: [length(fc, fr) as f32, 0.0, 0.0, 0.0],
                rgba: inst.rgba,
            });
        }
        out_owners.extend(std::iter::repeat_n(owner, out.len() - before));
    }
    (out, out_owners)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmsheets_document::value::style::Dash;
    use lmsheets_render::scene::Source;

    fn item(primitive: Primitive, dashes: Vec<f64>) -> StyledItem {
        StyledItem {
            primitive,
            dashes,
            colour: Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            layer: "visible".into(),
            source: Source::Sheet("x".into()),
        }
    }

    fn stroke(path: Path) -> Primitive {
        Primitive::Stroke {
            path,
            weight: 0.5,
            dash: Dash::SOLID,
            anchors: Vec::new(),
        }
    }

    #[test]
    fn a_dashed_line_becomes_its_dashes() {
        let line = Path::Line {
            a: P2::new(0.0, 0.0),
            b: P2::new(10.0, 0.0),
        };
        let mut out = Vec::new();
        item_instances(&item(stroke(line), vec![3.0, 2.0, 3.0, 2.0]), &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].k_a, [SEGMENT, 0.0, 0.0, 0.5]);
        assert_eq!(out[0].b[..2], [3.0, 0.0]);
        assert_eq!(out[1].k_a[1..3], [5.0, 0.0]);
        assert_eq!(out[1].b[..2], [8.0, 0.0]);
    }

    #[test]
    fn a_dashed_circle_becomes_arcs_and_a_solid_one_a_full_arc() {
        let circle = Path::Circle {
            centre: P2::new(0.0, 0.0),
            radius: 1.0,
        };
        let mut out = Vec::new();
        item_instances(&item(stroke(circle), Vec::new()), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].k_a[0], ARC);
        assert!((out[0].b[2] - std::f32::consts::TAU).abs() < 1e-6);
        let quarter = std::f64::consts::TAU / 4.0;
        let mut out = Vec::new();
        item_instances(
            &item(stroke(circle), vec![quarter, quarter, quarter, quarter]),
            &mut out,
        );
        assert_eq!(out.len(), 2);
        assert!((out[1].b[1] - std::f32::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn a_cubic_is_flattened_within_the_flatness() {
        let cubic = Path::Cubic {
            a: P2::new(0.0, 0.0),
            c1: P2::new(0.0, 5.0),
            c2: P2::new(10.0, 5.0),
            b: P2::new(10.0, 0.0),
        };
        let mut out = Vec::new();
        item_instances(&item(stroke(cubic), Vec::new()), &mut out);
        assert!(out.len() > 10 && out.len() < 256, "{}", out.len());
        // Every chord's midpoint is within the flatness of the curve's
        // point at the middle parameter.
        let n = out.len();
        for (i, inst) in out.iter().enumerate() {
            let mid = (
                ((inst.k_a[1] + inst.b[0]) / 2.0) as f64,
                ((inst.k_a[2] + inst.b[1]) / 2.0) as f64,
            );
            let on = cubic.at((i as f64 + 0.5) / n as f64);
            let d = ((on.x - mid.0).powi(2) + (on.y - mid.1).powi(2)).sqrt();
            assert!(d <= FLATNESS * 1.01, "chord {i} is {d} off");
        }
    }

    #[test]
    fn an_arrowhead_is_a_fan_and_its_outline() {
        let fill = Primitive::Fill {
            fill: Fill::Polygon {
                points: vec![P2::new(0.0, 0.0), P2::new(3.0, 1.0), P2::new(3.0, -1.0)],
            },
            outline: Some(0.5),
        };
        let mut out = Vec::new();
        item_instances(&item(fill, Vec::new()), &mut out);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].k_a[0], TRIANGLE);
        assert!(out[1..].iter().all(|i| i.k_a[0] == SEGMENT));
    }

    #[test]
    fn a_warp_cuts_strokes_follows_the_map_and_scales_weights() {
        // A map that scales by 10 and bends: y += 0.01 x^2.
        let f = |p: (f64, f64)| (p.0 * 10.0, (p.1 + 0.01 * p.0 * p.0) * 10.0);
        let mut base = Vec::new();
        item_instances(
            &item(
                stroke(Path::Line {
                    a: P2::new(0.0, 0.0),
                    b: P2::new(9.0, 0.0),
                }),
                Vec::new(),
            ),
            &mut base,
        );
        item_instances(
            &item(
                stroke(Path::Circle {
                    centre: P2::new(0.0, 5.0),
                    radius: 1.0,
                }),
                Vec::new(),
            ),
            &mut base,
        );
        let owners = vec![3, 7];
        let (out, own) = warp(&base, &owners, &f, 2.0);
        // 9 mm in pieces of at most 2: five; the circle by its sagitta.
        assert_eq!(own[..5], [3; 5]);
        assert!(own[5..].iter().all(|o| *o == 7) && own.len() == out.len());
        assert!(out.len() > 5 + 30);
        // The chain of pieces is connected and starts and ends on the map's images.
        for w in out[..5].windows(2) {
            assert_eq!(w[0].b[..2], w[1].k_a[1..3]);
        }
        assert_eq!(out[0].k_a[1..3], [0.0, 0.0]);
        assert_eq!(out[4].b[..2], [90.0, 8.1]);
        // The weight scales by the local length ratio, about 10.
        assert!((out[0].k_a[3] - 5.0).abs() < 0.1, "{}", out[0].k_a[3]);
        // Every chord of the circle is within the sagitta of the mapped circle.
        let n = out.len() - 5;
        for (j, inst) in out[5..].iter().enumerate() {
            let mid = (
                f64::from(inst.k_a[1] + inst.b[0]) / 2.0,
                f64::from(inst.k_a[2] + inst.b[1]) / 2.0,
            );
            let t = std::f64::consts::TAU * (j as f64 + 0.5) / n as f64;
            let on = f((t.cos(), 5.0 + t.sin()));
            let d = (on.0 - mid.0).hypot(on.1 - mid.1);
            assert!(d <= WARP_SAGITTA * 10.0 * 1.2, "chord {j} is {d} off");
        }
    }
}
