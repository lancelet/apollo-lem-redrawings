//! Dash fitting: how a layer's pattern is laid along each curve, the
//! draughtsman's way.
//!
//! The rules, stated once. A dashed curve starts and ends on a dash
//! (a closed one takes whole periods). A centre line crosses a circle's
//! centre on its short group ([`Primitive::Stroke`]'s anchors, found by
//! `draw`). Every crossing with another curve of the drawing falls
//! inside a dash with clear room either side, so no gap sits on
//! another line and no dash ends on one. The gaps and the short dashes
//! keep their standard sizes; the long dashes take up the stretch; a
//! gap group that cannot be moved clear of a crossing within the
//! allowed dash lengths is left out. Lettering is not crossing
//! geometry.
//!
//! The crossings come from a uniform [`Grid`] over the scene's lines,
//! arcs and circles, built once per resolve, so the cost is linear in
//! the strokes and the crossings found: this runs on every edit.

use crate::conventions::Conventions;
use crate::scene::{Item, Part, Path, Primitive, Source};
use lmsheets_document::value::bounds::Bounded;
use lmsheets_document::value::geometry::Rect;
use lmsheets_document::value::geometry::{Paper, P2};
use std::f64::consts::TAU;

type V = P2<Paper>;

// ---- crossings

/// The distance along an arc or circle at angle `a`, if the angle lies
/// on it: a circle runs from angle 0 the positive way; an arc from its
/// start through its sweep.
fn along_arc(path: &Path, a: f64) -> Option<f64> {
    match *path {
        Path::Circle { radius, .. } => Some(a.rem_euclid(TAU) * radius),
        Path::Arc {
            radius,
            start,
            sweep,
            ..
        } => {
            let turned = (a - start).rem_euclid(TAU);
            let along = if sweep >= 0.0 { turned } else { TAU - turned };
            let along = if along >= TAU - 1e-12 { 0.0 } else { along };
            (along <= sweep.abs() + 1e-12).then_some(along * radius)
        }
        _ => None,
    }
}

/// Whether a point at angle `a` about a circle's or arc's centre lies on
/// it (always, for a circle).
fn on_arc(path: &Path, a: f64) -> bool {
    along_arc(path, a).is_some()
}

/// The points where a line from `p` to `q` meets a circle `(c, r)`, as
/// fractions along the line.
fn line_circle(p: V, q: V, c: V, r: f64) -> Vec<f64> {
    let (dx, dy) = (q.x - p.x, q.y - p.y);
    let (fx, fy) = (p.x - c.x, p.y - c.y);
    let a = dx * dx + dy * dy;
    let b = 2.0 * (fx * dx + fy * dy);
    let k = fx * fx + fy * fy - r * r;
    let disc = b * b - 4.0 * a * k;
    if a <= 0.0 || disc < 0.0 {
        return Vec::new();
    }
    let s = disc.sqrt();
    vec![(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)]
}

