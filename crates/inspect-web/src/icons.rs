//! The constraint icons as GPU instances: screen-sized (constant device px
//! whatever the zoom), anchored at a point of the paper. Pure, so it is
//! tested natively; only the wasm build draws them.
//!
//! Each cluster is a paper-coloured disc, a ring and the type's glyph in the
//! trust class's colour; an icon standing for several constraints carries a
//! count badge. The icon under the pointer is drawn filled; icons whose
//! constraints bind the lit feature are filled and the others dimmed.

use lmsheets_gpu::instances::{arc, filled_disc, segment, Instance};
use lmsheets_inspector::constraints::{
    count_digits, glyph, Cluster, ConstraintItem, GlyphPart, Link, ICON_PX,
};
use lmsheets_inspector::Theme;

/// How an icon is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emphasis {
    /// Neither under the pointer nor bound to what is lit.
    Plain,
    /// Binds the lit feature directly: filled.
    Related,
    /// A constraint of nothing lit while something is: dimmed.
    Dim,
    /// Under the pointer: filled.
    Hovered,
}

/// The constraint list the clusters index, and whether every icon draws its
/// connections (the connections view) or only the one under the pointer its
/// relation.
#[derive(Clone, Copy)]
pub struct Connect<'a> {
    /// The constraint list.
    pub items: &'a [ConstraintItem],
    /// Draw connectors and relations for every icon.
    pub all: bool,
}

/// The instances of `clusters`. `to_unit` carries a paper point (mm) to the
/// drawing's own units (mm for the redraw pane, crop px under the overlay),
/// `zoom` is device px per such unit and `dpr` the device pixel ratio.
///
/// `items` is the constraint list the clusters index. The icon under the
/// pointer draws its relation between two points, dashed.
pub fn build(
    clusters: &[Cluster],
    connect: Connect,
    to_unit: &dyn Fn((f64, f64)) -> (f64, f64),
    zoom: f64,
    dpr: f64,
    theme: Theme,
    emphasis: &dyn Fn(&Cluster) -> Emphasis,
) -> Vec<Instance> {
    let px = 1.0 / zoom;
    let r = ICON_PX * dpr / 2.0;
    let paper = theme.scan_palette(false).1;
    let mut out = Vec::new();
    for c in clusters {
        let col = theme.constraint(c.trust);
        let ink = [col[0], col[1], col[2], 1.0];
        let e = emphasis(c);
        let (fill, line) = match e {
            Emphasis::Hovered | Emphasis::Related => (ink, paper),
            _ => ([paper[0], paper[1], paper[2], 0.92], ink),
        };
        let anchor = to_unit(c.at);
        let at = (
            anchor.0 + c.offset.0 * dpr * px,
            anchor.1 + c.offset.1 * dpr * px,
        );
        let p = |x: f64, y: f64| (at.0 + x * px, at.1 + y * px);
        let weight = |w: f64| w * dpr * px;
        let first = out.len();
        // The icon under the pointer draws its own relation between two
        // points, dashed, beneath it.
        if let (true, [m]) = (e == Emphasis::Hovered || connect.all, c.members.as_slice()) {
            let item = connect.items.get(*m);
            if connect.all {
                // A thin connector to every point the constraint binds.
                let faint = [col[0], col[1], col[2], 0.6];
                for t in item.map_or(&[][..], |i| &i.targets[..]) {
                    let t = to_unit(*t);
                    out.push(segment(at, t, weight(1.0), faint, 0.0));
                    out.push(filled_disc(t, 2.8 * dpr * px, ink));
                }
            }
            if let Some((a, b)) = item.and_then(|i| i.relation_line) {
                dashed(
                    to_unit(a),
                    to_unit(b),
                    7.0 * dpr * px,
                    5.0 * dpr * px,
                    weight(1.5),
                    ink,
                    &mut out,
                );
            }
        }
        out.push(filled_disc(at, r * px, fill));
        let ring = if e == Emphasis::Hovered { 2.4 } else { 1.3 };
        out.push(arc(
            at,
            r * px,
            0.0,
            std::f64::consts::TAU,
            weight(ring),
            ink,
        ));
        let g = r * 0.6;
        for part in glyph(c.kind) {
            match part {
                GlyphPart::Line(a, b) => out.push(segment(
                    p(a.0 * g, a.1 * g),
                    p(b.0 * g, b.1 * g),
                    weight(1.5),
                    line,
                    0.0,
                )),
                GlyphPart::Arc(centre, radius, start, sweep) => out.push(arc(
                    p(centre.0 * g, centre.1 * g),
                    radius * g * px,
                    start,
                    sweep,
                    weight(1.5),
                    line,
                )),
                GlyphPart::Disc(centre, radius) => out.push(filled_disc(
                    p(centre.0 * g, centre.1 * g),
                    radius * g * px,
                    line,
                )),
            }
        }
        if c.members.len() > 1 {
            // A badge at the upper right with the count in seven segments.
            let centre = (r * 0.78, -r * 0.78);
            out.push(filled_disc(p(centre.0, centre.1), r * 0.5 * px, ink));
            let digits = count_digits(c.members.len());
            let k = r * 0.2;
            let n_digits = if c.members.len() >= 10 { 2.0 } else { 1.0 };
            let (x0, y0) = (
                centre.0 - ((n_digits - 1.0) * 1.5 + 1.0) * k / 2.0,
                centre.1 - k,
            );
            for (a, b) in digits {
                out.push(segment(
                    p(x0 + a.0 * k, y0 + a.1 * k),
                    p(x0 + b.0 * k, y0 + b.1 * k),
                    weight(1.2),
                    [paper[0], paper[1], paper[2], 1.0],
                    0.0,
                ));
            }
        }
        if e == Emphasis::Dim {
            for inst in &mut out[first..] {
                inst.rgba[3] *= 0.3;
            }
        }
    }
    out
}

