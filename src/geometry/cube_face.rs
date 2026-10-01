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
    /// Every face, in declaration order.
    pub const ALL: [Self; 6] = [
        Self::NegativeX,
        Self::PositiveX,
        Self::NegativeY,
        Self::PositiveY,
        Self::NegativeZ,
        Self::PositiveZ,
    ];

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

    /// World direction in which the face's texture coordinate `u` increases.
    ///
    /// Derived from the UV mapping in `CubeFace::uv`, not chosen independently, so it is the
    /// normal map's tangent-space +X axis:
    ///
    /// | face | `u` | tangent |
    /// | --- | --- | --- |
    /// | `+X` | `1 - z` | `-Z` |
    /// | `-X` | `z` | `+Z` |
    /// | `+Y` | `x` | `+X` |
    /// | `-Y` | `x` | `+X` |
    /// | `+Z` | `x` | `+X` |
    /// | `-Z` | `1 - x` | `-X` |
    pub const fn tangent(self) -> Vec3 {
        match self {
            Self::PositiveX => Vec3::new(0.0, 0.0, -1.0),
            Self::NegativeX => Vec3::new(0.0, 0.0, 1.0),
            Self::PositiveY | Self::NegativeY | Self::PositiveZ => Vec3::new(1.0, 0.0, 0.0),
            Self::NegativeZ => Vec3::new(-1.0, 0.0, 0.0),
        }
    }

    /// World direction in which the face's texture coordinate `v` increases (image down).
    ///
    /// This is the normal map's tangent-space +Y axis. Normal maps here use the image row
    /// direction as +Y, so no axis is flipped between the image and the surface:
    ///
    /// | face | `v` | bitangent |
    /// | --- | --- | --- |
    /// | `±X`, `±Z` | `1 - y` | `-Y` |
    /// | `+Y` | `z` | `+Z` |
    /// | `-Y` | `1 - z` | `-Z` |
    ///
    /// The basis `(tangent, bitangent, normal)` is orthonormal but not always right-handed:
    /// mirrored faces are left-handed. That does not matter here because a height-field normal
    /// `(-dh/du, -dh/dv, 1)` is expressed per-axis along the actual `du` and `dv` directions.
    pub const fn bitangent(self) -> Vec3 {
        match self {
            Self::NegativeX | Self::PositiveX | Self::NegativeZ | Self::PositiveZ => {
                Vec3::new(0.0, -1.0, 0.0)
            }
            Self::PositiveY => Vec3::new(0.0, 0.0, 1.0),
            Self::NegativeY => Vec3::new(0.0, 0.0, -1.0),
        }
    }

    /// Texture coordinates of a point on this face from its box-local coordinates.
    ///
    /// `local` is the point's position inside its box normalized to `[0, 1]` per axis, so the
    /// result is independent of box size and translation. This table is the single UV convention
    /// for every axis-aligned surface, AABB objects and voxels alike. The component along the
    /// face normal is constant on the face and ignored.
    pub fn uv(self, local: Vec3) -> Uv {
        match self {
            Self::PositiveX => Uv::new(1.0 - local.z, 1.0 - local.y),
            Self::NegativeX => Uv::new(local.z, 1.0 - local.y),
            Self::PositiveY => Uv::new(local.x, local.z),
            Self::NegativeY => Uv::new(local.x, 1.0 - local.z),
            Self::PositiveZ => Uv::new(local.x, 1.0 - local.y),
            Self::NegativeZ => Uv::new(1.0 - local.x, 1.0 - local.y),
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
