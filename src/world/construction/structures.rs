//! Island structures: on each outer island, a small base around its future egg, guarded by the
//! nine-position obsidian defense in that island's damage state; on the central island, a compact
//! ruin around an open obsidian-inlaid floor where loot will be contested.
//!
//! Every element stands on the anchor of its own column, so structures follow the actual grass
//! height: a floor fills a sunken column up to the floor level and leaves a raised grass block in
//! place as part of the floor, and pillars and walls start on whichever is there. All of it lies
//! well inside the smallest possible island silhouette, so every column has ground.

use std::ops::RangeInclusive;

use super::{ConstructionError, EGG_Y, FLOOR_Y, Frame, Plan, Terrain};
use crate::{
    voxel::{BlockType, VoxelPosition},
    world::{ArenaLayout, IslandBounds, IslandId, arena::SURFACE_LEVEL},
};

/// The nine defense positions as `(side, forward, up)` offsets from the egg, the Minecraft
/// EggWars enclosure: the eight cells around the egg at its level, front row first (the row facing
/// the center, where attackers arrive over the bridge), then the two flanks, then the back row,
/// and finally the one block directly above the egg. The egg's own cell stays empty.
///
/// ```text
/// O O O   forward +1 (toward the bridge)
/// O E O   forward  0   (E: the future egg, capped by a ninth block)
/// O O O   forward -1
/// ```
pub const DEFENSE_TEMPLATE: [(i32, i32, i32); DEFENSE_SLOTS] = [
    (-1, 1, 0),
    (0, 1, 0),
    (1, 1, 0),
    (-1, 0, 0),
    (1, 0, 0),
    (-1, -1, 0),
    (0, -1, 0),
    (1, -1, 0),
    (0, 0, 1),
];
/// Number of positions in `DEFENSE_TEMPLATE`.
pub const DEFENSE_SLOTS: usize = 9;

/// The egg's `(side, forward)` cell in its island's frame: on the bridge's lane `0`, two cells
/// behind the island's center, inside the always-flat middle of every 16×16 island.
pub(super) const EGG_CELL: (i32, i32) = (0, -2);
/// The outer base floor, five cells across and five deep around the egg.
pub(super) const BASE_SIDES: RangeInclusive<i32> = -2..=2;
pub(super) const BASE_FORWARD: RangeInclusive<i32> = -4..=0;
/// Two gate pillars flank the bridge's approach at this forward cell.
pub(super) const GATE_FORWARD: i32 = 3;
pub(super) const GATE_SIDES: [i32; 2] = [-3, 2];

/// The central ruin's square floor, `x` and `z` in `CENTRAL_MIN..=CENTRAL_MAX`.
const CENTRAL_MIN: i32 = -4;
const CENTRAL_MAX: i32 = 3;
/// The 2×2 obsidian inlay at the center of the floor, `x` and `z` in this range.
const INLAY: RangeInclusive<i32> = -1..=0;
/// The small lava pool set into the floor's open south-east interior (`x` and `z` in this range),
/// flush with the floor, clear of both chests, the wall arrows, and the pillars.
pub const LAVA_POOL: RangeInclusive<i32> = 1..=2;
/// Corner pillars: `(x, z, blocks above the floor, obsidian cap)`, crumbling toward the
/// south-east so the open interior faces the default camera.
const PILLARS: [(i32, i32, i32, bool); 4] = [
    (CENTRAL_MIN, CENTRAL_MIN, 4, true),
    (CENTRAL_MAX, CENTRAL_MIN, 3, false),
    (CENTRAL_MIN, CENTRAL_MAX, 2, false),
    (CENTRAL_MAX, CENTRAL_MAX, 1, false),
];
/// Heights above the floor of the partial north wall (`z = CENTRAL_MIN`, `x` from west to east
/// between the pillars) and west wall (`x = CENTRAL_MIN`, `z` from north to south).
const NORTH_WALL: [i32; 6] = [3, 3, 3, 3, 2, 1];
const WEST_WALL: [i32; 6] = [2, 2, 1, 1, 0, 0];
/// Two glass panes set in the north wall, one cell apart, with open air in front and behind and
/// cobblestone on every other side, never terrain.
pub(super) const WINDOWS: [VoxelPosition; 2] = [
    VoxelPosition::new(-2, FLOOR_Y + 2, CENTRAL_MIN),
    VoxelPosition::new(0, FLOOR_Y + 2, CENTRAL_MIN),
];
/// Top of the tallest pillar, the highest construction block.
pub(super) const CENTRAL_TOP: i32 = FLOOR_Y + 4;

