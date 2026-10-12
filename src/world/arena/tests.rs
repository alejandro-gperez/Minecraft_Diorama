use std::collections::{HashSet, VecDeque};

use super::{
    Arena, ArenaError, ArenaLayout, CENTER_FOOTPRINT, CENTER_THICKNESS, ISLAND_SEPARATION,
    IslandId, OUTER_FOOTPRINT, OUTER_THICKNESS, SURFACE_LEVEL, generate_arena, island_seed,
};
use crate::{
    voxel::{BlockType, VoxelGrid, VoxelGridError, VoxelPosition},
    world::{
        IslandBounds, IslandError, generate_island,
        island::tests::{blocks, column_blocks, fingerprint},
    },
};

const SEEDS: [u64; 6] = [0, 1, 42, 1847, 2026, u64::MAX];
const ORES: [BlockType; 4] = [
    BlockType::CoalOre,
    BlockType::IronOre,
    BlockType::GoldOre,
    BlockType::DiamondOre,
];
const TERRAIN: [BlockType; 7] = [
    BlockType::Grass,
    BlockType::Dirt,
    BlockType::Cobblestone,
    BlockType::CoalOre,
    BlockType::IronOre,
    BlockType::GoldOre,
    BlockType::DiamondOre,
];
const OUTER: [IslandId; 4] = [
    IslandId::North,
    IslandId::South,
    IslandId::East,
    IslandId::West,
];

fn arena(seed: u64) -> Arena {
    generate_arena(&ArenaLayout::eggwars(), seed).unwrap()
}

fn region(layout: &ArenaLayout, island: IslandId) -> IslandBounds {
    layout.config(island).bounds().unwrap()
}

/// Every island whose region contains `position`.
fn owners(layout: &ArenaLayout, position: VoxelPosition) -> Vec<IslandId> {
    IslandId::ALL
        .into_iter()
        .filter(|&id| region(layout, id).contains(position))
        .collect()
}

/// An island's blocks relative to its center, so islands at different places compare by shape.
fn local_blocks(
    grid: &VoxelGrid,
    layout: &ArenaLayout,
    island: IslandId,
) -> Vec<(i32, i32, i32, BlockType)> {
    let bounds = region(layout, island);
    let center = layout.config(island).center;
    blocks(grid)
        .into_iter()
        .filter(|&(position, _)| bounds.contains(position))
        .map(|(p, b)| (p.x - center.x, p.y - center.y, p.z - center.z, b))
        .collect()
}

fn empty_grid(layout: &ArenaLayout) -> VoxelGrid {
    let (width, height, depth) = layout.grid_dimensions();
    VoxelGrid::try_new(layout.bounds().min, width, height, depth).unwrap()
}

// ---- Layout ----

#[test]
fn arena_has_one_center_and_four_outer_islands() {
    assert_eq!(IslandId::ALL.len(), 5);
    let distinct: HashSet<IslandId> = IslandId::ALL.into_iter().collect();
    assert_eq!(distinct.len(), 5);
    assert_eq!(IslandId::ALL.iter().filter(|id| id.is_center()).count(), 1);
    assert!(OUTER.iter().all(|id| !id.is_center()));
    for (index, id) in IslandId::ALL.into_iter().enumerate() {
        assert_eq!(id as usize, index, "{id:?} indexes per-island arrays");
    }

    let info = arena(1847).info;
    assert_eq!(info.islands.len(), 5);
    for id in IslandId::ALL {
        assert_eq!(info.island(id).id, id);
    }
}

#[test]
fn central_island_is_24_by_24_and_outer_islands_16_by_16() {
    let layout = ArenaLayout::eggwars();
    let center = layout.config(IslandId::Center);
    assert_eq!((CENTER_FOOTPRINT, OUTER_FOOTPRINT), (24, 16));
    assert_eq!((center.footprint_width, center.footprint_depth), (24, 24));
    assert_eq!(center.thickness, CENTER_THICKNESS);
    for id in OUTER {
        let config = layout.config(id);
        assert_eq!((config.footprint_width, config.footprint_depth), (16, 16));
        assert_eq!(config.thickness, OUTER_THICKNESS);
    }
    // The center's underside reaches deeper than the outer islands'.
    assert_eq!((CENTER_THICKNESS, OUTER_THICKNESS), (15, 12));
}