/// The points where two circles meet.
fn circle_circle(c1: V, r1: f64, c2: V, r2: f64) -> Vec<V> {
    let (dx, dy) = (c2.x - c1.x, c2.y - c1.y);
    let d = (dx * dx + dy * dy).sqrt();
    if d <= 1e-12 || d > r1 + r2 + 1e-12 || d < (r1 - r2).abs() - 1e-12 {
        return Vec::new();
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let (mx, my) = (c1.x + a * dx / d, c1.y + a * dy / d);
    let (ox, oy) = (-dy / d * h, dx / d * h);
    vec![P2::new(mx + ox, my + oy), P2::new(mx - ox, my - oy)]
}

fn centre_radius(path: &Path) -> Option<(V, f64)> {
    match *path {
        Path::Circle { centre, radius } | Path::Arc { centre, radius, .. } => {
            Some((centre, radius))
        }
        _ => None,
    }
}

/// Where `other` crosses `path`, as distances along `path`. Only lines,
/// arcs and circles cross; a crossing at an open `path`'s own ends is
/// not one (the end is a dash already), but a closed path has no ends:
/// a crossing at its start counts, at distance 0. `other` may end on
/// `path`.
pub fn crossings_of(path: &Path, other: &Path, out: &mut Vec<f64>) {
    const EPS: f64 = 1e-9;
    let length = path.length();
    let mut accept = |d: f64| {
        if path.is_closed() {
            out.push(if d >= length - EPS { 0.0 } else { d });
        } else if d > EPS && d < length - EPS {
            out.push(d);
        }
    };
    match (*path, *other) {
        (Path::Line { a: p, b: q }, Path::Line { a: r, b: s }) => {
            let (dx, dy) = (q.x - p.x, q.y - p.y);
            let (ex, ey) = (s.x - r.x, s.y - r.y);
            let den = dx * ey - dy * ex;
            if den.abs() < 1e-12 {
                return;
            }
            let (rx, ry) = (r.x - p.x, r.y - p.y);
            let t = (rx * ey - ry * ex) / den;
            let u = (rx * dy - ry * dx) / den;
            if t > EPS && t < 1.0 - EPS && (-EPS..=1.0 + EPS).contains(&u) {
                accept(t * (dx * dx + dy * dy).sqrt());
            }
        }
        (Path::Line { a: p, b: q }, _) => {
            let Some((c, r)) = centre_radius(other) else {
                return;
            };
            let len = ((q.x - p.x).powi(2) + (q.y - p.y).powi(2)).sqrt();
            for t in line_circle(p, q, c, r) {
                if t > EPS && t < 1.0 - EPS {
                    let x: V = P2::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t);
                    if on_arc(other, (x.y - c.y).atan2(x.x - c.x)) {
                        accept(t * len);
                    }
                }
            }
        }
        (_, Path::Line { a: r, b: s }) => {
            let Some((c, rad)) = centre_radius(path) else {
                return;
            };
            for u in line_circle(r, s, c, rad) {
                if (-EPS..=1.0 + EPS).contains(&u) {
                    let x: V = P2::new(r.x + (s.x - r.x) * u, r.y + (s.y - r.y) * u);
                    if let Some(d) = along_arc(path, (x.y - c.y).atan2(x.x - c.x)) {
                        accept(d);
                    }
                }
            }
        }
        _ => {
            let (Some((c1, r1)), Some((c2, r2))) = (centre_radius(path), centre_radius(other))
            else {
                return;
            };
            for x in circle_circle(c1, r1, c2, r2) {
                if !on_arc(other, (x.y - c2.y).atan2(x.x - c2.x)) {
                    continue;
                }
                if let Some(d) = along_arc(path, (x.y - c1.y).atan2(x.x - c1.x)) {
                    accept(d);
                }
            }
        }
    }
}

/// A uniform grid over the scene's lines, arcs and circles, for finding
/// what crosses a dashed curve without testing everything.
pub struct Grid {
    paths: Vec<(usize, Path)>,
    min: V,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

impl Grid {
    /// The grid over these paths (each with the index of its scene item).
    pub fn build(paths: Vec<(usize, Path)>) -> Self {
        let bounds = Rect::union_all(paths.iter().filter_map(|(_, p)| p.bounds()))
            .or_else(|| Rect::around(P2::new(0.0, 0.0)))
            .expect("the origin is finite")
            .grow(1.0);
        let (w, h) = (bounds.width(), bounds.height());
        // Cells about the size of a typical stroke's share of the sheet,
        // at least 2 mm, at most 256 a side.
        let n = paths.len().max(1) as f64;
        let cell = ((w * h / n).sqrt() * 1.5)
            .max(2.0)
            .max(w / 256.0)
            .max(h / 256.0);
        let nx = ((w / cell).ceil() as usize).max(1);
        let ny = ((h / cell).ceil() as usize).max(1);
        let mut cells = vec![Vec::new(); nx * ny];
        let mut g = Self {
            paths: Vec::new(),
            min: bounds.min(),
            cell,
            nx,
            ny,
            cells: Vec::new(),
        };
        for (i, (_, p)) in paths.iter().enumerate() {
            let Some(b) = p.bounds() else { continue };
            let (x0, y0, x1, y1) = g.range(&b);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    cells[y * nx + x].push(i as u32);
                }
            }
        }
        g.paths = paths;
        g.cells = cells;
        g
    }

    /// The cell range a box covers, clamped to the grid.
    fn range(&self, b: &Rect<Paper>) -> (usize, usize, usize, usize) {
        let at = |v: f64, lo: f64, n: usize| {
            (((v - lo) / self.cell).floor().max(0.0) as usize).min(n - 1)
        };
        (
            at(b.min().x, self.min.x, self.nx),
            at(b.min().y, self.min.y, self.ny),
            at(b.max().x, self.min.x, self.nx),
            at(b.max().y, self.min.y, self.ny),
        )
    }

    /// Where the other paths cross `path` (the stroke of scene item
    /// `item`), as distances along it, sorted.
    pub fn crossings(&self, item: usize, path: &Path, seen: &mut [u32], stamp: u32) -> Vec<f64> {
        let mut out = Vec::new();
        let Some(b) = path.bounds() else {
            return out;
        };
        let (x0, y0, x1, y1) = self.range(&b);
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in &self.cells[y * self.nx + x] {
                    if seen[i as usize] == stamp {
                        continue;
                    }
                    seen[i as usize] = stamp;
                    let (owner, other) = &self.paths[i as usize];
                    if *owner != item {
                        crossings_of(path, other, &mut out);
                    }
                }
            }
        }
        out.sort_by(f64::total_cmp);
        out
    }

    /// How many paths the grid holds.
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// The crossing geometry of a scene: every line, arc and circle stroke
/// that is not lettering (a text's, or a dimension's value).
pub fn grid_of<'a>(items: impl Iterator<Item = &'a Item>) -> Grid {
    Grid::build(
        items
            .enumerate()
            .filter(|(_, i)| {
                !matches!(
                    i.source,
                    Source::Text(_) | Source::Dimension(_, Part::Lettering)
                )
            })
            .filter_map(|(i, item)| match &item.primitive {
                Primitive::Stroke { path, .. }
                    if matches!(
                        path,
                        Path::Line { .. } | Path::Arc { .. } | Path::Circle { .. }
                    ) =>
                {
                    Some((i, *path))
                }
                _ => None,
            })
            .collect(),
    )
}

