use std::collections::{HashMap, HashSet, VecDeque};

use super::{
    CONSTRUCTION_TOP, CentralStructure, ConstructionError, DECK_Y, EGG_Y, EggWarsArena, FLOOR_Y,
    Frame, OUTER_ISLANDS, Plan, build_eggwars_arena, construct, construction_bounds,
    plan_construction,
    structures::{BASE_FORWARD, BASE_SIDES, EGG_CELL, GATE_FORWARD, GATE_SIDES, WINDOWS},
    surface_anchor,
};
use crate::{
    voxel::{BlockType, Voxel, VoxelGrid, VoxelPosition},
    world::{
        Arena, ArenaError, ArenaLayout, BridgeState, DefenseState, IslandBounds, IslandId,
        arena::{CENTER_FOOTPRINT, OUTER_FOOTPRINT, SURFACE_LEVEL},
        construction::{DEFENSE_SLOTS, DEFENSE_TEMPLATE, EggBaseDesign},
        generate_arena, generate_arena_in, generate_island,
        island::{
            SILHOUETTE_MIN_RADIUS,
            tests::{blocks, column_blocks, fingerprint},
        },
        island_seed,
    },
};

const SEEDS: [u64; 6] = [0, 1, 42, 1847, 2026, u64::MAX];
/// Side offsets of the two bridge lanes, straddling each island's center line.
const LANES: [i32; 2] = [-1, 0];
/// Index of the cap block directly above the egg in `DEFENSE_TEMPLATE`.
const CAP: usize = 8;

type Cell = (i32, i32, i32);

fn built(seed: u64) -> EggWarsArena {
    build_eggwars_arena(seed).unwrap()
}

/// The Mission 29 terrain-only arena, in its exact grid.
fn terrain(seed: u64) -> Arena {
    generate_arena(&ArenaLayout::eggwars(), seed).unwrap()
}

/// The terrain in the construction grid, before construction.
fn terrain_grid(seed: u64) -> VoxelGrid {
    let layout = ArenaLayout::eggwars();
    generate_arena_in(&layout, seed, construction_bounds(&layout))
        .unwrap()
        .grid
}

fn cell(p: VoxelPosition) -> Cell {
    (p.x, p.y, p.z)
}

fn at(grid: &VoxelGrid, (x, y, z): Cell) -> Option<BlockType> {
    grid.get_world(VoxelPosition::new(x, y, z))
        .and_then(Voxel::block)
}

fn occupied(grid: &VoxelGrid, (x, y, z): Cell) -> bool {
    grid.is_occupied_world(VoxelPosition::new(x, y, z))
}

/// Blocks construction added: in the constructed grid but not in the terrain.
fn construction_blocks(arena: &EggWarsArena) -> Vec<(VoxelPosition, BlockType)> {
    let terrain = terrain_grid(arena.terrain.global_seed);
    blocks(&arena.grid)
        .into_iter()
        .filter(|&(p, _)| !terrain.is_occupied_world(p))
        .collect()
}

fn frame(island: IslandId) -> Frame {
    Frame::facing_center(&ArenaLayout::eggwars(), island)
}

/// Forward cells of the void between an outer island's region and the center's region.
fn void_cells() -> std::ops::RangeInclusive<i32> {
    let separation = ArenaLayout::eggwars().separation() as i32;
    OUTER_FOOTPRINT as i32 / 2..=separation - CENTER_FOOTPRINT as i32 / 2 - 1
}

/// Whether any cell of the full-height column at lane `side`, forward cell `f` holds a block.
fn lane_column_occupied(grid: &VoxelGrid, island: IslandId, side: i32, f: i32) -> bool {
    let (x, z) = frame(island).cell(side, f);
    let origin = grid.origin();
    (origin.y..origin.y + grid.height() as i32).any(|y| occupied(grid, (x, y, z)))
}

/// For each void cell, whether either lane holds anything there.
fn void_pattern(grid: &VoxelGrid, island: IslandId) -> Vec<bool> {
    void_cells()
        .map(|f| {
            LANES
                .iter()
                .any(|&side| lane_column_occupied(grid, island, side, f))
        })
        .collect()
}

/// Lengths of the runs of void cells where neither lane holds a block.
fn missing_runs(pattern: &[bool]) -> Vec<usize> {
    pattern
        .split(|&built| built)
        .map(<[bool]>::len)
        .filter(|&run| run > 0)
        .collect()
}

/// Face-connected components of every occupied cell.
fn components(grid: &VoxelGrid) -> HashMap<Cell, usize> {
    let mut labels = HashMap::new();
    for (start, _) in blocks(grid) {
        if labels.contains_key(&cell(start)) {
            continue;
        }
        let label = labels.len();
        let mut queue = VecDeque::from([cell(start)]);
        labels.insert(cell(start), label);
        while let Some((x, y, z)) = queue.pop_front() {
            for (dx, dy, dz) in [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                let next = (x + dx, y + dy, z + dz);
                if occupied(grid, next) && !labels.contains_key(&next) {
                    labels.insert(next, label);
                    queue.push_back(next);
                }
            }
        }
    }
    labels
}

/// The component holding the grass at an island's center.
fn island_component(labels: &HashMap<Cell, usize>, island: IslandId) -> usize {
    let center = ArenaLayout::eggwars().config(island).center;
    labels[&cell(center)]
}

fn deck_cell(island: IslandId, side: i32, f: i32) -> Cell {
    let (x, z) = frame(island).cell(side, f);
    (x, DECK_Y, z)
}

// ---- Grid expansion and terrain stability ----

#[test]
fn construction_grid_raises_only_the_top_to_the_tallest_structure() {
    let layout = ArenaLayout::eggwars();
    let bounds = construction_bounds(&layout);
    assert_eq!(CONSTRUCTION_TOP, 5);
    assert_eq!(bounds.min, layout.bounds().min);
    assert_eq!(bounds.min, VoxelPosition::new(-40, -14, -40));
    assert_eq!(bounds.max, VoxelPosition::new(39, CONSTRUCTION_TOP, 39));

    for seed in SEEDS {
        let arena = built(seed);
        let grid = &arena.grid;
        assert_eq!(grid.origin(), bounds.min);
        assert_eq!((grid.width(), grid.height(), grid.depth()), (80, 20, 80));
        assert_eq!(grid.volume(), 128_000);
        assert_eq!(arena.terrain.grid_dimensions, (80, 20, 80));
        assert_eq!(arena.terrain.grid_origin, bounds.min);
        // The top layer is exactly the tallest construction: no empty volume above it.
        let added = construction_blocks(&arena);
        assert_eq!(added.iter().map(|(p, _)| p.y).max(), Some(CONSTRUCTION_TOP));
        // Construction stays inside the terrain's horizontal extent.
        let terrain_bounds = layout.bounds();
        for (p, _) in &added {
            assert!((terrain_bounds.min.x..=terrain_bounds.max.x).contains(&p.x));
            assert!((terrain_bounds.min.z..=terrain_bounds.max.z).contains(&p.z));
        }
    }
}

