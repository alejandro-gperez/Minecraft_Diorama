use std::collections::VecDeque;

use super::{
    Column, DEFAULT_FOOTPRINT, FLAT_CENTER_FRACTION, IslandConfig, IslandError, IslandPlan,
    MAX_SURFACE_RISE, MIN_THICKNESS, SILHOUETTE_MAX_RADIUS, SILHOUETTE_MIN_RADIUS, generate_island,
};
use crate::{
    voxel::{BlockType, Voxel, VoxelGrid, VoxelPosition},
    world::noise::{Domain, hash3, value_noise_2d},
};

const THICKNESS: u32 = 12;
const SEEDS: [u64; 8] = [0, 1, 7, 42, 99, 1847, 2026, u64::MAX];
const ORES: [BlockType; 4] = [
    BlockType::CoalOre,
    BlockType::IronOre,
    BlockType::GoldOre,
    BlockType::DiamondOre,
];

fn config_at(x: i32, y: i32, z: i32) -> IslandConfig {
    IslandConfig::new(VoxelPosition::new(x, y, z), THICKNESS)
}

fn grid_for(config: &IslandConfig) -> VoxelGrid {
    let bounds = config.bounds().unwrap();
    VoxelGrid::try_new(
        bounds.min,
        (bounds.max.x - bounds.min.x + 1) as usize,
        (bounds.max.y - bounds.min.y + 1) as usize,
        (bounds.max.z - bounds.min.z + 1) as usize,
    )
    .unwrap()
}

/// A grid holding exactly the island's region, with the island generated into it.
fn island(config: &IslandConfig, seed: u64) -> VoxelGrid {
    let mut grid = grid_for(config);
    generate_island(&mut grid, config, seed).unwrap();
    grid
}

/// Every world position of a grid, in storage order (Y, then Z, then X).
fn positions(grid: &VoxelGrid) -> Vec<VoxelPosition> {
    let origin = grid.origin();
    let mut positions = Vec::with_capacity(grid.volume());
    for y in 0..grid.height() as i32 {
        for z in 0..grid.depth() as i32 {
            for x in 0..grid.width() as i32 {
                positions.push(VoxelPosition::new(origin.x + x, origin.y + y, origin.z + z));
            }
        }
    }
    positions
}

/// Occupied voxels in deterministic storage order.
fn blocks(grid: &VoxelGrid) -> Vec<(VoxelPosition, BlockType)> {
    positions(grid)
        .into_iter()
        .filter_map(|p| grid.get_world(p).unwrap().block().map(|b| (p, b)))
        .collect()
}

/// Occupied voxels relative to a reference position, for comparing translated islands.
fn relative_blocks(grid: &VoxelGrid, reference: VoxelPosition) -> Vec<(i32, i32, i32, BlockType)> {
    blocks(grid)
        .into_iter()
        .map(|(p, b)| (p.x - reference.x, p.y - reference.y, p.z - reference.z, b))
        .collect()
}

/// Test-only FNV-1a fingerprint of a grid's placement and contents in storage order.
fn fingerprint(grid: &VoxelGrid) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325_u64;
    let mut feed = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01B3);
    };
    let origin = grid.origin();
    for value in [origin.x, origin.y, origin.z] {
        value.to_le_bytes().into_iter().for_each(&mut feed);
    }
    for extent in [grid.width(), grid.height(), grid.depth()] {
        (extent as u64)
            .to_le_bytes()
            .into_iter()
            .for_each(&mut feed);
    }
    for position in positions(grid) {
        feed(match grid.get_world(position).unwrap() {
            Voxel::Empty => 0,
            Voxel::Block(block) => block as u8 + 1,
        });
    }
    hash
}

fn is_ore(block: BlockType) -> bool {
    ORES.contains(&block)
}

/// The column of blocks at a world `(x, z)`, top to bottom, with the top position.
fn column_blocks(grid: &VoxelGrid, x: i32, z: i32) -> Vec<(i32, BlockType)> {
    let origin = grid.origin();
    (0..grid.height() as i32)
        .rev()
        .map(|y| origin.y + y)
        .filter_map(|y| {
            grid.get_world(VoxelPosition::new(x, y, z))
                .unwrap()
                .block()
                .map(|b| (y, b))
        })
        .collect()
}