// ---- fitting

/// A piece of a sequence: its length and whether it is drawn.
type Piece = (f64, bool);

/// A dash pattern laid along a curve of `length`: the explicit sequence
/// of dashes and gaps, dash first, summing to the length (empty when
/// solid, or when the curve is too short for one dash). `anchors` are
/// where the pattern's short group is centred; `crossings` where other
/// curves cross, to be kept inside dashes.
pub fn fit(
    pattern: &[f64],
    length: f64,
    closed: bool,
    anchors: &[f64],
    crossings: &[f64],
    conv: &Conventions,
) -> Vec<f64> {
    if pattern.is_empty() || length <= 0.0 {
        return Vec::new();
    }
    let long = pattern[0];
    // The group between long dashes: a gap, or gap, short, gap, ….
    let group: Vec<Piece> = pattern[1..]
        .iter()
        .enumerate()
        .map(|(i, v)| (*v, i % 2 == 1))
        .collect();
    let group: Vec<Piece> = if group.is_empty() {
        // An odd pattern repeats doubled: its gap is its own length.
        vec![(long, false)]
    } else {
        group
    };
    let g_len: f64 = group.iter().map(|p| p.0).sum();
    let period = long + g_len;
    if length <= long || (closed && length < period) {
        return Vec::new();
    }
    let long_min = long * conv.long_min;
    let clear = conv.dash_clear;
    let (left, right) = halves(&group);
    let pieces = if closed {
        // The seam (the sequence's start) goes in the widest stretch
        // free of crossings; the sequence is rotated to the path's
        // start afterwards.
        let seam = seam_of(length, crossings, clear);
        let shifted: Vec<f64> = crossings
            .iter()
            .map(|x| (x - seam).rem_euclid(length))
            .collect();
        let pieces = stretch(
            length,
            &[],
            &[],
            &group,
            long,
            long_min,
            &shifted,
            clear,
            true,
        );
        rotate(pieces, (length - seam).rem_euclid(length))
    } else {
        // An anchor needs room for half its group and a long dash on
        // either side; nearer an end the pattern just runs from the end.
        let room = g_len / 2.0 + long_min;
        let mut bounds = vec![0.0];
        let mut inner: Vec<f64> = anchors
            .iter()
            .copied()
            .filter(|t| *t > room && *t < length - room)
            .collect();
        inner.sort_by(f64::total_cmp);
        inner.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        bounds.extend(inner);
        bounds.push(length);
        let mut pieces = Vec::new();
        for (i, w) in bounds.windows(2).enumerate() {
            let first = i == 0;
            let last = i + 2 == bounds.len();
            let head: &[Piece] = if first { &[] } else { &right };
            let tail: &[Piece] = if last { &[] } else { &left };
            // Crossings in this stretch, in its own distances; those at
            // the anchor are the anchor's own (the crossing centre line).
            let local: Vec<f64> = crossings
                .iter()
                .map(|x| x - w[0])
                .filter(|x| *x > 0.0 && *x < w[1] - w[0])
                .collect();
            pieces.extend(stretch(
                w[1] - w[0],
                head,
                tail,
                &group,
                long,
                long_min,
                &local,
                clear,
                false,
            ));
        }
        pieces
    };
    finish(pieces)
}

