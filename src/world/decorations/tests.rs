use std::collections::HashSet;

use super::{
    ArrowHeading, DecorationError, DecorationKind, DecorationMaterial, DecorationSite, Decorations,
    Frame, Mount, egg_survives,
    models::{
        CHEST_FLOOR_PX, CHEST_LID_TOP_PX, CHEST_RIM_PX, EGG_LAYERS, FLETCHING_SPAN, HEAD_SPAN,
        Model, chest, egg,
    },
    place_decorations,
    validation::{cell_box, overlapped_cells, overlaps, validate},
};
use crate::{
    geometry::Aabb,
    math::Vec3,
    voxel::{BlockType, Voxel, VoxelGrid, VoxelPosition},
    world::{
        BridgeState, DefenseState, EggWarsArena, IslandId, build_eggwars_arena,
        construction::{DEFENSE_TEMPLATE, EGG_Y, FLOOR_Y, OUTER_ISLANDS},
        island::tests::fingerprint,
    },
};

const SEEDS: [u64; 6] = [0, 1, 42, 1847, 2026, u64::MAX];
/// Index of the cap block directly above the egg in `DEFENSE_TEMPLATE`.
const CAP: usize = 8;
const PX: f32 = 1.0 / 16.0;

fn built(seed: u64) -> EggWarsArena {
    build_eggwars_arena(seed).unwrap()
}

fn placed(seed: u64) -> (EggWarsArena, Decorations) {
    let arena = built(seed);
    let decorations = place_decorations(&arena).unwrap();
    (arena, decorations)
}

fn base_site(island: IslandId) -> DecorationSite {
    DecorationSite::Base(island)
}

/// The smallest box holding every part of `parts`.
fn hull(parts: &[super::DecorationPart]) -> Aabb {
    let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for part in parts {
        let (a, b) = (part.bounds.min(), part.bounds.max());
        min = Vec3::new(min.x.min(a.x), min.y.min(a.y), min.z.min(a.z));
        max = Vec3::new(max.x.max(b.x), max.y.max(b.y), max.z.max(b.z));
    }
    Aabb::try_new(min, max).unwrap()
}

fn contains(outer: Aabb, inner: Aabb) -> bool {
    let (o0, o1, i0, i1) = (outer.min(), outer.max(), inner.min(), inner.max());
    i0.x >= o0.x && i0.y >= o0.y && i0.z >= o0.z && i1.x <= o1.x && i1.y <= o1.y && i1.z <= o1.z
}

/// Whether two boxes touch or overlap (share at least a face, edge, or corner).
fn touches(a: Aabb, b: Aabb) -> bool {
    let (a0, a1, b0, b1) = (a.min(), a.max(), b.min(), b.max());
    a0.x <= b1.x && b0.x <= a1.x && a0.y <= b1.y && b0.y <= a1.y && a0.z <= b1.z && b0.z <= a1.z
}

fn center(bounds: Aabb) -> Vec3 {
    (bounds.min() + bounds.max()) * 0.5
}

// ---- Eggs ----

#[test]
fn exactly_two_eggs_survive_in_north_and_west() {
    for seed in SEEDS {
        let (_, decorations) = placed(seed);
        let sites: Vec<_> = decorations
            .of_kind(DecorationKind::Egg)
            .map(|egg| egg.site)
            .collect();
        assert_eq!(
            sites,
            [base_site(IslandId::North), base_site(IslandId::West)]
        );
    }
}

#[test]
fn eggs_survive_exactly_behind_the_intact_and_damaged_defenses() {
    assert!(egg_survives(DefenseState::Intact));
    assert!(egg_survives(DefenseState::Damaged));
    assert!(!egg_survives(DefenseState::Breached));
    assert!(!egg_survives(DefenseState::Destroyed));
}

#[test]
fn east_and_south_hold_no_intact_egg_in_or_near_their_egg_cells() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        for island in [IslandId::East, IslandId::South] {
            let base = arena.construction.base(island).unwrap();
            assert!(!decorations.decorations().iter().any(|decoration| {
                decoration.kind == DecorationKind::Egg && decoration.site == base_site(island)
            }));
            // What lies in the egg cell is a few flat shell fragments, nothing egg-sized.
            let cell = cell_box(base.egg);
            for decoration in decorations.decorations() {
                for part in decorations.parts_of(decoration) {
                    if overlaps(part.bounds, cell) {
                        assert_eq!(decoration.kind, DecorationKind::ShellFragment);
                        let size = part.bounds.max() - part.bounds.min();
                        assert!(size.y <= 3.0 * PX && size.x <= 5.0 * PX && size.z <= 5.0 * PX);
                    }
                }
            }
        }
    }
}