#[test]
fn islands_sit_on_their_cardinal_axes_at_one_surface_level() {
    let layout = ArenaLayout::eggwars();
    let center = |id| layout.config(id).center;
    let s = ISLAND_SEPARATION as i32;
    assert_eq!(s, 32);
    assert_eq!(
        center(IslandId::Center),
        VoxelPosition::new(0, SURFACE_LEVEL, 0)
    );
    // +X east, +Z south: north is -Z and west is -X.
    assert_eq!(
        center(IslandId::North),
        VoxelPosition::new(0, SURFACE_LEVEL, -s)
    );
    assert_eq!(
        center(IslandId::South),
        VoxelPosition::new(0, SURFACE_LEVEL, s)
    );
    assert_eq!(
        center(IslandId::East),
        VoxelPosition::new(s, SURFACE_LEVEL, 0)
    );
    assert_eq!(
        center(IslandId::West),
        VoxelPosition::new(-s, SURFACE_LEVEL, 0)
    );
    for id in OUTER {
        let (dx, dz) = id.direction();
        let position = center(id);
        assert_eq!((position.x, position.z), (dx * s, dz * s), "{id:?}");
        assert_eq!(dx.abs() + dz.abs(), 1, "{id:?} is one cardinal step out");
    }
    assert_eq!(IslandId::Center.direction(), (0, 0));
    assert_eq!(layout.separation(), ISLAND_SEPARATION);
}

#[test]
fn nominal_gaps_between_the_center_and_each_outer_island_are_twelve_blocks() {
    let layout = ArenaLayout::eggwars();
    let center = region(&layout, IslandId::Center);
    let gap = |id| {
        let outer = region(&layout, id);
        match id {
            IslandId::North => center.min.z - outer.max.z - 1,
            IslandId::South => outer.min.z - center.max.z - 1,
            IslandId::East => outer.min.x - center.max.x - 1,
            IslandId::West => center.min.x - outer.max.x - 1,
            IslandId::Center => unreachable!(),
        }
    };
    let nominal =
        ISLAND_SEPARATION as i32 - CENTER_FOOTPRINT as i32 / 2 - OUTER_FOOTPRINT as i32 / 2;
    assert_eq!(nominal, 12);
    for id in OUTER {
        assert_eq!(gap(id), nominal, "{id:?}");
    }
}

#[test]
fn island_regions_are_pairwise_disjoint() {
    let layout = ArenaLayout::eggwars();
    for (a, &first) in IslandId::ALL.iter().enumerate() {
        for &second in &IslandId::ALL[a + 1..] {
            let (p, q) = (region(&layout, first), region(&layout, second));
            let disjoint = p.max.x < q.min.x
                || q.max.x < p.min.x
                || p.max.y < q.min.y
                || q.max.y < p.min.y
                || p.max.z < q.min.z
                || q.max.z < p.min.z;
            assert!(disjoint, "{first:?} and {second:?}");
        }
    }
}

#[test]
fn overlapping_or_touching_islands_are_rejected() {
    // Separation 20 makes the 24- and 16-wide footprints touch; anything less overlaps.
    for separation in [0, 1, 8, 12, 19, 20] {
        assert_eq!(
            ArenaLayout::new(separation),
            Err(ArenaError::Overlap {
                first: IslandId::Center,
                second: IslandId::North,
            }),
            "separation {separation}"
        );
    }
    let closest = ArenaLayout::new(21).unwrap();
    let center = region(&closest, IslandId::Center);
    let east = region(&closest, IslandId::East);
    assert_eq!(east.min.x - center.max.x - 1, 1);
}

#[test]
fn unrepresentable_layouts_are_rejected() {
    for separation in [u32::MAX, i32::MAX as u32, i32::MAX as u32 - 6] {
        assert_eq!(
            ArenaLayout::new(separation),
            Err(ArenaError::Island {
                island: IslandId::North,
                error: IslandError::CoordinateOverflow,
            }),
            "separation {separation}"
        );
    }
    // Representable, but its enclosing grid is not: the error comes before any island is written,
    // so there is no partial arena.
    let vast = ArenaLayout::new(i32::MAX as u32 - 7).unwrap();
    assert_eq!(
        generate_arena(&vast, 1847),
        Err(ArenaError::Grid(VoxelGridError::VolumeOverflow))
    );
    assert!(
        ArenaError::Overlap {
            first: IslandId::Center,
            second: IslandId::East,
        }
        .to_string()
        .contains("Center and East")
    );
}

// ---- Seeds ----

