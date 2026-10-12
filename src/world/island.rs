//! Deterministic Minecraft-style floating-island generation into an existing `VoxelGrid`.
//!
//! An island is planned as one vertical run of blocks per footprint column, then written in a
//! single pass. Each column is grass on top, a shallow dirt layer, and a cobblestone body down to
//! its bottom; exposed cobblestone may become ore. Every value comes from `noise` in local
//! footprint coordinates, so the same seed and config reproduce the same blocks at any world
//! position, and one feature's randomness never shifts another's (see `noise` for the strategy).
//!
//! Shape invariants the plan guarantees by construction:
//!
//! - every column contains the level just below the nominal surface, so a 4-connected footprint
//!   gives one face-connected landmass with no floating fragments or cavities;
//! - the silhouette boundary depends on direction only (star-shaped around the center), so the
//!   footprint has no detached cells;
//! - below the lowest possible grass, each layer is a subset of the layer above it: the underside
//!   tapers monotonically toward its tip.

use std::{error::Error, fmt};

use super::noise::{
    Domain, hash3, periodic_noise_1d, signed, turn_fraction, unit_interval, value_noise_2d,
};
use crate::voxel::{BlockType, Voxel, VoxelGrid, VoxelPosition};

#[cfg(test)]
mod tests;

/// Footprint side length of a standard outer island, the course's 16×16 terrain requirement.
pub const DEFAULT_FOOTPRINT: u32 = 16;

/// Thinnest valid island: a rim column needs grass, dirt, and rock below the nominal surface.
pub const MIN_THICKNESS: u32 = (RIM_DEPTH + 1) as u32;

/// Highest a grass surface may rise above the configured surface level.
pub const MAX_SURFACE_RISE: i32 = 1;

/// Blocks below the surface level that every column reaches, so even the rim has dirt and rock.
const RIM_DEPTH: i32 = 4;

// Silhouette: the boundary radius, in units of the footprint's half extent, as a function of
// direction. Two smooth periodic octaves keep it organic without spikes; the extremes stay inside
// `[SILHOUETTE_MIN_RADIUS, SILHOUETTE_MAX_RADIUS]`, so corners are always empty and the center
// region always land.
const SILHOUETTE_BASE_RADIUS: f32 = 0.96;
const SILHOUETTE_BROAD_PERIOD: u32 = 5;
const SILHOUETTE_BROAD_AMPLITUDE: f32 = 0.12;
const SILHOUETTE_DETAIL_PERIOD: u32 = 9;
const SILHOUETTE_DETAIL_AMPLITUDE: f32 = 0.05;
pub const SILHOUETTE_MIN_RADIUS: f32 =
    SILHOUETTE_BASE_RADIUS - SILHOUETTE_BROAD_AMPLITUDE - SILHOUETTE_DETAIL_AMPLITUDE;
pub const SILHOUETTE_MAX_RADIUS: f32 =
    SILHOUETTE_BASE_RADIUS + SILHOUETTE_BROAD_AMPLITUDE + SILHOUETTE_DETAIL_AMPLITUDE;

// Surface: the inner part of the island stays exactly at the surface level for future structures.
// Outside it, coherent noise patches rise one block in a middle ring and sink one near the rim, so
// the rim never stands above the land behind it.
/// Fraction of the boundary radius that is always flat.
pub const FLAT_CENTER_FRACTION: f32 = 0.45;
const RISE_END_FRACTION: f32 = 0.85;
const RIM_DROP_FRACTION: f32 = 0.8;
const SURFACE_NOISE_SCALE: f32 = 4.0;
const SURFACE_RISE_THRESHOLD: f32 = 0.62;
const SURFACE_DROP_THRESHOLD: f32 = 0.38;

// Dirt: one to three blocks under the grass in smooth patches, mostly one or two.
const DIRT_NOISE_SCALE: f32 = 4.0;
const DIRT_DEPTH_RANGE: f32 = 2.2;

// Underside: depth below the surface level falls from the full thickness under a slightly
// off-center tip to `RIM_DEPTH` at the boundary, along a profile between a cone and a spike,
// with smooth jitter that fades toward the rim.
const TIP_MAX_OFFSET: f32 = 0.2;
const UNDERSIDE_NOISE_SCALE: f32 = 3.0;
const UNDERSIDE_JITTER: f32 = 1.5;
const UNDERSIDE_CONE_WEIGHT: f32 = 0.4;