#[test]
fn eggs_occupy_their_reserved_cells_standing_on_the_base_floor() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        for egg in decorations.of_kind(DecorationKind::Egg) {
            let DecorationSite::Base(island) = egg.site else {
                panic!("eggs stand in bases");
            };
            let base = arena.construction.base(island).unwrap();
            assert_eq!(base.egg.y, EGG_Y);
            let cell = cell_box(base.egg);
            let parts = decorations.parts_of(egg);
            assert!(parts.iter().all(|part| contains(cell, part.bounds)));
            assert_eq!(hull(parts).min().y, EGG_Y as f32);
            let support = VoxelPosition::new(base.egg.x, FLOOR_Y, base.egg.z);
            assert_eq!(egg.mount, Mount::Resting { support });
            assert_eq!(
                arena.grid.get_world(support),
                Some(Voxel::Block(BlockType::Cobblestone))
            );
        }
    }
}

#[test]
fn eggs_keep_clear_of_every_defense_block_and_the_cap() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        for egg in decorations.of_kind(DecorationKind::Egg) {
            let DecorationSite::Base(island) = egg.site else {
                unreachable!()
            };
            let base = arena.construction.base(island).unwrap();
            let parts = decorations.parts_of(egg);
            for slot in base.defense {
                let block = cell_box(slot.position);
                for part in parts {
                    assert!(!overlaps(part.bounds, block), "{island:?}");
                    assert!(
                        !touches(part.bounds, block),
                        "{island:?} egg touches {slot:?}"
                    );
                }
            }
            // At least 2 px of air on every side and above.
            let cell = cell_box(base.egg);
            let egg_hull = hull(parts);
            assert!(egg_hull.min().x - cell.min().x >= 2.0 * PX);
            assert!(cell.max().x - egg_hull.max().x >= 2.0 * PX);
            assert!(egg_hull.min().z - cell.min().z >= 2.0 * PX);
            assert!(cell.max().z - egg_hull.max().z >= 2.0 * PX);
            assert!(cell.max().y - egg_hull.max().y >= 2.0 * PX);
            assert!(base.defense[CAP].position.y as f32 >= egg_hull.max().y);
        }
    }
    // North's cap stands above its egg; West's too.
    let (arena, _) = placed(1847);
    for island in [IslandId::North, IslandId::West] {
        let base = arena.construction.base(island).unwrap();
        assert!(base.defense[CAP].intact);
    }
}

/// The six face neighbors of `cell`.
fn face_neighbors(cell: VoxelPosition) -> [VoxelPosition; 6] {
    [
        (-1, 0, 0),
        (1, 0, 0),
        (0, -1, 0),
        (0, 1, 0),
        (0, 0, -1),
        (0, 0, 1),
    ]
    .map(|(dx, dy, dz)| VoxelPosition::new(cell.x + dx, cell.y + dy, cell.z + dz))
}

#[test]
fn only_the_north_egg_is_sealed_and_its_cell_is_closed_on_all_six_faces() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        let sealed: Vec<_> = decorations
            .decorations()
            .iter()
            .filter(|decoration| decoration.sealed)
            .collect();
        assert_eq!(sealed.len(), 1, "{seed}");
        let egg = sealed[0];
        assert_eq!(egg.kind, DecorationKind::Egg);
        assert_eq!(egg.site, base_site(IslandId::North));
        // The story still has North's egg surviving behind its intact defense.
        let base = arena.construction.base(IslandId::North).unwrap();
        assert_eq!(base.defense_state, DefenseState::Intact);
        assert!(base.defense.iter().all(|slot| slot.intact));
        assert!(egg_survives(base.defense_state));
        // Every part is in the egg cell; obsidian on the four sides and above, the base's
        // cobblestone floor below: the underside is closed too.
        let parts = decorations.parts_of(egg);
        assert_eq!(parts.len(), EGG_LAYERS.len());
        assert!(
            parts
                .iter()
                .all(|part| overlapped_cells(part.bounds).all(|cell| cell == base.egg))
        );
        for neighbor in face_neighbors(base.egg) {
            let expected = if neighbor.y < base.egg.y {
                BlockType::Cobblestone
            } else {
                BlockType::Obsidian
            };
            assert_eq!(
                arena.grid.get_world(neighbor),
                Some(Voxel::Block(expected)),
                "{seed} {neighbor:?}"
            );
        }
        // West's egg shows through its notch: not sealed, and its front face is open air.
        let west = decorations
            .of_kind(DecorationKind::Egg)
            .find(|egg| egg.site == base_site(IslandId::West))
            .unwrap();
        assert!(!west.sealed);
        let west_base = arena.construction.base(IslandId::West).unwrap();
        assert!(
            face_neighbors(west_base.egg)
                .iter()
                .any(|&cell| !arena.grid.is_occupied_world(cell))
        );
    }
}

#[test]
fn visible_props_are_all_but_the_sealed_north_egg_in_placement_order() {
    let (_, decorations) = placed(1847);
    let visible: Vec<_> = decorations.visible().copied().collect();
    let expected: Vec<_> = decorations
        .decorations()
        .iter()
        .filter(|decoration| {
            !(decoration.kind == DecorationKind::Egg
                && decoration.site == base_site(IslandId::North))
        })
        .copied()
        .collect();
    assert_eq!(visible, expected);
    assert_eq!(visible.len(), 24);
    assert_eq!(decorations.visible_part_count(), 54);
    assert_eq!(decorations.parts().len(), 59);
}