#[test]
fn terrain_in_the_taller_grid_is_identical_to_the_mission_29_arena() {
    let layout = ArenaLayout::eggwars();
    for seed in SEEDS {
        let reference = terrain(seed);
        let taller = generate_arena_in(&layout, seed, construction_bounds(&layout)).unwrap();
        assert_eq!(taller.info.islands, reference.info.islands, "seed {seed}");
        assert_eq!(taller.info.occupied_voxels, reference.info.occupied_voxels);
        assert_eq!(built(seed).terrain.islands, reference.info.islands);

        // Cell for cell inside the old grid, and nothing at all above it.
        let mut copy = VoxelGrid::try_new(
            reference.grid.origin(),
            reference.grid.width(),
            reference.grid.height(),
            reference.grid.depth(),
        )
        .unwrap();
        for (position, voxel) in blocks(&taller.grid) {
            assert!(
                reference.grid.contains_world(position),
                "seed {seed}: {position:?}"
            );
            copy.set_world(position, Voxel::Block(voxel)).unwrap();
        }
        assert_eq!(copy, reference.grid, "seed {seed}");
    }
    // The pinned Mission 29 fingerprint still describes the terrain stage of the taller grid.
    let taller = generate_arena_in(&layout, 1847, construction_bounds(&layout)).unwrap();
    let mut copy = VoxelGrid::try_new(layout.bounds().min, 80, 16, 80).unwrap();
    for (position, block) in blocks(&taller.grid) {
        copy.set_world(position, Voxel::Block(block)).unwrap();
    }
    assert_eq!(fingerprint(&copy), 0xDA3ED2F2536D9EC6);
}

#[test]
fn construction_never_replaces_terrain() {
    for seed in SEEDS {
        let arena = built(seed);
        let reference = terrain(seed);
        for (position, block) in blocks(&reference.grid) {
            assert_eq!(
                arena.grid.get_world(position),
                Some(Voxel::Block(block)),
                "seed {seed}: terrain at {position:?}"
            );
        }
        let added = construction_blocks(&arena);
        assert_eq!(added.len(), arena.construction.total());
        assert_eq!(
            blocks(&arena.grid).len(),
            reference.info.occupied_voxels + added.len()
        );
        assert_eq!(arena.occupied_voxels(), blocks(&arena.grid).len());
        for block in BlockType::ALL {
            let count = added.iter().filter(|&&(_, b)| b == block).count();
            assert_eq!(count, arena.construction.count(block), "{block:?}");
        }
        // Only the designed building blocks (and the ruin's lava pool): no terrain types.
        for (_, block) in &added {
            assert!(matches!(
                block,
                BlockType::Cobblestone | BlockType::Obsidian | BlockType::Glass | BlockType::Lava
            ));
        }
    }
}

#[test]
fn arena_bounds_must_contain_every_island() {
    let layout = ArenaLayout::eggwars();
    let mut clipped = construction_bounds(&layout);
    clipped.min.y += 1;
    assert_eq!(
        generate_arena_in(&layout, 1847, clipped).map(|_| ()),
        Err(ArenaError::GridTooSmall)
    );
    let mut narrow = layout.bounds();
    narrow.max.x -= 1;
    assert_eq!(
        generate_arena_in(&layout, 1847, narrow).map(|_| ()),
        Err(ArenaError::GridTooSmall)
    );
    assert!(
        ArenaError::GridTooSmall
            .to_string()
            .contains("every island")
    );
}

#[test]
fn construction_refuses_to_clip_or_overwrite_and_leaves_the_grid_untouched() {
    // The Mission 29 grid ends at y = 1: the design does not fit (its raised grass has no free
    // cell above it, and structures rise above y = 1), so nothing is written.
    let mut exact = terrain(1847).grid;
    let before = exact.clone();
    assert!(matches!(
        construct(&mut exact, 1847),
        Err(ConstructionError::OutsideGrid(_) | ConstructionError::MissingTerrain { .. })
    ));
    assert_eq!(exact, before);

    // A block already standing where the design puts obsidian is never overwritten.
    let slot = built(1847).construction.bases[0].defense[0].position;
    let mut blocked = terrain_grid(1847);
    blocked
        .set_world(slot, Voxel::Block(BlockType::Dirt))
        .unwrap();
    let before = blocked.clone();
    assert_eq!(
        construct(&mut blocked, 1847),
        Err(ConstructionError::Occupied(slot))
    );
    assert_eq!(blocked, before);

    // Building twice fails as well: the first construction now tops the columns.
    let mut twice = built(1847).grid;
    let before = twice.clone();
    assert!(construct(&mut twice, 1847).is_err());
    assert_eq!(twice, before);

    // A plan naming one cell twice is rejected as well.
    let grid = terrain_grid(1847);
    let mut plan = Plan::default();
    let free = VoxelPosition::new(0, 3, 0);
    plan.put(free, BlockType::Cobblestone);
    plan.put(free, BlockType::Glass);
    assert_eq!(plan.validate(&grid), Err(ConstructionError::Occupied(free)));
    assert!(
        ConstructionError::MissingTerrain { x: 3, z: -4 }
            .to_string()
            .contains("(3, -4)")
    );
}

#[test]
fn negative_coordinates_hold_construction() {
    let arena = built(1847);
    let added = construction_blocks(&arena);
    // North (-Z) and West (-X) bases and bridges lie entirely at negative coordinates.
    for island in [IslandId::North, IslandId::West] {
        let base = arena.construction.base(island).unwrap();
        assert!(base.egg.x < 0 || base.egg.z < 0);
        assert_eq!(
            at(&arena.grid, cell(base.defense[3].position)),
            Some(BlockType::Obsidian)
        );
    }
    assert!(added.iter().any(|&(p, _)| p.x < -24));
    assert!(added.iter().any(|&(p, _)| p.z < -24));
    assert!(added.iter().any(|&(p, _)| p.y < 0));
}