/// Edge length of the cubic cells that share one ore decision, which groups ore into small
/// clusters without any order-dependent spreading.
const ORE_CLUSTER_SIZE: i32 = 2;
/// Per-cluster probability of each ore and the probability that an eligible block inside a chosen
/// cluster becomes that ore, rarest last. Only cobblestone with a side face open to the air is
/// eligible: that is the rock surface an orbiting camera sees, while downward faces and the
/// interior would hide ore.
const ORE_TABLE: [(BlockType, f32, f32); 4] = [
    (BlockType::CoalOre, 0.10, 0.75),
    (BlockType::IronOre, 0.06, 0.65),
    (BlockType::GoldOre, 0.03, 0.55),
    (BlockType::DiamondOre, 0.015, 0.35),
];

const SILHOUETTE: Domain = Domain::from_tag(*b"isl-silh");
const SILHOUETTE_DETAIL: Domain = Domain::from_tag(*b"isl-sdet");
const SURFACE: Domain = Domain::from_tag(*b"isl-surf");
const DIRT: Domain = Domain::from_tag(*b"isl-dirt");
const UNDERSIDE: Domain = Domain::from_tag(*b"isl-undr");
const UNDERSIDE_TIP: Domain = Domain::from_tag(*b"isl-utip");
const ORE_CLUSTER: Domain = Domain::from_tag(*b"isl-orec");
const ORE_FILL: Domain = Domain::from_tag(*b"isl-oref");

/// Placement and size of one floating island. The seed is passed separately to `generate_island`,
/// so one placement can be regenerated with any variation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IslandConfig {
    /// `x`/`z`: the footprint's center line; `y`: the surface level, where the flat central grass
    /// sits. The footprint covers world `x` in `center.x - width / 2 ..` (`width` cells, integer
    /// division), and likewise for `z`, so an even footprint is centered exactly on `center.x`.
    pub center: VoxelPosition,
    /// Footprint extent along X, in voxels.
    pub footprint_width: u32,
    /// Footprint extent along Z, in voxels.
    pub footprint_depth: u32,
    /// Blocks from the surface level down to the deepest possible block, inclusive.
    pub thickness: u32,
}

impl IslandConfig {
    /// A standard 16×16 island.
    pub const fn new(center: VoxelPosition, thickness: u32) -> Self {
        Self {
            center,
            footprint_width: DEFAULT_FOOTPRINT,
            footprint_depth: DEFAULT_FOOTPRINT,
            thickness,
        }
    }

    /// The box every block of this island lies in, for any seed, after validating the config.
    ///
    /// The box spans the whole footprint and every level from the deepest possible tip to the
    /// highest possible grass.
    pub fn bounds(&self) -> Result<IslandBounds, IslandError> {
        if self.footprint_width == 0 || self.footprint_depth == 0 {
            return Err(IslandError::ZeroFootprint);
        }
        if self.thickness < MIN_THICKNESS {
            return Err(IslandError::ThicknessTooSmall);
        }

        let (min_x, max_x) = footprint_span(self.center.x, self.footprint_width);
        let (min_z, max_z) = footprint_span(self.center.z, self.footprint_depth);
        let min_y = i64::from(self.center.y) - (i64::from(self.thickness) - 1);
        let max_y = i64::from(self.center.y) + i64::from(MAX_SURFACE_RISE);
        let coordinate =
            |value: i64| i32::try_from(value).map_err(|_| IslandError::CoordinateOverflow);

        Ok(IslandBounds {
            min: VoxelPosition::new(coordinate(min_x)?, coordinate(min_y)?, coordinate(min_z)?),
            max: VoxelPosition::new(coordinate(max_x)?, coordinate(max_y)?, coordinate(max_z)?),
        })
    }
}

/// Inclusive world-voxel box: `min` and `max` are both cells of the island's region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IslandBounds {
    pub min: VoxelPosition,
    pub max: VoxelPosition,
}