#[test]
fn the_egg_is_a_stepped_oval_taller_than_wide_and_widest_low() {
    let frame = Frame::new(VoxelPosition::new(0, 0, 0), (0, -1));
    let model = egg(frame);
    let parts: Vec<_> = model.parts().collect();
    assert_eq!(parts.len(), EGG_LAYERS.len());
    assert!(parts.len() > 1, "not a single cube");

    let widths: Vec<f32> = parts
        .iter()
        .map(|part| part.bounds.max().x - part.bounds.min().x)
        .collect();
    let egg_hull = hull(&parts);
    let (width, height) = (
        egg_hull.max().x - egg_hull.min().x,
        egg_hull.max().y - egg_hull.min().y,
    );
    assert!(height > width);
    // Layers stack without gaps or overlap, the widest is the second (the low belly), and every
    // layer above it is no wider than the one below, narrowing to a small crown.
    for pair in parts.windows(2) {
        assert_eq!(pair[0].bounds.max().y, pair[1].bounds.min().y);
    }
    let widest = widths
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    assert_eq!(widest, 1);
    assert!(widths[0] < widths[1]);
    assert!(widths[widest..].windows(2).all(|pair| pair[1] <= pair[0]));
    assert!(widths[widths.len() - 1] <= widths[1] / 2.0);
    // Square courses centered on the cell.
    for part in &parts {
        let size = part.bounds.max() - part.bounds.min();
        assert_eq!(size.x, size.z);
        assert_eq!(center(part.bounds).x, 0.5);
        assert_eq!(center(part.bounds).z, 0.5);
        assert_eq!(part.material, DecorationMaterial::EggShell);
    }
}

#[test]
fn egg_geometry_is_deterministic_and_identical_in_both_surviving_bases() {
    let (arena, decorations) = placed(1847);
    let (_, again) = placed(1847);
    assert_eq!(decorations, again);
    let eggs: Vec<_> = decorations.of_kind(DecorationKind::Egg).collect();
    let local = |index: usize, island| {
        let corner = arena.construction.base(island).unwrap().egg.min_corner();
        decorations
            .parts_of(eggs[index])
            .iter()
            .map(|part| (part.bounds.min() - corner, part.bounds.max() - corner))
            .collect::<Vec<_>>()
    };
    assert_eq!(local(0, IslandId::North), local(1, IslandId::West));
}

// ---- Destroyed eggs ----

#[test]
fn broken_eggs_leave_a_few_small_shell_fragments_near_their_cells() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        let shells: Vec<_> = decorations.of_kind(DecorationKind::ShellFragment).collect();
        assert_eq!(shells.len(), 6);
        for island in OUTER_ISLANDS {
            let base = arena.construction.base(island).unwrap();
            let here: Vec<_> = shells
                .iter()
                .filter(|shell| shell.site == base_site(island))
                .collect();
            let expected = if egg_survives(base.defense_state) {
                0
            } else {
                3
            };
            assert_eq!(here.len(), expected, "{island:?}");
            for shell in here {
                let parts = decorations.parts_of(shell);
                assert_eq!(parts.len(), 1);
                let part = parts[0];
                assert_eq!(part.material, DecorationMaterial::EggShell);
                // On the floor at the egg level, within one cell of the egg.
                assert_eq!(part.bounds.min().y, EGG_Y as f32);
                let near = Aabb::try_new(
                    base.egg.min_corner() - Vec3::new(1.0, 0.0, 1.0),
                    base.egg.min_corner() + Vec3::new(2.0, 1.0, 2.0),
                )
                .unwrap();
                assert!(contains(near, part.bounds), "{island:?} {part:?}");
                let Mount::Resting { support } = shell.mount else {
                    panic!("shells rest on the floor");
                };
                assert_eq!(support.y, FLOOR_Y);
            }
        }
    }
}

// ---- Chests ----

#[test]
fn two_open_chests_stand_on_the_central_ruins_floor_inside_its_open_area() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        let open = arena.construction.central.open_area;
        let area = Aabb::try_new(
            open.min.min_corner(),
            open.max.min_corner() + Vec3::new(1.0, 2.0, 1.0),
        )
        .unwrap();
        let chests: Vec<_> = decorations.of_kind(DecorationKind::Chest).collect();
        assert_eq!(chests.len(), 2);
        for chest in chests {
            assert_eq!(chest.site, DecorationSite::Center);
            let parts = decorations.parts_of(chest);
            assert_eq!(parts.len(), 7);
            assert!(contains(area, hull(parts)));
            let Mount::Resting { support } = chest.mount else {
                panic!("chests rest on the floor");
            };
            assert_eq!(support.y, FLOOR_Y);
            assert!(arena.grid.is_occupied_world(support));
            assert_eq!(hull(parts).min().y, (FLOOR_Y + 1) as f32);
            // Nothing of the ruin (walls, pillars, panes) or terrain is inside a chest.
            for part in parts {
                for cell in overlapped_cells(part.bounds) {
                    assert!(!arena.grid.is_occupied_world(cell), "{cell:?}");
                }
            }
        }
    }
}