// ---- Anchors ----

#[test]
fn anchors_rest_on_the_topmost_terrain_block() {
    for seed in [0, 1847, u64::MAX] {
        let grid = terrain_grid(seed);
        let origin = grid.origin();
        for z in origin.z..origin.z + grid.depth() as i32 {
            for x in origin.x..origin.x + grid.width() as i32 {
                let column = column_blocks(&grid, x, z);
                match surface_anchor(&grid, x, z) {
                    None => assert!(column.is_empty(), "({x}, {z})"),
                    Some(anchor) => {
                        let (top, block) = column[0];
                        assert_eq!(block, BlockType::Grass);
                        assert_eq!((anchor.x, anchor.y, anchor.z), (x, top + 1, z));
                        assert!(!grid.is_occupied_world(anchor), "anchor is never buried");
                        assert!((SURFACE_LEVEL..=SURFACE_LEVEL + 2).contains(&anchor.y));
                    }
                }
            }
        }
        // Outside the grid there is no anchor.
        assert_eq!(surface_anchor(&grid, origin.x - 1, 0), None);
    }
}

#[test]
fn anchors_are_measured_not_assumed() {
    // A column whose top sits at the grid's top layer has no free cell above it.
    let mut grid = VoxelGrid::try_new(VoxelPosition::new(-2, -3, -2), 4, 4, 4).unwrap();
    grid.set_world(VoxelPosition::new(0, -2, 0), Voxel::Block(BlockType::Grass))
        .unwrap();
    grid.set_world(VoxelPosition::new(1, 0, 1), Voxel::Block(BlockType::Grass))
        .unwrap();
    assert_eq!(
        surface_anchor(&grid, 0, 0),
        Some(VoxelPosition::new(0, -1, 0))
    );
    assert_eq!(surface_anchor(&grid, 1, 1), None);
    assert_eq!(surface_anchor(&grid, -1, -1), None);

    for seed in SEEDS {
        let arena = built(seed);
        let layout = ArenaLayout::eggwars();
        for id in IslandId::ALL {
            let center = layout.config(id).center;
            let measured = column_blocks(&terrain(seed).grid, center.x, center.z)[0].0;
            assert_eq!(arena.construction.surface_levels[id as usize], measured);
            assert_eq!(measured, SURFACE_LEVEL);
        }
    }
}

#[test]
fn designed_cells_lie_inside_every_possible_island_silhouette() {
    // Structures need ground under every column for any seed: the cell centers lie within the
    // smallest radius the silhouette can take, so their columns always exist.
    let inside = |side: i32, forward: i32, half: f32| {
        let (a, b) = (side as f32 + 0.5, forward as f32 + 0.5);
        (a * a + b * b).sqrt() / half <= SILHOUETTE_MIN_RADIUS
    };
    let outer_half = OUTER_FOOTPRINT as f32 / 2.0;
    for f in BASE_FORWARD {
        for s in BASE_SIDES {
            assert!(inside(s, f, outer_half), "floor ({s}, {f})");
        }
    }
    for s in GATE_SIDES {
        assert!(inside(s, GATE_FORWARD, outer_half), "gate {s}");
    }
    let center = super::structures::CENTRAL_TOP;
    assert_eq!(center, CONSTRUCTION_TOP);
    let center_half = CENTER_FOOTPRINT as f32 / 2.0;
    for (x, z) in [(-4, -4), (3, 3), (-4, 3), (3, -4)] {
        assert!(inside(x, z, center_half), "central corner ({x}, {z})");
    }
}

#[test]
fn future_egg_cells_are_empty_and_stand_on_the_base_floor() {
    let layout = ArenaLayout::eggwars();
    for seed in SEEDS {
        let arena = built(seed);
        for base in arena.construction.bases {
            let (x, y, z) = cell(base.egg);
            assert_eq!(y, EGG_Y);
            assert!(arena.grid.contains_world(base.egg));
            assert!(!occupied(&arena.grid, (x, y, z)), "{:?}", base.island);
            assert!(matches!(
                at(&arena.grid, (x, y - 1, z)),
                Some(BlockType::Cobblestone | BlockType::Grass)
            ));
            // Only the defense's cap may stand above the egg, and it is the one block there.
            let cap = base.defense[CAP];
            assert_eq!(cell(cap.position), (x, y + 1, z));
            assert_eq!(occupied(&arena.grid, (x, y + 1, z)), cap.intact);
            for above in y + 2..=CONSTRUCTION_TOP {
                assert!(!occupied(&arena.grid, (x, above, z)));
            }
            // On its own island, on that island's center-line lane.
            let region = layout.config(base.island).bounds().unwrap();
            assert!(region.contains(VoxelPosition::new(x, SURFACE_LEVEL, z)));
            let (ex, ez) = frame(base.island).cell(EGG_CELL.0, EGG_CELL.1);
            assert_eq!((ex, ez), (x, z));
        }
    }
}

// ---- Bridges ----

#[test]
fn bridge_states_follow_the_narrative() {
    let info = built(1847).construction;
    assert_eq!(
        OUTER_ISLANDS.map(|id| info.bridge(id).unwrap().state),
        [
            BridgeState::Complete,
            BridgeState::Severed,
            BridgeState::Damaged,
            BridgeState::Incomplete,
        ]
    );
    assert!(info.bridge(IslandId::Center).is_none());
    assert_eq!(BridgeState::of(IslandId::Center), None);
    for bridge in info.bridges {
        // Every bridge points from its island toward the center.
        let (dx, dz) = bridge.island.direction();
        assert_eq!(bridge.toward_center, (-dx, -dz));
        for lane in bridge.lanes {
            assert_eq!(lane.outer_end.y, DECK_Y);
            assert_eq!(lane.center_end.y, DECK_Y);
            let along = |p: VoxelPosition| p.x * -dx + p.z * -dz;
            assert!(along(lane.outer_end) < along(lane.center_end));
        }
    }
}