/// The two halves of a group about its middle.
fn halves(group: &[Piece]) -> (Vec<Piece>, Vec<Piece>) {
    let total: f64 = group.iter().map(|p| p.0).sum();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut run = 0.0;
    for &(len, dash) in group {
        if run + len <= total / 2.0 {
            left.push((len, dash));
        } else if run >= total / 2.0 {
            right.push((len, dash));
        } else {
            left.push((total / 2.0 - run, dash));
            right.push((run + len - total / 2.0, dash));
        }
        run += len;
    }
    (left, right)
}

/// Where to put a closed curve's seam: the middle of the widest run of
/// the circumference clear of every crossing.
fn seam_of(length: f64, crossings: &[f64], clear: f64) -> f64 {
    if crossings.is_empty() {
        return 0.0;
    }
    let mut xs: Vec<f64> = crossings.iter().map(|x| x.rem_euclid(length)).collect();
    xs.sort_by(f64::total_cmp);
    let mut best = (f64::MIN, 0.0);
    for i in 0..xs.len() {
        let a = xs[i] + clear;
        let b = if i + 1 < xs.len() {
            xs[i + 1] - clear
        } else {
            xs[0] + length - clear
        };
        if b - a > best.0 {
            best = (b - a, ((a + b) / 2.0).rem_euclid(length));
        }
    }
    best.1
}

/// One stretch of length `s`, from `head` (half a group at an anchor,
/// or nothing) to `tail`, with as many whole groups between as fit,
/// each moved clear of the crossings or left out. For a closed curve
/// the first and last long dashes are halves of the seam's.
#[allow(clippy::too_many_arguments)]
fn stretch(
    s: f64,
    head: &[Piece],
    tail: &[Piece],
    group: &[Piece],
    long: f64,
    long_min: f64,
    crossings: &[f64],
    clear: f64,
    closed: bool,
) -> Vec<Piece> {
    let h: f64 = head.iter().map(|p| p.0).sum();
    let t: f64 = tail.iter().map(|p| p.0).sum();
    let g: f64 = group.iter().map(|p| p.0).sum();
    // An open stretch has a whole long dash at each end; a closed curve
    // has half of its seam's at each.
    let ends = if closed { 2.0 } else { 1.0 };
    let (end_min, end_share) = (long_min / ends, 1.0 / ends);
    // How many groups: the nominal count, reduced until the longs fit.
    let mut k = (((s - h - t) - long) / (long + g)).round().max(0.0) as usize;
    if closed {
        k = ((s / (long + g)).round() as usize).max(1);
    }
    while k > 0 && s - h - t - k as f64 * g < 2.0 * end_min + (k as f64 - 1.0) * long_min {
        k -= 1;
    }
    let forbidden: Vec<(f64, f64)> = crossings.iter().map(|x| (x - clear, x + clear)).collect();
    let hits = |p: f64| {
        forbidden
            .iter()
            .find(|(f0, f1)| p < *f1 && p + g > *f0)
            .copied()
    };
    // Nominal positions: the longs share the room equally, the two end
    // longs being halves of one on a closed curve.
    let room = s - h - t - k as f64 * g;
    let nominal_long = room / (k as f64 - 1.0 + 2.0 * end_share).max(1e-12);
    let mut placed: Vec<f64> = Vec::new();
    let mut prev_end = h;
    for i in 0..k {
        let nominal = h + nominal_long * (end_share + i as f64) + i as f64 * g;
        let lo = prev_end + if i == 0 { end_min } else { long_min };
        let hi = s - t - g - (k - 1 - i) as f64 * (g + long_min) - end_min;
        if lo > hi {
            continue;
        }
        let mut p = nominal.clamp(lo, hi);
        let mut ok = true;
        for _ in 0..8 {
            let Some((f0, f1)) = hits(p) else {
                break;
            };
            let before = f0 - g;
            let after = f1;
            let cands: Vec<f64> = [before, after]
                .into_iter()
                .filter(|c| *c >= lo && *c <= hi && hits(*c).is_none())
                .collect();
            match cands
                .into_iter()
                .min_by(|a, b| (a - nominal).abs().total_cmp(&(b - nominal).abs()))
            {
                Some(c) => {
                    p = c;
                    break;
                }
                None => {
                    // Try the nearest side even if it still hits, and go
                    // round again; else leave the group out.
                    let c = if (before - nominal).abs() <= (after - nominal).abs() {
                        before
                    } else {
                        after
                    };
                    if c < lo || c > hi {
                        ok = false;
                        break;
                    }
                    p = c;
                }
            }
        }
        if !ok || hits(p).is_some() {
            continue;
        }
        placed.push(p);
        prev_end = p + g;
    }
    let mut out: Vec<Piece> = head.to_vec();
    let mut at = h;
    for p in placed {
        out.push((p - at, true));
        out.extend_from_slice(group);
        at = p + g;
    }
    out.push((s - t - at, true));
    out.extend_from_slice(tail);
    out
}