/// Damage state of an outer island's egg defense. The counts are artistic decisions, never random.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefenseState {
    /// North: all nine blocks remain; the base was never breached.
    Intact,
    /// West: seven remain; the front-left and front blocks were broken but the cap held.
    Damaged,
    /// East: five remain; the whole front row and the cap were broken through, leaving a U of
    /// flanks and back wall open toward the bridge.
    Breached,
    /// South: three remain; only a back-corner fragment of the enclosure is left standing.
    Destroyed,
}

impl DefenseState {
    /// The designed state of `island`'s defense; `None` for the center.
    pub const fn of(island: IslandId) -> Option<Self> {
        match island {
            IslandId::Center => None,
            IslandId::North => Some(Self::Intact),
            IslandId::West => Some(Self::Damaged),
            IslandId::East => Some(Self::Breached),
            IslandId::South => Some(Self::Destroyed),
        }
    }

    /// Which `DEFENSE_TEMPLATE` positions still hold obsidian. Blocks are lost from the front, the
    /// side the attackers came from, so every breach opens the egg toward the attack.
    pub const fn intact_slots(self) -> [bool; DEFENSE_SLOTS] {
        const Y: bool = true;
        const N: bool = false;
        match self {
            Self::Intact => [Y; DEFENSE_SLOTS],
            Self::Damaged => [N, N, Y, Y, Y, Y, Y, Y, Y],
            Self::Breached => [N, N, N, Y, Y, Y, Y, Y, N],
            Self::Destroyed => [N, N, N, Y, N, Y, Y, N, N],
        }
    }
}

/// One of the nine defense positions and whether its obsidian survived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefenseSlot {
    pub position: VoxelPosition,
    pub intact: bool,
}

/// An outer island's base: the anchors a later mission places its egg and decorations on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EggBase {
    pub island: IslandId,
    pub defense_state: DefenseState,
    /// The future egg's cell: empty, standing on the base floor.
    pub egg: VoxelPosition,
    /// Unit step `(dx, dz)` toward the center: where attackers come from and the defense faces.
    pub attack_direction: (i32, i32),
    /// `DEFENSE_TEMPLATE` placed around the egg, in template order.
    pub defense: [DefenseSlot; DEFENSE_SLOTS],
}

/// The per-island variations of the shared base design.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EggBaseDesign {
    pub defense: DefenseState,
    /// Gate pillar heights above the grass level, in `GATE_SIDES` order; zero is destroyed.
    pub gate_heights: [i32; 2],
    /// `(side, forward)` floor cells blasted away.
    pub missing_floor: &'static [(i32, i32)],
}

impl EggBaseDesign {
    /// Damage grows with the defense's: intact gates on North, worn on West, broken on East,
    /// and on South one gate stump and a cratered floor corner on the attack side.
    pub const fn of(island: IslandId) -> Option<Self> {
        let (gate_heights, missing_floor): ([i32; 2], &'static [(i32, i32)]) = match island {
            IslandId::Center => return None,
            IslandId::North => ([3, 3], &[]),
            IslandId::West => ([3, 2], &[]),
            IslandId::East => ([2, 1], &[(2, 0)]),
            IslandId::South => ([1, 0], &[(-2, 0), (-1, 0), (-2, -1)]),
        };
        match DefenseState::of(island) {
            Some(defense) => Some(Self {
                defense,
                gate_heights,
                missing_floor,
            }),
            None => None,
        }
    }
}

