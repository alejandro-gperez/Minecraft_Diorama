//! Designed EggWars architecture on top of the generated arena terrain: four bridges in their
//! narrative states, a small base with an obsidian egg defense on each outer island, and a ruined
//! central structure.
//!
//! Construction runs once, after terrain generation, as a pure function of the terrain:
//!
//! 1. read anchors (the first free cell above a column's topmost terrain block) from the
//!    terrain-only grid;
//! 2. plan every block of every bridge and structure from those anchors;
//! 3. validate the whole plan: every cell inside the grid, empty, and planned once;
//! 4. only then write it.
//!
//! Construction never edits terrain, and an error leaves the grid untouched. Because the plan
//! reads only terrain, no element depends on another element or on the order they are planned or
//! written. The composition is fixed by design: a seed reaches it only through terrain heights
//! and silhouettes and a few small optional variations drawn from stateless hashes, none of
//! which can change a bridge's or a defense's narrative state.

use std::{error::Error, fmt};

use super::{
    ArenaError, ArenaInfo, ArenaLayout, IslandBounds, IslandId, arena::SURFACE_LEVEL,
    generate_arena_in,
};
use crate::voxel::{BlockType, Voxel, VoxelGrid, VoxelPosition};

mod bridges;
mod structures;
#[cfg(test)]
mod tests;

pub use bridges::{BridgeInfo, BridgeLane, BridgeState};
pub use structures::{
    CentralStructure, DEFENSE_SLOTS, DEFENSE_TEMPLATE, DefenseSlot, DefenseState, EggBase,
    EggBaseDesign, LavaSpill,
};

/// The four outer islands, in `IslandId::ALL` order: one bridge and one base each.
pub const OUTER_ISLANDS: [IslandId; 4] = [
    IslandId::North,
    IslandId::South,
    IslandId::East,
    IslandId::West,
];

/// Level of every bridge deck: the islands' shared flat grass level, so a deck runs level with
/// the grass it leaves and reaches.
pub const DECK_Y: i32 = SURFACE_LEVEL;
/// The one layer of cobblestone floor that structures lay on the flat grass.
pub const FLOOR_Y: i32 = SURFACE_LEVEL + 1;
/// Level of every future egg and the eight defense blocks around it, standing on the base floor.
pub const EGG_Y: i32 = FLOOR_Y + 1;

/// Highest cell any construction occupies: the central structure's tallest pillar, four blocks
/// above its floor and capped with obsidian. Everything else is lower: outer gate pillars reach
/// `SURFACE_LEVEL + 3`, defenses (their caps one above `EGG_Y`) and eggs `EGG_Y`, bridges `DECK_Y`. The global grid's top
/// layer is exactly this level.
pub const CONSTRUCTION_TOP: i32 = structures::CENTRAL_TOP;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructionError {
    /// The terrain arena could not be generated.
    Arena(ArenaError),
    /// A column the design builds on holds no terrain.
    MissingTerrain { x: i32, z: i32 },
    /// A planned block lies outside the grid.
    OutsideGrid(VoxelPosition),
    /// A planned block would replace terrain or another planned block.
    Occupied(VoxelPosition),
}

impl fmt::Display for ConstructionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arena(error) => write!(formatter, "{error}"),
            Self::MissingTerrain { x, z } => {
                write!(formatter, "construction column ({x}, {z}) holds no terrain")
            }
            Self::OutsideGrid(p) => write!(
                formatter,
                "construction block ({}, {}, {}) lies outside the grid",
                p.x, p.y, p.z
            ),
            Self::Occupied(p) => write!(
                formatter,
                "construction block ({}, {}, {}) would replace an existing block",
                p.x, p.y, p.z
            ),
        }
    }
}

impl Error for ConstructionError {}

/// Everything construction placed, plus the anchors later decorations build on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstructionInfo {
    /// In `OUTER_ISLANDS` order.
    pub bridges: [BridgeInfo; 4],
    /// In `OUTER_ISLANDS` order.
    pub bases: [EggBase; 4],
    pub central: CentralStructure,
    /// Grass level measured at each island's center column, indexed by `IslandId as usize`.
    pub surface_levels: [i32; 5],
    counts: [usize; BlockType::ALL.len()],
}

impl ConstructionInfo {
    /// The bridge from `island` toward the center; `None` for the center itself.
    pub fn bridge(&self, island: IslandId) -> Option<&BridgeInfo> {
        self.bridges.iter().find(|bridge| bridge.island == island)
    }

