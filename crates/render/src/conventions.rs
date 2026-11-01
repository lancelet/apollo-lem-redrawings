//! Rendering conventions every writer shares: how strokes are capped and
//! joined, and how dash patterns are laid along a curve. Not document
//! data; the renderer's agreed way of drawing it, carried in the styled
//! scene so SVG, PDF and the GPU cannot disagree.

/// The shape of a stroke's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    /// Cut square at the end point.
    Butt,
    /// A half-disc beyond the end point.
    Round,
    /// Extended by half the weight, square.
    Square,
}

/// The shape where two segments of a stroke meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// A sharp corner.
    Miter,
    /// A rounded corner.
    Round,
    /// A cut corner.
    Bevel,
}

/// How a dash pattern is laid along a curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashFit {
    /// Scaled so an open curve starts and ends on a dash and a closed one
    /// takes whole periods: the draughtsman's convention.
    EndsOnDash,
    /// The pattern as given from the curve's start, unscaled.
    Plain,
}

/// The conventions in force.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conventions {
    /// Stroke ends.
    pub cap: Cap,
    /// Stroke corners.
    pub join: Join,
    /// Dash placement.
    pub dash_fit: DashFit,
    /// How far a dash reaches past a crossing with another curve on
    /// either side, mm, so no gap sits on a line and no dash ends on one.
    pub dash_clear: f64,
    /// The shortest a long dash may be stretched or squeezed to, as a
    /// share of the pattern's; below it a gap group is left out instead.
    pub long_min: f64,
}

impl Default for Conventions {
    /// Round caps and joins (ink on paper), dashes ending on a dash and
    /// clearing every crossing by 1.2 mm, long dashes never under half.
    fn default() -> Self {
        Self {
            cap: Cap::Round,
            join: Join::Round,
            dash_fit: DashFit::EndsOnDash,
            dash_clear: 1.2,
            long_min: 0.5,
        }
    }
}
