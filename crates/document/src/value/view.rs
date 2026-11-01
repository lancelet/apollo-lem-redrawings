//! A view: a sketch in the model's coordinates along two named model axes,
//! placed on the paper.
//!
//! The sketch's coordinates are millimetres along the view's two model
//! axes, so a feature's model X is the same number in every view that
//! shows X; where two views sit on the page, and how large, is the
//! `Transform`, a similarity from the view's plane to the paper. Whether
//! the sketch's coordinates really are the model's is a claim with
//! provenance, `model_frame`: made when a station on the sheet fixes
//! them, absent (coordinates known up to an offset) until then. The
//! sketch itself knows nothing of any of this.

use crate::value::geometry::{Model, Paper, Rect, P2};
use crate::value::provenance::Provenance;
use crate::value::sketch::Sketch;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// The name of a view, an interned `Copy` handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ViewId(Ustr);

impl ViewId {
    /// A name.
    pub fn new(name: &str) -> Self {
        Self(Ustr::from(name))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&ViewId> for ViewId {
    fn from(id: &ViewId) -> Self {
        *id
    }
}

impl From<&str> for ViewId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for ViewId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An axis of the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAxis {
    /// The model's X.
    X,
    /// The model's Y.
    Y,
    /// The model's Z.
    Z,
}

impl ModelAxis {
    /// The axis's letter, as a station callout writes it.
    pub fn letter(self) -> &'static str {
        match self {
            ModelAxis::X => "X",
            ModelAxis::Y => "Y",
            ModelAxis::Z => "Z",
        }
    }
}

/// Where a view sits on the paper: a similarity from the view's plane to
/// the sheet. `paper = origin + scale · R(rotation) · M · model`, with `M`
/// a mirror in y when `mirror` is set (a model plane whose second axis
/// points up the page).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Where the view's origin lands, mm on the paper.
    pub origin: P2<Paper>,
    /// Paper millimetres per model millimetre (`1.0` for FULL).
    pub scale: f64,
    /// Rotation of the view on the page, radians.
    pub rotation: f64,
    /// Whether the view's y axis points up the page.
    pub mirror: bool,
}

/// What can be wrong with a transform or a view.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A scale that is not finite or not positive.
    BadScale(f64),
    /// A value that is NaN or infinite.
    NotFinite,
    /// The two axes of a view are the same.
    SameAxes(ModelAxis),
    /// The frame claim's provenance fails its checks.
    Provenance(crate::value::provenance::Error),
}

