//! The registration chain the panes are locked through, and its loader.
//!
//! - **scan to paper** (crop px to paper mm) is the closed form: the
//!   document's own [`Registration`] evaluated forward, as the bundle
//!   manifest says (`registration.scan_to_paper`).
//! - **paper to scan** is the bundle's `registration.bin` grid, interpolated
//!   bilinearly (the warp's inverse is a fixed point; the grid saves the
//!   viewer iterating). Outside the grid the edge cell extrapolates linearly,
//!   so panning past the sheet stays continuous.
//!
//! The model holds an already-loaded [`Chain`]; the loaders here take the
//! bundle's bytes ([`Chain::from_bundle`], usable on the web after a fetch)
//! or a directory ([`Chain::load`]).

use lmsheets_document::value::frame::{Registration, Space};
use serde_json::Value;
use std::fmt;

/// Why a chain could not be built.
#[derive(Debug, PartialEq)]
pub struct ChainError(pub String);

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ChainError {}

fn err<T>(s: impl Into<String>) -> Result<T, ChainError> {
    Err(ChainError(s.into()))
}

/// Paper mm to crop px on a regular grid, bilinear.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    /// The paper point of cell (0, 0), mm.
    pub origin: [f64; 2],
    /// The cell size, mm.
    pub step: f64,
    /// Columns and rows of points.
    pub n: [usize; 2],
    /// The crop px of each point, row-major.
    pub cells: Vec<[f32; 2]>,
}

impl Grid {
    /// A grid by sampling `f` (paper mm to crop px) at every point, with the
    /// f32 rounding the file format has.
    pub fn from_fn(
        origin: [f64; 2],
        step: f64,
        n: [usize; 2],
        f: impl Fn([f64; 2]) -> [f64; 2],
    ) -> Self {
        let mut cells = Vec::with_capacity(n[0] * n[1]);
        for j in 0..n[1] {
            for i in 0..n[0] {
                let v = f([origin[0] + i as f64 * step, origin[1] + j as f64 * step]);
                cells.push([v[0] as f32, v[1] as f32]);
            }
        }
        Self {
            origin,
            step,
            n,
            cells,
        }
    }

    /// From `registration.bin` (little-endian f32 pairs, row-major) and the
    /// manifest's `registration.grid` description.
    pub fn from_bundle(desc: &Value, bytes: &[u8]) -> Result<Self, ChainError> {
        let num = |k: &str| desc[k].as_f64().ok_or(ChainError(format!("grid.{k}")));
        let origin = &desc["origin_mm"];
        let origin = [
            origin[0]
                .as_f64()
                .ok_or(ChainError("grid.origin_mm".into()))?,
            origin[1]
                .as_f64()
                .ok_or(ChainError("grid.origin_mm".into()))?,
        ];
        let n = [num("nx")? as usize, num("ny")? as usize];
        let step = num("step_mm")?;
        if n[0] < 2 || n[1] < 2 || step.is_nan() || step <= 0.0 {
            return err("grid: fewer than 2 x 2 points, or no step");
        }
        if bytes.len() != n[0] * n[1] * 8 {
            return err(format!(
                "registration.bin: {} bytes, the grid needs {}",
                bytes.len(),
                n[0] * n[1] * 8
            ));
        }
        let f = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let cells = bytes
            .chunks_exact(8)
            .map(|c| [f(&c[0..4]), f(&c[4..8])])
            .collect();
        Ok(Self {
            origin,
            step,
            n,
            cells,
        })
    }

    /// The crop px of a paper point, mm.
    pub fn at(&self, mm: [f64; 2]) -> [f64; 2] {
        let u = [
            (mm[0] - self.origin[0]) / self.step,
            (mm[1] - self.origin[1]) / self.step,
        ];
        let i = (u[0].floor().max(0.0) as usize).min(self.n[0] - 2);
        let j = (u[1].floor().max(0.0) as usize).min(self.n[1] - 2);
        let (fx, fy) = (u[0] - i as f64, u[1] - j as f64);
        let c = |a: usize, b: usize| self.cells[b * self.n[0] + a];
        let lerp = |k: usize| {
            f64::from(c(i, j)[k]) * (1.0 - fx) * (1.0 - fy)
                + f64::from(c(i + 1, j)[k]) * fx * (1.0 - fy)
                + f64::from(c(i, j + 1)[k]) * (1.0 - fx) * fy
                + f64::from(c(i + 1, j + 1)[k]) * fx * fy
        };
        [lerp(0), lerp(1)]
    }
}

/// Half the step of the central difference for the warp's local scale,
/// crop px.
const JACOBIAN_H: f64 = 0.5;

/// The registration between the paper and the scan's crop pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    registration: Registration,
    /// The frame pixel of the crop's top-left (manifest `crop.offset`).
    offset: [f64; 2],
    /// The crop's size, px.
    extent: [f64; 2],
    grid: Grid,
}