fn occupied(grid: &VoxelGrid, x: i32, y: i32, z: i32) -> bool {
    grid.is_occupied_world(VoxelPosition::new(x, y, z))
}

fn normalized_offset(config: &IslandConfig, i: u32, j: u32) -> (f32, f32) {
    let half_width = config.footprint_width as f32 * 0.5;
    let half_depth = config.footprint_depth as f32 * 0.5;
    (
        (i as f32 + 0.5 - half_width) / half_width,
        (j as f32 + 0.5 - half_depth) / half_depth,
    )
}

// ---- Seed reproducibility ----

#[test]
fn same_seed_produces_identical_voxels() {
    let config = config_at(0, 0, 0);
    for seed in SEEDS {
        assert_eq!(island(&config, seed), island(&config, seed), "seed {seed}");
    }
}

#[test]
fn repeated_generation_into_the_same_grid_is_idempotent() {
    let config = config_at(0, 0, 0);
    let mut grid = grid_for(&config);
    let first = generate_island(&mut grid, &config, 1847).unwrap();
    let after_one = grid.clone();
    for _ in 0..3 {
        assert_eq!(generate_island(&mut grid, &config, 1847).unwrap(), first);
        assert_eq!(grid, after_one);
    }
}

#[test]
fn different_seeds_produce_different_islands() {
    let config = config_at(0, 0, 0);
    let islands: Vec<VoxelGrid> = SEEDS.iter().map(|&seed| island(&config, seed)).collect();
    let silhouettes: Vec<Vec<bool>> = SEEDS
        .iter()
        .map(|&seed| {
            IslandPlan::new(&config, seed)
                .columns
                .iter()
                .map(Option::is_some)
                .collect()
        })
        .collect();

    for a in 0..SEEDS.len() {
        for b in a + 1..SEEDS.len() {
            assert_ne!(
                islands[a], islands[b],
                "seeds {} and {}",
                SEEDS[a], SEEDS[b]
            );
            assert_ne!(
                silhouettes[a], silhouettes[b],
                "seeds {} and {} share a silhouette",
                SEEDS[a], SEEDS[b]
            );
        }
    }
}

#[test]
fn seed_zero_and_maximum_seed_generate_valid_islands() {
    let config = config_at(0, 0, 0);
    for seed in [0, u64::MAX] {
        let grid = island(&config, seed);
        assert!(blocks(&grid).len() > 500, "seed {seed}");
        assert_column_layers(&grid, &config);
        assert_single_landmass(&grid);
    }
}

#[test]
fn generation_does_not_depend_on_order_or_previous_contents() {
    let first = config_at(-12, 0, 0);
    let second = config_at(12, 4, 3);
    let mut shared = VoxelGrid::try_new(VoxelPosition::new(-24, -16, -12), 48, 24, 24).unwrap();
    let mut reversed = shared.clone();

    generate_island(&mut shared, &first, 5).unwrap();
    generate_island(&mut shared, &second, 6).unwrap();
    generate_island(&mut reversed, &second, 6).unwrap();
    generate_island(&mut reversed, &first, 5).unwrap();
    assert_eq!(shared, reversed);

    // Generating another seed first, into the same cells, leaves no trace in the island blocks.
    let mut dirty = grid_for(&first);
    generate_island(&mut dirty, &first, 999).unwrap();
    generate_island(&mut dirty, &first, 5).unwrap();
    let clean = island(&first, 5);
    for (position, block) in blocks(&clean) {
        assert_eq!(dirty.get_world(position), Some(Voxel::Block(block)));
    }
}

#[test]
fn sampling_unrelated_domains_does_not_alter_generation() {
    let config = config_at(0, 0, 0);
    let before = island(&config, 1847);

    // A hypothetical later feature drawing many values from its own domain in between.
    let future_feature = Domain::from_tag(*b"future-x");
    let mut sink = 0.0;
    for x in -50..50 {
        sink += value_noise_2d(1847, future_feature, x as f32 * 0.3, 2.0);
        sink += hash3(1847, future_feature, x, 1, 2) as f32;
    }
    assert!(sink.is_finite());

    assert_eq!(island(&config, 1847), before);
}

#[test]
fn reference_seed_fingerprint_is_stable() {
    // Pins the exact output of this generator version for the reference seed. Noise uses only
    // exactly rounded float operations, so this holds on every IEEE 754 platform. A deliberate
    // algorithm or constant change updates this value; an accidental one fails here.
    let grid = island(&config_at(0, 0, 0), 1847);
    assert_eq!(fingerprint(&grid), REFERENCE_FINGERPRINT);
}

