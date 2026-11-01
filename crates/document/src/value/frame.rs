//! A frame: one scanned image the sheet was read from, and its
//! registration onto the paper (docs/redraw-plan.md §2.2).
//!
//! The registration is a chain of typed maps through five spaces, each
//! map with its provenance and, where it was solved from observations,
//! its residual:
//!
//! ```text
//! frame px ─crop─▶ scan px ─warp─▶ sheet px ─rotation─▶ paper px ─paper─▶ paper mm
//! ```
//!
//! - **frame px**: the pixels of the file as we hold it;
//! - **scan px**: the crop of it the readers saw (the whole frame when
//!   there is no crop);
//! - **sheet px**: the scan with its [`Warp`] taken out (a scanner's
//!   shear, the bow of a long ruled line): a flat sheet, still turned as
//!   scanned;
//! - **paper px**: the sheet turned by its [`Rotation`], so that the
//!   drawing's horizontals and verticals are the axes;
//! - **paper mm**: the paper, by the paper [`Similarity`].
//!
//! Each map has a forward form (towards the paper) and its inverse
//! ([`Map`]); [`Registration::map`] composes them between any two
//! spaces. The chain is the order of the fields, fixed by the types: a
//! list of maps would need its order checked on load to say the same.

use crate::value::claims::{self, Evidence};
use crate::value::geometry::{Paper, P2};
use crate::value::hops::{self, Hop};
use crate::value::provenance::{self, Provenance};
use serde::{Deserialize, Serialize};

id! {
    /// The name of a frame in a document.
    FrameId
}

/// A map of one space of the chain onto the next, and back.
pub trait Map {
    /// A point onward, towards the paper.
    fn forward(&self, p: [f64; 2]) -> [f64; 2];
    /// A point back, towards the frame.
    fn inverse(&self, q: [f64; 2]) -> [f64; 2];
}

/// The spaces of the chain, frame first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Space {
    /// The file's pixels.
    Frame,
    /// The crop's pixels.
    Scan,
    /// The scan with its warp taken out.
    Sheet,
    /// The sheet turned square.
    PaperPx,
    /// The paper, mm.
    Paper,
}

/// The crop: frame px to scan px, `scan = frame − (x0, y0)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crop {
    /// The box kept, frame px: `[x0, y0, x1, y1]`, pixel edges.
    pub box_px: [u32; 4],
}

impl Crop {
    /// The whole of a frame of `size_px` (w, h).
    pub fn whole(size_px: [u32; 2]) -> Self {
        Self {
            box_px: [0, 0, size_px[0], size_px[1]],
        }
    }
}

impl Map for Crop {
    fn forward(&self, p: [f64; 2]) -> [f64; 2] {
        [
            p[0] - f64::from(self.box_px[0]),
            p[1] - f64::from(self.box_px[1]),
        ]
    }

    fn inverse(&self, q: [f64; 2]) -> [f64; 2] {
        [
            q[0] + f64::from(self.box_px[0]),
            q[1] + f64::from(self.box_px[1]),
        ]
    }
}

/// The monomials of the warp's displacement, in normalised coordinates:
/// degrees 2 and 3 (lower degrees are a translation, a scale, a turn and
/// a shear: the first three are the paper's own placement, the rotation
/// and [`Warp::shear`] are kept apart).
pub const MONOMIALS: [(i32, i32); 7] = [(2, 0), (1, 1), (0, 2), (3, 0), (2, 1), (1, 2), (0, 3)];

/// A smooth displacement of the scan from a flat sheet: scan px = sheet
/// px + D(scan px), with D a shear and a cubic polynomial in the scan's
/// coordinates about `centre`, scaled by `half` (the half diagonal), so
/// its coefficients are px at the extent's corners. The default is no
/// warp.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Warp {
    /// The centre of the normalised coordinates, scan px.
    pub centre: [f64; 2],
    /// Their scale: the half diagonal of the extent, px.
    pub half: f64,
    /// x displacement per px of y (a scanner's shear), radians.
    pub shear: f64,
    /// The coefficients of [`MONOMIALS`], px: x's, then y's (empty for
    /// none).
    pub dx: Vec<f64>,
    /// See `dx`.
    pub dy: Vec<f64>,
    /// Where it was solved: the box (x0, y0, x1, y1, scan px) of the ink
    /// it was solved from. Beyond it the polynomial only extrapolates.
    #[serde(default)]
    pub extent: [f64; 4],
}

