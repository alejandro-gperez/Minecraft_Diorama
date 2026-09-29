use crate::math::Vec3;

/// One of the six outward-facing orientations of an axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CubeFace {
    NegativeX,
    PositiveX,
    NegativeY,
    PositiveY,
    NegativeZ,
    PositiveZ,
}

impl CubeFace {
    pub const fn normal(self) -> Vec3 {
        match self {
            Self::NegativeX => Vec3::new(-1.0, 0.0, 0.0),
            Self::PositiveX => Vec3::new(1.0, 0.0, 0.0),
            Self::NegativeY => Vec3::new(0.0, -1.0, 0.0),
            Self::PositiveY => Vec3::new(0.0, 1.0, 0.0),
            Self::NegativeZ => Vec3::new(0.0, 0.0, -1.0),
            Self::PositiveZ => Vec3::new(0.0, 0.0, 1.0),
        }
    }
}

/// Normalized texture coordinates, with `u` increasing right and `v` increasing down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Uv {
    pub u: f32,
    pub v: f32,
}

impl Uv {
    pub const fn new(u: f32, v: f32) -> Self {
        Self { u, v }
    }
}