impl From<crate::value::provenance::Error> for Error {
    fn from(e: crate::value::provenance::Error) -> Self {
        Error::Provenance(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

impl Transform {
    /// A transform, checked: finite values, positive scale.
    pub fn new(origin: P2<Paper>, scale: f64, rotation: f64, mirror: bool) -> Result<Self, Error> {
        let t = Self {
            origin,
            scale,
            rotation,
            mirror,
        };
        t.check()?;
        Ok(t)
    }

    /// Full scale, unrotated, y down, origin at `(x, y)` mm on the paper;
    /// checked when the view is added or edited.
    pub fn at(x: f64, y: f64) -> Self {
        Self {
            origin: P2::new(x, y),
            scale: 1.0,
            rotation: 0.0,
            mirror: false,
        }
    }

    /// Finite values, positive scale.
    pub fn check(&self) -> Result<(), Error> {
        if !(self.origin.x.is_finite() && self.origin.y.is_finite() && self.rotation.is_finite()) {
            return Err(Error::NotFinite);
        }
        if !(self.scale.is_finite() && self.scale > 0.0) {
            return Err(Error::BadScale(self.scale));
        }
        Ok(())
    }

    /// A model point on the paper.
    pub fn to_paper(&self, p: P2<Model>) -> P2<Paper> {
        let y = if self.mirror { -p.y } else { p.y };
        let (s, c) = self.rotation.sin_cos();
        P2::new(
            self.origin.x + self.scale * (c * p.x - s * y),
            self.origin.y + self.scale * (s * p.x + c * y),
        )
    }

    /// A paper point in the view's plane.
    pub fn to_model(&self, p: P2<Paper>) -> P2<Model> {
        let dx = (p.x - self.origin.x) / self.scale;
        let dy = (p.y - self.origin.y) / self.scale;
        let (s, c) = self.rotation.sin_cos();
        let x = c * dx + s * dy;
        let y = -s * dx + c * dy;
        P2::new(x, if self.mirror { -y } else { y })
    }
}

/// A view of the model: a sketch along two model axes, a transform onto
/// the paper, and perhaps a claim that the sketch's coordinates are the
/// model's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    /// The model axes the sketch's x and y run along.
    pub axes: (ModelAxis, ModelAxis),
    /// The claim that sketch coordinates are model coordinates, with why
    /// we believe it; `None` while they are known only up to an offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_frame: Option<Provenance>,
    /// The geometry and constraints of the view.
    pub sketch: Sketch<Model>,
    /// Where it sits on the paper.
    pub transform: Transform,
    /// The region of the paper the view occupies, once laid out: what the
    /// cursor is over, and where new entities go.
    #[serde(default)]
    pub region: Option<Rect<Paper>>,
}

impl View {
    /// A view with no frame claim; the two axes must differ and the
    /// transform must be valid.
    pub fn new(
        axes: (ModelAxis, ModelAxis),
        sketch: Sketch<Model>,
        transform: Transform,
    ) -> Result<Self, Error> {
        let v = Self {
            axes,
            model_frame: None,
            sketch,
            transform,
            region: None,
        };
        v.check()?;
        Ok(v)
    }

    /// The same view claiming model coordinates, on this evidence.
    pub fn claim_model(mut self, provenance: Provenance) -> Result<Self, Error> {
        provenance.check()?;
        self.model_frame = Some(provenance);
        Ok(self)
    }

    /// Everything a view must satisfy on its own: distinct axes, a frame
    /// claim whose provenance passes its checks, a valid transform. The
    /// store runs this on every view in, including from a file.
    pub fn check(&self) -> Result<(), Error> {
        if self.axes.0 == self.axes.1 {
            return Err(Error::SameAxes(self.axes.0));
        }
        if let Some(p) = &self.model_frame {
            p.check()?;
        }
        self.transform.check()
    }

    /// The same view with its region set.
    pub fn with_region(mut self, region: Rect<Paper>) -> Self {
        self.region = Some(region);
        self
    }

    /// Which of the view's sketch axes (`0` for x, `1` for y) shows the
    /// model axis, if either does.
    pub fn axis_index(&self, axis: ModelAxis) -> Option<usize> {
        if self.axes.0 == axis {
            Some(0)
        } else if self.axes.1 == axis {
            Some(1)
        } else {
            None
        }
    }

    /// The model axis two views share, if any.
    pub fn shared_axis(&self, other: &View) -> Option<ModelAxis> {
        [ModelAxis::X, ModelAxis::Y, ModelAxis::Z]
            .into_iter()
            .find(|a| self.axis_index(*a).is_some() && other.axis_index(*a).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn transform_round_trips() {
        let p = Transform::new(P2::new(100.0, 200.0), 0.5, FRAC_PI_2, true).unwrap();
        for m in [P2::new(0.0, 0.0), P2::new(10.0, 0.0), P2::new(3.0, -4.0)] {
            let back = p.to_model(p.to_paper(m));
            assert_abs_diff_eq!(back, m);
        }
        // The origin lands at the origin; +x model turns to +y paper at a
        // quarter turn, at half scale.
        assert_abs_diff_eq!(p.to_paper(P2::new(0.0, 0.0)), P2::new(100.0, 200.0));
        assert_abs_diff_eq!(p.to_paper(P2::new(10.0, 0.0)), P2::new(100.0, 205.0));
        // Mirror: model +y goes up the page (−y) before the rotation.
        let q = Transform::new(P2::new(0.0, 0.0), 1.0, 0.0, true).unwrap();
        assert_abs_diff_eq!(q.to_paper(P2::new(0.0, 1.0)), P2::new(0.0, -1.0));
        assert_eq!(
            Transform::new(P2::new(0.0, 0.0), 0.0, 0.0, false),
            Err(Error::BadScale(0.0))
        );
        assert_eq!(
            Transform::new(P2::new(f64::NAN, 0.0), 1.0, 0.0, false),
            Err(Error::NotFinite)
        );
    }
}