#[test]
fn chests_are_hollow_with_a_dark_recess_and_an_upright_open_lid() {
    for forward in [(0, 1), (1, 0), (0, -1), (-1, 0)] {
        let frame = Frame::new(VoxelPosition::new(3, 2, -5), forward);
        let parts: Vec<_> = chest(frame).parts().collect();
        let floor = 2.0;
        let rim = floor + CHEST_RIM_PX * PX;
        let interior: Vec<_> = parts
            .iter()
            .filter(|part| part.material == DecorationMaterial::ChestInterior)
            .collect();
        assert_eq!(interior.len(), 1);
        let interior = interior[0].bounds;
        assert_eq!(interior.max().y, floor + CHEST_FLOOR_PX * PX);
        assert!(interior.max().y < rim, "the recess sits below the rim");

        // The four walls reach the rim and enclose the interior on all four sides.
        let walls: Vec<_> = parts
            .iter()
            .filter(|part| {
                part.material == DecorationMaterial::ChestWood && part.bounds.max().y == rim
            })
            .collect();
        assert_eq!(walls.len(), 4);
        for wall in &walls {
            assert!(!overlaps(wall.bounds, interior));
            assert!(touches(wall.bounds, interior));
        }

        // The lid is the tallest part: upright, on the back wall, behind the opening.
        let lid = parts
            .iter()
            .max_by(|a, b| a.bounds.max().y.total_cmp(&b.bounds.max().y))
            .unwrap();
        assert_eq!(lid.material, DecorationMaterial::ChestWood);
        assert_eq!(lid.bounds.min().y, rim);
        assert_eq!(lid.bounds.max().y, floor + CHEST_LID_TOP_PX * PX);
        let back = Vec3::new(-forward.0 as f32, 0.0, -forward.1 as f32);
        let behind = (center(lid.bounds) - center(interior)).dot(back);
        assert!(behind > 0.3, "the lid stands at the back: {behind}");
        let lid_size = lid.bounds.max() - lid.bounds.min();
        let thickness = lid_size.x.min(lid_size.z);
        assert!(lid_size.y > 4.0 * thickness, "upright, not closed");

        // Straight down onto the middle of the chest there is only the dark interior.
        let middle = center(interior);
        let column = Aabb::try_new(
            Vec3::new(middle.x - 0.01, interior.max().y, middle.z - 0.01),
            Vec3::new(middle.x + 0.01, floor + 2.0, middle.z + 0.01),
        )
        .unwrap();
        assert!(parts.iter().all(|part| !overlaps(part.bounds, column)));

        // The latch hangs on the lid's front face.
        let latch = parts
            .iter()
            .find(|part| part.material == DecorationMaterial::ChestLatch)
            .unwrap();
        assert!(touches(latch.bounds, lid.bounds) && !overlaps(latch.bounds, lid.bounds));
        assert!((center(latch.bounds) - center(lid.bounds)).dot(back) < 0.0);

        // Everything stays inside the chest's own cell column.
        let cell = Aabb::try_new(
            Vec3::new(3.0, 2.0, -5.0),
            Vec3::new(4.0, 2.0 + CHEST_LID_TOP_PX * PX, -4.0),
        )
        .unwrap();
        assert!(contains(cell, hull(&parts)));
    }
}

#[test]
fn chests_keep_their_lids_against_the_ruin_walls_and_fronts_toward_the_open_side() {
    let (arena, decorations) = placed(1847);
    let open = arena.construction.central.open_area;
    let chests: Vec<_> = decorations.of_kind(DecorationKind::Chest).collect();
    for (chest, wall_side) in chests.into_iter().zip([
        Vec3::new(0.0, 0.0, -1.0), // north wall
        Vec3::new(-1.0, 0.0, 0.0), // west wall
    ]) {
        let parts = decorations.parts_of(chest);
        let lid = parts
            .iter()
            .max_by(|a, b| a.bounds.max().y.total_cmp(&b.bounds.max().y))
            .unwrap();
        assert!((center(lid.bounds) - center(hull(parts))).dot(wall_side) > 0.3);
        // A wall block stands right behind the chest.
        let Mount::Resting { support } = chest.mount else {
            unreachable!()
        };
        let behind = VoxelPosition::new(
            support.x + wall_side.x as i32,
            support.y + 1,
            support.z + wall_side.z as i32,
        );
        assert!(arena.grid.is_occupied_world(behind), "{behind:?}");
        assert!(support.x >= open.min.x && support.z >= open.min.z);
    }
}

#[test]
fn chests_use_only_wood_interior_and_latch_and_are_identical_on_every_seed() {
    let (_, reference) = placed(1847);
    for seed in SEEDS {
        let (_, decorations) = placed(seed);
        for (chest, expected) in decorations
            .of_kind(DecorationKind::Chest)
            .zip(reference.of_kind(DecorationKind::Chest))
        {
            assert_eq!(decorations.parts_of(chest), reference.parts_of(expected));
            for part in decorations.parts_of(chest) {
                assert!(matches!(
                    part.material,
                    DecorationMaterial::ChestWood
                        | DecorationMaterial::ChestInterior
                        | DecorationMaterial::ChestLatch
                ));
            }
        }
    }
}

