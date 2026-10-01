use std::{error::Error, fmt};

use super::{BlockType, LocalVoxelPosition, VoxelPosition};

/// Contents of one voxel cell: empty space or exactly one block.
///
/// One byte: `BlockType` is `repr(u8)` with few variants, so `Empty` fits in an unused
/// discriminant value without a separate tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Voxel {
    #[default]
    Empty,
    Block(BlockType),
}

impl Voxel {
    pub const fn block(self) -> Option<BlockType> {
        match self {
            Self::Empty => None,
            Self::Block(block) => Some(block),
        }
    }

    pub const fn is_occupied(self) -> bool {
        matches!(self, Self::Block(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoxelGridError {
    /// Width, height, or depth is zero.
    ZeroDimension,
    /// `width * height * depth` does not fit in `usize`.
    VolumeOverflow,
    /// The grid's last cell on some axis lies beyond the `i32` world-coordinate range.
    WorldRangeOverflow,
    /// The voxel storage exceeds the maximum allocation size or could not be allocated.
    AllocationFailed,
}

impl fmt::Display for VoxelGridError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroDimension => "voxel grid dimensions must all be positive",
            Self::VolumeOverflow => "voxel grid volume overflows usize",
            Self::WorldRangeOverflow => "voxel grid extends beyond the i32 world-coordinate range",
            Self::AllocationFailed => "voxel grid storage could not be allocated",
        };
        formatter.write_str(message)
    }
}

impl Error for VoxelGridError {}

/// A write addressed a coordinate outside the grid; the grid was left unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelOutOfBounds;

impl fmt::Display for VoxelOutOfBounds {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("voxel coordinate lies outside the grid")
    }
}

impl Error for VoxelOutOfBounds {}

/// Dense, contiguous block-identity storage for a box of integer world voxel cells.
///
/// Local `(0, 0, 0)` is world voxel `origin`; in general `world = origin + local`, and the grid
/// covers world voxels `origin.x .. origin.x + width` (and likewise for Y with `height` and Z with
/// `depth`). The origin may be negative so the diorama can be centered on the world origin. World
/// voxel `(x, y, z)` owns the unit cell `[x, x + 1) × [y, y + 1) × [z, z + 1)`.
///
/// Storage is one `Vec<Voxel>` with X changing fastest, then Z, then Y:
///
/// ```text
/// index = x + width * (z + depth * y)
/// ```
///
/// so `+X` neighbors are adjacent, `+Z` neighbors are `width` apart, and each horizontal layer
/// of constant Y is one contiguous `width * depth` run.
///
/// The grid stores identity only: no per-voxel geometry, material, or visibility. Fully enclosed
/// voxels stay stored like any other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoxelGrid {
    origin: VoxelPosition,
    width: usize,
    height: usize,
    depth: usize,
    voxels: Vec<Voxel>,
}

impl VoxelGrid {
    /// Creates an all-empty grid.
    pub fn try_new(
        origin: VoxelPosition,
        width: usize,
        height: usize,
        depth: usize,
    ) -> Result<Self, VoxelGridError> {
        if width == 0 || height == 0 || depth == 0 {
            return Err(VoxelGridError::ZeroDimension);
        }
        let volume = width
            .checked_mul(height)
            .and_then(|area| area.checked_mul(depth))
            .ok_or(VoxelGridError::VolumeOverflow)?;
        // Every valid local coordinate must map to a representable world coordinate.
        if !axis_fits_world(origin.x, width)
            || !axis_fits_world(origin.y, height)
            || !axis_fits_world(origin.z, depth)
        {
            return Err(VoxelGridError::WorldRangeOverflow);
        }

        let mut voxels = Vec::new();
        voxels
            .try_reserve_exact(volume)
            .map_err(|_| VoxelGridError::AllocationFailed)?;
        voxels.resize(volume, Voxel::Empty);

        Ok(Self {
            origin,
            width,
            height,
            depth,
            voxels,
        })
    }