const REFERENCE_FINGERPRINT: u64 = 0x7EA27F269F65A6AD;

// ---- Grid placement ----

#[test]
fn translated_islands_keep_the_same_local_shape() {
    let at_origin = config_at(0, 0, 0);
    let reference = relative_blocks(&island(&at_origin, 2026), at_origin.center);
    for center in [
        VoxelPosition::new(-40, -25, -73),
        VoxelPosition::new(-1, 0, -1),
        VoxelPosition::new(17, 9, 301),
        VoxelPosition::new(1_000_000, -500, -2_000_000),
    ] {
        let config = IslandConfig::new(center, THICKNESS);
        assert_eq!(
            relative_blocks(&island(&config, 2026), center),
            reference,
            "{center:?}"
        );
    }
}

#[test]
fn island_stays_inside_its_documented_bounds() {
    for center in [VoxelPosition::new(0, 0, 0), VoxelPosition::new(-9, -30, 4)] {
        let config = IslandConfig::new(center, THICKNESS);
        let bounds = config.bounds().unwrap();
        assert_eq!(bounds.min.x, center.x - 8);
        assert_eq!(bounds.max.x, center.x + 7);
        assert_eq!(bounds.min.z, center.z - 8);
        assert_eq!(bounds.max.z, center.z + 7);
        assert_eq!(bounds.min.y, center.y - (THICKNESS as i32 - 1));
        assert_eq!(bounds.max.y, center.y + MAX_SURFACE_RISE);

        for seed in SEEDS {
            for (position, _) in blocks(&island(&config, seed)) {
                assert!(bounds.contains(position), "{position:?}");
            }
        }
    }
}

#[test]
fn generation_writes_nothing_outside_the_island_region() {
    let config = config_at(-3, 2, 5);
    let bounds = config.bounds().unwrap();
    let marker = Voxel::Block(BlockType::Obsidian);
    let mut grid = VoxelGrid::try_new(VoxelPosition::new(-20, -20, -20), 40, 30, 40).unwrap();
    grid.fill(marker);

    let stats = generate_island(&mut grid, &config, 77).unwrap();

    let mut written = 0;
    for position in positions(&grid) {
        let voxel = grid.get_world(position).unwrap();
        if !bounds.contains(position) {
            assert_eq!(voxel, marker, "{position:?}");
        } else if voxel != marker {
            written += 1;
        }
    }
    assert_eq!(written, stats.total());
}

#[test]
fn invalid_configurations_are_rejected_without_writing() {
    let grid_origin = VoxelPosition::new(-8, -11, -8);
    let mut grid = VoxelGrid::try_new(grid_origin, 16, 13, 16).unwrap();
    grid.fill(Voxel::Block(BlockType::Glass));
    let untouched = grid.clone();

    // The grid fits exactly the island centered at the origin.
    assert!(generate_island(&mut grid.clone(), &config_at(0, 0, 0), 1).is_ok());

    let cases = [
        (config_at(1, 0, 0), IslandError::OutsideGrid),
        (config_at(-1, 0, 0), IslandError::OutsideGrid),
        (config_at(0, 1, 0), IslandError::OutsideGrid),
        (config_at(0, -1, 0), IslandError::OutsideGrid),
        (config_at(0, 0, 1), IslandError::OutsideGrid),
        (config_at(0, 0, -1), IslandError::OutsideGrid),
        (
            config_at(0, 0, 0).with_footprint(0, 16),
            IslandError::ZeroFootprint,
        ),
        (
            config_at(0, 0, 0).with_footprint(16, 0),
            IslandError::ZeroFootprint,
        ),
        (
            config_at(0, 0, 0).with_footprint(17, 16),
            IslandError::OutsideGrid,
        ),
        (
            IslandConfig::new(VoxelPosition::new(0, 0, 0), MIN_THICKNESS - 1),
            IslandError::ThicknessTooSmall,
        ),
        (
            IslandConfig::new(VoxelPosition::new(0, 0, 0), 0),
            IslandError::ThicknessTooSmall,
        ),
        (
            IslandConfig::new(VoxelPosition::new(0, 0, 0), 14),
            IslandError::OutsideGrid,
        ),
        (config_at(i32::MAX, 0, 0), IslandError::CoordinateOverflow),
        (config_at(i32::MIN, 0, 0), IslandError::CoordinateOverflow),
        (config_at(0, i32::MAX, 0), IslandError::CoordinateOverflow),
        (
            config_at(0, i32::MIN + 5, 0),
            IslandError::CoordinateOverflow,
        ),
        (
            config_at(0, 0, i32::MAX - 6),
            IslandError::CoordinateOverflow,
        ),
    ];
    for (config, expected) in cases {
        assert_eq!(
            generate_island(&mut grid, &config, 1),
            Err(expected),
            "{config:?}"
        );
        assert_eq!(grid, untouched, "{config:?} modified the grid");
    }
}