#[test]
fn north_bridge_is_a_continuous_route_to_the_center() {
    for seed in SEEDS {
        let arena = built(seed);
        let bridge = arena.construction.bridge(IslandId::North).unwrap();
        assert_eq!(bridge.missing_span, 0);
        assert_eq!(
            missing_runs(&void_pattern(&arena.grid, IslandId::North)),
            []
        );
        // Both lanes hold cobblestone from end to end.
        for lane in bridge.lanes {
            let (start, end) = (lane.outer_end, lane.center_end);
            let (dx, dz) = bridge.toward_center;
            let steps = (end.x - start.x) * dx + (end.z - start.z) * dz;
            for k in 0..=steps {
                let p = (start.x + dx * k, DECK_Y, start.z + dz * k);
                assert_eq!(
                    at(&arena.grid, p),
                    Some(BlockType::Cobblestone),
                    "seed {seed}"
                );
            }
        }
        let labels = components(&arena.grid);
        assert_eq!(
            island_component(&labels, IslandId::North),
            island_component(&labels, IslandId::Center),
            "seed {seed}"
        );
    }
}

#[test]
fn east_bridge_has_one_real_missing_section() {
    for seed in SEEDS {
        let arena = built(seed);
        let bridge = arena.construction.bridge(IslandId::East).unwrap();
        let pattern = void_pattern(&arena.grid, IslandId::East);
        // One gap of exactly three empty columns, with bridge left on both sides of it.
        assert_eq!(missing_runs(&pattern), [3], "seed {seed}");
        assert_eq!(bridge.missing_span, 3);
        assert!(pattern[0] && pattern[pattern.len() - 1]);

        let labels = components(&arena.grid);
        let (east, center) = (
            island_component(&labels, IslandId::East),
            island_component(&labels, IslandId::Center),
        );
        assert_ne!(east, center, "seed {seed}");
        let (first, last) = (*void_cells().start(), *void_cells().end());
        for side in LANES {
            assert_eq!(labels[&deck_cell(IslandId::East, side, first)], east);
            assert_eq!(labels[&deck_cell(IslandId::East, side, last)], center);
        }
    }
}

#[test]
fn south_bridge_is_severed_by_a_longer_span() {
    for seed in SEEDS {
        let arena = built(seed);
        let bridge = arena.construction.bridge(IslandId::South).unwrap();
        let pattern = void_pattern(&arena.grid, IslandId::South);
        let runs = missing_runs(&pattern);
        assert_eq!(runs, [6], "seed {seed}");
        assert_eq!(bridge.missing_span, 6);
        assert!(
            bridge.missing_span
                > arena
                    .construction
                    .bridge(IslandId::East)
                    .unwrap()
                    .missing_span
        );
        assert!(pattern[0] && pattern[pattern.len() - 1]);

        // Two surviving segments, each still attached to its own island and not to the other.
        let labels = components(&arena.grid);
        let (south, center) = (
            island_component(&labels, IslandId::South),
            island_component(&labels, IslandId::Center),
        );
        assert_ne!(south, center, "seed {seed}");
        let first = *void_cells().start();
        let last = *void_cells().end();
        assert!(
            LANES
                .iter()
                .any(|&s| labels.get(&deck_cell(IslandId::South, s, first)) == Some(&south))
        );
        assert!(
            LANES
                .iter()
                .any(|&s| labels.get(&deck_cell(IslandId::South, s, last)) == Some(&center))
        );
    }
}

#[test]
fn west_bridge_stops_short_of_the_center() {
    for seed in SEEDS {
        let arena = built(seed);
        let bridge = arena.construction.bridge(IslandId::West).unwrap();
        let pattern = void_pattern(&arena.grid, IslandId::West);
        // Built from the island outward, then nothing all the way to the center's region.
        let runs = missing_runs(&pattern);
        assert_eq!(runs.len(), 1, "seed {seed}");
        assert!(pattern[0]);
        assert!(!pattern[pattern.len() - 1]);
        assert!(runs[0] >= 6, "seed {seed}: {runs:?}");
        assert!(bridge.missing_span as usize >= runs[0]);

        // No matching segment grows from the central island: the planned center end is bare.
        for lane in bridge.lanes {
            assert!(!occupied(&arena.grid, cell(lane.center_end)));
        }
        let labels = components(&arena.grid);
        let west = island_component(&labels, IslandId::West);
        assert_ne!(west, island_component(&labels, IslandId::Center));
        let first = *void_cells().start();
        for side in LANES {
            assert_eq!(labels[&deck_cell(IslandId::West, side, first)], west);
        }
    }
}

#[test]
fn bridge_ends_touch_terrain_face_to_face() {
    for seed in SEEDS {
        let arena = built(seed);
        let terrain = terrain_grid(seed);
        for bridge in arena.construction.bridges {
            let (dx, dz) = bridge.toward_center;
            for lane in bridge.lanes {
                let outer = cell(lane.outer_end);
                assert_eq!(at(&arena.grid, outer), Some(BlockType::Cobblestone));
                assert!(!occupied(&terrain, outer));
                // The block toward the island at deck level is terrain: no gap at the end.
                assert!(
                    occupied(&terrain, (outer.0 - dx, DECK_Y, outer.2 - dz)),
                    "seed {seed}"
                );

                let center = cell(lane.center_end);
                assert!(!occupied(&terrain, center));
                assert!(occupied(&terrain, (center.0 + dx, DECK_Y, center.2 + dz)));
                let attached = bridge.state != BridgeState::Incomplete;
                assert_eq!(
                    occupied(&arena.grid, center),
                    attached,
                    "{:?}",
                    bridge.island
                );
            }
        }
    }
}

#[test]
fn bridge_decks_run_level_with_the_grass_and_are_never_buried() {
    for seed in SEEDS {
        let arena = built(seed);
        for (p, block) in construction_blocks(&arena) {
            if p.y > DECK_Y {
                continue;
            }
            // The lava spill hangs below deck level beside the rim; the rest is bridge.
            let spill = arena.construction.central.lava_spill;
            if block == BlockType::Lava {
                assert!(
                    p == spill.source || spill.fall.contains(p),
                    "seed {seed}: {p:?}"
                );
                continue;
            }
            // Everything at or below deck level is bridge: cobblestone on a bridge lane.
            assert_eq!(block, BlockType::Cobblestone);
            // Bridges reach onto sunken rim grass, but never in to the central ruin's floor.
            let on_lane =
                (LANES.contains(&p.x) && p.z.abs() > 4) || (LANES.contains(&p.z) && p.x.abs() > 4);
            assert!(on_lane, "seed {seed}: {p:?}");
            if p.y == DECK_Y {
                assert!(
                    !occupied(&arena.grid, (p.x, p.y + 1, p.z)),
                    "buried at {p:?}"
                );
            } else {
                // A scaffold block hangs directly under the deck.
                assert_eq!(p.y, DECK_Y - 1);
                assert!(occupied(&arena.grid, (p.x, DECK_Y, p.z)));
            }
        }
    }
}

