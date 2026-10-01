//! Logical Minecraft-style voxel world: block identity, integer voxel coordinates, and dense
//! occupancy storage.
//!
//! The grid stores block identity only. It owns no geometry and no materials: a voxel is never
//! turned into an `Aabb`, and a block resolves to its optical behavior through a lightweight
//! `BlockType -> MaterialId` mapping. `VoxelGrid::intersect` is the 3D DDA traversal: it steps
//! through the grid's integer cells directly and reports the first occupied surface as a
//! `VoxelHit`, which normalizes into the same `SceneHit` that AABB objects produce. The renderer
//! and `Scene` queries do not call it yet.

pub mod block;
pub mod grid;
pub mod hit;
pub mod position;
mod traversal;

pub use block::{BlockMaterials, BlockType};
pub use grid::{Voxel, VoxelGrid, VoxelGridError, VoxelOutOfBounds};
pub use hit::VoxelHit;
pub use position::{LocalVoxelPosition, VoxelPosition};