impl IslandBounds {
    pub fn contains(&self, position: VoxelPosition) -> bool {
        (self.min.x..=self.max.x).contains(&position.x)
            && (self.min.y..=self.max.y).contains(&position.y)
            && (self.min.z..=self.max.z).contains(&position.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IslandError {
    /// The footprint width or depth is zero.
    ZeroFootprint,
    /// The thickness cannot hold grass, dirt, and rock under the rim (`MIN_THICKNESS`).
    ThicknessTooSmall,
    /// Some cell of the island's region lies beyond the `i32` world-coordinate range.
    CoordinateOverflow,
    /// The island's region does not lie entirely inside the target grid.
    OutsideGrid,
}

impl fmt::Display for IslandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ZeroFootprint => "island footprint dimensions must be positive",
            Self::ThicknessTooSmall => "island thickness is below the minimum",
            Self::CoordinateOverflow => "island extends beyond the i32 world-coordinate range",
            Self::OutsideGrid => "island region does not fit inside the voxel grid",
        };
        formatter.write_str(message)
    }
}

impl Error for IslandError {}

/// How many blocks of each type an island generation wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IslandStats {
    counts: [usize; BlockType::ALL.len()],
}

impl IslandStats {
    pub const fn count(&self, block: BlockType) -> usize {
        self.counts[block as usize]
    }

    pub fn total(&self) -> usize {
        self.counts.iter().sum()
    }

    pub fn ores(&self) -> usize {
        ORE_TABLE.iter().map(|&(ore, _, _)| self.count(ore)).sum()
    }
}

/// Generates one floating island for `seed` into `grid`.
///
/// The whole region from `IslandConfig::bounds` must lie inside the grid; it is validated before
/// anything is written, so an error leaves the grid untouched and an island is never clipped.
/// Only the island's own blocks are written: other cells, inside the region or not, keep their
/// contents, so several islands can share one grid. The result depends only on the seed and the
/// config, never on the grid's previous contents or on generation order.
pub fn generate_island(
    grid: &mut VoxelGrid,
    config: &IslandConfig,
    seed: u64,
) -> Result<IslandStats, IslandError> {
    let bounds = config.bounds()?;
    if !grid.contains_world(bounds.min) || !grid.contains_world(bounds.max) {
        return Err(IslandError::OutsideGrid);
    }

    let plan = IslandPlan::new(config, seed);
    let mut stats = IslandStats::default();
    let footprint_min_x = i64::from(bounds.min.x);
    let footprint_min_z = i64::from(bounds.min.z);
    let surface = i64::from(config.center.y);
    let world = |value: i64| i32::try_from(value).expect("validated island coordinates fit i32");

    for (i, j, column) in plan.columns() {
        for level in column.bottom..=column.top {
            let block = plan.block_at(seed, i, level, j, column);
            let position = VoxelPosition::new(
                world(footprint_min_x + i64::from(i)),
                world(surface + i64::from(level)),
                world(footprint_min_z + i64::from(j)),
            );
            grid.set_world(position, Voxel::Block(block))
                .expect("validated island region lies inside the grid");
            stats.counts[block as usize] += 1;
        }
    }

    Ok(stats)
}

/// First and last world coordinate of a footprint of `extent` cells centered on `center`.
fn footprint_span(center: i32, extent: u32) -> (i64, i64) {
    let min = i64::from(center) - i64::from(extent / 2);
    (min, min + i64::from(extent) - 1)
}

/// One solid footprint column, in levels relative to the surface level (`0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Column {
    /// Level of the grass block.
    top: i32,
    /// Level of the lowest block; always `<= -RIM_DEPTH`.
    bottom: i32,
    /// Dirt blocks directly under the grass; at least one cobblestone block always remains below.
    dirt_depth: i32,
}

impl Column {
    const fn contains(self, level: i32) -> bool {
        self.bottom <= level && level <= self.top
    }
}

/// The island's columns over its whole footprint domain, in local footprint coordinates.
///
/// Every one of the `width × depth` cells is evaluated; cells outside the silhouette are `None`.
struct IslandPlan {
    width: u32,
    depth: u32,
    columns: Vec<Option<Column>>,
}

impl IslandPlan {
    fn new(config: &IslandConfig, seed: u64) -> Self {
        let (width, depth) = (config.footprint_width, config.footprint_depth);
        let max_depth = i32::try_from(config.thickness - 1).unwrap_or(i32::MAX);
        let tip = tip_offset(seed);
        let columns = (0..depth)
            .flat_map(|j| (0..width).map(move |i| (i, j)))
            .map(|(i, j)| plan_column(seed, width, depth, max_depth, tip, i, j))
            .collect();

        Self {
            width,
            depth,
            columns,
        }
    }