#[test]
fn only_the_north_bridge_connects_two_islands() {
    for seed in [0, 1847, u64::MAX] {
        let arena = built(seed);
        let labels = components(&arena.grid);
        let of = |id| island_component(&labels, id);
        assert_eq!(of(IslandId::North), of(IslandId::Center));
        for (a, &first) in IslandId::ALL.iter().enumerate() {
            for &second in &IslandId::ALL[a + 1..] {
                if [first, second] != [IslandId::Center, IslandId::North] {
                    assert_ne!(of(first), of(second), "seed {seed}: {first:?}-{second:?}");
                }
            }
        }
        // Nothing floats but the defense caps: every other block belongs to an island's landmass,
        // and an intact cap, one block above the empty egg cell, touches the ring only by edges.
        let landmasses: HashSet<usize> = IslandId::ALL.iter().map(|&id| of(id)).collect();
        assert_eq!(landmasses.len(), 4);
        let caps: HashSet<Cell> = arena
            .construction
            .bases
            .iter()
            .filter(|base| base.defense[CAP].intact)
            .map(|base| cell(base.defense[CAP].position))
            .collect();
        for (&position, label) in &labels {
            assert_eq!(
                landmasses.contains(label),
                !caps.contains(&position),
                "seed {seed}: {position:?}"
            );
        }
    }
}

#[test]
fn bridge_narratives_survive_every_seed() {
    for seed in (0..48).chain([1847, 2026, u64::MAX - 1, u64::MAX]) {
        let arena = built(seed);
        let runs = |id| missing_runs(&void_pattern(&arena.grid, id));
        assert_eq!(runs(IslandId::North), [], "seed {seed}");
        assert_eq!(runs(IslandId::East), [3], "seed {seed}");
        assert_eq!(runs(IslandId::South), [6], "seed {seed}");
        let west = runs(IslandId::West);
        assert!(west.len() == 1 && west[0] >= 6, "seed {seed}: {west:?}");
        let pattern = void_pattern(&arena.grid, IslandId::West);
        assert!(pattern[0] && !pattern[pattern.len() - 1]);
    }
}

// ---- Egg defenses ----

/// `(side, forward, up)` of `position` relative to its base's egg, in the island's own frame.
fn local_offset(base: &super::EggBase, position: VoxelPosition) -> (i32, i32, i32) {
    let (ax, az) = base.attack_direction;
    let (sx, sz) = (az, -ax);
    let (ox, oz) = (position.x - base.egg.x, position.z - base.egg.z);
    (
        ox * sx + oz * sz,
        ox * ax + oz * az,
        position.y - base.egg.y,
    )
}

/// Template offsets of the positions that are missing in `island`'s defense.
fn missing_offsets(base: &super::EggBase) -> Vec<(i32, i32, i32)> {
    base.defense
        .iter()
        .filter(|slot| !slot.intact)
        .map(|slot| local_offset(base, slot.position))
        .collect()
}

#[test]
fn the_template_is_the_nine_block_eggwars_enclosure() {
    assert_eq!(DEFENSE_SLOTS, 9);
    assert_eq!(DEFENSE_TEMPLATE.len(), 9);
    let unique: HashSet<_> = DEFENSE_TEMPLATE.into_iter().collect();
    assert_eq!(unique.len(), 9, "no duplicate positions");

    // Eight positions around the egg at its own level, none in the egg's cell.
    let ring: Vec<_> = DEFENSE_TEMPLATE.iter().filter(|t| t.2 == 0).collect();
    assert_eq!(ring.len(), 8);
    for &&(side, forward, _) in &ring {
        assert!(side.abs() <= 1 && forward.abs() <= 1);
        assert_ne!((side, forward), (0, 0));
    }
    let ring_cells: HashSet<_> = ring.iter().map(|t| (t.0, t.1)).collect();
    assert_eq!(ring_cells.len(), 8, "the full 3x3 minus the center");

    // One position directly above the egg; no 3x3x3 solid: nothing below, nothing else above.
    let above: Vec<_> = DEFENSE_TEMPLATE.iter().filter(|t| t.2 != 0).collect();
    assert_eq!(above, [&(0, 0, 1)]);
    assert_eq!(DEFENSE_TEMPLATE[CAP], (0, 0, 1));
    assert_eq!(
        DEFENSE_TEMPLATE.iter().filter(|t| t.2 == 1).count(),
        1,
        "exactly one upper block"
    );
}

#[test]
fn defenses_hold_nine_seven_five_and_three_obsidian_blocks() {
    for seed in SEEDS {
        let arena = built(seed);
        for (island, state, intact) in [
            (IslandId::North, DefenseState::Intact, 9),
            (IslandId::East, DefenseState::Breached, 5),
            (IslandId::South, DefenseState::Destroyed, 3),
            (IslandId::West, DefenseState::Damaged, 7),
        ] {
            let base = arena.construction.base(island).unwrap();
            assert_eq!(base.defense_state, state);
            assert_eq!(DefenseState::of(island), Some(state));
            assert_eq!(base.defense.len(), 9);
            // Real voxels, not labels: obsidian where intact, air where broken.
            let mut held = 0;
            for slot in base.defense {
                let block = at(&arena.grid, cell(slot.position));
                if slot.intact {
                    assert_eq!(block, Some(BlockType::Obsidian), "{island:?}");
                    held += 1;
                } else {
                    assert_eq!(block, None, "{island:?}");
                }
            }
            assert_eq!(held, intact, "seed {seed}: {island:?}");
            assert_eq!(9 - held, 9 - intact);
            assert_eq!(state.intact_slots().iter().filter(|&&i| i).count(), intact);

            // Counted again straight from the grid over the whole 3x3x3 volume around the egg:
            // the defense is the only obsidian there, and the egg's own cell is empty.
            let egg = cell(base.egg);
            let mut volume = 0;
            for dy in -1..=2 {
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let p = (egg.0 + dx, egg.1 + dy, egg.2 + dz);
                        if at(&arena.grid, p) == Some(BlockType::Obsidian) {
                            volume += 1;
                        }
                    }
                }
            }
            assert_eq!(volume, intact, "seed {seed}: {island:?}");
            assert!(!occupied(&arena.grid, egg));
        }
        assert!(arena.construction.base(IslandId::Center).is_none());
    }
}