#[test]
fn derived_island_seeds_are_stable_and_distinct() {
    // Pinned: changing a tag or the derivation would silently move every published seed.
    assert_eq!(
        IslandId::ALL.map(|id| island_seed(1847, id)),
        REFERENCE_ISLAND_SEEDS
    );
    for global in SEEDS {
        let seeds = IslandId::ALL.map(|id| island_seed(global, id));
        let distinct: HashSet<u64> = seeds.into_iter().collect();
        assert_eq!(distinct.len(), 5, "global seed {global}");
        assert!(!seeds.contains(&global), "global seed {global}");
        assert_eq!(seeds, arena(global).info.islands.map(|island| island.seed));
        // Pure per island: deriving in another order gives the same values.
        for id in IslandId::ALL.into_iter().rev() {
            assert_eq!(island_seed(global, id), seeds[id as usize]);
        }
    }
}

/// Center, North, South, East, West for global seed 1847.
const REFERENCE_ISLAND_SEEDS: [u64; 5] = [
    0x56B8_FA15_9F0F_A4D1,
    0x9AC9_775B_3752_D971,
    0x6A98_AB90_EABC_579B,
    0x0300_42D5_1439_D772,
    0xE604_4E3D_FDB5_92BA,
];

#[test]
fn same_seed_produces_an_identical_arena() {
    for seed in SEEDS {
        assert_eq!(arena(seed), arena(seed), "seed {seed}");
    }
}

#[test]
fn reference_arena_fingerprint_is_stable() {
    assert_eq!(fingerprint(&arena(1847).grid), REFERENCE_FINGERPRINT);
}

const REFERENCE_FINGERPRINT: u64 = 0xDA3ED2F2536D9EC6;

#[test]
fn different_seeds_change_every_island_but_never_the_layout() {
    let layout = ArenaLayout::eggwars();
    let arenas: Vec<Arena> = SEEDS.iter().map(|&seed| arena(seed)).collect();
    for (a, first) in arenas.iter().enumerate() {
        for second in &arenas[a + 1..] {
            assert_eq!(first.grid.origin(), second.grid.origin());
            assert_eq!(first.info.grid_dimensions, second.info.grid_dimensions);
            for id in IslandId::ALL {
                assert_eq!(first.info.island(id).config, second.info.island(id).config);
                assert_ne!(
                    local_blocks(&first.grid, &layout, id),
                    local_blocks(&second.grid, &layout, id),
                    "{id:?}: seeds {} and {}",
                    first.info.global_seed,
                    second.info.global_seed
                );
            }
        }
    }
}

#[test]
fn generation_order_does_not_change_the_arena() {
    let layout = ArenaLayout::eggwars();
    let orders = [
        IslandId::ALL,
        [
            IslandId::West,
            IslandId::East,
            IslandId::South,
            IslandId::North,
            IslandId::Center,
        ],
        [
            IslandId::North,
            IslandId::Center,
            IslandId::West,
            IslandId::South,
            IslandId::East,
        ],
    ];
    for seed in [0, 1847, u64::MAX] {
        let reference = arena(seed);
        for order in orders {
            let mut grid = empty_grid(&layout);
            for id in order {
                let stats =
                    generate_island(&mut grid, &layout.config(id), island_seed(seed, id)).unwrap();
                assert_eq!(stats, reference.info.island(id).stats);
            }
            assert_eq!(grid, reference.grid, "seed {seed}, order {order:?}");
        }
    }
}