// ---- Arrows ----

#[test]
fn seven_arrows_fly_in_all_five_supported_orientations() {
    for seed in SEEDS {
        let (_, decorations) = placed(seed);
        let headings: HashSet<_> = decorations
            .of_kind(DecorationKind::Arrow)
            .map(|arrow| match arrow.mount {
                Mount::Embedded { heading, .. } => heading,
                Mount::Resting { .. } => panic!("arrows are embedded"),
            })
            .collect();
        assert_eq!(decorations.count(DecorationKind::Arrow), 7);
        assert_eq!(
            headings,
            HashSet::from([
                ArrowHeading::PositiveX,
                ArrowHeading::NegativeX,
                ArrowHeading::PositiveZ,
                ArrowHeading::NegativeZ,
                ArrowHeading::Down,
            ])
        );
    }
}

#[test]
fn arrows_are_head_shaft_and_fletching_along_their_flight_axis() {
    for seed in SEEDS {
        let (_, decorations) = placed(seed);
        for arrow in decorations.of_kind(DecorationKind::Arrow) {
            let Mount::Embedded { heading, .. } = arrow.mount else {
                unreachable!()
            };
            let parts = decorations.parts_of(arrow);
            let materials: Vec<_> = parts.iter().map(|part| part.material).collect();
            assert_eq!(
                materials,
                [
                    DecorationMaterial::ArrowHead,
                    DecorationMaterial::ArrowShaft,
                    DecorationMaterial::ArrowFletching,
                ]
            );
            // Consecutive parts touch end to end, and the tail lies against the flight.
            for pair in parts.windows(2) {
                assert!(touches(pair[0].bounds, pair[1].bounds));
                assert!(!overlaps(pair[0].bounds, pair[1].bounds));
            }
            let flight = heading.direction();
            let tail = center(parts[2].bounds) - center(parts[0].bounds);
            assert!(tail.dot(flight) < -0.5);
            let length = hull(parts).max() - hull(parts).min();
            let along = (length.x * flight.x + length.y * flight.y + length.z * flight.z).abs();
            assert_eq!(along, (FLETCHING_SPAN[1] - HEAD_SPAN[0]) * PX);
            // The shaft is thinner than the head and fletching.
            let width = |bounds: Aabb| {
                let size = bounds.max() - bounds.min();
                size.x.min(size.y).min(size.z)
            };
            assert!(width(parts[1].bounds) < width(parts[0].bounds));
            assert!(width(parts[1].bounds) < width(parts[2].bounds));
        }
    }
}

#[test]
fn arrowheads_are_embedded_in_their_target_and_the_rest_stays_in_open_air() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        for arrow in decorations.of_kind(DecorationKind::Arrow) {
            let Mount::Embedded { target, heading } = arrow.mount else {
                unreachable!()
            };
            assert!(arena.grid.is_occupied_world(target));
            let parts = decorations.parts_of(arrow);
            let head = parts[0].bounds;
            let block = cell_box(target);
            assert!(overlaps(head, block), "embedded");
            assert!(!contains(block, head), "not buried");
            let embedded: Vec<_> = overlapped_cells(head)
                .filter(|&cell| arena.grid.is_occupied_world(cell))
                .collect();
            assert_eq!(embedded, [target]);
            // Shaft and fletching: visible, touching no block, in the empty cell in front of the
            // struck face.
            let face = heading.entry_face().normal();
            let outside = VoxelPosition::new(
                target.x + face.x as i32,
                target.y + face.y as i32,
                target.z + face.z as i32,
            );
            assert!(!arena.grid.is_occupied_world(outside));
            for part in &parts[1..] {
                assert!(contains(cell_box(outside), part.bounds));
            }
            // The struck face is fully exposed where the head enters.
            assert!(overlaps(head, cell_box(outside)));
        }
    }
}

#[test]
fn arrows_strike_cobblestone_obsidian_and_grass_from_the_attack_side() {
    let (arena, decorations) = placed(1847);
    let mut struck = Vec::new();
    for arrow in decorations.of_kind(DecorationKind::Arrow) {
        let Mount::Embedded { target, heading } = arrow.mount else {
            unreachable!()
        };
        let block = arena.grid.get_world(target).unwrap().block().unwrap();
        if !struck.contains(&block) {
            struck.push(block);
        }
        if let DecorationSite::Base(island) = arrow.site {
            let base = arena.construction.base(island).unwrap();
            assert_eq!(heading, ArrowHeading::against(base.attack_direction));
            let slot = base
                .defense
                .iter()
                .find(|slot| slot.position == target)
                .expect("base arrows strike the defense");
            assert!(slot.intact);
        }
    }
    struck.sort_by_key(|&block| block as u8);
    assert_eq!(
        struck,
        [
            BlockType::Grass,
            BlockType::Cobblestone,
            BlockType::Obsidian
        ]
    );
}

// ---- Remnants ----