#[test]
fn defense_positions_are_distinct_in_bounds_and_overwrite_no_terrain() {
    for seed in SEEDS {
        let arena = built(seed);
        let terrain = terrain_grid(seed);
        let mut all = HashSet::new();
        for base in arena.construction.bases {
            for slot in base.defense {
                assert!(arena.grid.contains_world(slot.position));
                assert!(all.insert(cell(slot.position)), "duplicate {slot:?}");
                assert_ne!(slot.position, base.egg);
                // Missing or intact, the cell was free in the terrain, so nothing was overwritten
                // and a destroyed block has not carved the ground.
                assert!(!terrain.is_occupied_world(slot.position), "{slot:?}");
                // The ring stands on the base floor; the cap stands on the egg's own level.
                let (side, forward, up) = local_offset(&base, slot.position);
                assert_eq!(slot.position.y, EGG_Y + up);
                assert!(BASE_SIDES.contains(&(EGG_CELL.0 + side)));
                assert!(BASE_FORWARD.contains(&(EGG_CELL.1 + forward)));
                let floor = (slot.position.x, FLOOR_Y, slot.position.z);
                if up == 0 {
                    assert!(
                        matches!(
                            at(&arena.grid, floor),
                            Some(BlockType::Cobblestone | BlockType::Grass)
                        ),
                        "seed {seed}: {:?} floor under {slot:?}",
                        base.island
                    );
                }
            }
        }
        assert_eq!(all.len(), 36);
    }
}

#[test]
fn every_island_uses_the_same_template_turned_toward_the_center() {
    let arena = built(1847);
    let mut shapes = HashSet::new();
    for base in arena.construction.bases {
        let (ax, az) = base.attack_direction;
        let (dx, dz) = base.island.direction();
        assert_eq!((ax, az), (-dx, -dz));
        for (slot, template) in base.defense.iter().zip(DEFENSE_TEMPLATE) {
            assert_eq!(
                local_offset(&base, slot.position),
                template,
                "{:?}",
                base.island
            );
        }
        // The world-space rotation really differs per island: the front row faces the center.
        let front = base.defense[1].position;
        assert_eq!(
            (front.x - base.egg.x, front.z - base.egg.z),
            (ax, az),
            "{:?}",
            base.island
        );
        shapes.insert((ax, az));
    }
    assert_eq!(shapes.len(), 4, "four distinct orientations");
}

#[test]
fn north_defense_is_complete() {
    for seed in SEEDS {
        let arena = built(seed);
        let base = arena.construction.base(IslandId::North).unwrap();
        assert!(base.defense.iter().all(|slot| slot.intact));
        assert!(missing_offsets(base).is_empty());
        // The egg is sealed on all four sides, the corners, and from above.
        let (x, y, z) = cell(base.egg);
        for dz in -1..=1 {
            for dx in -1..=1 {
                assert_eq!(
                    occupied(&arena.grid, (x + dx, y, z + dz)),
                    (dx, dz) != (0, 0)
                );
            }
        }
        assert_eq!(at(&arena.grid, (x, y + 1, z)), Some(BlockType::Obsidian));
    }
}

#[test]
fn east_defense_is_breached_at_the_front_and_cap() {
    for seed in SEEDS {
        let arena = built(seed);
        let base = arena.construction.base(IslandId::East).unwrap();
        let mut missing = missing_offsets(base);
        missing.sort_unstable();
        // The whole front row facing the bridge and the cap: a U of flanks and back remains.
        assert_eq!(missing, [(-1, 1, 0), (0, 0, 1), (0, 1, 0), (1, 1, 0)]);
        let (x, y, z) = cell(base.egg);
        let (ax, az) = base.attack_direction;
        assert!(
            !occupied(&arena.grid, (x + ax, y, z + az)),
            "exposed in front"
        );
        assert!(!occupied(&arena.grid, (x, y + 1, z)), "exposed from above");
        for slot in &base.defense {
            if slot.intact {
                assert!(local_offset(base, slot.position).1 <= 0);
            }
        }
    }
}

#[test]
fn south_defense_is_a_recognizable_corner_fragment() {
    for seed in SEEDS {
        let arena = built(seed);
        let base = arena.construction.base(IslandId::South).unwrap();
        let mut standing: Vec<_> = base
            .defense
            .iter()
            .filter(|slot| slot.intact)
            .map(|slot| local_offset(base, slot.position))
            .collect();
        standing.sort_unstable();
        // An L around the back-left corner: the left flank, the back-left corner, and the back
        // middle, face-connected, with the egg open in front, to the right, and above.
        assert_eq!(standing, [(-1, -1, 0), (-1, 0, 0), (0, -1, 0)]);
        assert_eq!(missing_offsets(base).len(), 6);
        let (x, y, z) = cell(base.egg);
        let (ax, az) = base.attack_direction;
        assert!(!occupied(&arena.grid, (x + ax, y, z + az)));
        assert!(!occupied(&arena.grid, (x, y + 1, z)));
        // The three blocks are one face-connected piece in the world as well.
        let labels = components(&arena.grid);
        let slots: HashSet<_> = base
            .defense
            .iter()
            .filter(|slot| slot.intact)
            .map(|slot| labels[&cell(slot.position)])
            .collect();
        assert_eq!(
            slots.len(),
            1,
            "seed {seed}: one fragment, not scattered blocks"
        );
    }
}

#[test]
fn west_defense_is_mostly_intact_with_a_front_breach() {
    for seed in SEEDS {
        let arena = built(seed);
        let base = arena.construction.base(IslandId::West).unwrap();
        let mut missing = missing_offsets(base);
        missing.sort_unstable();
        assert_eq!(missing, [(-1, 1, 0), (0, 1, 0)]);
        let (x, y, z) = cell(base.egg);
        let (ax, az) = base.attack_direction;
        // The breach faces the attack and exposes the egg's front; the cap still stands.
        assert!(!occupied(&arena.grid, (x + ax, y, z + az)));
        assert!(occupied(&arena.grid, (x, y + 1, z)));
    }
}