    pub const fn origin(&self) -> VoxelPosition {
        self.origin
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// Number of voxel cells, `width * height * depth`.
    pub fn volume(&self) -> usize {
        self.voxels.len()
    }

    /// Converts a world voxel coordinate to this grid's local coordinate, or `None` outside it.
    pub fn world_to_local(&self, world: VoxelPosition) -> Option<LocalVoxelPosition> {
        Some(LocalVoxelPosition::new(
            local_axis(world.x, self.origin.x, self.width)?,
            local_axis(world.y, self.origin.y, self.height)?,
            local_axis(world.z, self.origin.z, self.depth)?,
        ))
    }

    /// Converts a local coordinate to its world voxel coordinate, or `None` outside the grid.
    pub fn local_to_world(&self, local: LocalVoxelPosition) -> Option<VoxelPosition> {
        if !self.contains_local(local) {
            return None;
        }
        Some(VoxelPosition::new(
            world_axis(self.origin.x, local.x)?,
            world_axis(self.origin.y, local.y)?,
            world_axis(self.origin.z, local.z)?,
        ))
    }

    pub fn contains_world(&self, world: VoxelPosition) -> bool {
        self.world_to_local(world).is_some()
    }

    pub const fn contains_local(&self, local: LocalVoxelPosition) -> bool {
        local.x < self.width && local.y < self.height && local.z < self.depth
    }

    /// Voxel at a world coordinate, or `None` outside the grid.
    pub fn get_world(&self, world: VoxelPosition) -> Option<Voxel> {
        let local = self.world_to_local(world)?;
        self.voxels.get(self.linear_index(local)).copied()
    }

    /// Voxel at a local coordinate, or `None` outside the grid.
    pub fn get_local(&self, local: LocalVoxelPosition) -> Option<Voxel> {
        self.index(local)
            .and_then(|index| self.voxels.get(index))
            .copied()
    }

    /// Writes a voxel at a world coordinate; outside the grid nothing changes.
    pub fn set_world(
        &mut self,
        world: VoxelPosition,
        voxel: Voxel,
    ) -> Result<(), VoxelOutOfBounds> {
        let local = self.world_to_local(world).ok_or(VoxelOutOfBounds)?;
        self.set_local(local, voxel)
    }

    /// Writes a voxel at a local coordinate; outside the grid nothing changes.
    pub fn set_local(
        &mut self,
        local: LocalVoxelPosition,
        voxel: Voxel,
    ) -> Result<(), VoxelOutOfBounds> {
        let index = self.index(local).ok_or(VoxelOutOfBounds)?;
        let slot = self.voxels.get_mut(index).ok_or(VoxelOutOfBounds)?;
        *slot = voxel;
        Ok(())
    }

    /// Whether a block occupies a world coordinate. Space outside the grid is unoccupied.
    pub fn is_occupied_world(&self, world: VoxelPosition) -> bool {
        self.get_world(world).is_some_and(Voxel::is_occupied)
    }

    /// Sets every voxel to the same value.
    pub fn fill(&mut self, voxel: Voxel) {
        self.voxels.fill(voxel);
    }

    fn index(&self, local: LocalVoxelPosition) -> Option<usize> {
        self.contains_local(local).then(|| self.linear_index(local))
    }

    /// Storage index of an in-bounds local coordinate; see the type-level layout description.
    ///
    /// Cannot overflow for an in-bounds coordinate: the result is below the validated volume.
    const fn linear_index(&self, local: LocalVoxelPosition) -> usize {
        local.x + self.width * (local.z + self.depth * local.y)
    }
}

/// Whether the last cell `origin + extent - 1` of a positive extent is an `i32` world coordinate.
fn axis_fits_world(origin: i32, extent: usize) -> bool {
    i64::try_from(extent - 1)
        .ok()
        .and_then(|last_offset| i64::from(origin).checked_add(last_offset))
        .is_some_and(|last| last <= i64::from(i32::MAX))
}

/// Offset of `world` from `origin` along one axis when it lies in `0..extent`.
///
/// The difference is taken in `i64`, where no pair of `i32` values overflows, and a negative
/// offset is rejected by `try_from` instead of wrapping to a huge `usize`.
fn local_axis(world: i32, origin: i32, extent: usize) -> Option<usize> {
    let offset = usize::try_from(i64::from(world) - i64::from(origin)).ok()?;
    (offset < extent).then_some(offset)
}

fn world_axis(origin: i32, offset: usize) -> Option<i32> {
    let offset = i64::try_from(offset).ok()?;
    i32::try_from(i64::from(origin).checked_add(offset)?).ok()
}

#[cfg(test)]
mod tests {
    use super::{Voxel, VoxelGrid, VoxelGridError, VoxelOutOfBounds};
    use crate::voxel::{BlockType, LocalVoxelPosition, VoxelPosition};