/// An item smaller than this on the screen, CSS px, is ringed so a click on
/// it shows.
pub const SMALL_PX: f64 = 12.0;

/// A thin ring round a lit item of `extent_px` CSS px at `centre` (the
/// drawing's units) when it is small; none otherwise.
pub fn halo(
    centre: (f64, f64),
    extent_px: f64,
    zoom: f64,
    dpr: f64,
    rgb: [f32; 3],
) -> Option<Instance> {
    (extent_px < SMALL_PX).then(|| {
        let r = (extent_px / 2.0 + 6.0).max(SMALL_PX * 0.8) * dpr;
        arc(
            centre,
            r / zoom,
            0.0,
            std::f64::consts::TAU,
            1.4 * dpr / zoom,
            [rgb[0], rgb[1], rgb[2], 1.0],
        )
    })
}

/// The connectors of links no constraint states: from the item to where it
/// joins what it relates to, in the bound colour.
pub fn link_connectors<'a>(
    links: impl Iterator<Item = &'a Link>,
    to_unit: &dyn Fn((f64, f64)) -> (f64, f64),
    zoom: f64,
    dpr: f64,
    rgb: [f32; 3],
) -> Vec<Instance> {
    let px = 1.0 / zoom;
    let ink = [rgb[0], rgb[1], rgb[2], 1.0];
    let mut out = Vec::new();
    for l in links {
        let a = to_unit(l.anchor);
        for t in &l.targets {
            let t = to_unit(*t);
            out.push(segment(a, t, dpr * px, [rgb[0], rgb[1], rgb[2], 0.7], 0.0));
            out.push(filled_disc(t, 2.8 * dpr * px, ink));
        }
    }
    out
}