#[test]
fn damage_grows_from_north_through_west_and_east_to_south() {
    let info = built(1847).construction;
    let held = |id: IslandId| {
        info.base(id)
            .unwrap()
            .defense
            .iter()
            .filter(|slot| slot.intact)
            .count()
    };
    assert!(held(IslandId::North) > held(IslandId::West));
    assert!(held(IslandId::West) > held(IslandId::East));
    assert!(held(IslandId::East) > held(IslandId::South));
    // Everything missing was lost from the front or the cap, never from behind the egg.
    for base in info.bases {
        for (side, forward, up) in missing_offsets(&base) {
            assert!(
                forward == 1 || (side, forward, up) == (0, 0, 1) || base.island == IslandId::South
            );
        }
    }
}

#[test]
fn obsidian_is_exposed_on_top_and_never_buried() {
    for seed in SEEDS {
        let arena = built(seed);
        let terrain = terrain_grid(seed);
        let mut obsidian = 0;
        for (p, block) in construction_blocks(&arena) {
            if block != BlockType::Obsidian {
                continue;
            }
            obsidian += 1;
            assert!(
                !occupied(&arena.grid, (p.x, p.y + 1, p.z)),
                "covered at {p:?}"
            );
            assert!(p.y > SURFACE_LEVEL, "set into the ground at {p:?}");
            assert!(!occupied(&terrain, (p.x, p.y + 1, p.z)));
        }
        // 9 + 7 + 5 + 3 defense blocks, a 2×2 central inlay, and one pillar cap.
        assert_eq!(obsidian, 24 + 4 + 1);
        assert_eq!(arena.construction.count(BlockType::Obsidian), 29);
    }
}

// ---- Structures ----

#[test]
fn each_outer_island_has_its_base_floor_and_gate_pillars() {
    for seed in SEEDS {
        let arena = built(seed);
        for island in OUTER_ISLANDS {
            let design = EggBaseDesign::of(island).unwrap();
            let frame = frame(island);
            for f in BASE_FORWARD {
                for s in BASE_SIDES {
                    let (x, z) = frame.cell(s, f);
                    if design.missing_floor.contains(&(s, f)) {
                        continue;
                    }
                    // The floor layer is cobblestone, or grass that already rises to it.
                    assert!(
                        matches!(
                            at(&arena.grid, (x, FLOOR_Y, z)),
                            Some(BlockType::Cobblestone | BlockType::Grass)
                        ),
                        "seed {seed}: {island:?} floor ({s}, {f})"
                    );
                }
            }
            for (side, height) in GATE_SIDES.into_iter().zip(design.gate_heights) {
                let (x, z) = frame.cell(side, GATE_FORWARD);
                let top = column_blocks(&arena.grid, x, z)[0];
                let ground = column_blocks(&terrain_grid(seed), x, z)[0].0;
                assert_eq!(
                    top.0,
                    ground.max(SURFACE_LEVEL + height),
                    "{island:?} gate {side}"
                );
            }
        }
        assert_eq!(
            EggBaseDesign::of(IslandId::North).unwrap().gate_heights,
            [3, 3]
        );
        assert!(EggBaseDesign::of(IslandId::Center).is_none());
    }
}

#[test]
fn central_ruin_has_an_obsidian_floor_pillars_walls_and_an_open_middle() {
    for seed in SEEDS {
        let arena = built(seed);
        let central = arena.construction.central;
        let IslandBounds { min, max } = central.floor;
        assert_eq!((min.y, max.y), (FLOOR_Y, FLOOR_Y));
        for z in min.z..=max.z {
            for x in min.x..=max.x {
                let floor = at(&arena.grid, (x, FLOOR_Y, z));
                if central.inlay.contains(VoxelPosition::new(x, FLOOR_Y, z)) {
                    assert_eq!(floor, Some(BlockType::Obsidian));
                } else if super::structures::LAVA_POOL.contains(&x)
                    && super::structures::LAVA_POOL.contains(&z)
                {
                    assert_eq!(floor, Some(BlockType::Lava), "seed {seed}: ({x}, {z})");
                } else {
                    // Cobblestone, or grass that already rises to the floor.
                    assert!(
                        matches!(floor, Some(BlockType::Cobblestone | BlockType::Grass)),
                        "seed {seed}: ({x}, {z})"
                    );
                }
            }
        }
        // The middle is open to the sky for future loot.
        let open = central.open_area;
        assert_eq!(open.min, VoxelPosition::new(-3, FLOOR_Y + 1, -3));
        assert_eq!(open.max, VoxelPosition::new(2, FLOOR_Y + 1, 2));
        for z in open.min.z..=open.max.z {
            for x in open.min.x..=open.max.x {
                for y in open.min.y..=CONSTRUCTION_TOP {
                    assert!(!occupied(&arena.grid, (x, y, z)), "({x}, {y}, {z})");
                }
            }
        }
        // The tallest block is the central obsidian-capped pillar, and the center outranks every
        // outer structure.
        assert_eq!(
            at(&arena.grid, (-4, CONSTRUCTION_TOP, -4)),
            Some(BlockType::Obsidian)
        );
        for (p, _) in construction_blocks(&arena) {
            if p.y > SURFACE_LEVEL + 3 {
                assert!(
                    central
                        .floor
                        .contains(VoxelPosition::new(p.x, FLOOR_Y, p.z))
                );
            }
        }
        // Partial walls: high on the north and west, gone on the open south and east sides.
        assert!(occupied(&arena.grid, (-3, FLOOR_Y + 3, -4)));
        assert!(!occupied(&arena.grid, (2, FLOOR_Y + 2, -4)));
        assert!(!occupied(&arena.grid, (-4, FLOOR_Y + 1, 2)));
        for x in -3..=2 {
            assert!(!occupied(&arena.grid, (x, FLOOR_Y + 1, 3)));
        }
    }
}