    fn world(x: i32, y: i32, z: i32) -> VoxelPosition {
        VoxelPosition::new(x, y, z)
    }

    fn local(x: usize, y: usize, z: usize) -> LocalVoxelPosition {
        LocalVoxelPosition::new(x, y, z)
    }

    fn grid(origin: VoxelPosition, width: usize, height: usize, depth: usize) -> VoxelGrid {
        VoxelGrid::try_new(origin, width, height, depth).unwrap()
    }

    /// The local coordinates of a grid in storage order.
    fn local_positions(grid: &VoxelGrid) -> impl Iterator<Item = LocalVoxelPosition> + '_ {
        (0..grid.height()).flat_map(move |y| {
            (0..grid.depth()).flat_map(move |z| (0..grid.width()).map(move |x| local(x, y, z)))
        })
    }

    #[test]
    fn voxel_representation_is_one_byte() {
        assert_eq!(size_of::<Voxel>(), 1);
        assert_eq!(size_of::<Voxel>(), size_of::<BlockType>());
    }

    #[test]
    fn voxel_reports_its_block() {
        assert_eq!(Voxel::Empty.block(), None);
        assert!(!Voxel::Empty.is_occupied());
        assert_eq!(Voxel::default(), Voxel::Empty);
        for block in BlockType::ALL {
            assert_eq!(Voxel::Block(block).block(), Some(block));
            assert!(Voxel::Block(block).is_occupied());
        }
    }

    #[test]
    fn valid_grid_retains_origin_and_dimensions() {
        let origin = world(-32, -8, -32);
        let grid = grid(origin, 64, 32, 48);

        assert_eq!(grid.origin(), origin);
        assert_eq!(grid.width(), 64);
        assert_eq!(grid.height(), 32);
        assert_eq!(grid.depth(), 48);
        assert_eq!(grid.volume(), 64 * 32 * 48);
    }

    #[test]
    fn new_grid_is_empty_with_one_slot_per_voxel() {
        let grid = grid(world(3, -2, 5), 4, 3, 5);

        assert_eq!(grid.volume(), 60);
        assert_eq!(grid.voxels.len(), 60);
        assert!(local_positions(&grid).all(|p| grid.get_local(p) == Some(Voxel::Empty)));
    }

    #[test]
    fn single_voxel_grid_is_valid() {
        let mut grid = grid(world(-1, -1, -1), 1, 1, 1);

        assert_eq!(grid.volume(), 1);
        grid.set_world(world(-1, -1, -1), Voxel::Block(BlockType::Lava))
            .unwrap();
        assert_eq!(
            grid.get_world(world(-1, -1, -1)),
            Some(Voxel::Block(BlockType::Lava))
        );
    }

    #[test]
    fn zero_dimensions_are_rejected() {
        let origin = world(0, 0, 0);
        for (width, height, depth) in [(0, 4, 4), (4, 0, 4), (4, 4, 0), (0, 0, 0)] {
            assert_eq!(
                VoxelGrid::try_new(origin, width, height, depth),
                Err(VoxelGridError::ZeroDimension)
            );
        }
    }