/// A closed curve's sequence, rotated to start `by` along it.
fn rotate(pieces: Vec<Piece>, by: f64) -> Vec<Piece> {
    let mut at = 0.0;
    let mut head = Vec::new();
    let mut tail = Vec::new();
    for (len, dash) in pieces {
        if at + len <= by {
            head.push((len, dash));
        } else if at >= by {
            tail.push((len, dash));
        } else {
            head.push((by - at, dash));
            tail.push((at + len - by, dash));
        }
        at += len;
    }
    tail.extend(head);
    tail
}

/// Merge neighbours of one kind, drop empty pieces, and start on a dash
/// (an empty one if the sequence opens on a gap).
fn finish(pieces: Vec<Piece>) -> Vec<f64> {
    let mut merged: Vec<Piece> = Vec::new();
    for (len, dash) in pieces {
        if len <= 0.0 {
            continue;
        }
        match merged.last_mut() {
            Some((l, d)) if *d == dash => *l += len,
            _ => merged.push((len, dash)),
        }
    }
    if merged.first().is_some_and(|p| !p.1) {
        merged.insert(0, (0.0, true));
    }
    merged.into_iter().map(|p| p.0).collect()
}

/// A pattern fitted to a curve with no anchors and nothing crossing it.
pub fn fit_dash(pattern: &[f64], path: &Path) -> Vec<f64> {
    fit(
        pattern,
        path.length(),
        path.is_closed(),
        &[],
        &[],
        &Conventions::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn line(len: f64) -> Path {
        Path::Line {
            a: P2::new(0.0, 0.0),
            b: P2::new(len, 0.0),
        }
    }

    /// The dashes of a sequence as (start, end) along the curve.
    fn dashes_of(seq: &[f64]) -> Vec<(f64, f64)> {
        let mut at = 0.0;
        seq.iter()
            .enumerate()
            .filter_map(|(i, len)| {
                let d = (at, at + len);
                at += len;
                (i % 2 == 0 && *len > 0.0).then_some(d)
            })
            .collect()
    }

    fn inside_a_dash(seq: &[f64], x: f64, clear: f64) -> bool {
        dashes_of(seq)
            .iter()
            .any(|(a, b)| x - a >= clear - 1e-9 && b - x >= clear - 1e-9)
    }

    const CENTRE: [f64; 4] = [12.0, 1.5, 1.5, 1.5];
    const HIDDEN: [f64; 2] = [3.0, 1.5];

    #[test]
    fn dashes_start_and_end_on_a_dash_with_standard_gaps() {
        // Hidden 3/1.5 on 20 mm: four gaps of 1.5 and five dashes sharing
        // the rest, filling the line exactly.
        let d = fit_dash(&HIDDEN, &line(20.0));
        assert_eq!(d.len(), 9);
        assert!(d
            .iter()
            .skip(1)
            .step_by(2)
            .all(|g| (*g - 1.5).abs() < 1e-12));
        assert!(
            d.iter().step_by(2).all(|l| (*l - 2.8).abs() < 1e-9),
            "{d:?}"
        );
        assert!((d.iter().sum::<f64>() - 20.0).abs() < 1e-9);
        // Too short for a dash: solid. Solid stays solid.
        assert!(fit_dash(&HIDDEN, &line(2.0)).is_empty());
        assert!(fit_dash(&[], &line(50.0)).is_empty());
        // A circle takes whole periods, its gaps standard, and its seam
        // in the middle of a dash so the repeat joins seamlessly.
        let c = Path::Circle {
            centre: P2::new(0.0, 0.0),
            radius: 10.0,
        };
        let d = fit_dash(&HIDDEN, &c);
        assert!((d.iter().sum::<f64>() - c.length()).abs() < 1e-9);
        assert!(d
            .iter()
            .skip(1)
            .step_by(2)
            .all(|g| (*g - 1.5).abs() < 1e-12));
        assert_eq!(d.len() % 2, 1);
        assert!((d[0] - d[d.len() - 1]).abs() < 1e-9);
    }

    #[test]
    fn anchored_centre_lines_cross_on_their_short_dashes() {
        let conv = Conventions::default();
        let seq = fit(&CENTRE, 70.0, false, &[50.0], &[], &conv);
        let dashes = dashes_of(&seq);
        assert!((seq.iter().sum::<f64>() - 70.0).abs() < 1e-9);
        assert!(dashes[0].0 == 0.0 && dashes[0].1 > 5.0);
        assert!((dashes.last().unwrap().1 - 70.0).abs() < 1e-9);
        let short = dashes
            .iter()
            .find(|(a, b)| ((a + b) / 2.0 - 50.0).abs() < 1e-9)
            .expect("a dash centred on the anchor");
        assert!((short.1 - short.0 - 1.5).abs() < 1e-9, "{short:?}");
        let seq = fit(&CENTRE, 100.0, false, &[30.0, 70.0], &[], &conv);
        let dashes = dashes_of(&seq);
        for at in [30.0, 70.0] {
            assert!(dashes
                .iter()
                .any(|(a, b)| ((a + b) / 2.0 - at).abs() < 1e-9 && (b - a - 1.5).abs() < 1e-9));
        }
        // A phantom pattern centres its pair of shorts on the anchor: the
        // middle gap straddles it, and the sequence still opens on a dash.
        let seq = fit(
            &[12.0, 1.5, 1.5, 1.5, 1.5, 1.5],
            60.0,
            false,
            &[20.0],
            &[],
            &conv,
        );
        assert!((seq.iter().sum::<f64>() - 60.0).abs() < 1e-9);
        let dashes = dashes_of(&seq);
        assert!(dashes[0].0 == 0.0);
        assert!(dashes.iter().all(|(a, b)| !(a < &20.0 && b > &20.0)));
    }

    #[test]
    fn crossings_fall_inside_dashes() {
        let conv = Conventions::default();
        let clear = conv.dash_clear;
        // A hidden line crossed where a gap would nominally be: the gap
        // moves, the crossing sits in a dash with room, nothing else
        // changes size.
        let plain = fit(&HIDDEN, 40.0, false, &[], &[], &conv);
        let gap_at = dashes_of(&plain)[2].1 + 0.75;
        let seq = fit(&HIDDEN, 40.0, false, &[], &[gap_at], &conv);
        assert!(inside_a_dash(&seq, gap_at, clear), "{seq:?}");
        assert!((seq.iter().sum::<f64>() - 40.0).abs() < 1e-9);
        assert!(seq
            .iter()
            .skip(1)
            .step_by(2)
            .all(|g| (*g - 1.5).abs() < 1e-12));
        // A centre line crossing an edge and anchored at a circle: both.
        let seq = fit(&CENTRE, 70.0, false, &[50.0], &[20.0, 62.0], &conv);
        for x in [20.0, 62.0] {
            assert!(inside_a_dash(&seq, x, clear), "{seq:?}");
        }
        assert!(dashes_of(&seq)
            .iter()
            .any(|(a, b)| ((a + b) / 2.0 - 50.0).abs() < 1e-9));
        // Crossings too dense for a gap to fit between: the group is left
        // out and one long dash spans them.
        let dense: Vec<f64> = (10..=30).map(|i| i as f64).collect();
        let seq = fit(&HIDDEN, 40.0, false, &[], &dense, &conv);
        assert!(
            dense.iter().all(|x| inside_a_dash(&seq, *x, clear)),
            "{seq:?}"
        );
        assert!(dashes_of(&seq)
            .iter()
            .any(|(a, b)| a <= &10.0 && b >= &30.0));
        // A hidden circle crossed by two centre lines: all four crossings
        // in dashes, the circumference filled.
        let circ = Path::Circle {
            centre: P2::new(0.0, 0.0),
            radius: 8.0,
        };
        let xs: Vec<f64> = [0.0, 0.5, 1.0, 1.5].iter().map(|q| q * PI * 8.0).collect();
        let seq = fit(&HIDDEN, circ.length(), true, &[], &xs, &conv);
        assert!((seq.iter().sum::<f64>() - circ.length()).abs() < 1e-9);
        for x in xs {
            let on = dashes_of(&seq).iter().any(|(a, b)| {
                (x - a >= clear - 1e-9 && b - x >= clear - 1e-9)
                    || (x < clear && seq[0] + seq[seq.len() - 1] >= clear)
            });
            assert!(on, "{x} in {seq:?}");
        }
    }

    #[test]
    fn crossings_are_found_for_every_kind() {
        let mut out = Vec::new();
        let l = line(10.0);
        let v = Path::Line {
            a: P2::new(4.0, -5.0),
            b: P2::new(4.0, 5.0),
        };
        crossings_of(&l, &v, &mut out);
        assert_eq!(out, vec![4.0]);
        out.clear();
        // A line ending on the dashed line still crosses it; the dashed
        // line's own end does not count.
        let t = Path::Line {
            a: P2::new(7.0, 0.0),
            b: P2::new(7.0, 5.0),
        };
        crossings_of(&l, &t, &mut out);
        assert_eq!(out, vec![7.0]);
        out.clear();
        let c = Path::Circle {
            centre: P2::new(5.0, 0.0),
            radius: 2.0,
        };
        crossings_of(&l, &c, &mut out);
        out.sort_by(f64::total_cmp);
        assert!((out[0] - 3.0).abs() < 1e-9 && (out[1] - 7.0).abs() < 1e-9);
        out.clear();
        // The circle crossed by the line: at angles π and 0, distances
        // π·r and 0 (its start, which counts: a circle has no ends).
        crossings_of(&c, &l, &mut out);
        out.sort_by(f64::total_cmp);
        assert_eq!(out.len(), 2);
        assert!(out[0].abs() < 1e-9 && (out[1] - PI * 2.0).abs() < 1e-9);
        out.clear();
        // An arc of the top half only meets a vertical line once.
        let arc = Path::Arc {
            centre: P2::new(5.0, 0.0),
            radius: 2.0,
            start: PI,
            sweep: PI,
        };
        let up = Path::Line {
            a: P2::new(5.0, -5.0),
            b: P2::new(5.0, 5.0),
        };
        crossings_of(&arc, &up, &mut out);
        assert_eq!(out.len(), 1);
        assert!((out[0] - PI / 2.0 * 2.0).abs() < 1e-9);
        out.clear();
        crossings_of(&up, &arc, &mut out);
        assert_eq!(out.len(), 1);
        out.clear();
        // Two circles.
        let d = Path::Circle {
            centre: P2::new(8.0, 0.0),
            radius: 2.0,
        };
        crossings_of(&c, &d, &mut out);
        assert_eq!(out.len(), 2);
        // The grid finds the same, skipping the stroke's own item.
        let item = |path: Path, source: Source| Item {
            primitive: Primitive::Stroke {
                path,
                weight: 0.3,
                dash: lmsheets_document::value::style::Dash::SOLID,
                anchors: Vec::new(),
            },
            layer: "visible".into(),
            tint: None,
            source,
        };
        let items = [
            item(l, Source::Sheet("l".into())),
            item(v, Source::Sheet("v".into())),
            item(c, Source::Sheet("c".into())),
            // Lettering is not crossing geometry.
            item(
                Path::Line {
                    a: P2::new(2.0, -1.0),
                    b: P2::new(2.0, 1.0),
                },
                Source::Text("note".into()),
            ),
        ];
        let grid = grid_of(items.iter());
        let mut seen = vec![0; grid.len()];
        let xs = grid.crossings(0, &l, &mut seen, 1);
        assert_eq!(xs, vec![3.0, 4.0, 7.0]);
    }
}
