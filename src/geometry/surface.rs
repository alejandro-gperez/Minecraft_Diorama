use crate::math::Vec3;

use super::{CubeFace, Uv};

/// Where a ray met an axis-aligned surface: the geometry the renderer shades.
///
/// Every geometry source reports this same value, so shading does not depend on how the hit was
/// found: `Aabb::intersect` produces it for arbitrary scene objects, and `VoxelHit` carries one
/// for voxel faces. `normal` is always the outward geometric normal of `face`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceHit {
    pub t: f32,
    pub position: Vec3,
    pub normal: Vec3,
    pub face: CubeFace,
    /// Absent only when a dimension required by the selected face is exactly degenerate.
    pub uv: Option<Uv>,
}
