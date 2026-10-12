//! Battle-aftermath props on the constructed EggWars arena: the two surviving eggs, shell
//! fragments where the other two were broken, open chests in the central ruin, arrows stuck in
//! blocks, and a few displaced block fragments.
//!
//! Props are small, so they are not voxels: each is a handful of project-owned AABBs that the
//! scene scans alongside the one global voxel grid. Placement runs once, after construction, as a
//! pure function of the constructed arena:
//!
//! 1. read anchors from construction metadata (egg cells, defense slots, bridge lanes, the central
//!    structure) and from the grid (break edges, surface anchors);
//! 2. build every prop from its model in that anchor's frame;
//! 3. validate the whole set (see `validate`);
//! 4. only then return it.
//!
//! Placement never writes the grid. The story is fixed by design and follows the construction's
//! narrative states; a seed reaches it only through terrain heights and bridge break positions and
//! a few pixel-sized offsets drawn from stateless hashes, none of which can add, remove, or move a
//! prop to another site.

use std::{error::Error, fmt};

use super::{
    BridgeState, DefenseState, EggWarsArena, IslandId,
    construction::{BridgeInfo, CentralStructure, EggBase, surface_anchor},
    noise::{Domain, hash3},
};
use crate::{
    geometry::Aabb,
    material::{DecorativeMaterials, MaterialId},
    math::Vec3,
    voxel::{BlockMaterials, BlockType, VoxelGrid, VoxelPosition},
};

mod models;
#[cfg(test)]
mod tests;
mod validation;

pub use models::ArrowHeading;
use models::{Model, arrow, chest, chunk, egg, shard};
use validation::validate;

/// Pixel offsets of shell fragments and block fragments, each `0..=2` px per horizontal axis.
const SHARD_JITTER: Domain = Domain::from_tag(*b"dec-shrd");
const REMNANT_JITTER: Domain = Domain::from_tag(*b"dec-rmnt");
/// Pixel offsets of arrows on the face they are stuck in, `0..=2` px per in-face axis.
const ARROW_JITTER: Domain = Domain::from_tag(*b"dec-arrw");

/// What a prop represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecorationKind {
    /// A surviving egg, inside its defense.
    Egg,
    /// A fragment of a broken egg's shell.
    ShellFragment,
    /// An open chest.
    Chest,
    /// An arrow stuck in a block.
    Arrow,
    /// A piece of a broken block knocked loose during the fight.
    Remnant,
}

/// Where a prop belongs in the story.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecorationSite {
    /// The base of an outer island.
    Base(IslandId),
    /// The bridge from an outer island toward the center.
    Bridge(IslandId),
    /// The central ruin and the grass around it.
    Center,
}

/// How a prop meets the voxel world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mount {
    /// Every bottom face rests on the top of `support`.
    Resting { support: VoxelPosition },
    /// The arrowhead is driven into `target`, the arrow flying along `heading`.
    Embedded {
        target: VoxelPosition,
        heading: ArrowHeading,
    },
}

/// The surface of one prop part. Props reuse the canonical cobblestone and obsidian for block
/// fragments; everything else is a decorative material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecorationMaterial {
    EggShell,
    ChestWood,
    ChestInterior,
    ChestLatch,
    ArrowShaft,
    ArrowHead,
    ArrowFletching,
    Cobblestone,
    Obsidian,
}

/// Resolves every `DecorationMaterial` to a registered scene material, the way `BlockMaterials`
/// resolves block types: a constant-time exhaustive match, no strings or hashing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecorationMaterials {
    pub blocks: BlockMaterials,
    pub decorative: DecorativeMaterials,
}

impl DecorationMaterials {
    pub const fn new(blocks: BlockMaterials, decorative: DecorativeMaterials) -> Self {
        Self { blocks, decorative }
    }

    pub const fn material(&self, material: DecorationMaterial) -> MaterialId {
        let d = &self.decorative;
        match material {
            DecorationMaterial::EggShell => d.egg_shell,
            DecorationMaterial::ChestWood => d.chest_wood,
            DecorationMaterial::ChestInterior => d.chest_interior,
            DecorationMaterial::ChestLatch => d.chest_latch,
            DecorationMaterial::ArrowShaft => d.arrow_shaft,
            DecorationMaterial::ArrowHead => d.arrow_head,
            DecorationMaterial::ArrowFletching => d.arrow_fletching,
            DecorationMaterial::Cobblestone => self.blocks.material(BlockType::Cobblestone),
            DecorationMaterial::Obsidian => self.blocks.material(BlockType::Obsidian),
        }
    }
}