    #[test]
    fn volume_overflow_is_rejected() {
        let origin = world(i32::MIN, i32::MIN, i32::MIN);
        for (width, height, depth) in [
            (usize::MAX, 2, 1),
            (2, usize::MAX, 1),
            (1, 2, usize::MAX),
            (1 << 31, 1 << 31, 1 << 31),
        ] {
            assert_eq!(
                VoxelGrid::try_new(origin, width, height, depth),
                Err(VoxelGridError::VolumeOverflow)
            );
        }
    }

    #[test]
    fn grids_beyond_the_world_coordinate_range_are_rejected() {
        let edge = world(i32::MAX, i32::MAX, i32::MAX);
        assert!(VoxelGrid::try_new(edge, 1, 1, 1).is_ok());
        for (width, height, depth) in [(2, 1, 1), (1, 2, 1), (1, 1, 2)] {
            assert_eq!(
                VoxelGrid::try_new(edge, width, height, depth),
                Err(VoxelGridError::WorldRangeOverflow)
            );
        }
        assert_eq!(
            VoxelGrid::try_new(world(i32::MIN, 0, 0), (1 << 32) + 1, 1, 1),
            Err(VoxelGridError::WorldRangeOverflow)
        );
    }

    #[test]
    fn storage_beyond_the_allocation_limit_fails_without_allocating() {
        // 2^63 one-byte voxels exceed `isize::MAX` bytes, which the allocator rejects up front.
        let origin = world(i32::MIN, i32::MIN, i32::MIN);
        assert_eq!(
            VoxelGrid::try_new(origin, 1 << 32, 1 << 31, 1),
            Err(VoxelGridError::AllocationFailed)
        );
    }

    #[test]
    fn local_origin_maps_to_world_origin() {
        let origin = world(-32, -8, -32);
        let grid = grid(origin, 64, 32, 64);

        assert_eq!(grid.world_to_local(origin), Some(local(0, 0, 0)));
        assert_eq!(grid.local_to_world(local(0, 0, 0)), Some(origin));
    }

    #[test]
    fn world_to_local_subtracts_the_origin() {
        let grid = grid(world(-32, -8, -32), 64, 32, 64);

        assert_eq!(
            grid.world_to_local(world(-1, -1, -1)),
            Some(local(31, 7, 31))
        );
        assert_eq!(grid.world_to_local(world(0, 0, 0)), Some(local(32, 8, 32)));
        assert_eq!(
            grid.world_to_local(world(5, 20, -30)),
            Some(local(37, 28, 2))
        );
        assert_eq!(
            grid.world_to_local(world(31, 23, 31)),
            Some(local(63, 31, 63))
        );
    }

    #[test]
    fn world_and_local_conversions_round_trip_for_every_voxel() {
        let grid = grid(world(-3, 2, -1), 4, 3, 5);

        for position in local_positions(&grid) {
            let world = grid.local_to_world(position).unwrap();
            assert_eq!(grid.world_to_local(world), Some(position));
        }
    }

    #[test]
    fn boundaries_are_inclusive_below_and_exclusive_above() {
        let grid = grid(world(-4, -2, -6), 8, 4, 12);
        let min = world(-4, -2, -6);
        let max = world(3, 1, 5);

        assert!(grid.contains_world(min));
        assert!(grid.contains_world(max));
        for outside in [
            world(min.x - 1, min.y, min.z),
            world(min.x, min.y - 1, min.z),
            world(min.x, min.y, min.z - 1),
            world(max.x + 1, max.y, max.z),
            world(max.x, max.y + 1, max.z),
            world(max.x, max.y, max.z + 1),
        ] {
            assert!(!grid.contains_world(outside), "{outside:?}");
            assert_eq!(grid.world_to_local(outside), None);
            assert_eq!(grid.get_world(outside), None);
        }
    }