#[test]
fn bounds_validation_accepts_extreme_but_representable_placements() {
    assert!(
        config_at(i32::MAX - 7, i32::MAX - 1, i32::MIN + 8)
            .bounds()
            .is_ok()
    );
    assert!(
        IslandConfig::new(VoxelPosition::new(0, i32::MIN + 11, 0), THICKNESS)
            .bounds()
            .is_ok()
    );
    let minimal = IslandConfig::new(VoxelPosition::new(3, -2, 1), MIN_THICKNESS);
    assert_column_layers(&island(&minimal, 9), &minimal);
}

impl IslandConfig {
    fn with_footprint(mut self, width: u32, depth: u32) -> Self {
        self.footprint_width = width;
        self.footprint_depth = depth;
        self
    }
}

// ---- Footprint ----

#[test]
fn generator_evaluates_the_full_sixteen_by_sixteen_domain() {
    let config = config_at(0, 0, 0);
    assert_eq!(config.footprint_width, 16);
    assert_eq!(config.footprint_depth, 16);
    assert_eq!(DEFAULT_FOOTPRINT, 16);
    for seed in SEEDS {
        let plan = IslandPlan::new(&config, seed);
        assert_eq!(plan.columns.len(), 256);
        assert_eq!((plan.width, plan.depth), (16, 16));
    }
}

#[test]
fn silhouette_is_round_not_square() {
    let config = config_at(0, 0, 0);
    for seed in 0..64 {
        let plan = IslandPlan::new(&config, seed);
        let land = plan.columns.iter().filter(|c| c.is_some()).count();
        assert!(land < 256 && land > 150, "seed {seed}: {land} columns");
        for (i, j) in [(0, 0), (15, 0), (0, 15), (15, 15), (1, 0), (0, 1), (14, 15)] {
            assert!(
                plan.column(i, j).is_none(),
                "seed {seed}: corner ({i}, {j})"
            );
        }
        // The island spans most of the footprint on both axes.
        let span = |coordinate: fn(u32, u32) -> u32| {
            let values: Vec<u32> = plan.columns().map(|(i, j, _)| coordinate(i, j)).collect();
            values.iter().max().unwrap() - values.iter().min().unwrap() + 1
        };
        assert!(span(|i, _| i) >= 12, "seed {seed}");
        assert!(span(|_, j| j) >= 12, "seed {seed}");
    }
}

#[test]
fn silhouette_follows_its_radial_bounds_and_keeps_the_center() {
    let config = config_at(0, 0, 0);
    for seed in 0..64 {
        let plan = IslandPlan::new(&config, seed);
        for j in 0..16 {
            for i in 0..16 {
                let (nx, nz) = normalized_offset(&config, i, j);
                let distance = (nx * nx + nz * nz).sqrt();
                let present = plan.column(i64::from(i), i64::from(j)).is_some();
                if distance <= SILHOUETTE_MIN_RADIUS {
                    assert!(present, "seed {seed}: ({i}, {j}) inside the minimum radius");
                }
                if distance > SILHOUETTE_MAX_RADIUS {
                    assert!(
                        !present,
                        "seed {seed}: ({i}, {j}) beyond the maximum radius"
                    );
                }
            }
        }
        for (i, j) in [(7, 7), (8, 7), (7, 8), (8, 8)] {
            assert_eq!(plan.column(i, j).unwrap().top, 0, "seed {seed}");
        }
    }
}