/// One box of a prop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecorationPart {
    pub bounds: Aabb,
    pub material: DecorationMaterial,
}

/// One prop: a few consecutive parts of `Decorations::parts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoration {
    pub kind: DecorationKind,
    pub site: DecorationSite,
    pub mount: Mount,
    /// Shut inside one cell whose six face neighbors are opaque blocks (validated): North's egg
    /// behind its intact defense. It is part of the story, but no ray can reach it, so the scene
    /// does not instantiate its parts (see `Decorations::visible`).
    pub sealed: bool,
    first_part: usize,
    part_count: usize,
}

/// Every prop of the arena and all their parts, in placement order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decorations {
    decorations: Vec<Decoration>,
    parts: Vec<DecorationPart>,
}

impl Decorations {
    pub fn decorations(&self) -> &[Decoration] {
        &self.decorations
    }

    /// Every part of every prop: one `SceneObject` each.
    pub fn parts(&self) -> &[DecorationPart] {
        &self.parts
    }

    pub fn parts_of(&self, decoration: &Decoration) -> &[DecorationPart] {
        &self.parts[decoration.first_part..decoration.first_part + decoration.part_count]
    }

    /// Props of `kind`.
    pub fn count(&self, kind: DecorationKind) -> usize {
        self.decorations
            .iter()
            .filter(|decoration| decoration.kind == kind)
            .count()
    }

    pub fn of_kind(&self, kind: DecorationKind) -> impl Iterator<Item = &Decoration> {
        self.decorations
            .iter()
            .filter(move |decoration| decoration.kind == kind)
    }

    /// The props a ray can reach, in placement order: every prop except the sealed ones. These
    /// are the props the scene instantiates.
    pub fn visible(&self) -> impl Iterator<Item = &Decoration> {
        self.decorations
            .iter()
            .filter(|decoration| !decoration.sealed)
    }

    /// Parts of the visible props: one `SceneObject` each.
    pub fn visible_part_count(&self) -> usize {
        self.visible().map(|decoration| decoration.part_count).sum()
    }

    fn push(&mut self, kind: DecorationKind, site: DecorationSite, mount: Mount, model: Model) {
        let first_part = self.parts.len();
        self.parts.extend(model.parts());
        self.decorations.push(Decoration {
            kind,
            site,
            mount,
            sealed: false,
            first_part,
            part_count: self.parts.len() - first_part,
        });
    }

    /// Marks the last placed prop as sealed in; `validate` checks that it really is.
    fn seal_last(&mut self) {
        if let Some(decoration) = self.decorations.last_mut() {
            decoration.sealed = true;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecorationError {
    /// A planned anchor holds no block, so a prop cannot rest on or be stuck in it.
    MissingAnchor(VoxelPosition),
    /// A part of prop `decoration` (an index into `Decorations::decorations`) leaves the grid.
    OutsideGrid { decoration: usize },
    /// A part other than an arrowhead overlaps a block.
    IntersectsBlock {
        decoration: usize,
        block: VoxelPosition,
    },
    /// An arrowhead is not embedded in exactly its target, or is buried entirely inside it.
    NotEmbedded { decoration: usize },
    /// A resting prop's bottom does not lie on its support.
    Unsupported { decoration: usize },
    /// Parts of two different props overlap.
    Overlap { first: usize, second: usize },
    /// A sealed prop is not shut inside one cell with an opaque block on each of its six faces.
    NotSealed { decoration: usize },
}

impl fmt::Display for DecorationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAnchor(p) => write!(
                formatter,
                "decoration anchor ({}, {}, {}) holds no block",
                p.x, p.y, p.z
            ),
            Self::OutsideGrid { decoration } => {
                write!(formatter, "decoration {decoration} leaves the grid")
            }
            Self::IntersectsBlock { decoration, block } => write!(
                formatter,
                "decoration {decoration} intersects the block at ({}, {}, {})",
                block.x, block.y, block.z
            ),
            Self::NotEmbedded { decoration } => write!(
                formatter,
                "arrow {decoration} is not embedded in exactly its target"
            ),
            Self::Unsupported { decoration } => {
                write!(
                    formatter,
                    "decoration {decoration} does not rest on its support"
                )
            }
            Self::Overlap { first, second } => {
                write!(formatter, "decorations {first} and {second} overlap")
            }
            Self::NotSealed { decoration } => {
                write!(formatter, "sealed decoration {decoration} is not enclosed")
            }
        }
    }
}