#[test]
fn each_island_matches_its_standalone_generation() {
    let layout = ArenaLayout::eggwars();
    for seed in [42, 1847] {
        let arena = arena(seed);
        for id in IslandId::ALL {
            let config = layout.config(id);
            let bounds = config.bounds().unwrap();
            let mut alone = VoxelGrid::try_new(
                bounds.min,
                (bounds.max.x - bounds.min.x + 1) as usize,
                (bounds.max.y - bounds.min.y + 1) as usize,
                (bounds.max.z - bounds.min.z + 1) as usize,
            )
            .unwrap();
            let stats = generate_island(&mut alone, &config, island_seed(seed, id)).unwrap();
            assert_eq!(stats, arena.info.island(id).stats, "{id:?}");
            // The arena holds exactly the standalone island in this island's region: nothing
            // clipped, nothing added.
            for y in bounds.min.y..=bounds.max.y {
                for z in bounds.min.z..=bounds.max.z {
                    for x in bounds.min.x..=bounds.max.x {
                        let position = VoxelPosition::new(x, y, z);
                        assert_eq!(
                            arena.grid.get_world(position),
                            alone.get_world(position),
                            "{id:?} at {position:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn outer_islands_are_independent_variations_not_copies() {
    let layout = ArenaLayout::eggwars();
    for seed in SEEDS {
        let arena = arena(seed);
        let shapes: Vec<_> = OUTER
            .iter()
            .map(|&id| local_blocks(&arena.grid, &layout, id))
            .collect();
        for a in 0..shapes.len() {
            for b in a + 1..shapes.len() {
                assert_ne!(
                    shapes[a], shapes[b],
                    "seed {seed}: {:?} vs {:?}",
                    OUTER[a], OUTER[b]
                );
            }
        }
    }
}

#[test]
fn zero_and_maximum_seeds_generate_complete_arenas() {
    for seed in [0, u64::MAX] {
        let info = arena(seed).info;
        assert_eq!(info.global_seed, seed);
        for island in info.islands {
            assert!(island.stats.total() > 500, "seed {seed}: {:?}", island.id);
        }
    }
}

// ---- Global grid ----

#[test]
fn global_grid_is_exactly_the_union_of_the_island_regions() {
    let layout = ArenaLayout::eggwars();
    let bounds = layout.bounds();
    assert_eq!(bounds.min, VoxelPosition::new(-40, -14, -40));
    assert_eq!(bounds.max, VoxelPosition::new(39, 1, 39));
    assert_eq!(layout.grid_dimensions(), (80, 16, 80));

    let arena = arena(1847);
    assert_eq!(arena.grid.origin(), bounds.min);
    assert_eq!(arena.info.grid_origin, bounds.min);
    assert_eq!(
        (arena.grid.width(), arena.grid.height(), arena.grid.depth()),
        (80, 16, 80)
    );
    assert_eq!(arena.info.grid_dimensions, (80, 16, 80));
    assert_eq!(arena.grid.volume(), 102_400);

    // Every region fits; each face of the global box is some region's face, so it is tight.
    for id in IslandId::ALL {
        let region = region(&layout, id);
        assert!(arena.grid.contains_world(region.min) && arena.grid.contains_world(region.max));
    }
    let regions: Vec<IslandBounds> = IslandId::ALL.map(|id| region(&layout, id)).into();
    assert_eq!(regions.iter().map(|r| r.min.x).min(), Some(bounds.min.x));
    assert_eq!(regions.iter().map(|r| r.max.x).max(), Some(bounds.max.x));
    assert_eq!(regions.iter().map(|r| r.min.y).min(), Some(bounds.min.y));
    assert_eq!(regions.iter().map(|r| r.max.y).max(), Some(bounds.max.y));
    assert_eq!(regions.iter().map(|r| r.min.z).min(), Some(bounds.min.z));
    assert_eq!(regions.iter().map(|r| r.max.z).max(), Some(bounds.max.z));
}

#[test]
fn every_block_belongs_to_exactly_one_island_region() {
    let layout = ArenaLayout::eggwars();
    for seed in SEEDS {
        let arena = arena(seed);
        let mut per_island = [0; 5];
        let all = blocks(&arena.grid);
        for &(position, _) in &all {
            let owners = owners(&layout, position);
            assert_eq!(owners.len(), 1, "seed {seed}: {position:?} in {owners:?}");
            per_island[owners[0] as usize] += 1;
        }
        assert_eq!(
            per_island,
            arena.info.islands.map(|island| island.stats.total())
        );
        assert_eq!(all.len(), arena.info.occupied_voxels);
        // Negative coordinates hold terrain: the west and north islands lie entirely below zero.
        assert!(all.iter().any(|&(p, _)| p.x < -24));
        assert!(all.iter().any(|&(p, _)| p.z < -24));
    }
}

#[test]
fn void_between_the_center_and_each_outer_island_is_empty() {
    let layout = ArenaLayout::eggwars();
    let center = region(&layout, IslandId::Center);
    let grid_bounds = layout.bounds();
    for seed in SEEDS {
        let grid = arena(seed).grid;
        for id in OUTER {
            let outer = region(&layout, id);
            // The corridor between the two regions, across the outer island's full width and
            // the grid's full height: where a bridge will go, there is no rock yet.
            let (min, max) = match id {
                IslandId::North => (
                    (outer.min.x, outer.max.z + 1),
                    (outer.max.x, center.min.z - 1),
                ),
                IslandId::South => (
                    (outer.min.x, center.max.z + 1),
                    (outer.max.x, outer.min.z - 1),
                ),
                IslandId::East => (
                    (center.max.x + 1, outer.min.z),
                    (outer.min.x - 1, outer.max.z),
                ),
                IslandId::West => (
                    (outer.max.x + 1, outer.min.z),
                    (center.min.x - 1, outer.max.z),
                ),
                IslandId::Center => unreachable!(),
            };
            let mut cells = 0;
            for y in grid_bounds.min.y..=grid_bounds.max.y {
                for z in min.1..=max.1 {
                    for x in min.0..=max.0 {
                        assert!(
                            !grid.is_occupied_world(VoxelPosition::new(x, y, z)),
                            "seed {seed}: rock at ({x}, {y}, {z}) toward {id:?}"
                        );
                        cells += 1;
                    }
                }
            }
            assert_eq!(cells, 12 * 16 * 16, "{id:?}");
        }
    }
}

#[test]
fn islands_are_physically_separate_landmasses() {
    let layout = ArenaLayout::eggwars();
    for seed in [0, 1847, u64::MAX] {
        let arena = arena(seed);
        for id in IslandId::ALL {
            // The flat grass at the island's center, then everything face-connected to it.
            let center = layout.config(id).center;
            assert_eq!(
                arena.grid.get_world(center).and_then(|v| v.block()),
                Some(BlockType::Grass)
            );
            let mut seen = HashSet::from([(center.x, center.y, center.z)]);
            let mut queue = VecDeque::from([center]);
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
                    if arena.grid.is_occupied_world(n) && seen.insert((n.x, n.y, n.z)) {
                        queue.push_back(n);
                    }
                }
            }
            // The landmass is the whole island and nothing beyond its region.
            assert_eq!(seen.len(), arena.info.island(id).stats.total(), "{id:?}");
            let bounds = region(&layout, id);
            assert!(
                seen.iter()
                    .all(|&(x, y, z)| bounds.contains(VoxelPosition::new(x, y, z)))
            );
        }
    }
}

// ---- Materials ----

#[test]
fn every_island_is_grass_over_dirt_over_rock_with_terrain_blocks_only() {
    let layout = ArenaLayout::eggwars();
    for seed in SEEDS {
        let arena = arena(seed);
        for island in arena.info.islands {
            let stats = island.stats;
            for block in [BlockType::Grass, BlockType::Dirt, BlockType::Cobblestone] {
                assert!(
                    stats.count(block) > 0,
                    "seed {seed}: {:?} lacks {block:?}",
                    island.id
                );
            }
            for block in BlockType::ALL {
                if !TERRAIN.contains(&block) {
                    assert_eq!(stats.count(block), 0, "{block:?}");
                }
            }

            let bounds = region(&layout, island.id);
            for z in bounds.min.z..=bounds.max.z {
                for x in bounds.min.x..=bounds.max.x {
                    let column = column_blocks(&arena.grid, x, z);
                    if column.is_empty() {
                        continue;
                    }
                    assert_eq!(column[0].1, BlockType::Grass, "({x}, {z})");
                    assert_eq!(column[1].1, BlockType::Dirt, "({x}, {z})");
                    let bottom = column.last().unwrap().1;
                    assert!(
                        bottom == BlockType::Cobblestone || ORES.contains(&bottom),
                        "({x}, {z}) ends in {bottom:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn ores_keep_their_relative_rarity_across_arenas() {
    let mut totals = [0; 4];
    for seed in 0..24 {
        let info = arena(seed).info;
        for (total, ore) in totals.iter_mut().zip(ORES) {
            *total += info.count(ore);
        }
        assert_eq!(info.ores(), ORES.iter().map(|&ore| info.count(ore)).sum());
    }
    let [coal, iron, gold, diamond] = totals;
    assert!(diamond > 0, "{totals:?}");
    assert!(coal > iron && iron > gold && gold > diamond, "{totals:?}");
}

#[test]
fn central_island_is_larger_with_a_flat_middle() {
    let layout = ArenaLayout::eggwars();
    for seed in SEEDS {
        let arena = arena(seed);
        let center = arena.info.island(IslandId::Center).stats.total();
        for id in OUTER {
            assert!(
                center > arena.info.island(id).stats.total(),
                "seed {seed}: {id:?}"
            );
        }
        // A 6×6 square around the center line lies inside the always-flat radius.
        let surface = layout.config(IslandId::Center).center.y;
        for z in -3..=2 {
            for x in -3..=2 {
                let column = column_blocks(&arena.grid, x, z);
                assert_eq!(
                    column[0],
                    (surface, BlockType::Grass),
                    "seed {seed}: ({x}, {z})"
                );
            }
        }
    }
}