#[test]
fn remnants_are_cobblestone_and_obsidian_fragments_resting_on_blocks() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        assert_eq!(decorations.count(DecorationKind::Remnant), 8);
        for remnant in decorations.of_kind(DecorationKind::Remnant) {
            let parts = decorations.parts_of(remnant);
            assert_eq!(parts.len(), 1);
            let part = parts[0];
            assert!(matches!(
                part.material,
                DecorationMaterial::Cobblestone | DecorationMaterial::Obsidian
            ));
            let size = part.bounds.max() - part.bounds.min();
            assert!(
                size.x < 1.0 && size.x == size.y && size.y == size.z,
                "a fragment"
            );
            let Mount::Resting { support } = remnant.mount else {
                panic!("remnants rest");
            };
            assert!(arena.grid.is_occupied_world(support));
            assert_eq!(part.bounds.min().y, (support.y + 1) as f32);
            assert!(contains(cell_box(support), {
                let (min, max) = (part.bounds.min(), part.bounds.max());
                Aabb::try_new(
                    Vec3::new(min.x, support.y as f32, min.z),
                    Vec3::new(max.x, support.y as f32 + 1.0, max.z),
                )
                .unwrap()
            }));
        }
    }
}

// ---- Narrative ----

/// `(eggs, shell fragments, arrows, remnants)` at a site.
fn tally(decorations: &Decorations, site: DecorationSite) -> [usize; 4] {
    let count = |kind| {
        decorations
            .decorations()
            .iter()
            .filter(|decoration| decoration.kind == kind && decoration.site == site)
            .count()
    };
    [
        count(DecorationKind::Egg),
        count(DecorationKind::ShellFragment),
        count(DecorationKind::Arrow),
        count(DecorationKind::Remnant),
    ]
}

#[test]
fn decorations_follow_each_bases_narrative_state() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        let states: Vec<_> = OUTER_ISLANDS
            .map(|island| arena.construction.base(island).unwrap().defense_state)
            .to_vec();
        assert_eq!(
            states,
            [
                DefenseState::Intact,
                DefenseState::Destroyed,
                DefenseState::Breached,
                DefenseState::Damaged
            ]
        );
        // North: sealed and untouched. West: notched, one arrow and one dropped block. East:
        // breached, the egg broken. South: destroyed, the egg broken and the most debris.
        assert_eq!(
            tally(&decorations, base_site(IslandId::North)),
            [1, 0, 0, 0]
        );
        assert_eq!(tally(&decorations, base_site(IslandId::West)), [1, 0, 1, 1]);
        assert_eq!(tally(&decorations, base_site(IslandId::East)), [0, 3, 1, 1]);
        assert_eq!(
            tally(&decorations, base_site(IslandId::South)),
            [0, 3, 1, 2]
        );
        // The defenses keep their Mission 30.5 obsidian counts.
        let intact: Vec<_> = OUTER_ISLANDS
            .map(|island| {
                let base = arena.construction.base(island).unwrap();
                base.defense.iter().filter(|slot| slot.intact).count()
            })
            .to_vec();
        assert_eq!(intact, [9, 3, 5, 7]);
    }
}

#[test]
fn bridge_debris_marks_only_the_battle_damaged_bridges() {
    for seed in SEEDS {
        let (arena, decorations) = placed(seed);
        for bridge in arena.construction.bridges {
            let site = DecorationSite::Bridge(bridge.island);
            let expected = match bridge.state {
                BridgeState::Complete | BridgeState::Incomplete => [0, 0, 0, 0],
                BridgeState::Damaged => [0, 0, 1, 1],
                BridgeState::Severed => [0, 0, 0, 2],
            };
            assert_eq!(tally(&decorations, site), expected, "{:?}", bridge.island);
            // Bridge debris lies on deck blocks right at a break: the next cell along the bridge
            // in one direction is open void.
            for decoration in decorations.decorations().iter().filter(|d| d.site == site) {
                let deck = match decoration.mount {
                    Mount::Resting { support } => support,
                    Mount::Embedded { target, .. } => target,
                };
                assert_eq!(deck.y, 0);
                assert_eq!(
                    arena.grid.get_world(deck),
                    Some(Voxel::Block(BlockType::Cobblestone))
                );
                let (dx, dz) = bridge.toward_center;
                let ahead = VoxelPosition::new(deck.x + dx, 0, deck.z + dz);
                let behind = VoxelPosition::new(deck.x - dx, 0, deck.z - dz);
                assert!(
                    !arena.grid.is_occupied_world(ahead) || !arena.grid.is_occupied_world(behind)
                );
            }
        }
        // Bridge states are unchanged.
        let states: Vec<_> = arena.construction.bridges.map(|b| b.state).to_vec();
        assert_eq!(
            states,
            [
                BridgeState::Complete,
                BridgeState::Severed,
                BridgeState::Damaged,
                BridgeState::Incomplete
            ]
        );
    }
}

#[test]
fn the_center_holds_the_chests_and_a_contested_scatter_of_arrows() {
    let (_, decorations) = placed(1847);
    assert_eq!(tally(&decorations, DecorationSite::Center), [0, 0, 3, 1]);
    assert!(
        decorations
            .of_kind(DecorationKind::Chest)
            .all(|chest| chest.site == DecorationSite::Center)
    );
}