    /// The egg base of `island`; `None` for the center itself.
    pub fn base(&self, island: IslandId) -> Option<&EggBase> {
        self.bases.iter().find(|base| base.island == island)
    }

    /// Blocks of `block` type that construction placed.
    pub const fn count(&self, block: BlockType) -> usize {
        self.counts[block as usize]
    }

    /// Every block construction placed.
    pub fn total(&self) -> usize {
        self.counts.iter().sum()
    }
}

/// The constructed EggWars arena: the one global grid holding terrain and construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EggWarsArena {
    pub grid: VoxelGrid,
    /// The terrain stage, exactly as `generate_arena` reports it apart from the taller grid.
    pub terrain: ArenaInfo,
    pub construction: ConstructionInfo,
}

impl EggWarsArena {
    /// Terrain and construction blocks together.
    pub fn occupied_voxels(&self) -> usize {
        self.terrain.occupied_voxels + self.construction.total()
    }
}

/// The global grid of the constructed arena: the terrain layout's bounds, raised to
/// `CONSTRUCTION_TOP`. Bridges run between the islands inside the layout's horizontal extent,
/// so only the top grows.
pub fn construction_bounds(layout: &ArenaLayout) -> IslandBounds {
    let bounds = layout.bounds();
    IslandBounds {
        min: bounds.min,
        max: VoxelPosition::new(
            bounds.max.x,
            bounds.max.y.max(CONSTRUCTION_TOP),
            bounds.max.z,
        ),
    }
}

/// Generates the EggWars arena terrain for `global_seed` into the construction grid, then builds
/// the designed bridges and structures on it.
pub fn build_eggwars_arena(global_seed: u64) -> Result<EggWarsArena, ConstructionError> {
    let layout = ArenaLayout::eggwars();
    let mut arena = generate_arena_in(&layout, global_seed, construction_bounds(&layout))
        .map_err(ConstructionError::Arena)?;
    let construction = construct(&mut arena.grid, global_seed)?;
    Ok(EggWarsArena {
        grid: arena.grid,
        terrain: arena.info,
        construction,
    })
}

/// Plans, validates, and writes all construction onto the EggWars terrain in `grid`.
fn construct(
    grid: &mut VoxelGrid,
    global_seed: u64,
) -> Result<ConstructionInfo, ConstructionError> {
    let (plan, info) = plan_construction(grid, global_seed)?;
    plan.validate(grid)?;
    plan.write(grid);
    Ok(info)
}

/// Every block of the design for the terrain in `grid`, and the metadata describing it.
fn plan_construction(
    grid: &VoxelGrid,
    global_seed: u64,
) -> Result<(Plan, ConstructionInfo), ConstructionError> {
    let layout = ArenaLayout::eggwars();
    let terrain = Terrain(grid);
    let mut plan = Plan::default();

    let surface_levels = per_island(IslandId::ALL, |id| {
        let center = layout.config(id).center;
        Ok(terrain.anchor(center.x, center.z)?.y - 1)
    })?;
    let bridges = per_island(OUTER_ISLANDS, |id| {
        bridges::plan_bridge(&terrain, &layout, id, global_seed, &mut plan)
    })?;
    let bases = per_island(OUTER_ISLANDS, |id| {
        structures::plan_base(&terrain, &layout, id, &mut plan)
    })?;
    let central = structures::plan_central(&terrain, &mut plan)?;

    let mut counts = [0; BlockType::ALL.len()];
    for &(_, block) in &plan.blocks {
        counts[block as usize] += 1;
    }
    let info = ConstructionInfo {
        bridges,
        bases,
        central,
        surface_levels,
        counts,
    };
    Ok((plan, info))
}

fn per_island<T: Copy, const N: usize>(
    islands: [IslandId; N],
    mut build: impl FnMut(IslandId) -> Result<T, ConstructionError>,
) -> Result<[T; N], ConstructionError> {
    let mut results = [None; N];
    for (result, id) in results.iter_mut().zip(islands) {
        *result = Some(build(id)?);
    }
    Ok(results.map(|result| result.expect("every island was built above")))
}

/// Read-only view of the terrain-only grid that construction builds on.
#[derive(Clone, Copy)]
struct Terrain<'a>(&'a VoxelGrid);

impl Terrain<'_> {
    /// The buildable cell of column `(x, z)`; see `surface_anchor`.
    fn anchor(self, x: i32, z: i32) -> Result<VoxelPosition, ConstructionError> {
        surface_anchor(self.0, x, z).ok_or(ConstructionError::MissingTerrain { x, z })
    }

    /// Like `anchor`, but `None` for a column of open void.
    fn anchor_if_land(self, x: i32, z: i32) -> Option<VoxelPosition> {
        surface_anchor(self.0, x, z)
    }
}