/// Fixed-point steps the warp's inverse takes at most, and the step
/// below which it has converged, px.
const INVERSE_STEPS: usize = 64;
const INVERSE_TOLERANCE_PX: f64 = 1e-10;

impl Warp {
    /// No warp over `extent` (x0, y0, x1, y1, scan px), to be solved.
    pub fn flat(extent: [f64; 4]) -> Warp {
        let half = [(extent[2] - extent[0]) / 2.0, (extent[3] - extent[1]) / 2.0];
        Warp {
            centre: [(extent[0] + extent[2]) / 2.0, (extent[1] + extent[3]) / 2.0],
            half: half[0].hypot(half[1]).max(1.0),
            shear: 0.0,
            dx: vec![0.0; MONOMIALS.len()],
            dy: vec![0.0; MONOMIALS.len()],
            extent,
        }
    }

    /// No warp: a flat scan.
    pub fn is_none(&self) -> bool {
        self.shear == 0.0 && self.dx.iter().chain(&self.dy).all(|a| *a == 0.0)
    }

    /// The displacement at a scan point, px.
    pub fn displacement(&self, p: [f64; 2]) -> [f64; 2] {
        if self.is_none() {
            return [0.0, 0.0];
        }
        let (u, v) = (
            (p[0] - self.centre[0]) / self.half,
            (p[1] - self.centre[1]) / self.half,
        );
        let mut d = [self.shear * (p[1] - self.centre[1]), 0.0];
        for (k, &(i, j)) in MONOMIALS.iter().enumerate() {
            let m = u.powi(i) * v.powi(j);
            d[0] += self.dx.get(k).copied().unwrap_or(0.0) * m;
            d[1] += self.dy.get(k).copied().unwrap_or(0.0) * m;
        }
        d
    }

    /// Points on a grid over its extent, `n` a side.
    pub fn grid(&self, n: usize) -> Vec<[f64; 2]> {
        let [x0, y0, x1, y1] = self.extent;
        let f = |k: usize| k as f64 / (n - 1).max(1) as f64;
        (0..n)
            .flat_map(|i| (0..n).map(move |j| [x0 + (x1 - x0) * f(i), y0 + (y1 - y0) * f(j)]))
            .collect()
    }

    /// The largest displacement over its extent, px (on a grid). The
    /// shear's share is left out: it is a turn of the verticals, not a
    /// bend, and grows with the sheet.
    pub fn largest(&self) -> f64 {
        let bend = Warp {
            shear: 0.0,
            ..self.clone()
        };
        self.grid(17)
            .into_iter()
            .map(|p| {
                let d = bend.displacement(p);
                d[0].hypot(d[1])
            })
            .fold(0.0, f64::max)
    }

    /// Finite, with a positive scale and a coefficient per monomial
    /// unless it is none.
    pub fn check(&self) -> Result<(), Error> {
        let values = self.centre.iter().chain(&self.extent).chain(&self.dx);
        if !(values.chain(&self.dy).all(|a| a.is_finite()) && self.shear.is_finite()) {
            return Err(Error::NotFinite);
        }
        let sized = |c: &[f64]| c.is_empty() || c.len() == MONOMIALS.len();
        if !(sized(&self.dx) && sized(&self.dy)) {
            return Err(Error::BadWarp);
        }
        let scaled = self.half.is_finite() && self.half > 0.0;
        if !(scaled || self.is_none()) {
            return Err(Error::BadScale(self.half));
        }
        Ok(())
    }
}