// ---- Validity and reproducibility ----

#[test]
fn every_seed_places_a_valid_decoration_set_with_the_same_story() {
    let (_, reference) = placed(1847);
    let story = |decorations: &Decorations| {
        decorations
            .decorations()
            .iter()
            .map(|decoration| {
                (
                    decoration.kind,
                    decoration.site,
                    decoration.part_count,
                    match decoration.mount {
                        Mount::Resting { .. } => None,
                        Mount::Embedded { heading, .. } => Some(heading),
                    },
                )
            })
            .collect::<Vec<_>>()
    };
    let mut differs = false;
    for seed in 0..64 {
        let arena = built(seed);
        let decorations = place_decorations(&arena).unwrap_or_else(|e| panic!("{seed}: {e}"));
        assert_eq!(story(&decorations), story(&reference));
        differs |= decorations.parts() != reference.parts();
    }
    assert!(differs, "seeds vary terrain anchors and pixel offsets");
}

#[test]
fn decoration_counts_and_parts_are_modest_and_valid() {
    let (arena, decorations) = placed(1847);
    assert_eq!(decorations.decorations().len(), 25);
    assert_eq!(decorations.parts().len(), 59);
    let origin = arena.grid.origin().min_corner();
    let extent = Vec3::new(
        arena.grid.width() as f32,
        arena.grid.height() as f32,
        arena.grid.depth() as f32,
    );
    let grid = Aabb::try_new(origin, origin + extent).unwrap();
    for part in decorations.parts() {
        let (min, max) = (part.bounds.min(), part.bounds.max());
        assert!(min.is_finite() && max.is_finite());
        assert!(min.x < max.x && min.y < max.y && min.z < max.z, "{part:?}");
        assert!(contains(grid, part.bounds));
    }
    // Every part belongs to exactly one prop.
    let total: usize = decorations
        .decorations()
        .iter()
        .map(|decoration| decorations.parts_of(decoration).len())
        .sum();
    assert_eq!(total, decorations.parts().len());
}

#[test]
fn the_parts_of_each_prop_hold_together_and_props_never_overlap() {
    for seed in SEEDS {
        let (_, decorations) = placed(seed);
        for decoration in decorations.decorations() {
            let parts = decorations.parts_of(decoration);
            let mut reached = vec![false; parts.len()];
            let mut stack = vec![0];
            reached[0] = true;
            while let Some(index) = stack.pop() {
                for (other, part) in parts.iter().enumerate() {
                    if !reached[other] && touches(parts[index].bounds, part.bounds) {
                        reached[other] = true;
                        stack.push(other);
                    }
                }
            }
            assert!(reached.iter().all(|&r| r), "{decoration:?}");
        }
        let all = decorations.decorations();
        for (index, a) in all.iter().enumerate() {
            for b in &all[index + 1..] {
                for pa in decorations.parts_of(a) {
                    for pb in decorations.parts_of(b) {
                        assert!(!overlaps(pa.bounds, pb.bounds));
                    }
                }
            }
        }
    }
}

#[test]
fn placement_reads_the_arena_without_altering_it() {
    let arena = built(1847);
    let before = arena.clone();
    let decorations = place_decorations(&arena).unwrap();
    assert_eq!(arena, before);
    assert_eq!(fingerprint(&arena.grid), 0x1E49_D73E_6655_CEBC);
    // Placing twice gives the same props, and nothing a prop occupies is a block.
    assert_eq!(place_decorations(&arena).unwrap(), decorations);
    assert_eq!(arena.occupied_voxels(), built(1847).occupied_voxels());
}

#[test]
fn frames_match_the_construction_frames_on_every_island() {
    for seed in [1847, 0] {
        let arena = built(seed);
        for base in arena.construction.bases {
            let frame = Frame::new(base.egg, base.attack_direction);
            for (slot, (side, forward, up)) in base.defense.iter().zip(DEFENSE_TEMPLATE) {
                assert_eq!(frame.cell(side, forward, up), slot.position);
            }
            // A local box maps to the world cell `Frame::cell` names, with the same size.
            for (side, forward) in [(0, 0), (1, 0), (0, 1), (-1, 2)] {
                let local = frame.at(side, forward, 0);
                let bounds = local.aabb([0.25, 0.5], [0.0, 0.125], [0.0, 0.75]);
                let cell = frame.cell(side, forward, 0);
                assert!(contains(cell_box(cell), bounds));
                let size = bounds.max() - bounds.min();
                assert_eq!(size.y, 0.125);
                assert_eq!(size.x * size.z, 0.25 * 0.75);
            }
        }
    }
}

// ---- Validation ----

fn single(kind: DecorationKind, mount: Mount, model: Model) -> Decorations {
    let mut decorations = Decorations::default();
    decorations.push(kind, DecorationSite::Center, mount, model);
    decorations
}