    /// Column at local footprint coordinates; `None` outside the island or the footprint.
    fn column(&self, i: i64, j: i64) -> Option<Column> {
        let i = u32::try_from(i).ok().filter(|&i| i < self.width)?;
        let j = u32::try_from(j).ok().filter(|&j| j < self.depth)?;
        let index = i as usize + self.width as usize * j as usize;
        self.columns[index]
    }

    fn columns(&self) -> impl Iterator<Item = (u32, u32, Column)> + '_ {
        let width = self.width as usize;
        self.columns
            .iter()
            .enumerate()
            .filter_map(move |(index, column)| {
                column.map(|column| ((index % width) as u32, (index / width) as u32, column))
            })
    }

    fn is_occupied(&self, i: i64, level: i32, j: i64) -> bool {
        self.column(i, j)
            .is_some_and(|column| column.contains(level))
    }

    /// Whether a block of this island has a side (`±X` or `±Z`) face open to empty space. Judged
    /// from the island's own plan only, so it does not depend on what else the grid holds.
    fn has_exposed_side(&self, i: u32, level: i32, j: u32) -> bool {
        let (i, j) = (i64::from(i), i64::from(j));
        !self.is_occupied(i - 1, level, j)
            || !self.is_occupied(i + 1, level, j)
            || !self.is_occupied(i, level, j - 1)
            || !self.is_occupied(i, level, j + 1)
    }

    /// The block at `level` of a column: grass, then dirt, then cobblestone, where cobblestone
    /// with an exposed side may be ore.
    fn block_at(&self, seed: u64, i: u32, level: i32, j: u32, column: Column) -> BlockType {
        let below_top = column.top - level;
        if below_top == 0 {
            BlockType::Grass
        } else if below_top <= column.dirt_depth {
            BlockType::Dirt
        } else if self.has_exposed_side(i, level, j) {
            ore_at(seed, i, level, j).unwrap_or(BlockType::Cobblestone)
        } else {
            BlockType::Cobblestone
        }
    }
}

/// Plans the column at local footprint cell `(i, j)`, or `None` outside the silhouette.
fn plan_column(
    seed: u64,
    width: u32,
    depth: u32,
    max_depth: i32,
    tip: (f32, f32),
    i: u32,
    j: u32,
) -> Option<Column> {
    // Cell center relative to the footprint center, normalized so the footprint edge is at 1.
    let half_width = width as f32 * 0.5;
    let half_depth = depth as f32 * 0.5;
    let nx = (i as f32 + 0.5 - half_width) / half_width;
    let nz = (j as f32 + 0.5 - half_depth) / half_depth;
    let radius = silhouette_radius(seed, nx, nz);
    let distance = (nx * nx + nz * nz).sqrt();
    if distance > radius {
        return None;
    }

    let edge = distance / radius;
    let top = surface_level(seed, i, j, edge);
    let bottom = -underside_depth(seed, i, j, nx - tip.0, nz - tip.1, radius, max_depth);
    let dirt_depth = dirt_depth(seed, i, j).min(top - bottom - 1);

    Some(Column {
        top,
        bottom,
        dirt_depth,
    })
}

/// Boundary radius in the direction of `(nx, nz)`, in `[SILHOUETTE_MIN_RADIUS,
/// SILHOUETTE_MAX_RADIUS]`.
fn silhouette_radius(seed: u64, nx: f32, nz: f32) -> f32 {
    let turn = turn_fraction(nx, nz);
    let broad = periodic_noise_1d(
        seed,
        SILHOUETTE,
        turn * SILHOUETTE_BROAD_PERIOD as f32,
        SILHOUETTE_BROAD_PERIOD,
    );
    let detail = periodic_noise_1d(
        seed,
        SILHOUETTE_DETAIL,
        turn * SILHOUETTE_DETAIL_PERIOD as f32,
        SILHOUETTE_DETAIL_PERIOD,
    );
    SILHOUETTE_BASE_RADIUS
        + SILHOUETTE_BROAD_AMPLITUDE * signed(broad)
        + SILHOUETTE_DETAIL_AMPLITUDE * signed(detail)
}