/// Scan px to sheet px: `sheet = scan − D(scan)`; back by fixed point
/// (the displacement changes by far less than a pixel per pixel), to
/// convergence.
impl Map for Warp {
    fn forward(&self, p: [f64; 2]) -> [f64; 2] {
        let d = self.displacement(p);
        [p[0] - d[0], p[1] - d[1]]
    }

    fn inverse(&self, q: [f64; 2]) -> [f64; 2] {
        let mut p = q;
        for _ in 0..INVERSE_STEPS {
            let d = self.displacement(p);
            let next = [q[0] + d[0], q[1] + d[1]];
            let step = (next[0] - p[0]).hypot(next[1] - p[1]);
            p = next;
            if step <= INVERSE_TOLERANCE_PX {
                break;
            }
        }
        p
    }
}

/// Sheet px to paper px: a turn by `angle` about `centre`, `paper =
/// centre + R(−angle)(sheet − centre)`; the drawing's x axis runs at
/// `angle` in the sheet (from its x towards its y).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rotation {
    /// The point it turns about, sheet px.
    pub centre: [f64; 2],
    /// The drawing's rotation in the sheet, radians.
    pub angle: f64,
}

impl Map for Rotation {
    fn forward(&self, p: [f64; 2]) -> [f64; 2] {
        let (c, (s, k)) = (self.centre, self.angle.sin_cos());
        let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
        [c[0] + k * dx + s * dy, c[1] - s * dx + k * dy]
    }

    fn inverse(&self, q: [f64; 2]) -> [f64; 2] {
        let (c, (s, k)) = (self.centre, self.angle.sin_cos());
        let (dx, dy) = (q[0] - c[0], q[1] - c[1]);
        [c[0] + k * dx - s * dy, c[1] + s * dx + k * dy]
    }
}

/// Paper px to paper mm: a similarity, `px = origin + px_per_mm ·
/// R(rotation) · mm`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Similarity {
    /// Where the paper's origin lands, px.
    pub origin_px: [f64; 2],
    /// Pixels per paper millimetre.
    pub px_per_mm: f64,
    /// Rotation of the paper in the pixels, radians.
    pub rotation: f64,
}

impl Similarity {
    /// A similarity; finite, with a positive scale.
    pub fn new(origin_px: [f64; 2], px_per_mm: f64, rotation: f64) -> Result<Self, Error> {
        let s = Self {
            origin_px,
            px_per_mm,
            rotation,
        };
        s.check()?;
        Ok(s)
    }

    /// Finite, with a positive scale.
    pub fn check(&self) -> Result<(), Error> {
        if !(self.origin_px.iter().all(|a| a.is_finite()) && self.rotation.is_finite()) {
            return Err(Error::NotFinite);
        }
        if !(self.px_per_mm.is_finite() && self.px_per_mm > 0.0) {
            return Err(Error::BadScale(self.px_per_mm));
        }
        Ok(())
    }
}

impl Map for Similarity {
    fn forward(&self, p: [f64; 2]) -> [f64; 2] {
        let dx = (p[0] - self.origin_px[0]) / self.px_per_mm;
        let dy = (p[1] - self.origin_px[1]) / self.px_per_mm;
        let (s, c) = self.rotation.sin_cos();
        [c * dx + s * dy, -s * dx + c * dy]
    }

    fn inverse(&self, q: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.rotation.sin_cos();
        [
            self.origin_px[0] + self.px_per_mm * (c * q[0] - s * q[1]),
            self.origin_px[1] + self.px_per_mm * (s * q[0] + c * q[1]),
        ]
    }
}

/// How well a solved map fits what it was solved from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Residual {
    /// The observations' RMS residual, px of the map's output space.
    pub rms_px: f64,
    /// How many observations.
    pub samples: u64,
}

/// One map of the chain, with where it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step<M> {
    /// The map.
    pub map: M,
    /// How it was obtained.
    pub provenance: Provenance,
    /// Its fit to the observations it was solved from; none when it was
    /// not solved (exact by construction, or assumed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub residual: Option<Residual>,
}