    #[test]
    fn local_coordinates_outside_the_dimensions_are_rejected() {
        let grid = grid(world(0, 0, 0), 2, 3, 4);

        assert!(grid.contains_local(local(1, 2, 3)));
        for outside in [local(2, 0, 0), local(0, 3, 0), local(0, 0, 4)] {
            assert!(!grid.contains_local(outside));
            assert_eq!(grid.local_to_world(outside), None);
            assert_eq!(grid.get_local(outside), None);
        }
    }

    #[test]
    fn negative_offsets_never_wrap_into_the_grid() {
        let at_origin = grid(world(0, 0, 0), 4, 4, 4);
        for below in [world(-1, 0, 0), world(0, -1, 0), world(0, 0, -1)] {
            assert_eq!(at_origin.world_to_local(below), None);
        }
        assert_eq!(at_origin.world_to_local(world(i32::MIN, 0, 0)), None);

        // The extreme differences overflow `i32` subtraction in either direction.
        let at_max = grid(world(i32::MAX, i32::MAX, i32::MAX), 1, 1, 1);
        assert_eq!(
            at_max.world_to_local(world(i32::MIN, i32::MIN, i32::MIN)),
            None
        );
        let at_min = grid(world(i32::MIN, i32::MIN, i32::MIN), 1, 1, 1);
        assert_eq!(
            at_min.world_to_local(world(i32::MAX, i32::MAX, i32::MAX)),
            None
        );
        assert_eq!(
            at_min.world_to_local(world(i32::MIN, i32::MIN, i32::MIN)),
            Some(local(0, 0, 0))
        );
    }

    #[test]
    fn layout_puts_x_fastest_then_z_then_y() {
        let grid = grid(world(0, 0, 0), 4, 3, 5);
        let origin = grid.index(local(0, 0, 0)).unwrap();

        assert_eq!(origin, 0);
        assert_eq!(grid.index(local(1, 0, 0)), Some(origin + 1));
        assert_eq!(grid.index(local(0, 0, 1)), Some(origin + 4));
        assert_eq!(grid.index(local(0, 1, 0)), Some(origin + 4 * 5));
        assert_eq!(grid.index(local(3, 0, 0)), Some(3));
        assert_eq!(grid.index(local(0, 0, 4)), Some(16));
        assert_eq!(grid.index(local(2, 2, 3)), Some(2 + 4 * (3 + 5 * 2)));
        assert_eq!(grid.index(local(3, 2, 4)), Some(grid.volume() - 1));
    }

    #[test]
    fn every_valid_coordinate_has_a_distinct_index() {
        let grid = grid(world(-2, -1, -3), 4, 3, 5);

        let indices: Vec<usize> = local_positions(&grid)
            .map(|position| grid.index(position).unwrap())
            .collect();
        assert_eq!(indices, (0..grid.volume()).collect::<Vec<_>>());
    }

    #[test]
    fn set_then_get_returns_the_block_in_both_spaces() {
        let mut grid = grid(world(-8, -4, -8), 16, 8, 16);
        let position = world(2, -3, -7);
        let block = Voxel::Block(BlockType::Cobblestone);

        grid.set_world(position, block).unwrap();

        assert_eq!(grid.get_world(position), Some(block));
        assert_eq!(grid.get_local(local(10, 1, 1)), Some(block));
        assert!(grid.is_occupied_world(position));
    }

    #[test]
    fn writes_overwrite_and_clear_voxels() {
        let mut grid = grid(world(0, 0, 0), 4, 4, 4);
        let position = local(1, 2, 3);

        grid.set_local(position, Voxel::Block(BlockType::Grass))
            .unwrap();
        grid.set_local(position, Voxel::Block(BlockType::Obsidian))
            .unwrap();
        assert_eq!(
            grid.get_local(position),
            Some(Voxel::Block(BlockType::Obsidian))
        );

        grid.set_local(position, Voxel::Empty).unwrap();
        assert_eq!(grid.get_local(position), Some(Voxel::Empty));
        assert!(!grid.is_occupied_world(world(1, 2, 3)));
    }