impl Error for DecorationError {}

/// Whether the egg behind a defense in `state` survived the battle: the defenses that kept their
/// cap and most of their ring (North intact, West damaged) protected it; behind the breached
/// (East) and destroyed (South) defenses it was broken.
pub const fn egg_survives(state: DefenseState) -> bool {
    matches!(state, DefenseState::Intact | DefenseState::Damaged)
}

/// Places every prop on the constructed `arena`, reading only its grid and metadata.
pub fn place_decorations(arena: &EggWarsArena) -> Result<Decorations, DecorationError> {
    let mut placer = Placer {
        grid: &arena.grid,
        seed: arena.terrain.global_seed,
        decorations: Decorations::default(),
    };
    let construction = &arena.construction;
    for base in &construction.bases {
        placer.base(base)?;
    }
    for bridge in &construction.bridges {
        placer.bridge(bridge)?;
    }
    placer.center(construction.central)?;
    validate(&arena.grid, &placer.decorations)?;
    Ok(placer.decorations)
}

/// The horizontal plane around an anchor cell, rotated like the construction's island frames:
/// local `side` and `forward` count blocks from the `origin` cell's local corner (`0..1` is the
/// origin cell itself), `up` counts blocks from the origin cell's floor, and `side` is `forward`
/// turned a quarter turn the same way on every island.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    origin: VoxelPosition,
    forward: (i32, i32),
}

impl Frame {
    pub(super) const fn new(origin: VoxelPosition, forward: (i32, i32)) -> Self {
        Self { origin, forward }
    }

    const fn side(self) -> (i32, i32) {
        (self.forward.1, -self.forward.0)
    }

    /// The cell `side` and `forward` whole cells from the origin, `up` levels above it.
    pub(super) const fn cell(self, side: i32, forward: i32, up: i32) -> VoxelPosition {
        let (sx, sz) = self.side();
        let (fx, fz) = self.forward;
        VoxelPosition::new(
            self.origin.x + side * sx + forward * fx,
            self.origin.y + up,
            self.origin.z + side * sz + forward * fz,
        )
    }

    /// This frame moved to the cell `side`, `forward`, `up` cells away, keeping its orientation.
    pub(super) const fn at(self, side: i32, forward: i32, up: i32) -> Self {
        Self::new(self.cell(side, forward, up), self.forward)
    }

    /// The world box of the local box `side` × `up` × `forward`.
    ///
    /// A quarter turn maps axis-aligned boxes to axis-aligned boxes, so every model stays a set
    /// of AABBs on every island. Along a negative world axis a local interval `[a, b]` of the
    /// origin cell maps to `[o + 1 - b, o + 1 - a]`. Models use sixteenths and thirty-seconds,
    /// which this arithmetic keeps exact in `f32`.
    pub(super) fn aabb(self, side: [f32; 2], up: [f32; 2], forward: [f32; 2]) -> Aabb {
        let (sx, sz) = self.side();
        let (fx, fz) = self.forward;
        let axis = |origin: i32, side_unit: i32, forward_unit: i32| {
            let (unit, [a, b]) = if side_unit != 0 {
                (side_unit, side)
            } else {
                (forward_unit, forward)
            };
            let o = origin as f32;
            if unit > 0 {
                (o + a, o + b)
            } else {
                (o + 1.0 - b, o + 1.0 - a)
            }
        };
        let (x0, x1) = axis(self.origin.x, sx, fx);
        let (z0, z1) = axis(self.origin.z, sz, fz);
        let y = self.origin.y as f32;
        Aabb::try_new(Vec3::new(x0, y + up[0], z0), Vec3::new(x1, y + up[1], z1))
            .expect("decoration models are finite, ordered boxes")
    }
}