impl<M> Step<M> {
    /// A step with no residual.
    pub fn new(map: M, provenance: Provenance) -> Self {
        Self {
            map,
            provenance,
            residual: None,
        }
    }

    /// With its residual.
    pub fn residual(mut self, r: Residual) -> Self {
        self.residual = Some(r);
        self
    }
}

/// A frame's registration onto the paper: the chain, frame first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Registration {
    /// Frame px to scan px.
    pub crop: Step<Crop>,
    /// Scan px to sheet px.
    pub warp: Step<Warp>,
    /// Sheet px to paper px.
    pub rotation: Step<Rotation>,
    /// Paper px to paper mm.
    pub paper: Step<Similarity>,
}

impl Registration {
    /// A frame of `size_px` (w, h) registered by a similarity alone: the
    /// whole frame, no warp, no turn, each step under `provenance`.
    pub fn similarity(size_px: [u32; 2], paper: Similarity, provenance: Provenance) -> Self {
        Self {
            crop: Step::new(Crop::whole(size_px), provenance.clone()),
            warp: Step::new(Warp::default(), provenance.clone()),
            rotation: Step::new(Rotation::default(), provenance.clone()),
            paper: Step::new(paper, provenance),
        }
    }

    /// The maps in order: `maps()[k]` takes space `k` to `k + 1`.
    fn maps(&self) -> [&dyn Map; 4] {
        [
            &self.crop.map,
            &self.warp.map,
            &self.rotation.map,
            &self.paper.map,
        ]
    }

    /// A point of space `from` in space `to`: the maps between, forward
    /// towards the paper, inverse back.
    pub fn map(&self, p: [f64; 2], from: Space, to: Space) -> [f64; 2] {
        let (a, b) = (from as usize, to as usize);
        let maps = self.maps();
        if a <= b {
            maps[a..b].iter().fold(p, |p, m| m.forward(p))
        } else {
            maps[b..a].iter().rev().fold(p, |p, m| m.inverse(p))
        }
    }

    /// A point of space `from` on the paper.
    pub fn to_paper(&self, p: [f64; 2], from: Space) -> P2<Paper> {
        let q = self.map(p, from, Space::Paper);
        P2::new(q[0], q[1])
    }

    /// A paper point in space `to`.
    pub fn from_paper(&self, p: P2<Paper>, to: Space) -> [f64; 2] {
        self.map([p.x, p.y], Space::Paper, to)
    }

    /// Paper px per paper mm: the paper similarity's scale (the maps
    /// before it keep the pixel's size, near enough).
    pub fn px_per_mm(&self) -> f64 {
        self.paper.map.px_per_mm
    }

    /// Every map valid within a frame of `size_px` (w, h).
    pub fn check(&self, size_px: [u32; 2]) -> Result<(), Error> {
        let [x0, y0, x1, y1] = self.crop.map.box_px;
        if !(x0 < x1 && y0 < y1 && x1 <= size_px[0] && y1 <= size_px[1]) {
            return Err(Error::BadCrop(self.crop.map.box_px));
        }
        self.warp.map.check()?;
        let r = self.rotation.map;
        if !(r.centre.iter().all(|a| a.is_finite()) && r.angle.is_finite()) {
            return Err(Error::NotFinite);
        }
        self.paper.map.check()?;
        for p in [
            &self.crop.provenance,
            &self.warp.provenance,
            &self.rotation.provenance,
            &self.paper.provenance,
        ] {
            p.check()?;
        }
        let residuals = [
            self.crop.residual,
            self.warp.residual,
            self.rotation.residual,
            self.paper.residual,
        ];
        for r in residuals.into_iter().flatten() {
            if !(r.rms_px.is_finite() && r.rms_px >= 0.0) {
                return Err(Error::BadResidual(r.rms_px));
            }
        }
        Ok(())
    }
}