    #[test]
    fn out_of_bounds_reads_and_writes_fail_without_changing_the_grid() {
        let mut grid = grid(world(-2, -2, -2), 4, 4, 4);
        let original = grid.clone();
        let block = Voxel::Block(BlockType::Glass);

        assert_eq!(grid.get_world(world(2, 0, 0)), None);
        assert_eq!(grid.get_local(local(4, 0, 0)), None);
        assert_eq!(grid.set_world(world(2, 0, 0), block), Err(VoxelOutOfBounds));
        assert_eq!(
            grid.set_world(world(-3, 0, 0), block),
            Err(VoxelOutOfBounds)
        );
        assert_eq!(grid.set_local(local(0, 4, 0), block), Err(VoxelOutOfBounds));
        assert_eq!(grid, original);
        assert!(!grid.is_occupied_world(world(2, 0, 0)));
    }

    #[test]
    fn every_block_type_is_stored_and_read_back() {
        let mut grid = grid(world(-4, 0, 0), 9, 1, 1);

        for (x, block) in (-4..).zip(BlockType::ALL) {
            grid.set_world(world(x, 0, 0), Voxel::Block(block)).unwrap();
        }
        for (x, block) in (-4..).zip(BlockType::ALL) {
            assert_eq!(grid.get_world(world(x, 0, 0)), Some(Voxel::Block(block)));
        }
    }

    #[test]
    fn fill_sets_every_voxel() {
        let mut grid = grid(world(0, 0, 0), 3, 2, 4);

        grid.fill(Voxel::Block(BlockType::Cobblestone));
        assert!(
            local_positions(&grid)
                .all(|p| grid.get_local(p) == Some(Voxel::Block(BlockType::Cobblestone)))
        );

        grid.fill(Voxel::Empty);
        assert!(local_positions(&grid).all(|p| grid.get_local(p) == Some(Voxel::Empty)));
    }

    #[test]
    fn grid_centered_on_world_origin_maps_cells_around_it() {
        // 16 x 8 x 16 cells spanning world voxels -8..8, -4..4, -8..8.
        let mut grid = grid(world(-8, -4, -8), 16, 8, 16);

        assert_eq!(grid.world_to_local(world(0, 0, 0)), Some(local(8, 4, 8)));
        assert_eq!(grid.world_to_local(world(-1, -1, -1)), Some(local(7, 3, 7)));
        assert_eq!(grid.world_to_local(world(-8, -4, -8)), Some(local(0, 0, 0)));
        assert_eq!(grid.world_to_local(world(7, 3, 7)), Some(local(15, 7, 15)));
        assert!(!grid.contains_world(world(8, 0, 0)));
        assert!(!grid.contains_world(world(-9, 0, 0)));
        assert!(!grid.contains_world(world(0, 4, 0)));
        assert!(!grid.contains_world(world(0, -5, 0)));

        grid.set_world(world(0, 0, 0), Voxel::Block(BlockType::Lava))
            .unwrap();
        assert!(grid.is_occupied_world(world(0, 0, 0)));
        for neighbor in [
            world(-1, 0, 0),
            world(1, 0, 0),
            world(0, -1, 0),
            world(0, 1, 0),
            world(0, 0, -1),
            world(0, 0, 1),
        ] {
            assert_eq!(grid.get_world(neighbor), Some(Voxel::Empty), "{neighbor:?}");
        }
    }

    #[test]
    fn interior_voxels_remain_stored() {
        let mut grid = grid(world(-1, -1, -1), 3, 3, 3);
        grid.fill(Voxel::Block(BlockType::Cobblestone));
        grid.set_world(world(0, 0, 0), Voxel::Block(BlockType::DiamondOre))
            .unwrap();

        // The center voxel is fully enclosed but keeps its own identity.
        assert_eq!(
            grid.get_world(world(0, 0, 0)),
            Some(Voxel::Block(BlockType::DiamondOre))
        );
    }
}