#[test]
fn footprint_is_one_connected_region() {
    let config = config_at(0, 0, 0);
    for seed in 0..64 {
        let plan = IslandPlan::new(&config, seed);
        let land: Vec<(u32, u32)> = plan.columns().map(|(i, j, _)| (i, j)).collect();
        let mut seen = vec![false; 256];
        let mut queue = VecDeque::from([(8_i64, 8_i64)]);
        seen[8 + 16 * 8] = true;
        let mut reached = 1;
        while let Some((i, j)) = queue.pop_front() {
            for (ni, nj) in [(i - 1, j), (i + 1, j), (i, j - 1), (i, j + 1)] {
                if plan.column(ni, nj).is_some() && !seen[(ni + 16 * nj) as usize] {
                    seen[(ni + 16 * nj) as usize] = true;
                    reached += 1;
                    queue.push_back((ni, nj));
                }
            }
        }
        assert_eq!(reached, land.len(), "seed {seed}");
    }
}

// ---- Terrain layers ----

/// Every column is, top to bottom: one grass, one to three dirt, then at least one rock block
/// (cobblestone or ore), contiguous, with ore only where a side face is open.
fn assert_column_layers(grid: &VoxelGrid, config: &IslandConfig) {
    let bounds = config.bounds().unwrap();
    let surface = config.center.y;
    let mut columns = 0;
    for z in bounds.min.z..=bounds.max.z {
        for x in bounds.min.x..=bounds.max.x {
            let column = column_blocks(grid, x, z);
            if column.is_empty() {
                continue;
            }
            columns += 1;
            let (top, _) = column[0];
            for (offset, &(y, _)) in column.iter().enumerate() {
                assert_eq!(y, top - offset as i32, "gap in column ({x}, {z})");
            }
            assert!((surface - 1..=surface + MAX_SURFACE_RISE).contains(&top));
            assert!(
                column.last().unwrap().0 <= surface - 4,
                "rim column too shallow"
            );

            let kinds: Vec<BlockType> = column.iter().map(|&(_, b)| b).collect();
            assert_eq!(kinds[0], BlockType::Grass, "({x}, {z})");
            let dirt = kinds[1..]
                .iter()
                .take_while(|&&b| b == BlockType::Dirt)
                .count();
            assert!((1..=3).contains(&dirt), "({x}, {z}): {dirt} dirt");
            let rock = &column[1 + dirt..];
            assert!(!rock.is_empty(), "({x}, {z}) has no rock");
            for &(y, block) in rock {
                assert!(
                    block == BlockType::Cobblestone || is_ore(block),
                    "({x}, {y}, {z}): {block:?} in the rock body"
                );
                if is_ore(block) {
                    let open_side = [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)]
                        .into_iter()
                        .any(|(nx, nz)| !occupied(grid, nx, y, nz));
                    assert!(open_side, "({x}, {y}, {z}): hidden ore");
                }
            }
        }
    }
    assert!(columns > 0);
}

/// Every occupied voxel is face-connected to every other one.
fn assert_single_landmass(grid: &VoxelGrid) {
    let occupied_blocks = blocks(grid);
    let start = occupied_blocks[0].0;
    let mut seen = std::collections::HashSet::from([(start.x, start.y, start.z)]);
    let mut queue = VecDeque::from([start]);
    while let Some(p) = queue.pop_front() {
        for (dx, dy, dz) in [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            let n = VoxelPosition::new(p.x + dx, p.y + dy, p.z + dz);
            if grid.is_occupied_world(n) && seen.insert((n.x, n.y, n.z)) {
                queue.push_back(n);
            }
        }
    }
    assert_eq!(seen.len(), occupied_blocks.len(), "detached voxels");
}

#[test]
fn columns_are_grass_over_dirt_over_rock() {
    for center in [VoxelPosition::new(0, 0, 0), VoxelPosition::new(-50, -7, 13)] {
        let config = IslandConfig::new(center, THICKNESS);
        for seed in 0..48 {
            assert_column_layers(&island(&config, seed), &config);
        }
    }
}

#[test]
fn island_is_one_landmass_without_detached_voxels() {
    let config = config_at(0, 0, 0);
    for seed in 0..48 {
        assert_single_landmass(&island(&config, seed));
    }
}