/// One scanned image: its hops back to the archive, its pixels as we
/// hold them, and its registration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// The image file, a path the server resolves within the archive.
    pub file: String,
    /// Its size, px (w, h).
    pub size_px: [u32; 2],
    /// The file's hash, `algorithm:hex` (`fnv1a64:815e1921c1dd6dd9`),
    /// when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Its registration onto the paper.
    pub registration: Registration,
    /// How its pixels came to us, oldest first ([`hops`]); none when
    /// not recorded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hops: Vec<Hop>,
    /// What its ink was read with, which the claims cite
    /// ([`claims`]); none when not read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Evidence>,
}

impl Frame {
    /// Its registration valid within it, its hops, if any, walking
    /// back from its hash to the archive, and its evidence, if any.
    pub fn check(&self) -> Result<(), Error> {
        self.registration.check(self.size_px)?;
        hops::check(&self.hops, self.hash.as_deref())?;
        if let Some(e) = &self.evidence {
            e.check().map_err(Error::Evidence)?;
        }
        Ok(())
    }

    /// The archive's canonical original of it: its upload's tar member
    /// URL, walking the hops back from its hash.
    pub fn archive_url(&self) -> Result<&str, hops::Error> {
        Ok(&hops::walk(&self.hops, self.hash.as_deref())?.original.url)
    }
}

/// What can be wrong with a frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A value that is NaN or infinite.
    NotFinite,
    /// A scale that is not finite or not positive.
    BadScale(f64),
    /// A crop that is empty or leaves the frame.
    BadCrop([u32; 4]),
    /// A warp's coefficients not one per monomial.
    BadWarp,
    /// A residual that is negative or not finite.
    BadResidual(f64),
    /// A map's provenance fails its checks.
    Provenance(provenance::Error),
    /// The hops are broken.
    Hops(hops::Error),
    /// The evidence is not valid.
    Evidence(claims::Error),
}

impl From<hops::Error> for Error {
    fn from(e: hops::Error) -> Self {
        Error::Hops(e)
    }
}

impl From<provenance::Error> for Error {
    fn from(e: provenance::Error) -> Self {
        Error::Provenance(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::provenance::{Agent, Date, Method};

    fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) <= tol
    }

    const POINTS: [[f64; 2]; 5] = [
        [0.0, 0.0],
        [700.0, 500.0],
        [1400.0, 1000.0],
        [123.0, 877.0],
        [-50.0, 1200.0],
    ];

    /// A warp of a few px, sheared.
    fn warp() -> Warp {
        Warp {
            shear: 0.002,
            dx: vec![3.0, -2.0, 4.0, 1.0, 0.5, -1.0, 2.0],
            dy: vec![-4.0, 1.0, 2.0, -0.5, 1.5, 1.0, -2.0],
            ..Warp::flat([0.0, 0.0, 1400.0, 1000.0])
        }
    }

    fn registration() -> Registration {
        let p = Provenance::new(
            Method::Assumed,
            Agent::Claude,
            Date::new(2026, 10, 26).unwrap(),
        );
        Registration {
            crop: Step::new(
                Crop {
                    box_px: [20, 30, 1420, 1030],
                },
                p.clone(),
            ),
            warp: Step::new(warp(), p.clone()),
            rotation: Step::new(
                Rotation {
                    centre: [700.0, 500.0],
                    angle: -0.4f64.to_radians(),
                },
                p.clone(),
            ),
            paper: Step::new(Similarity::new([5.0, -3.0], 7.874, 0.01).unwrap(), p),
        }
    }

    #[test]
    fn each_map_inverts() {
        let r = registration();
        for m in r.maps() {
            for p in POINTS {
                let there = m.forward(p);
                assert!(near(m.inverse(there), p, 1e-9), "{p:?}");
                assert!(near(m.forward(m.inverse(p)), p, 1e-9), "{p:?}");
            }
        }
    }