#[test]
fn validation_rejects_buried_floating_overlapping_and_misplaced_props() {
    let arena = built(1847);
    let grid: &VoxelGrid = &arena.grid;
    let north = arena.construction.base(IslandId::North).unwrap();
    let support = VoxelPosition::new(north.egg.x, FLOOR_Y, north.egg.z);
    let resting = Mount::Resting { support };
    let egg_at = |cell: VoxelPosition| egg(Frame::new(cell, north.attack_direction));

    // The real egg is valid.
    assert_eq!(
        validate(
            grid,
            &single(DecorationKind::Egg, resting, egg_at(north.egg))
        ),
        Ok(())
    );
    // Moved into the obsidian beside it.
    let flank = north.defense[3].position;
    assert!(matches!(
        validate(grid, &single(DecorationKind::Egg, resting, egg_at(flank))),
        Err(DecorationError::IntersectsBlock { .. })
    ));
    // Lifted off the floor.
    let above = VoxelPosition::new(north.egg.x, north.egg.y + 2, north.egg.z);
    assert_eq!(
        validate(grid, &single(DecorationKind::Egg, resting, egg_at(above))),
        Err(DecorationError::Unsupported { decoration: 0 })
    );
    // Two eggs in one cell.
    let mut twice = single(DecorationKind::Egg, resting, egg_at(north.egg));
    twice.push(
        DecorationKind::Egg,
        DecorationSite::Center,
        resting,
        egg_at(north.egg),
    );
    assert_eq!(
        validate(grid, &twice),
        Err(DecorationError::Overlap {
            first: 0,
            second: 1
        })
    );
    // Outside the grid.
    let outside = VoxelPosition::new(grid.origin().x - 3, EGG_Y, grid.origin().z);
    assert!(matches!(
        validate(grid, &single(DecorationKind::Egg, resting, egg_at(outside))),
        Err(DecorationError::OutsideGrid { .. })
    ));

    // An arrow whose head is buried entirely inside its block, and one floating off it.
    let target = north.defense[1].position;
    let heading = ArrowHeading::against(north.attack_direction);
    let embedded = Mount::Embedded { target, heading };
    let valid = super::models::arrow(target, heading, (8.0, 8.0));
    assert_eq!(
        validate(grid, &single(DecorationKind::Arrow, embedded, valid)),
        Ok(())
    );
    let shifted = |offset: Vec3| {
        let mut model = super::models::arrow(target, heading, (8.0, 8.0));
        for part in &mut model.0 {
            part.bounds =
                Aabb::try_new(part.bounds.min() + offset, part.bounds.max() + offset).unwrap();
        }
        model
    };
    let flight = heading.direction();
    for offset in [flight * 0.5, flight * -0.5] {
        assert_eq!(
            validate(
                grid,
                &single(DecorationKind::Arrow, embedded, shifted(offset))
            ),
            Err(DecorationError::NotEmbedded { decoration: 0 }),
            "{offset:?}"
        );
    }
}

#[test]
fn validation_rejects_a_sealed_prop_that_a_ray_could_reach() {
    let arena = built(1847);
    let north = arena.construction.base(IslandId::North).unwrap();
    let west = arena.construction.base(IslandId::West).unwrap();
    let sealed_egg = |base: &crate::world::construction::EggBase| {
        let support = VoxelPosition::new(base.egg.x, FLOOR_Y, base.egg.z);
        let mut decorations = Decorations::default();
        decorations.push(
            DecorationKind::Egg,
            base_site(base.island),
            Mount::Resting { support },
            egg(Frame::new(base.egg, base.attack_direction)),
        );
        decorations.seal_last();
        decorations
    };

    assert_eq!(validate(&arena.grid, &sealed_egg(north)), Ok(()));
    // West's front is open: sealing its egg is rejected.
    assert_eq!(
        validate(&arena.grid, &sealed_egg(west)),
        Err(DecorationError::NotSealed { decoration: 0 })
    );
    // North with its cap removed, or its front block turned to glass.
    for (slot, voxel) in [
        (CAP, Voxel::Empty),
        (1, Voxel::Block(BlockType::Glass)),
        (3, Voxel::Empty),
    ] {
        let mut grid = arena.grid.clone();
        grid.set_world(north.defense[slot].position, voxel).unwrap();
        assert_eq!(
            validate(&grid, &sealed_egg(north)),
            Err(DecorationError::NotSealed { decoration: 0 }),
            "{slot} {voxel:?}"
        );
    }
    // Without its floor block the egg is both unsupported and open underneath.
    let mut grid = arena.grid.clone();
    let floor = VoxelPosition::new(north.egg.x, FLOOR_Y, north.egg.z);
    grid.set_world(floor, Voxel::Empty).unwrap();
    assert!(validate(&grid, &sealed_egg(north)).is_err());
    // A prop spanning two cells cannot be sealed in one.
    let mut two_cells = Decorations::default();
    let target = north.defense[1].position;
    let heading = ArrowHeading::against(north.attack_direction);
    two_cells.push(
        DecorationKind::Arrow,
        DecorationSite::Center,
        Mount::Embedded { target, heading },
        super::models::arrow(target, heading, (8.0, 8.0)),
    );
    two_cells.seal_last();
    assert_eq!(
        validate(&arena.grid, &two_cells),
        Err(DecorationError::NotSealed { decoration: 0 })
    );
}