/// Plans the base of outer `island`: floor, gate pillars, and the egg defense.
pub(super) fn plan_base(
    terrain: &Terrain<'_>,
    layout: &ArenaLayout,
    island: IslandId,
    plan: &mut Plan,
) -> Result<EggBase, ConstructionError> {
    let design = EggBaseDesign::of(island).expect("bases stand on outer islands");
    let frame = Frame::facing_center(layout, island);

    for f in BASE_FORWARD {
        for s in BASE_SIDES {
            if design.missing_floor.contains(&(s, f)) {
                continue;
            }
            let cell = frame.cell(s, f);
            let anchor = terrain.anchor(cell.0, cell.1)?;
            plan.column(cell, anchor.y, FLOOR_Y, BlockType::Cobblestone);
        }
    }
    for (side, height) in GATE_SIDES.into_iter().zip(design.gate_heights) {
        let cell = frame.cell(side, GATE_FORWARD);
        let anchor = terrain.anchor(cell.0, cell.1)?;
        plan.column(
            cell,
            anchor.y,
            SURFACE_LEVEL + height,
            BlockType::Cobblestone,
        );
    }

    let at = |(side, forward): (i32, i32), up: i32| {
        let (x, z) = frame.cell(side, forward);
        VoxelPosition::new(x, EGG_Y + up, z)
    };
    let intact = design.defense.intact_slots();
    let mut defense = [DefenseSlot {
        position: at(EGG_CELL, 0),
        intact: false,
    }; DEFENSE_SLOTS];
    for (slot, ((ds, df, du), intact)) in defense
        .iter_mut()
        .zip(DEFENSE_TEMPLATE.into_iter().zip(intact))
    {
        *slot = DefenseSlot {
            position: at((EGG_CELL.0 + ds, EGG_CELL.1 + df), du),
            intact,
        };
        if intact {
            plan.put(slot.position, BlockType::Obsidian);
        }
    }

    Ok(EggBase {
        island,
        defense_state: design.defense,
        egg: at(EGG_CELL, 0),
        attack_direction: frame.forward,
        defense,
    })
}

/// The central ruin and the cells it leaves open for later decoration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CentralStructure {
    /// The cobblestone floor layer at `FLOOR_Y`.
    pub floor: IslandBounds,
    /// The obsidian inlay in the middle of the floor.
    pub inlay: IslandBounds,
    /// The open interior one level above the floor, inside the walls and pillars: empty, for
    /// future loot.
    pub open_area: IslandBounds,
}

/// Plans the central ruin on the central island, whose center line is the world origin.
pub(super) fn plan_central(
    terrain: &Terrain<'_>,
    plan: &mut Plan,
) -> Result<CentralStructure, ConstructionError> {
    let floor_block = |x: i32, z: i32| {
        if INLAY.contains(&x) && INLAY.contains(&z) {
            BlockType::Obsidian
        } else {
            BlockType::Cobblestone
        }
    };
    for z in CENTRAL_MIN..=CENTRAL_MAX {
        for x in CENTRAL_MIN..=CENTRAL_MAX {
            let anchor = terrain.anchor(x, z)?;
            if LAVA_POOL.contains(&x) && LAVA_POOL.contains(&z) {
                // Lava on top, cobblestone filling any sunken column below it.
                plan.column((x, z), anchor.y, FLOOR_Y - 1, BlockType::Cobblestone);
                plan.put(VoxelPosition::new(x, FLOOR_Y, z), BlockType::Lava);
            } else {
                plan.column((x, z), anchor.y, FLOOR_Y, floor_block(x, z));
            }
        }
    }

    // Pillars and walls start one level above the floor: on the floor, or on grass raised to it.
    for (x, z, height, capped) in PILLARS {
        let top = FLOOR_Y + height;
        let cap = if capped { top } else { top + 1 };
        plan.column((x, z), FLOOR_Y + 1, cap - 1, BlockType::Cobblestone);
        plan.column((x, z), cap, top, BlockType::Obsidian);
    }
    let walls = NORTH_WALL
        .into_iter()
        .zip(CENTRAL_MIN + 1..)
        .map(|(height, x)| (x, CENTRAL_MIN, height))
        .chain(
            WEST_WALL
                .into_iter()
                .zip(CENTRAL_MIN + 1..)
                .map(|(height, z)| (CENTRAL_MIN, z, height)),
        );
    for (x, z, height) in walls {
        for y in FLOOR_Y + 1..=FLOOR_Y + height {
            let position = VoxelPosition::new(x, y, z);
            let block = if WINDOWS.contains(&position) {
                BlockType::Glass
            } else {
                BlockType::Cobblestone
            };
            plan.put(position, block);
        }
    }

    let square = |min: i32, max: i32, y: i32| IslandBounds {
        min: VoxelPosition::new(min, y, min),
        max: VoxelPosition::new(max, y, max),
    };
    Ok(CentralStructure {
        floor: square(CENTRAL_MIN, CENTRAL_MAX, FLOOR_Y),
        inlay: square(*INLAY.start(), *INLAY.end(), FLOOR_Y),
        open_area: square(CENTRAL_MIN + 1, CENTRAL_MAX - 1, FLOOR_Y + 1),
    })
}
