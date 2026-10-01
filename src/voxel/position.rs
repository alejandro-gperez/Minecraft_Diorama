use crate::math::Vec3;

/// Integer world-space voxel coordinate.
///
/// Axes follow the project convention: `+X` right/east, `+Y` up, `+Z` forward/south. The voxel at
/// `(x, y, z)` owns the half-open unit cell `[x, x + 1) × [y, y + 1) × [z, z + 1)` in world space,
/// so its identity is its minimum corner, not its center. Negative coordinates are ordinary
/// positions; this is a cell index, not a floating-point world point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl VoxelPosition {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// World-space minimum corner of this voxel's cell, `(x, y, z)` as floating point.
    ///
    /// Exact while every component's magnitude is at most `2^24`, far beyond any diorama grid.
    pub const fn min_corner(self) -> Vec3 {
        Vec3::new(self.x as f32, self.y as f32, self.z as f32)
    }
}

/// Coordinate relative to a grid's origin, in `0..width`, `0..height`, `0..depth` when valid.
///
/// Unsigned components make negative local coordinates unrepresentable. Grid accessors still
/// check the upper bounds, so a local position from one grid cannot index past another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalVoxelPosition {
    pub x: usize,
    pub y: usize,
    pub z: usize,
}

impl LocalVoxelPosition {
    pub const fn new(x: usize, y: usize, z: usize) -> Self {
        Self { x, y, z }
    }
}