#[test]
fn center_is_flat_and_surface_varies_by_one_level() {
    let config = config_at(0, 0, 0);
    let flat_radius = FLAT_CENTER_FRACTION * SILHOUETTE_MIN_RADIUS;
    let mut raised = 0;
    let mut sunken = 0;
    for seed in 0..64 {
        let plan = IslandPlan::new(&config, seed);
        for (i, j, column) in plan.columns() {
            assert!((-1..=MAX_SURFACE_RISE).contains(&column.top));
            let (nx, nz) = normalized_offset(&config, i, j);
            if (nx * nx + nz * nz).sqrt() < flat_radius {
                assert_eq!(column.top, 0, "seed {seed}: ({i}, {j})");
            }
            raised += usize::from(column.top > 0);
            sunken += usize::from(column.top < 0);
        }
    }
    assert!(raised > 0 && sunken > 0, "surface never varies");
}

#[test]
fn underside_tapers_toward_a_small_tip() {
    let config = config_at(0, 0, 0);
    for seed in 0..48 {
        let plan = IslandPlan::new(&config, seed);
        let layer = |level: i32| -> Vec<bool> {
            plan.columns
                .iter()
                .map(|column| column.is_some_and(|c: Column| c.contains(level)))
                .collect()
        };
        let lowest = plan.columns().map(|(_, _, c)| c.bottom).min().unwrap();
        // Below the lowest possible grass, each layer lies within the layer above it.
        for level in lowest..-2 {
            let (lower, upper) = (layer(level), layer(level + 1));
            assert!(
                lower.iter().zip(&upper).all(|(&l, &u)| !l || u),
                "seed {seed}: level {level} overhangs level {}",
                level + 1
            );
        }
        let count = |level| layer(level).into_iter().filter(|&c| c).count();
        let widest = count(-2);
        let tip = count(lowest);
        assert!(lowest <= -9, "seed {seed}: underside only reaches {lowest}");
        assert!(
            tip * 8 <= widest,
            "seed {seed}: tip {tip} vs widest {widest}"
        );
    }
}

// ---- Ores ----

fn ore_counts(grid: &VoxelGrid) -> [usize; 4] {
    let mut counts = [0; 4];
    for (_, block) in blocks(grid) {
        if let Some(index) = ORES.iter().position(|&ore| ore == block) {
            counts[index] += 1;
        }
    }
    counts
}

#[test]
fn ores_have_sensible_relative_rarity_across_seeds() {
    let config = config_at(0, 0, 0);
    let mut totals = [0; 4];
    for seed in 0..64 {
        for (total, count) in totals.iter_mut().zip(ore_counts(&island(&config, seed))) {
            *total += count;
        }
    }
    let [coal, iron, gold, diamond] = totals;
    assert!(diamond > 0, "{totals:?}");
    assert!(coal > iron && iron > gold && gold > diamond, "{totals:?}");
    assert!(coal >= 4 * diamond, "{totals:?}");
}

#[test]
fn ore_arrangements_are_deterministic_and_vary_by_seed() {
    let config = config_at(0, 0, 0);
    let ore_positions = |seed| -> Vec<(VoxelPosition, BlockType)> {
        blocks(&island(&config, seed))
            .into_iter()
            .filter(|&(_, b)| is_ore(b))
            .collect()
    };
    assert_eq!(ore_positions(1847), ore_positions(1847));
    assert!(!ore_positions(1847).is_empty());
    assert_ne!(ore_positions(1847), ore_positions(2026));
    assert_ne!(ore_positions(0), ore_positions(u64::MAX));
}

#[test]
fn only_terrain_block_types_are_generated_and_stats_match() {
    let config = config_at(0, 0, 0);
    let allowed = [
        BlockType::Grass,
        BlockType::Dirt,
        BlockType::Cobblestone,
        BlockType::CoalOre,
        BlockType::IronOre,
        BlockType::GoldOre,
        BlockType::DiamondOre,
    ];
    for seed in SEEDS {
        let mut grid = grid_for(&config);
        let stats = generate_island(&mut grid, &config, seed).unwrap();
        let generated = blocks(&grid);
        assert_eq!(stats.total(), generated.len());
        for block in BlockType::ALL {
            let count = generated.iter().filter(|&&(_, b)| b == block).count();
            assert_eq!(stats.count(block), count, "{block:?}");
            if !allowed.contains(&block) {
                assert_eq!(count, 0, "{block:?}");
            }
        }
        assert_eq!(stats.ores(), ore_counts(&grid).iter().sum::<usize>());
    }
}
