//! The prop models: a few AABBs each, in pixels of the 16-pixel Minecraft block, built in an
//! anchor's `Frame` so one model serves every orientation.

use super::{DecorationMaterial, DecorationPart, Frame};
use crate::{
    geometry::{Aabb, CubeFace},
    math::Vec3,
    voxel::VoxelPosition,
};

/// One pixel of a 16×16 block texture, in blocks.
const PX: f32 = 1.0 / 16.0;

/// The boxes of one prop, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Model(pub(super) Vec<DecorationPart>);

impl Model {
    fn add(&mut self, bounds: Aabb, material: DecorationMaterial) {
        self.0.push(DecorationPart { bounds, material });
    }

    pub(super) fn parts(self) -> impl Iterator<Item = DecorationPart> {
        self.0.into_iter()
    }
}

/// Local box in pixels: `[min, max]` across, up, and forward.
type PixelBox = ([f32; 2], [f32; 2], [f32; 2]);

fn place(frame: Frame, (side, up, forward): PixelBox) -> Aabb {
    let px = |[a, b]: [f32; 2]| [a * PX, b * PX];
    frame.aabb(px(side), px(up), px(forward))
}

/// Egg layers, bottom to top: `(half width, bottom, top)` in pixels around the cell's center.
///
/// A stepped oval 12 px wide and 14 px tall, widest in its lower third like a real egg: a narrow
/// foot, a broad belly, then three narrowing courses to a small crown. Square courses read as the
/// blocky pixel-art oval of the era at 640×360. It stays 2 px clear of the eight cells around it
/// and of the cap above, so it touches only the floor it stands on.
pub(super) const EGG_LAYERS: [(f32, f32, f32); 5] = [
    (4.0, 0.0, 1.0),
    (6.0, 1.0, 6.0),
    (5.0, 6.0, 9.0),
    (4.0, 9.0, 12.0),
    (2.0, 12.0, 14.0),
];

/// A surviving egg standing in the frame's origin cell.
pub(super) fn egg(frame: Frame) -> Model {
    let mut model = Model::default();
    for (half, bottom, top) in EGG_LAYERS {
        let span = [8.0 - half, 8.0 + half];
        model.add(
            place(frame, (span, [bottom, top], span)),
            DecorationMaterial::EggShell,
        );
    }
    model
}

/// Chest heights above its floor, in pixels: the rim of the walls, the dark interior floor below
/// it, and the top of the open lid.
pub(super) const CHEST_RIM_PX: f32 = 10.0;
pub(super) const CHEST_FLOOR_PX: f32 = 7.0;
pub(super) const CHEST_LID_TOP_PX: f32 = 24.0;

/// An open chest in the frame's origin cell, its front toward local `forward`.
///
/// A 14 × 10 × 14 px wooden box, hollow: four 2 px walls around a dark interior whose floor sits
/// 3 px below the rim, so the open top shows a recess. The lid stands upright on the back wall,
/// swung open (an axis-aligned stand-in for the tilted lid), 14 px tall, with its latch near the
/// top of the face turned toward the viewer in front. Seven boxes.
pub(super) fn chest(frame: Frame) -> Model {
    const RIM: f32 = CHEST_RIM_PX;
    const FLOOR: f32 = CHEST_FLOOR_PX;
    const LID: f32 = CHEST_LID_TOP_PX;
    let wood = DecorationMaterial::ChestWood;
    let mut model = Model::default();
    for (bounds, material) in [
        (([1.0, 15.0], [0.0, RIM], [13.0, 15.0]), wood), // front wall
        (([1.0, 15.0], [0.0, RIM], [1.0, 3.0]), wood),   // back wall
        (([1.0, 3.0], [0.0, RIM], [3.0, 13.0]), wood),   // left wall
        (([13.0, 15.0], [0.0, RIM], [3.0, 13.0]), wood), // right wall
        (
            ([3.0, 13.0], [0.0, FLOOR], [3.0, 13.0]),
            DecorationMaterial::ChestInterior,
        ),
        (([1.0, 15.0], [RIM, LID], [1.0, 4.0]), wood), // open lid
        (
            ([7.0, 9.0], [19.0, 23.0], [4.0, 5.0]),
            DecorationMaterial::ChestLatch,
        ),
    ] {
        model.add(place(frame, bounds), material);
    }
    model
}

/// A flat shell fragment lying in the frame's origin cell with its corner at pixel `(s, f)`,
/// `size` px across, up, and forward.
pub(super) fn shard(frame: Frame, [s, f]: [f32; 2], [across, up, forward]: [f32; 3]) -> Model {
    let mut model = Model::default();
    model.add(
        place(frame, ([s, s + across], [0.0, up], [f, f + forward])),
        DecorationMaterial::EggShell,
    );
    model
}

/// A cube-shaped block fragment, `size` px on a side, lying in the frame's origin cell with its
/// corner at pixel `(s, f)`.
pub(super) fn chunk(
    frame: Frame,
    [s, f]: [f32; 2],
    size: f32,
    material: DecorationMaterial,
) -> Model {
    let mut model = Model::default();
    model.add(
        place(frame, ([s, s + size], [0.0, size], [f, f + size])),
        material,
    );
    model
}