/// Grass level relative to the surface level: `0` in the flat center, `MAX_SURFACE_RISE` in
/// raised patches of the middle ring, and `-1` in sunken patches near the rim.
fn surface_level(seed: u64, i: u32, j: u32, edge: f32) -> i32 {
    if edge < FLAT_CENTER_FRACTION {
        return 0;
    }
    let noise = value_noise_2d(
        seed,
        SURFACE,
        i as f32 / SURFACE_NOISE_SCALE,
        j as f32 / SURFACE_NOISE_SCALE,
    );
    if noise > SURFACE_RISE_THRESHOLD && edge < RISE_END_FRACTION {
        MAX_SURFACE_RISE
    } else if noise < SURFACE_DROP_THRESHOLD && edge > RIM_DROP_FRACTION {
        -1
    } else {
        0
    }
}

/// Dirt blocks under the grass, `1..=3`, varying in smooth patches.
fn dirt_depth(seed: u64, i: u32, j: u32) -> i32 {
    let noise = value_noise_2d(
        seed,
        DIRT,
        i as f32 / DIRT_NOISE_SCALE,
        j as f32 / DIRT_NOISE_SCALE,
    );
    1 + (noise * DIRT_DEPTH_RANGE) as i32
}

/// Seed-dependent horizontal offset of the underside's tip, in normalized footprint units, so the
/// underside is not centered under the island.
fn tip_offset(seed: u64) -> (f32, f32) {
    let x = signed(unit_interval(hash3(seed, UNDERSIDE_TIP, 0, 0, 0)));
    let z = signed(unit_interval(hash3(seed, UNDERSIDE_TIP, 1, 0, 0)));
    (x * TIP_MAX_OFFSET, z * TIP_MAX_OFFSET)
}

/// Blocks below the surface level, `RIM_DEPTH..=max_depth`. `(tx, tz)` is the column's offset
/// from the tip in normalized units; `radius` is the silhouette radius in its direction.
fn underside_depth(
    seed: u64,
    i: u32,
    j: u32,
    tx: f32,
    tz: f32,
    radius: f32,
    max_depth: i32,
) -> i32 {
    let toward_tip = 1.0 - ((tx * tx + tz * tz).sqrt() / radius).min(1.0);
    // Between a cone (linear) and a spike (quadratic): wide shoulders that narrow to a small tip.
    let profile = toward_tip * (UNDERSIDE_CONE_WEIGHT + (1.0 - UNDERSIDE_CONE_WEIGHT) * toward_tip);
    let noise = value_noise_2d(
        seed,
        UNDERSIDE,
        i as f32 / UNDERSIDE_NOISE_SCALE,
        j as f32 / UNDERSIDE_NOISE_SCALE,
    );
    let jitter = signed(noise) * UNDERSIDE_JITTER * toward_tip;
    let span = (max_depth - RIM_DEPTH) as f32;
    let depth = RIM_DEPTH as f32 + span * profile + jitter;
    (depth.round() as i32).clamp(RIM_DEPTH, max_depth)
}

/// Ore for an exposed cobblestone block, if any.
///
/// Each `ORE_CLUSTER_SIZE`³ cell of local space draws one ore type (or none) from `ORE_TABLE`;
/// each eligible block inside it then independently becomes that ore with the type's fill
/// probability. Both draws are stateless hashes, so the result depends on the block's position
/// alone, never on visiting order.
fn ore_at(seed: u64, i: u32, level: i32, j: u32) -> Option<BlockType> {
    let (i, j) = (i as i32, j as i32);
    let cluster = unit_interval(hash3(
        seed,
        ORE_CLUSTER,
        i.div_euclid(ORE_CLUSTER_SIZE),
        level.div_euclid(ORE_CLUSTER_SIZE),
        j.div_euclid(ORE_CLUSTER_SIZE),
    ));

    let mut threshold = 0.0;
    let (ore, fill) = ORE_TABLE.iter().find_map(|&(ore, probability, fill)| {
        threshold += probability;
        (cluster < threshold).then_some((ore, fill))
    })?;
    (unit_interval(hash3(seed, ORE_FILL, i, level, j)) < fill).then_some(ore)
}