    #[test]
    fn the_maps_do_what_they_say() {
        let r = registration();
        assert_eq!(r.crop.map.forward([20.0, 30.0]), [0.0, 0.0]);
        // A level line on the paper runs along the angle in the sheet.
        let rot = r.rotation.map;
        let (a, b) = (rot.inverse([100.0, 500.0]), rot.inverse([900.0, 500.0]));
        assert!(((b[1] - a[1]).atan2(b[0] - a[0]) - rot.angle).abs() < 1e-12);
        // The warp's displacement is the scan's departure from the sheet.
        let p = [1300.0, 90.0];
        let d = r.warp.map.displacement(p);
        assert!(near(r.warp.map.forward(p), [p[0] - d[0], p[1] - d[1]], 0.0));
        assert!(r.warp.map.largest() > 3.0 && !r.warp.map.is_none());
        // The paper's origin is where the similarity puts it.
        assert_eq!(r.paper.map.inverse([0.0, 0.0]), [5.0, -3.0]);
    }

    #[test]
    fn the_chain_composes_both_ways() {
        let r = registration();
        let spaces = [
            Space::Frame,
            Space::Scan,
            Space::Sheet,
            Space::PaperPx,
            Space::Paper,
        ];
        for p in POINTS {
            let mm = r.to_paper(p, Space::Frame);
            assert!(near(r.from_paper(mm, Space::Frame), p, 1e-9), "{p:?}");
            // Between any two spaces (the frame's point in the first):
            // there and back, and by way of the paper.
            for (a, b) in spaces
                .iter()
                .flat_map(|a| spaces.iter().map(move |b| (*a, *b)))
            {
                let at = r.map(p, Space::Frame, a);
                let q = r.map(at, a, b);
                assert!(near(r.map(q, b, a), at, 1e-9), "{a:?} {b:?}");
                let via = r.map(r.map(at, a, Space::Paper), Space::Paper, b);
                assert!(near(via, q, 1e-9), "{a:?} {b:?}");
            }
        }
        assert_eq!(r.map([3.0, 4.0], Space::Sheet, Space::Sheet), [3.0, 4.0]);
    }

    #[test]
    fn the_frame_walks_back_to_the_archive() {
        let h = "fnv1a64:815e1921c1dd6dd9";
        let mut f = Frame {
            file: "1374.png".into(),
            size_px: [1500, 1100],
            hash: Some(h.into()),
            registration: registration(),
            hops: crate::value::hops::tests::trail(h),
            evidence: None,
        };
        assert_eq!(f.check(), Ok(()));
        assert_eq!(
            f.archive_url(),
            Ok("https://archive.org/download/box511/box511.tar/box511/1374.png")
        );
        f.hash = Some("fnv1a64:0".into());
        assert_eq!(f.check(), Err(Error::Hops(hops::Error::Unlinked(3))));
        f.hops.clear();
        assert_eq!(f.check(), Ok(()));
        assert_eq!(f.archive_url(), Err(hops::Error::Empty));
    }

    #[test]
    fn bad_maps_are_refused() {
        let mut r = registration();
        assert_eq!(r.check([1500, 1100]), Ok(()));
        assert_eq!(
            r.check([1400, 1100]),
            Err(Error::BadCrop([20, 30, 1420, 1030]))
        );
        r.warp.map.dx.pop();
        assert_eq!(r.check([1500, 1100]), Err(Error::BadWarp));
        r.warp.map = Warp::default();
        assert_eq!(r.check([1500, 1100]), Ok(()));
        r.rotation.map.angle = f64::NAN;
        assert_eq!(r.check([1500, 1100]), Err(Error::NotFinite));
        r.rotation.map.angle = 0.0;
        r.paper.residual = Some(Residual {
            rms_px: -1.0,
            samples: 3,
        });
        assert_eq!(r.check([1500, 1100]), Err(Error::BadResidual(-1.0)));
        assert_eq!(
            Similarity::new([0.0, 0.0], 0.0, 0.0),
            Err(Error::BadScale(0.0))
        );
    }
}