/// Direction an arrow flew: along a horizontal axis, into the side of a block, or straight down,
/// into its top (an arrow that fell from a high shot). No arbitrary rotation: an arrow is always
/// axis-aligned, so its three boxes stay AABBs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArrowHeading {
    PositiveX,
    NegativeX,
    PositiveZ,
    NegativeZ,
    Down,
}

impl ArrowHeading {
    /// The horizontal heading opposite unit step `(dx, dz)`: an arrow shot from where that step
    /// points.
    pub const fn against((dx, dz): (i32, i32)) -> Self {
        match (dx, dz) {
            (1, 0) => Self::NegativeX,
            (-1, 0) => Self::PositiveX,
            (0, 1) => Self::NegativeZ,
            _ => Self::PositiveZ,
        }
    }

    /// The cube face of the struck block that the arrow enters.
    pub const fn entry_face(self) -> CubeFace {
        match self {
            Self::PositiveX => CubeFace::NegativeX,
            Self::NegativeX => CubeFace::PositiveX,
            Self::PositiveZ => CubeFace::NegativeZ,
            Self::NegativeZ => CubeFace::PositiveZ,
            Self::Down => CubeFace::PositiveY,
        }
    }

    /// Unit direction of flight.
    pub const fn direction(self) -> Vec3 {
        match self {
            Self::PositiveX => Vec3::new(1.0, 0.0, 0.0),
            Self::NegativeX => Vec3::new(-1.0, 0.0, 0.0),
            Self::PositiveZ => Vec3::new(0.0, 0.0, 1.0),
            Self::NegativeZ => Vec3::new(0.0, 0.0, -1.0),
            Self::Down => Vec3::new(0.0, -1.0, 0.0),
        }
    }
}

/// Arrow cross-sections and lengths along the flight axis, in pixels measured outward from the
/// struck face (negative is inside the block).
///
/// The flint head is 2.5 px square and reaches 3 px into the block and 1 px out of it, so the
/// arrow is visibly stuck. The 1.5 px shaft runs from the head to the 2.5 px fletching at the
/// tail, 13 px from the face.
pub(super) const HEAD_WIDTH: f32 = 2.5;
pub(super) const SHAFT_WIDTH: f32 = 1.5;
pub(super) const FLETCHING_WIDTH: f32 = 2.5;
pub(super) const HEAD_SPAN: [f32; 2] = [-3.0, 1.0];
pub(super) const SHAFT_SPAN: [f32; 2] = [1.0, 9.0];
pub(super) const FLETCHING_SPAN: [f32; 2] = [9.0, 13.0];

/// An arrow that flew along `heading` into `target`, centered at pixel `(a, b)` of the struck
/// face: `a` along world X for a `±Z` or top face and along Z for a `±X` face, `b` along Y on a
/// side face and along Z on the top face. Three boxes: head, shaft, fletching.
pub(super) fn arrow(target: VoxelPosition, heading: ArrowHeading, (a, b): (f32, f32)) -> Model {
    let corner = target.min_corner();
    let box_of = |[d0, d1]: [f32; 2], width: f32| {
        let along = [a - width / 2.0, a + width / 2.0].map(|v| v * PX);
        let across = [b - width / 2.0, b + width / 2.0].map(|v| v * PX);
        // Distances outward from the face, mapped onto the flight axis: the arrow points into the
        // block, so its tail lies on the face's outward side.
        let outward = |plane: f32, sign: f32| {
            let (p, q) = (plane + sign * d0 * PX, plane + sign * d1 * PX);
            [p.min(q), p.max(q)]
        };
        let (x, y, z) = match heading {
            ArrowHeading::PositiveX => (outward(corner.x, -1.0), across, along),
            ArrowHeading::NegativeX => (outward(corner.x + 1.0, 1.0), across, along),
            ArrowHeading::PositiveZ => (along, across, outward(corner.z, -1.0)),
            ArrowHeading::NegativeZ => (along, across, outward(corner.z + 1.0, 1.0)),
            ArrowHeading::Down => (along, outward(corner.y + 1.0, 1.0), across),
        };
        let offset = |[low, high]: [f32; 2], base: f32, axis_is_flight: bool| {
            if axis_is_flight {
                [low, high]
            } else {
                [base + low, base + high]
            }
        };
        let flight_x = matches!(heading, ArrowHeading::PositiveX | ArrowHeading::NegativeX);
        let flight_y = heading == ArrowHeading::Down;
        let flight_z = matches!(heading, ArrowHeading::PositiveZ | ArrowHeading::NegativeZ);
        let [x0, x1] = offset(x, corner.x, flight_x);
        let [y0, y1] = offset(y, corner.y, flight_y);
        let [z0, z1] = offset(z, corner.z, flight_z);
        Aabb::try_new(Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1))
            .expect("arrow boxes are finite and ordered")
    };

    let mut model = Model::default();
    model.add(box_of(HEAD_SPAN, HEAD_WIDTH), DecorationMaterial::ArrowHead);
    model.add(
        box_of(SHAFT_SPAN, SHAFT_WIDTH),
        DecorationMaterial::ArrowShaft,
    );
    model.add(
        box_of(FLETCHING_SPAN, FLETCHING_WIDTH),
        DecorationMaterial::ArrowFletching,
    );
    model
}