/// A dashed segment from `a` to `b`, dash and gap in the drawing's units.
fn dashed(
    a: (f64, f64),
    b: (f64, f64),
    dash: f64,
    gap: f64,
    weight: f64,
    rgba: [f32; 4],
    out: &mut Vec<Instance>,
) {
    let len = (b.0 - a.0).hypot(b.1 - a.1);
    if len <= 0.0 || dash <= 0.0 {
        return;
    }
    let at = |s: f64| (a.0 + (b.0 - a.0) * s / len, a.1 + (b.1 - a.1) * s / len);
    let mut s = 0.0;
    while s < len {
        let e = (s + dash).min(len);
        out.push(segment(at(s), at(e), weight, rgba, 0.0));
        s = e + gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmsheets_inspector::{ConstraintKind, ConstraintTrust};

    fn cluster(n: usize, kind: ConstraintKind) -> Cluster {
        Cluster {
            at: (10.0, 20.0),
            members: (0..n).collect(),
            kind,
            trust: ConstraintTrust::Inferred,
            offset: (0.0, 0.0),
        }
    }

    #[test]
    fn an_icon_is_the_same_size_on_the_screen_at_any_zoom() {
        let c = [cluster(1, ConstraintKind::Vertical)];
        let extent = |zoom: f64| {
            let inst = build(
                &c,
                Connect {
                    items: &[],
                    all: false,
                },
                &|p| p,
                zoom,
                2.0,
                Theme::Hud,
                &|_| Emphasis::Plain,
            );
            // The ring's radius in device px.
            inst.iter()
                .find(|i| i.k_a[0] == 1.0)
                .map(|i| f64::from(i.b[0]) * zoom)
                .unwrap()
        };
        let (a, b) = (extent(4.0), extent(400.0));
        assert!(
            (a - b).abs() < 1e-3 && (a - ICON_PX).abs() < 1e-3,
            "{a} {b}"
        );
    }

    #[test]
    fn a_cluster_gets_a_badge_and_a_hovered_icon_is_filled() {
        let one = build(
            &[cluster(1, ConstraintKind::Equal)],
            Connect {
                items: &[],
                all: false,
            },
            &|p| p,
            1.0,
            1.0,
            Theme::WhiteLab,
            &|_| Emphasis::Plain,
        );
        let many = build(
            &[cluster(12, ConstraintKind::Equal)],
            Connect {
                items: &[],
                all: false,
            },
            &|p| p,
            1.0,
            1.0,
            Theme::WhiteLab,
            &|_| Emphasis::Plain,
        );
        // The badge: a disc and the seven-segment strokes of "12".
        assert_eq!(many.len(), one.len() + 1 + 2 + 5);
        let hovered = build(
            &[cluster(1, ConstraintKind::Equal)],
            Connect {
                items: &[],
                all: false,
            },
            &|p| p,
            1.0,
            1.0,
            Theme::WhiteLab,
            &|_| Emphasis::Hovered,
        );
        let c = Theme::WhiteLab.constraint(ConstraintTrust::Inferred);
        assert_eq!(hovered[0].rgba[..3], c);
        assert_ne!(one[0].rgba[..3], c, "a plain icon sits on paper");
    }

    #[test]
    fn the_icon_under_the_pointer_draws_its_relation_and_the_others_dim() {
        use lmsheets_inspector::ConstraintItem;
        let item = ConstraintItem {
            id: "h".into(),
            view: None,
            relation: "horizontal_points".into(),
            kind: ConstraintKind::Horizontal,
            trust: ConstraintTrust::Stated,
            level: None,
            detail: String::new(),
            binds: vec![],
            anchor: (50.0, 20.0),
            dimension: false,
            dimension_node: None,
            targets: vec![(0.0, 20.0), (100.0, 20.0)],
            relation_line: Some(((0.0, 20.0), (100.0, 20.0))),
            centre_of: vec![],
        };
        let c = Cluster {
            at: item.anchor,
            members: vec![0],
            kind: ConstraintKind::Horizontal,
            trust: ConstraintTrust::Stated,
            offset: (0.0, 0.0),
        };
        let items = [item];
        let at = |e: Emphasis| {
            build(
                std::slice::from_ref(&c),
                Connect {
                    items: &items,
                    all: false,
                },
                &|p| p,
                4.0,
                1.0,
                Theme::Hud,
                &|_| e,
            )
        };
        let (plain, hovered, dim) = (
            at(Emphasis::Plain),
            at(Emphasis::Hovered),
            at(Emphasis::Dim),
        );
        // A dashed line of many dashes beneath the hovered icon only.
        assert!(
            hovered.len() >= plain.len() + 6,
            "{} vs {}",
            hovered.len(),
            plain.len()
        );
        assert_eq!(dim.len(), plain.len());
        assert!(dim.iter().all(|i| i.rgba[3] < 0.5));
        assert!(
            at(Emphasis::Related)[0].rgba[..3] == hovered[hovered.len() - plain.len()].rgba[..3]
        );
    }

    #[test]
    fn the_connections_view_joins_every_icon_to_what_it_binds() {
        use lmsheets_inspector::ConstraintItem;
        let item = ConstraintItem {
            id: "h".into(),
            view: None,
            relation: "coincident".into(),
            kind: ConstraintKind::Coincident,
            trust: ConstraintTrust::Stated,
            level: None,
            detail: String::new(),
            binds: vec![],
            anchor: (5.0, 5.0),
            dimension: false,
            dimension_node: None,
            targets: vec![(0.0, 0.0), (10.0, 0.0)],
            relation_line: None,
            centre_of: vec![],
        };
        let c = Cluster {
            at: item.anchor,
            members: vec![0],
            kind: ConstraintKind::Coincident,
            trust: ConstraintTrust::Stated,
            offset: (0.0, 0.0),
        };
        let items = [item];
        let at = |all| {
            build(
                std::slice::from_ref(&c),
                Connect { items: &items, all },
                &|p| p,
                4.0,
                1.0,
                Theme::Hud,
                &|_| Emphasis::Plain,
            )
        };
        // Two connectors and two end discs more.
        assert_eq!(at(true).len(), at(false).len() + 4);
    }

    #[test]
    fn a_small_item_is_ringed_and_a_large_one_is_not() {
        let r = |ext| halo((0.0, 0.0), ext, 10.0, 2.0, [1.0, 0.0, 0.0]);
        let ring = r(4.0).expect("a few px long");
        // The ring is wider than the item, in device px.
        assert!(f64::from(ring.b[0]) * 10.0 > 4.0 * 2.0);
        assert!(r(SMALL_PX + 1.0).is_none());
    }
}