struct Placer<'a> {
    grid: &'a VoxelGrid,
    seed: u64,
    decorations: Decorations,
}

impl Placer<'_> {
    /// `0..=2` px offsets on two axes for prop `index` of `site`, from `domain`.
    fn jitter(&self, domain: Domain, site: DecorationSite, index: i32) -> (f32, f32) {
        let site = match site {
            DecorationSite::Base(island) => island as i32,
            DecorationSite::Bridge(island) => 8 + island as i32,
            DecorationSite::Center => 16,
        };
        let pixels = |axis| (hash3(self.seed, domain, site, index, axis) % 3) as f32;
        (pixels(0), pixels(1))
    }

    fn occupied(&self, position: VoxelPosition) -> Result<VoxelPosition, DecorationError> {
        if self.grid.is_occupied_world(position) {
            Ok(position)
        } else {
            Err(DecorationError::MissingAnchor(position))
        }
    }

    fn resting(
        &mut self,
        kind: DecorationKind,
        site: DecorationSite,
        frame: Frame,
        model: Model,
    ) -> Result<(), DecorationError> {
        let support = self.occupied(frame.cell(0, 0, -1))?;
        self.decorations
            .push(kind, site, Mount::Resting { support }, model);
        Ok(())
    }

    /// An arrow flying along `heading` into `target`, at in-face pixel position `(a, b)` plus a
    /// seed offset.
    fn arrow(
        &mut self,
        site: DecorationSite,
        index: i32,
        target: VoxelPosition,
        heading: ArrowHeading,
        (a, b): (f32, f32),
    ) -> Result<(), DecorationError> {
        let target = self.occupied(target)?;
        let (da, db) = self.jitter(ARROW_JITTER, site, index);
        self.decorations.push(
            DecorationKind::Arrow,
            site,
            Mount::Embedded { target, heading },
            arrow(target, heading, (a + da, b + db)),
        );
        Ok(())
    }

    /// A shell fragment or a block fragment resting in `frame`'s origin cell, built by `model` at
    /// pixel offset `(s, f)` plus a seed offset.
    fn fragment(
        &mut self,
        kind: DecorationKind,
        (site, index): (DecorationSite, i32),
        frame: Frame,
        (s, f): (f32, f32),
        model: impl FnOnce(Frame, [f32; 2]) -> Model,
    ) -> Result<(), DecorationError> {
        let domain = match kind {
            DecorationKind::ShellFragment => SHARD_JITTER,
            _ => REMNANT_JITTER,
        };
        let (ds, df) = self.jitter(domain, site, index);
        self.resting(kind, site, frame, model(frame, [s + ds, f + df]))
    }

    /// The egg or the broken egg's shell, and the debris and arrows of the base's state.
    ///
    /// Everything stands in the base frame: origin the egg cell, forward toward the center, so
    /// local cell `(0, 1)` is the front defense block and `(0, 2)` the floor in front of it.
    fn base(&mut self, base: &EggBase) -> Result<(), DecorationError> {
        let site = DecorationSite::Base(base.island);
        let frame = Frame::new(base.egg, base.attack_direction);
        let heading = ArrowHeading::against(base.attack_direction);
        // Arrows fired from the bridge stick in the front face of a surviving front or flank
        // block, mid-height, slightly off center.
        let front_arrow = |slot: usize| (base.defense[slot].position, (9.0, 7.0));
        let obsidian_chunk = |frame, s| chunk(frame, s, 7.0, DecorationMaterial::Obsidian);
        let cobble_chunk = |frame, s| chunk(frame, s, 8.0, DecorationMaterial::Cobblestone);

        if egg_survives(base.defense_state) {
            self.resting(DecorationKind::Egg, site, frame, egg(frame))?;
        }
        match base.defense_state {
            // North held: its egg stands sealed behind all nine blocks, untouched. Ring, cap, and
            // floor close every face of its cell, so it is story metadata only, never geometry.
            DefenseState::Intact => self.decorations.seal_last(),
            // West: one broken obsidian piece lies in front of the notch, an arrow is stuck in
            // the surviving front-right block, and the egg shows through the gap.
            DefenseState::Damaged => {
                let (target, at) = front_arrow(2);
                self.arrow(site, 0, target, heading, at)?;
                let floor = self.on_floor(frame, -1, 2)?;
                self.fragment(
                    DecorationKind::Remnant,
                    (site, 0),
                    floor,
                    (4.0, 4.0),
                    obsidian_chunk,
                )?;
            }
            // East: the egg was broken where it stood; its shell lies in the egg cell and just
            // in front, a front block lies on the floor, and an arrow is stuck in the left flank.
            DefenseState::Breached => {
                for (index, (cell, at, size)) in [
                    ((0, 0), (3.0, 3.0), [5.0, 2.0, 3.0]),
                    ((0, 0), (10.0, 9.0), [3.0, 3.0, 4.0]),
                    ((0, 1), (6.0, 5.0), [4.0, 2.0, 3.0]),
                ]
                .into_iter()
                .enumerate()
                {
                    let floor = self.on_floor(frame, cell.0, cell.1)?;
                    self.fragment(
                        DecorationKind::ShellFragment,
                        (site, index as i32),
                        floor,
                        at,
                        |frame, s| shard(frame, s, size),
                    )?;
                }
                let (target, at) = front_arrow(3);
                self.arrow(site, 0, target, heading, at)?;
                let floor = self.on_floor(frame, 1, 2)?;
                self.fragment(
                    DecorationKind::Remnant,
                    (site, 0),
                    floor,
                    (4.0, 4.0),
                    obsidian_chunk,
                )?;
            }
            // South: the shell is scattered across the opened side of the enclosure, obsidian
            // from the lost front row lies in front, cobblestone from the blasted floor lies in
            // the crater on the grass, and an arrow is stuck in the last flank.
            DefenseState::Destroyed => {
                for (index, (cell, at, size)) in [
                    ((0, 0), (5.0, 7.0), [4.0, 2.0, 3.0]),
                    ((1, 0), (4.0, 3.0), [3.0, 2.0, 4.0]),
                    ((1, 1), (8.0, 9.0), [5.0, 3.0, 3.0]),
                ]
                .into_iter()
                .enumerate()
                {
                    let floor = self.on_floor(frame, cell.0, cell.1)?;
                    self.fragment(
                        DecorationKind::ShellFragment,
                        (site, index as i32),
                        floor,
                        at,
                        |frame, s| shard(frame, s, size),
                    )?;
                }
                let (target, at) = front_arrow(3);
                self.arrow(site, 0, target, heading, at)?;
                let floor = self.on_floor(frame, 0, 2)?;
                self.fragment(
                    DecorationKind::Remnant,
                    (site, 0),
                    floor,
                    (4.0, 4.0),
                    obsidian_chunk,
                )?;
                let crater = self.on_ground(frame.at(-1, 2, 0))?;
                self.fragment(
                    DecorationKind::Remnant,
                    (site, 1),
                    crater,
                    (3.0, 3.0),
                    cobble_chunk,
                )?;
            }
        }
        Ok(())
    }

    /// Frame on the floor of local cell `(side, forward)`: the egg level, where the base floor
    /// supports it.
    fn on_floor(&self, frame: Frame, side: i32, forward: i32) -> Result<Frame, DecorationError> {
        let floor = frame.at(side, forward, 0);
        self.occupied(floor.cell(0, 0, -1))?;
        Ok(floor)
    }

    /// Frame on whatever is on top of the column of `frame`'s origin, at any height.
    fn on_ground(&self, frame: Frame) -> Result<Frame, DecorationError> {
        let cell = frame.cell(0, 0, 0);
        let anchor = surface_anchor(self.grid, cell.x, cell.z)
            .ok_or(DecorationError::MissingAnchor(cell))?;
        Ok(Frame::new(anchor, frame.forward))
    }

    /// Debris and arrows at the bridge breaks: none on the complete North bridge or the
    /// abandoned West stub, whose builders simply stopped; on the damaged East bridge an arrow in
    /// the island-side break edge and a fragment on the far edge; on the severed South bridge a
    /// fragment on each side of the long break.
    fn bridge(&mut self, bridge: &BridgeInfo) -> Result<(), DecorationError> {
        let site = DecorationSite::Bridge(bridge.island);
        let toward = bridge.toward_center;
        let away = (-toward.0, -toward.1);
        let island_edge = |lane: usize| {
            let lane = bridge.lanes[lane];
            break_edge(self.grid, lane.outer_end, toward, lane.center_end)
        };
        let center_edge = |lane: usize| {
            let lane = bridge.lanes[lane];
            break_edge(self.grid, lane.center_end, away, lane.outer_end)
        };
        let on_deck = |deck: VoxelPosition| {
            Frame::new(VoxelPosition::new(deck.x, deck.y + 1, deck.z), toward)
        };
        let cobble_chunk = |frame, s| chunk(frame, s, 8.0, DecorationMaterial::Cobblestone);

        match bridge.state {
            BridgeState::Complete | BridgeState::Incomplete => {}
            BridgeState::Damaged => {
                self.arrow(site, 0, island_edge(1), ArrowHeading::Down, (8.0, 6.0))?;
                let frame = on_deck(center_edge(0));
                self.fragment(
                    DecorationKind::Remnant,
                    (site, 0),
                    frame,
                    (3.0, 3.0),
                    cobble_chunk,
                )?;
            }
            BridgeState::Severed => {
                for (index, deck) in [island_edge(0), center_edge(1)].into_iter().enumerate() {
                    self.fragment(
                        DecorationKind::Remnant,
                        (site, index as i32),
                        on_deck(deck),
                        (3.0, 3.0),
                        cobble_chunk,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// The contested center: two open chests in the ruin's open interior, against the north and
    /// west walls with their fronts toward the open south-east; arrows in the inner faces of both
    /// walls (one between the two glass panes) and in the grass outside; and a cobblestone
    /// fragment fallen beside the crumbled south-east pillar.
    fn center(&mut self, central: CentralStructure) -> Result<(), DecorationError> {
        let site = DecorationSite::Center;
        let open = central.open_area;
        // North wall chest: east of both panes, facing south.
        let north_chest = Frame::new(
            VoxelPosition::new(open.max.x - 1, open.min.y, open.min.z),
            (0, 1),
        );
        // West wall chest: facing east.
        let west_chest = Frame::new(
            VoxelPosition::new(open.min.x, open.min.y, open.min.z + 2),
            (1, 0),
        );
        for frame in [north_chest, west_chest] {
            self.resting(DecorationKind::Chest, site, frame, chest(frame))?;
        }

        // The north wall's inner face between the panes, one level above the open interior.
        let north_wall = VoxelPosition::new(open.min.x + 2, open.min.y + 1, open.min.z - 1);
        self.arrow(site, 0, north_wall, ArrowHeading::NegativeZ, (7.0, 6.0))?;
        let west_wall = VoxelPosition::new(open.min.x - 1, open.min.y + 1, open.min.z + 1);
        self.arrow(site, 1, west_wall, ArrowHeading::NegativeX, (7.0, 8.0))?;

        // South-east of the ruin, beside the crumbled pillar, on whatever grass is there.
        let floor = central.floor;
        let lawn = self.on_ground(Frame::new(
            VoxelPosition::new(floor.max.x + 2, 0, floor.max.z + 1),
            (0, 1),
        ))?;
        let lawn_block = lawn.cell(0, 0, -1);
        self.arrow(site, 2, lawn_block, ArrowHeading::Down, (6.0, 7.0))?;
        let rubble = self.on_ground(Frame::new(
            VoxelPosition::new(floor.max.x + 1, 0, floor.max.z),
            (0, 1),
        ))?;
        self.fragment(
            DecorationKind::Remnant,
            (site, 0),
            rubble,
            (3.0, 3.0),
            |frame, s| chunk(frame, s, 8.0, DecorationMaterial::Cobblestone),
        )
    }
}

/// The last occupied deck cell walking from `from` along `step` toward `end`: the edge of a
/// bridge's break, or `end` itself when the lane is unbroken.
fn break_edge(
    grid: &VoxelGrid,
    from: VoxelPosition,
    step: (i32, i32),
    end: VoxelPosition,
) -> VoxelPosition {
    let mut cell = from;
    while cell != end {
        let next = VoxelPosition::new(cell.x + step.0, cell.y, cell.z + step.1);
        if !grid.is_occupied_world(next) {
            break;
        }
        cell = next;
    }
    cell
}
