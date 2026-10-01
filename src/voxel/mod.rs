//! Logical Minecraft-style voxel world: block identity, integer voxel coordinates, and dense
//! occupancy storage.
//!
//! The grid stores block identity only. It owns no geometry and no materials: a voxel is never
//! turned into an `Aabb`, and a block resolves to its optical behavior through a lightweight
//! `BlockType -> MaterialId` mapping. The renderer does not query the grid yet; future 3D DDA
//! traversal will step through its integer cells directly.

pub mod block;
pub mod grid;
pub mod position;

pub use block::{BlockMaterials, BlockType, OreMaterials};
pub use grid::{Voxel, VoxelGrid, VoxelGridError, VoxelOutOfBounds};
pub use position::{LocalVoxelPosition, VoxelPosition};