impl Chain {
    /// A chain from its parts: the document's registration of the frame, the
    /// crop's offset in the frame and its size, and the paper to scan grid.
    pub fn new(registration: Registration, offset: [f64; 2], extent: [f64; 2], grid: Grid) -> Self {
        Self {
            registration,
            offset,
            extent,
            grid,
        }
    }

    /// From a bundle's `manifest.json`, `document.json` and
    /// `registration.bin`. The inspector reads the bundle's files, never
    /// blobsketch's code.
    pub fn from_bundle(
        manifest: &str,
        document: &str,
        registration_bin: &[u8],
    ) -> Result<Self, ChainError> {
        let parse = |what: &str, s: &str| {
            serde_json::from_str::<Value>(s).map_err(|e| ChainError(format!("{what}: {e}")))
        };
        let (m, d) = (
            parse("manifest.json", manifest)?,
            parse("document.json", document)?,
        );
        let pair = |v: &Value, what: &str| -> Result<[f64; 2], ChainError> {
            match (v[0].as_f64(), v[1].as_f64()) {
                (Some(a), Some(b)) => Ok([a, b]),
                _ => err(format!("manifest.json: {what}")),
            }
        };
        let offset = pair(&m["crop"]["offset"], "crop.offset")?;
        let extent = pair(&m["crop"]["size"], "crop.size")?;
        let reg = &d["frames"]["scan"]["registration"];
        if reg.is_null() {
            return err("document.json: no frames.scan.registration");
        }
        let registration: Registration = serde_json::from_value(reg.clone())
            .map_err(|e| ChainError(format!("document.json registration: {e}")))?;
        let grid = Grid::from_bundle(&m["registration"]["grid"], registration_bin)?;
        Ok(Self::new(registration, offset, extent, grid))
    }

    /// From a bundle directory (native).
    pub fn load(dir: &std::path::Path) -> Result<Self, ChainError> {
        let read = |name: &str| {
            std::fs::read(dir.join(name))
                .map_err(|e| ChainError(format!("{}: {e}", dir.join(name).display())))
        };
        let text = |name: &str| {
            String::from_utf8(read(name)?).map_err(|e| ChainError(format!("{name}: {e}")))
        };
        Self::from_bundle(
            &text("manifest.json")?,
            &text("document.json")?,
            &read("registration.bin")?,
        )
    }

    /// The crop's size, px.
    pub fn extent(&self) -> (f64, f64) {
        (self.extent[0], self.extent[1])
    }

    /// The paper grid's description, for tests and reports.
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// Paper mm of a crop pixel: the closed form, through the document's
    /// registration chain.
    pub fn scan_to_paper(&self, crop: (f64, f64)) -> (f64, f64) {
        let p = self.registration.to_paper(
            [crop.0 + self.offset[0], crop.1 + self.offset[1]],
            Space::Frame,
        );
        (p.x, p.y)
    }

    /// The crop pixel of a paper point, mm: the bundle's grid.
    pub fn paper_to_scan(&self, mm: (f64, f64)) -> (f64, f64) {
        let c = self.grid.at([mm.0, mm.1]);
        (c[0], c[1])
    }

    /// Crop px per paper mm at a paper point: the square root of the
    /// area scale of the closed form there (central differences on the
    /// smooth scan to paper map, inverted), so it is the warp's local
    /// scale and not the grid's piecewise-constant slope.
    pub fn scan_per_mm(&self, mm: (f64, f64)) -> f64 {
        let c = self.paper_to_scan(mm);
        let h = JACOBIAN_H;
        let (xp, xm) = (
            self.scan_to_paper((c.0 + h, c.1)),
            self.scan_to_paper((c.0 - h, c.1)),
        );
        let (yp, ym) = (
            self.scan_to_paper((c.0, c.1 + h)),
            self.scan_to_paper((c.0, c.1 - h)),
        );
        let (a, b) = ((xp.0 - xm.0) / (2.0 * h), (yp.0 - ym.0) / (2.0 * h));
        let (c2, d) = ((xp.1 - xm.1) / (2.0 * h), (yp.1 - ym.1) / (2.0 * h));
        // mm per crop px is sqrt|det J|; crop px per mm is its reciprocal.
        1.0 / (a * d - b * c2).abs().sqrt()
    }

    /// The paper box `(min, max)`, mm, that holds the crop's four corners.
    pub fn paper_bounds(&self) -> ((f64, f64), (f64, f64)) {
        let (w, h) = (self.extent[0], self.extent[1]);
        let pts = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|c| self.scan_to_paper(c));
        let fold = |f: fn(f64, f64) -> f64, k: fn(&(f64, f64)) -> f64, s: f64| {
            pts.iter().map(k).fold(s, f)
        };
        (
            (
                fold(f64::min, |p| p.0, f64::MAX),
                fold(f64::min, |p| p.1, f64::MAX),
            ),
            (
                fold(f64::max, |p| p.0, f64::MIN),
                fold(f64::max, |p| p.1, f64::MIN),
            ),
        )
    }
}