#[test]
fn glass_panes_are_exposed_with_air_behind_and_no_terrain_contact() {
    for seed in SEEDS {
        let arena = built(seed);
        let terrain = terrain_grid(seed);
        let glass: Vec<VoxelPosition> = construction_blocks(&arena)
            .into_iter()
            .filter(|&(_, b)| b == BlockType::Glass)
            .map(|(p, _)| p)
            .collect();
        assert_eq!(glass, {
            let mut windows = WINDOWS.to_vec();
            windows.sort_by_key(|p| (p.y, p.z, p.x));
            windows
        });
        assert_eq!(arena.construction.count(BlockType::Glass), 2);
        for p in glass {
            // Open air in front and behind, so refraction shows the scene beyond.
            assert!(!occupied(&arena.grid, (p.x, p.y, p.z - 1)));
            assert!(!occupied(&arena.grid, (p.x, p.y, p.z + 1)));
            for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0)] {
                let n = (p.x + dx, p.y + dy, p.z + dz);
                assert!(!occupied(&terrain, n), "glass touches terrain at {n:?}");
                assert_ne!(
                    at(&arena.grid, n),
                    Some(BlockType::Glass),
                    "connected glass"
                );
            }
            assert!(p.y > SURFACE_LEVEL + 1);
        }
    }
}

// ---- Reproducibility ----

#[test]
fn same_seed_builds_an_identical_arena() {
    for seed in SEEDS {
        assert_eq!(built(seed), built(seed), "seed {seed}");
    }
}

#[test]
fn reference_constructed_arena_fingerprint_is_stable() {
    assert_eq!(fingerprint(&built(1847).grid), REFERENCE_FINGERPRINT);
}

const REFERENCE_FINGERPRINT: u64 = 0xD150_C0A7_73AA_E81A;

#[test]
fn different_seeds_keep_the_construction_layout() {
    let reference = built(1847).construction;
    for seed in SEEDS {
        let info = built(seed).construction;
        // The spill follows each seed's rim; the ruin around it is fixed.
        let central = CentralStructure {
            lava_spill: reference.central.lava_spill,
            ..info.central
        };
        assert_eq!(central, reference.central, "seed {seed}");
        for (base, expected) in info.bases.iter().zip(reference.bases) {
            assert_eq!(*base, expected, "seed {seed}: designed bases never move");
        }
        for (bridge, expected) in info.bridges.iter().zip(reference.bridges) {
            assert_eq!(bridge.state, expected.state);
            assert_eq!(bridge.toward_center, expected.toward_center);
        }
        for block in [BlockType::Obsidian, BlockType::Glass] {
            assert_eq!(info.count(block), reference.count(block));
        }
        assert_eq!(info.surface_levels, reference.surface_levels);
    }
    // The terrain and the small variations do change the bridges between seeds.
    let ends: HashSet<_> = (0..16)
        .map(|seed| {
            built(seed)
                .construction
                .bridges
                .map(|b| b.lanes.map(|l| (cell(l.outer_end), cell(l.center_end))))
        })
        .collect();
    assert!(ends.len() > 1);
}

#[test]
fn construction_does_not_depend_on_generation_planning_or_write_order() {
    let layout = ArenaLayout::eggwars();
    let bounds = construction_bounds(&layout);
    let extent = |min: i32, max: i32| (max - min + 1) as usize;
    for seed in [0, 1847, u64::MAX] {
        let reference = built(seed);

        // Terrain generated island by island in reverse order, then constructed.
        let mut grid = VoxelGrid::try_new(
            bounds.min,
            extent(bounds.min.x, bounds.max.x),
            extent(bounds.min.y, bounds.max.y),
            extent(bounds.min.z, bounds.max.z),
        )
        .unwrap();
        for id in IslandId::ALL.into_iter().rev() {
            generate_island(&mut grid, &layout.config(id), island_seed(seed, id)).unwrap();
        }
        let info = construct(&mut grid, seed).unwrap();
        assert_eq!(grid, reference.grid, "seed {seed}");
        assert_eq!(info, reference.construction);

        // The plan reads only terrain, so writing it in any order gives the same arena.
        let terrain = terrain_grid(seed);
        let (plan, _) = plan_construction(&terrain, seed).unwrap();
        let mut orders = vec![plan.blocks.clone(); 3];
        orders[1].reverse();
        orders[2].sort_by_key(|&(p, _)| (p.z, p.x, -p.y));
        for blocks in orders {
            let mut grid = terrain.clone();
            Plan { blocks }.write(&mut grid);
            assert_eq!(grid, reference.grid, "seed {seed}");
        }
    }
}

#[test]
fn lava_spill_hangs_from_the_central_rim_against_the_cliff() {
    for seed in 0..256 {
        let arena = built(seed);
        let spill = arena.construction.central.lava_spill;
        let (source, fall) = (spill.source, spill.fall);
        // Inside the grid, well clear of the ruin (x, z in -4..=3) and the south bridge's lanes.
        assert!(arena.grid.contains_world(source), "seed {seed}");
        assert!(arena.grid.contains_world(fall.min), "seed {seed}");
        assert!(source.z > 4 && !LANES.contains(&source.x), "seed {seed}");
        // Source on terrain; the column one block outside the rim, lip level with the source.
        assert_eq!(
            at(&arena.grid, (source.x, source.y, source.z)),
            Some(BlockType::Lava)
        );
        let below = at(&arena.grid, (source.x, source.y - 1, source.z));
        assert!(below.is_some_and(|b| b != BlockType::Lava), "seed {seed}");
        assert_eq!((fall.min.x, fall.max.x), (source.x, source.x));
        assert_eq!((fall.min.z, fall.max.z), (source.z + 1, source.z + 1));
        assert_eq!(fall.max.y, source.y);
        // Short: three to five blocks of lava with the source.
        let height = fall.max.y - fall.min.y + 1;
        assert!((3..=4).contains(&height), "seed {seed}: {height}");
        for y in fall.min.y..=fall.max.y {
            assert_eq!(
                at(&arena.grid, (source.x, y, fall.min.z)),
                Some(BlockType::Lava)
            );
            // Each cell runs down terrain (the lip against the source), and nothing sits beyond.
            let inward = at(&arena.grid, (source.x, y, source.z));
            assert!(inward.is_some(), "seed {seed}: y {y}");
            assert!(
                !arena
                    .grid
                    .is_occupied_world(VoxelPosition::new(source.x, y, fall.min.z + 1))
            );
        }
        // The pool and the spill are all the lava there is.
        assert_eq!(
            arena.construction.count(BlockType::Lava),
            4 + 1 + height as usize,
            "seed {seed}"
        );
    }
}
