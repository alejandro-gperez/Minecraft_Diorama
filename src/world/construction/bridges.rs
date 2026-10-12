//! The four player-built bridges: each runs from an outer island straight toward the center as a
//! two-block-wide cobblestone deck at `DECK_Y`, in its island's narrative state.
//!
//! A deck's two ends come from the actual terrain. Starting in the open void between the
//! islands' regions, each lane walks toward each island across void and across rim columns whose
//! grass sank below the deck (the deck rests on that grass), and stops beside the first column
//! whose ground reaches the deck. Each end block therefore touches terrain face to face: there is
//! no gap at an attached end, and no deck block is ever inside terrain.
//!
//! Damage is defined along the void between the two islands' regions, which every seed leaves
//! open (Mission 29), so a seed's terrain can move a deck's ends but can never close a gap,
//! reconnect a severed bridge, or complete the abandoned one.

use super::{ConstructionError, DECK_Y, Frame, Plan, Terrain};
use crate::{
    voxel::{BlockType, VoxelPosition},
    world::{
        ArenaLayout, IslandId,
        arena::{CENTER_FOOTPRINT, OUTER_FOOTPRINT},
        noise::{Domain, hash3},
    },
};

/// Side offsets of the two lanes, straddling the islands' shared center line.
const LANES: [i32; 2] = [-1, 0];

/// First forward cell beyond the outer island's footprint: where the guaranteed void begins.
const VOID_START: i32 = OUTER_FOOTPRINT as i32 / 2;

// Damage, in forward cells from `VOID_START`. The void is `separation - 20` = 12 cells long.
/// East: three cells missing from both lanes, shifted one cell either way by the seed; one lane
/// broke one cell further toward the East island, so the break is ragged.
const DAMAGED_GAP_START: i32 = 4;
const DAMAGED_GAP: i32 = 3;
/// South: six cells missing from both lanes, with one lane broken one cell further on one side.
const SEVERED_GAP_START: i32 = 3;
const SEVERED_GAP: i32 = 6;
/// West: one lane reaches this far, the other one cell less, ending mid-void like an abandoned
/// bridge whose builder was still placing blocks.
const INCOMPLETE_REACH: i32 = 5;
/// North: one leftover block hangs under the deck, as players leave when bridging, this far in
/// plus up to two cells chosen by the seed.
const SCAFFOLD_START: i32 = 3;

const GAP_SHIFT: Domain = Domain::from_tag(*b"con-gaps");
const RAGGED: Domain = Domain::from_tag(*b"con-ragd");
const SCAFFOLD: Domain = Domain::from_tag(*b"con-scaf");

/// The narrative state of a bridge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BridgeState {
    /// North: a continuous route from island to center survived the battle.
    Complete,
    /// East: partially destroyed; one short section is missing.
    Damaged,
    /// South: broken into two separated segments by a longer missing span.
    Severed,
    /// West: started from the island and abandoned before reaching the center.
    Incomplete,
}

impl BridgeState {
    /// The designed state of the bridge from `island`; `None` for the center.
    pub const fn of(island: IslandId) -> Option<Self> {
        match island {
            IslandId::Center => None,
            IslandId::North => Some(Self::Complete),
            IslandId::East => Some(Self::Damaged),
            IslandId::South => Some(Self::Severed),
            IslandId::West => Some(Self::Incomplete),
        }
    }
}

/// The planned extent of one lane. Whether each cell between the ends holds a block depends on
/// the bridge's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeLane {
    /// Deck cell touching the outer island's terrain.
    pub outer_end: VoxelPosition,
    /// Deck cell touching the central island's terrain (absent from an incomplete bridge).
    pub center_end: VoxelPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeInfo {
    /// The outer island the bridge starts from.
    pub island: IslandId,
    pub state: BridgeState,
    /// Unit step `(dx, dz)` from the outer island toward the center.
    pub toward_center: (i32, i32),
    pub lanes: [BridgeLane; 2],
    /// Cells along the bridge where neither lane has a block: the visible break.
    pub missing_span: u32,
}

/// The seed's small optional variations for one bridge. They move a break by at most a cell or
/// make one lane's edge ragged; none changes a gap's length in both lanes, closes it, or moves
/// it out of the void.
#[derive(Clone, Copy, Debug)]
struct Variation {
    /// `-1..=1`: where along the void the East gap sits.
    gap_shift: i32,
    /// The lane whose break is one cell longer, or which stops one cell short.
    ragged_lane: usize,
    /// Whether the South ragged lane breaks further toward the center rather than the island.
    ragged_toward_center: bool,
    /// `0..=2`: where along the North bridge its scaffold block hangs.
    scaffold: i32,
}