/// The first free cell directly above the topmost block of column `(x, z)`: where a structure
/// standing on that column starts. `None` if the column lies outside the grid, holds no block,
/// or its top block is in the grid's top layer.
///
/// Construction reads anchors from the terrain-only grid, so the topmost block is terrain and the
/// anchor rests on actual ground at whatever height that column's grass has, never inside it.
pub fn surface_anchor(grid: &VoxelGrid, x: i32, z: i32) -> Option<VoxelPosition> {
    let origin = grid.origin();
    let top = (0..grid.height() as i32)
        .rev()
        .map(|y| VoxelPosition::new(x, origin.y + y, z))
        .find(|&position| grid.is_occupied_world(position))?;
    let anchor = VoxelPosition::new(x, top.y + 1, z);
    grid.contains_world(anchor).then_some(anchor)
}

/// The horizontal plane seen from an outer island, so one design serves all four islands.
///
/// Local cell `(side, forward)` counts `forward` cells from the island's center line toward the
/// arena center and `side` cells across. Cells rotate a quarter turn at a time about the island's
/// center point (a cell corner, since footprints are even), so a design keeps its exact shape and
/// handedness on every island: `(0, f)` and `(-1, f)` straddle the center line.
#[derive(Clone, Copy, Debug)]
struct Frame {
    center_x: i32,
    center_z: i32,
    /// Unit step toward the arena center.
    forward: (i32, i32),
}

impl Frame {
    fn facing_center(layout: &ArenaLayout, island: IslandId) -> Self {
        let center = layout.config(island).center;
        let (dx, dz) = island.direction();
        Self {
            center_x: center.x,
            center_z: center.z,
            forward: (-dx, -dz),
        }
    }

    /// World `(x, z)` of local cell `(side, forward)`.
    fn cell(self, side: i32, forward: i32) -> (i32, i32) {
        let (fx, fz) = self.forward;
        // `side` is `forward` rotated a quarter turn about the vertical axis, the same way on
        // every island; North's `side` is +X.
        let (sx, sz) = (fz, -fx);
        // Along a negative axis, cell `k` of the half that the axis points into is `c - 1 - k`.
        let step = |center: i32, unit: i32, k: i32| {
            if unit > 0 { center + k } else { center - 1 - k }
        };
        let axis = |center: i32, side_unit: i32, forward_unit: i32| {
            if side_unit != 0 {
                step(center, side_unit, side)
            } else {
                step(center, forward_unit, forward)
            }
        };
        (axis(self.center_x, sx, fx), axis(self.center_z, sz, fz))
    }
}

/// The planned blocks, in planning order.
#[derive(Debug, Default)]
struct Plan {
    blocks: Vec<(VoxelPosition, BlockType)>,
}

impl Plan {
    fn put(&mut self, position: VoxelPosition, block: BlockType) {
        self.blocks.push((position, block));
    }

    /// `block` in column `(x, z)` from `bottom` up to `top`, inclusive; nothing if `bottom > top`.
    fn column(&mut self, (x, z): (i32, i32), bottom: i32, top: i32, block: BlockType) {
        for y in bottom..=top {
            self.put(VoxelPosition::new(x, y, z), block);
        }
    }

    /// Every block lies inside the grid, in an empty cell, and is planned exactly once.
    fn validate(&self, grid: &VoxelGrid) -> Result<(), ConstructionError> {
        let mut planned = vec![false; grid.volume()];
        let origin = grid.origin();
        let (width, depth) = (grid.width(), grid.depth());
        for &(position, _) in &self.blocks {
            match grid.get_world(position) {
                None => return Err(ConstructionError::OutsideGrid(position)),
                Some(Voxel::Block(_)) => return Err(ConstructionError::Occupied(position)),
                Some(Voxel::Empty) => {}
            }
            let local = |value: i32, min: i32| (value - min) as usize;
            let index = local(position.x, origin.x)
                + width * (local(position.z, origin.z) + depth * local(position.y, origin.y));
            if std::mem::replace(&mut planned[index], true) {
                return Err(ConstructionError::Occupied(position));
            }
        }
        Ok(())
    }

    fn write(&self, grid: &mut VoxelGrid) {
        for &(position, block) in &self.blocks {
            grid.set_world(position, Voxel::Block(block))
                .expect("validated construction lies inside the grid");
        }
    }
}