impl Variation {
    fn new(global_seed: u64, island: IslandId) -> Self {
        let hash = |domain| hash3(global_seed, domain, island as i32, 0, 0);
        let ragged = hash(RAGGED);
        Self {
            gap_shift: (hash(GAP_SHIFT) % 3) as i32 - 1,
            ragged_lane: (ragged & 1) as usize,
            ragged_toward_center: ragged & 2 != 0,
            scaffold: (hash(SCAFFOLD) % 3) as i32,
        }
    }

    /// Whether `lane` keeps its deck block at forward cell `f`.
    fn built(self, state: BridgeState, lane: usize, f: i32) -> bool {
        let ragged = i32::from(lane == self.ragged_lane);
        match state {
            BridgeState::Complete => true,
            BridgeState::Damaged => {
                let start = VOID_START + DAMAGED_GAP_START + self.gap_shift;
                !(start - ragged..start + DAMAGED_GAP).contains(&f)
            }
            BridgeState::Severed => {
                let start = VOID_START + SEVERED_GAP_START;
                let (before, after) = if self.ragged_toward_center {
                    (0, ragged)
                } else {
                    (ragged, 0)
                };
                !(start - before..start + SEVERED_GAP + after).contains(&f)
            }
            BridgeState::Incomplete => f <= VOID_START + INCOMPLETE_REACH - ragged,
        }
    }
}

/// Plans the bridge from outer `island` toward the center.
pub(super) fn plan_bridge(
    terrain: &Terrain<'_>,
    layout: &ArenaLayout,
    island: IslandId,
    global_seed: u64,
    plan: &mut Plan,
) -> Result<BridgeInfo, ConstructionError> {
    let state = BridgeState::of(island).expect("bridges start from outer islands");
    let frame = Frame::facing_center(layout, island);
    let separation = layout.separation() as i32;
    let void_end = separation - CENTER_FOOTPRINT as i32 / 2 - 1;
    let middle = (VOID_START + void_end) / 2;
    let variation = Variation::new(global_seed, island);
    let deck = |side: i32, f: i32| {
        let (x, z) = frame.cell(side, f);
        VoxelPosition::new(x, DECK_Y, z)
    };

    let mut ends = [(0, 0); 2];
    for (lane, &side) in LANES.iter().enumerate() {
        let first = deck_end(terrain, frame, side, middle, -1, separation)?;
        let last = deck_end(terrain, frame, side, middle, 1, separation)?;
        ends[lane] = (first, last);
        for f in first..=last {
            if variation.built(state, lane, f) {
                plan.put(deck(side, f), BlockType::Cobblestone);
            }
        }
    }
    if state == BridgeState::Complete {
        let side = LANES[variation.ragged_lane];
        let f = VOID_START + SCAFFOLD_START + variation.scaffold;
        let below = deck(side, f);
        plan.put(
            VoxelPosition::new(below.x, DECK_Y - 1, below.z),
            BlockType::Cobblestone,
        );
    }

    // The break: cells where both lanes have deck but neither has a block.
    let shared = ends[0].0.max(ends[1].0)..=ends[0].1.min(ends[1].1);
    let missing_span = shared
        .filter(|&f| (0..LANES.len()).all(|lane| !variation.built(state, lane, f)))
        .count() as u32;

    Ok(BridgeInfo {
        island,
        state,
        toward_center: frame.forward,
        lanes: [0, 1].map(|lane| BridgeLane {
            outer_end: deck(LANES[lane], ends[lane].0),
            center_end: deck(LANES[lane], ends[lane].1),
        }),
        missing_span,
    })
}

/// The last deck cell of lane `side` walking from void cell `from` in direction `step`: the walk
/// crosses open void and grass sunk below the deck and stops beside the first column whose ground
/// reaches `DECK_Y`.
fn deck_end(
    terrain: &Terrain<'_>,
    frame: Frame,
    side: i32,
    from: i32,
    step: i32,
    separation: i32,
) -> Result<i32, ConstructionError> {
    let mut f = from;
    loop {
        let next = f + step;
        let (x, z) = frame.cell(side, next);
        match terrain.anchor_if_land(x, z) {
            Some(anchor) if anchor.y > DECK_Y => return Ok(f),
            // Every island's center is flat ground, so a walk past it means no island here.
            _ if (next - from).abs() > separation => {
                return Err(ConstructionError::MissingTerrain { x, z });
            }
            _ => f = next,
        }
    }
}
