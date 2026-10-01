//! 3D DDA traversal: the first occupied voxel surface along a ray.
//!
//! The ray is clipped once against the grid's overall bounds, then steps from cell to cell in the
//! Amanatides–Woo manner, reading block identity straight from contiguous storage. No voxel is
//! turned into an `Aabb`, nothing is allocated, and the cost is proportional to the cells actually
//! crossed, not to the grid volume or the number of occupied voxels.
//!
//! # Conventions
//!
//! - **Segment.** The searched segment starts at `t_start = max(t_min, grid entry)`. With
//!   `t_min = 0` that is the ray origin when it lies inside the grid.
//! - **Cell ownership.** Voxel `x` owns `[x, x + 1)`, applied in the direction of travel: at any
//!   `t` the ray is in the cell it occupies just after `t`. A ray at `x = 1.0` moving `+X` is in
//!   cell 1; moving `-X` it is in cell 0. A direction component treated as parallel (see
//!   `PARALLEL_DIRECTION_EPSILON`) never changes cell and uses plain floor ownership.
//! - **Hits.** If the start cell is occupied and the ray enters it exactly at `t_start` (it
//!   starts on that cell's entry plane), the hit is that entry face at `t_start`, possibly
//!   `t = 0`. If the start cell is occupied and `t_start` is strictly inside it, the hit is the
//!   cell's exit face, never a `t_start` hit; this is what refraction rays inside glass need. Any
//!   later hit is the face crossed to enter the first occupied cell. A surface the ray is leaving
//!   at `t_start` is never reported.
//! - **Ties.** When the ray crosses two or three cell planes at the same `t` (an edge or corner),
//!   every tied axis steps together, straight to the diagonal cell. Cells the ray only touches
//!   along an edge or at a corner are never visited, consistent with the ownership rule.
//! - **Face priority.** Among faces crossed at the same `t`, X wins over Y, which wins over Z.
//!   This is the axis order of `Aabb`'s slab test, so a hit reports the face an `Aabb` around the
//!   same voxel would report.
//!
//! Plane-crossing parameters are always computed as `(plane - origin) / direction`, the slab
//! test's own expression, rather than by accumulating `t_delta`. A voxel hit's `t` is therefore
//! bit-identical to the equivalent unit `Aabb`'s, and no rounding builds up along long rays.

use crate::{
    geometry::{CubeFace, aabb::PARALLEL_DIRECTION_EPSILON},
    ray::Ray,
};

use super::{BlockMaterials, BlockType, LocalVoxelPosition, Voxel, VoxelGrid, VoxelHit};

impl VoxelGrid {
    /// First occupied voxel surface the ray crosses within `[t_min, t_max]`.
    ///
    /// Follows the module-level conventions. The interval is inclusive at both ends and is
    /// validated like `Aabb::intersect`: a NaN bound, `t_min > t_max`, or a non-finite ray origin
    /// returns `None`, and a reported hit always satisfies `t_min <= t <= t_max`. `t_max` may be
    /// infinite. Space outside the grid is empty.
    pub fn intersect(
        &self,
        ray: Ray,
        materials: &BlockMaterials,
        t_min: f32,
        t_max: f32,
    ) -> Option<VoxelHit> {
        self.traverse(ray, materials, t_min, t_max, |_| {})
    }

    /// `intersect` that reports each cell it examines to `visit`, in traversal order.
    ///
    /// Production passes a no-op that compiles away; tests use it to check cell progression and
    /// count visited cells.
    fn traverse(
        &self,
        ray: Ray,
        materials: &BlockMaterials,
        t_min: f32,
        t_max: f32,
        mut visit: impl FnMut(LocalVoxelPosition),
    ) -> Option<VoxelHit> {
        if t_min.is_nan() || t_max.is_nan() || t_min > t_max || !ray.origin().is_finite() {
            return None;
        }

        let origin = ray.origin();
        let direction = ray.direction();
        let grid_origin = self.origin();
        let mut axes = [
            Axis::new(
                origin.x,
                direction.x,
                grid_origin.x,
                self.width(),
                1,
                CubeFace::NegativeX,
                CubeFace::PositiveX,
            ),
            Axis::new(
                origin.y,
                direction.y,
                grid_origin.y,
                self.height(),
                self.width() * self.depth(),
                CubeFace::NegativeY,
                CubeFace::PositiveY,
            ),
            Axis::new(
                origin.z,
                direction.z,
                grid_origin.z,
                self.depth(),
                self.width(),
                CubeFace::NegativeZ,
                CubeFace::PositiveZ,
            ),
        ];

        let mut t_enter = f32::NEG_INFINITY;
        let mut t_exit = f32::INFINITY;
        for axis in &axes {
            let (near, far) = axis.grid_span()?;
            t_enter = t_enter.max(near);
            t_exit = t_exit.min(far);
        }
        // An empty or point-sized overlap leaves no cell to be inside of after `t_start`.
        let t_start = t_min.max(t_enter);
        if !t_start.is_finite() || t_start >= t_exit || t_start > t_max {
            return None;
        }

        for axis in &mut axes {
            if !axis.start(t_start) {
                return None;
            }
        }
        let mut cell = local_cell(&axes);
        let mut index = self.linear_index(cell);
        let voxels = self.voxels();
        visit(cell);

        if let Voxel::Block(block) = voxels[index] {
            let (t, face) = start_cell_surface(&axes, t_start)?;
            return (t <= t_max)
                .then(|| self.surface_hit(ray, cell, face, t, block, materials))
                .flatten();
        }

        // Every iteration advances at least one axis, and an axis can advance at most
        // `extent - 1` times before the next step leaves the grid.
        let step_limit = self.width() + self.height() + self.depth();
        for _ in 0..step_limit {
            let t_cross = axes[0].t_next.min(axes[1].t_next).min(axes[2].t_next);
            if !t_cross.is_finite() || t_cross > t_max {
                return None;
            }

            let mut entry_face = None;
            for axis in &mut axes {
                if axis.t_next == t_cross {
                    if !axis.advance(&mut index) {
                        return None;
                    }
                    entry_face.get_or_insert(axis.entry_face);
                }
            }
            cell = local_cell(&axes);
            visit(cell);

            if let Voxel::Block(block) = voxels[index] {
                return self.surface_hit(ray, cell, entry_face?, t_cross, block, materials);
            }
        }

        debug_assert!(
            false,
            "DDA exceeded its step bound without leaving the grid"
        );
        None
    }

    /// Builds the hit on `face` of the occupied local cell `cell` at parameter `t`.
    ///
    /// `ray.at(t)` misses the face plane by rounding error, so only the coordinate along the face
    /// normal is snapped onto the exact integer plane; the in-face coordinates stay ray-derived.
    fn surface_hit(
        &self,
        ray: Ray,
        cell: LocalVoxelPosition,
        face: CubeFace,
        t: f32,
        block: BlockType,
        materials: &BlockMaterials,
    ) -> Option<VoxelHit> {
        let voxel = self.local_to_world(cell)?;
        let min = voxel.min_corner();
        let mut position = ray.at(t);
        match face {
            CubeFace::NegativeX => position.x = min.x,
            CubeFace::PositiveX => position.x = min.x + 1.0,
            CubeFace::NegativeY => position.y = min.y,
            CubeFace::PositiveY => position.y = min.y + 1.0,
            CubeFace::NegativeZ => position.z = min.z,
            CubeFace::PositiveZ => position.z = min.z + 1.0,
        }

        let hit = VoxelHit::try_new(voxel, face, position, t, block, materials);
        debug_assert!(
            hit.is_some(),
            "DDA hit off its voxel face: {voxel:?} {face:?} t={t}"
        );
        hit
    }
}

/// The surface reported when the cell at `t_start` is occupied.
///
/// The cell's entry and exit parameters are its slab-test interval: entry is the latest
/// entry-plane crossing and exit the earliest exit-plane crossing, each won by the first axis in
/// X, Y, Z order on a tie. The ray is in this cell just after `t_start`, so entry is at most
/// `t_start` and exit strictly greater.
fn start_cell_surface(axes: &[Axis; 3], t_start: f32) -> Option<(f32, CubeFace)> {
    let mut entry: Option<(f32, CubeFace)> = None;
    let mut exit: Option<(f32, CubeFace)> = None;
    for axis in axes.iter().filter(|axis| axis.step != Step::Parallel) {
        let entry_t = axis.plane_t(axis.back_plane(axis.cell));
        if entry.is_none_or(|(t, _)| entry_t > t) {
            entry = Some((entry_t, axis.entry_face));
        }
        if exit.is_none_or(|(t, _)| axis.t_next < t) {
            exit = Some((axis.t_next, axis.exit_face));
        }
    }

    // Starting exactly on the entry plane is entering the cell; anything later is inside it.
    match entry {
        Some((entry_t, face)) if entry_t >= t_start => Some((t_start, face)),
        _ => exit,
    }
}

fn local_cell(axes: &[Axis; 3]) -> LocalVoxelPosition {
    LocalVoxelPosition::new(axes[0].cell, axes[1].cell, axes[2].cell)
}

/// Direction of travel along one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Negative,
    Parallel,
    Positive,
}

/// DDA state along one axis: the current cell and the parameter of its next boundary crossing.
///
/// `t_next` plays the role of Amanatides–Woo `tMax`; the per-cell `tDelta = 1 / |direction|` is
/// implicit because `t_next` is recomputed from the next integer plane on every step.
#[derive(Clone, Copy, Debug)]
struct Axis {
    origin: f32,
    direction: f32,
    /// World coordinate of local cell 0 along this axis.
    grid_origin: i64,
    extent: usize,
    /// Storage-index distance between neighboring cells along this axis.
    stride: usize,
    step: Step,
    /// Current local cell index along this axis.
    cell: usize,
    /// Parameter at which the ray leaves the current cell along this axis; infinite if parallel.
    t_next: f32,
    /// Face through which the ray enters a cell along this axis.
    entry_face: CubeFace,
    /// Face through which the ray leaves a cell along this axis.
    exit_face: CubeFace,
}

impl Axis {
    fn new(
        origin: f32,
        direction: f32,
        grid_origin: i32,
        extent: usize,
        stride: usize,
        negative_face: CubeFace,
        positive_face: CubeFace,
    ) -> Self {
        let step = if direction.abs() <= PARALLEL_DIRECTION_EPSILON {
            Step::Parallel
        } else if direction > 0.0 {
            Step::Positive
        } else {
            Step::Negative
        };
        let (entry_face, exit_face) = match step {
            Step::Negative => (positive_face, negative_face),
            Step::Parallel | Step::Positive => (negative_face, positive_face),
        };

        Self {
            origin,
            direction,
            grid_origin: i64::from(grid_origin),
            extent,
            stride,
            step,
            cell: 0,
            t_next: f32::INFINITY,
            entry_face,
            exit_face,
        }
    }

    /// Ray parameter at the world plane `plane` along this axis; only for a non-parallel axis.
    ///
    /// Integer plane coordinates are exact in `f32` within `±2^24`, the same range assumption as
    /// `VoxelPosition::min_corner`.
    fn plane_t(&self, plane: i64) -> f32 {
        (plane as f32 - self.origin) / self.direction
    }

    /// World coordinate of the plane through which the ray enters local cell `cell`.
    fn back_plane(&self, cell: usize) -> i64 {
        let min = self.grid_origin + cell as i64;
        match self.step {
            Step::Negative => min + 1,
            Step::Parallel | Step::Positive => min,
        }
    }

    /// World coordinate of the plane through which the ray leaves local cell `cell`.
    fn front_plane(&self, cell: usize) -> i64 {
        let min = self.grid_origin + cell as i64;
        match self.step {
            Step::Negative => min,
            Step::Parallel | Step::Positive => min + 1,
        }
    }

    /// Parameter interval during which the ray lies in the grid's slab along this axis, or `None`
    /// when a parallel ray lies outside the half-open slab.
    fn grid_span(&self) -> Option<(f32, f32)> {
        let last = self.extent - 1;
        match self.step {
            Step::Parallel => {
                let min = self.grid_origin as f32;
                let max = (self.grid_origin + self.extent as i64) as f32;
                (min <= self.origin && self.origin < max)
                    .then_some((f32::NEG_INFINITY, f32::INFINITY))
            }
            Step::Positive => Some((
                self.plane_t(self.back_plane(0)),
                self.plane_t(self.front_plane(last)),
            )),
            Step::Negative => Some((
                self.plane_t(self.back_plane(last)),
                self.plane_t(self.front_plane(0)),
            )),
        }
    }

    /// Places this axis on the cell the ray occupies just after `t_start`.
    ///
    /// The point `ray.at(t_start)` is rounded and may land one cell early or late near a plane,
    /// so its floor is only a guess, settled in parameter space with the same plane parameters
    /// stepping uses: afterwards the cell's entry plane is at or before `t_start` and its exit
    /// plane strictly after. Returns `false` if the ray has already left the grid along this
    /// axis, which the grid-span check rules out.
    fn start(&mut self, t_start: f32) -> bool {
        let coordinate = match self.step {
            Step::Parallel => self.origin,
            Step::Negative | Step::Positive => self.origin + self.direction * t_start,
        };
        let last = self.extent as i64 - 1;
        let guess = (coordinate.floor() as i64 - self.grid_origin).clamp(0, last);
        let mut cell = usize::try_from(guess).unwrap_or(0);

        if self.step != Step::Parallel {
            while self.plane_t(self.front_plane(cell)) <= t_start {
                let Some(next) = self.forward(cell) else {
                    return false;
                };
                cell = next;
            }
            while self.plane_t(self.back_plane(cell)) > t_start {
                let Some(previous) = self.backward(cell) else {
                    break;
                };
                cell = previous;
            }
            self.t_next = self.plane_t(self.front_plane(cell));
        }

        self.cell = cell;
        true
    }

    /// Moves into the next cell along the direction of travel, keeping the storage index in
    /// step; `false` when that cell would be outside the grid.
    fn advance(&mut self, index: &mut usize) -> bool {
        let Some(next) = self.forward(self.cell) else {
            return false;
        };
        match self.step {
            Step::Positive => *index += self.stride,
            Step::Negative => *index -= self.stride,
            Step::Parallel => return false,
        }
        self.cell = next;
        self.t_next = self.plane_t(self.front_plane(next));
        true
    }

    fn forward(&self, cell: usize) -> Option<usize> {
        match self.step {
            Step::Positive => (cell + 1 < self.extent).then_some(cell + 1),
            Step::Negative => cell.checked_sub(1),
            Step::Parallel => None,
        }
    }

    fn backward(&self, cell: usize) -> Option<usize> {
        match self.step {
            Step::Positive => cell.checked_sub(1),
            Step::Negative => (cell + 1 < self.extent).then_some(cell + 1),
            Step::Parallel => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::PI;

    use crate::{
        geometry::{Aabb, CubeFace, SurfaceHit},
        material::MaterialId,
        math::Vec3,
        ray::Ray,
        voxel::{BlockMaterials, BlockType, Voxel, VoxelGrid, VoxelHit, VoxelPosition},
    };

    const EPSILON: f32 = 1.0e-5;
    const INF: f32 = f32::INFINITY;
    /// The renderer's secondary-ray origin offset, mirrored to test realistically biased origins.
    const ORIGIN_BIAS: f32 = 1.0e-4;

    fn materials() -> BlockMaterials {
        BlockMaterials {
            grass: MaterialId::new(30),
            dirt: MaterialId::new(31),
            cobblestone: MaterialId::new(32),
            obsidian: MaterialId::new(33),
            glass: MaterialId::new(34),
            lava: MaterialId::new(35),
            coal_ore: MaterialId::new(36),
            iron_ore: MaterialId::new(37),
            gold_ore: MaterialId::new(38),
            diamond_ore: MaterialId::new(39),
        }
    }

    fn world(x: i32, y: i32, z: i32) -> VoxelPosition {
        VoxelPosition::new(x, y, z)
    }

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray::try_new(origin, direction).unwrap()
    }

    fn empty_grid(origin: VoxelPosition, width: usize, height: usize, depth: usize) -> VoxelGrid {
        VoxelGrid::try_new(origin, width, height, depth).unwrap()
    }

    /// A grid with a cobblestone block at each listed world voxel.
    fn grid_with(
        origin: VoxelPosition,
        (width, height, depth): (usize, usize, usize),
        blocks: &[VoxelPosition],
    ) -> VoxelGrid {
        let mut grid = empty_grid(origin, width, height, depth);
        for &position in blocks {
            grid.set_world(position, Voxel::Block(BlockType::Cobblestone))
                .unwrap();
        }
        grid
    }

    fn intersect(grid: &VoxelGrid, ray: Ray, t_min: f32, t_max: f32) -> Option<VoxelHit> {
        grid.intersect(ray, &materials(), t_min, t_max)
    }

    fn first_hit(grid: &VoxelGrid, ray: Ray) -> Option<VoxelHit> {
        intersect(grid, ray, 0.0, INF)
    }

    /// World voxels the traversal examines, in order.
    fn visited(grid: &VoxelGrid, ray: Ray, t_min: f32, t_max: f32) -> Vec<VoxelPosition> {
        let mut cells = Vec::new();
        grid.traverse(ray, &materials(), t_min, t_max, |cell| {
            cells.push(grid.local_to_world(cell).unwrap());
        });
        cells
    }

    fn visited_from_zero(grid: &VoxelGrid, ray: Ray) -> Vec<VoxelPosition> {
        visited(grid, ray, 0.0, INF)
    }

    /// Component of `vector` along the axis of `face`.
    fn along(face: CubeFace, vector: Vec3) -> f32 {
        match face {
            CubeFace::NegativeX | CubeFace::PositiveX => vector.x,
            CubeFace::NegativeY | CubeFace::PositiveY => vector.y,
            CubeFace::NegativeZ | CubeFace::PositiveZ => vector.z,
        }
    }

    fn face_plane(voxel: VoxelPosition, face: CubeFace) -> f32 {
        let min = along(face, voxel.min_corner());
        match face {
            CubeFace::PositiveX | CubeFace::PositiveY | CubeFace::PositiveZ => min + 1.0,
            CubeFace::NegativeX | CubeFace::NegativeY | CubeFace::NegativeZ => min,
        }
    }

    /// Checks identity, face, normal, and `t`, and that the position lies exactly on the face
    /// plane with ray-derived in-face coordinates.
    #[track_caller]
    fn assert_hit(
        hit: Option<VoxelHit>,
        ray: Ray,
        voxel: VoxelPosition,
        face: CubeFace,
        t: f32,
    ) -> VoxelHit {
        let hit = hit.unwrap_or_else(|| panic!("expected a hit on {voxel:?} {face:?}"));
        assert_eq!(hit.voxel, voxel);
        assert_eq!(hit.surface.face, face);
        assert_eq!(hit.surface.normal, face.normal());
        assert!(
            (hit.surface.t - t).abs() <= EPSILON,
            "t {} != {t}",
            hit.surface.t
        );

        let from_ray = ray.at(hit.surface.t);
        let position = hit.surface.position;
        assert_eq!(along(face, position), face_plane(voxel, face));
        for (component, actual, expected) in [
            (face.normal().x, position.x, from_ray.x),
            (face.normal().y, position.y, from_ray.y),
            (face.normal().z, position.z, from_ray.z),
        ] {
            if component == 0.0 {
                assert_eq!(
                    actual, expected,
                    "in-face coordinates must come from the ray"
                );
            }
        }
        hit
    }

    fn unit_aabb(voxel: VoxelPosition) -> Aabb {
        let min = voxel.min_corner();
        Aabb::try_new(min, min + v(1.0, 1.0, 1.0)).unwrap()
    }

    // ---------------------------------------------------------------- basic traversal

    #[test]
    fn empty_grid_never_reports_a_hit() {
        let grid = empty_grid(world(-2, -2, -2), 4, 4, 4);
        let rays = [
            ray(v(-5.0, 0.3, 0.4), v(1.0, 0.0, 0.0)),
            ray(v(0.3, 5.0, -0.4), v(0.0, -1.0, 0.0)),
            ray(v(-4.0, -3.0, -5.0), v(1.0, 0.9, 1.3)),
            ray(v(0.1, 0.2, 0.3), v(-0.4, 0.7, -0.2)),
            ray(v(1.5, 1.5, 1.5), v(-1.0, -1.0, -1.0)),
        ];

        for ray in rays {
            assert_eq!(first_hit(&grid, ray), None, "{ray:?}");
        }
    }

    #[test]
    fn rays_missing_the_grid_return_none_without_visiting_cells() {
        let mut grid = empty_grid(world(0, 0, 0), 4, 4, 4);
        grid.fill(Voxel::Block(BlockType::Obsidian));
        let rays = [
            // Passing beside, above, and pointing away from a completely solid grid.
            ray(v(-1.0, 0.5, 0.5), v(0.0, 0.0, 1.0)),
            ray(v(-2.0, 5.0, 2.0), v(1.0, 0.0, 0.0)),
            ray(v(-1.0, 2.0, 2.0), v(-1.0, 0.0, 0.0)),
            ray(v(6.0, 6.0, 6.0), v(1.0, 1.0, 1.0)),
            ray(v(-3.0, -1.0, 2.0), v(1.0, 0.1, 0.0)),
        ];

        for ray in rays {
            assert_eq!(first_hit(&grid, ray), None, "{ray:?}");
            assert!(visited_from_zero(&grid, ray).is_empty(), "{ray:?}");
        }
    }

    #[test]
    fn ray_crossing_an_empty_grid_visits_each_crossed_cell_once_and_misses() {
        let grid = empty_grid(world(0, 0, 0), 8, 4, 6);
        let along_x = ray(v(-3.0, 1.5, 2.5), v(1.0, 0.0, 0.0));

        assert_eq!(first_hit(&grid, along_x), None);
        assert_eq!(
            visited_from_zero(&grid, along_x),
            (0..8).map(|x| world(x, 1, 2)).collect::<Vec<_>>()
        );

        // An oblique crossing steps one face-adjacent cell at a time.
        let oblique = ray(v(-1.0, 0.3, 0.2), v(1.0, 0.37, 0.61));
        let cells = visited_from_zero(&grid, oblique);
        assert_eq!(first_hit(&grid, oblique), None);
        assert!(cells.len() <= 8 + 4 + 6);
        for pair in cells.windows(2) {
            let delta = (pair[1].x - pair[0].x) + (pair[1].y - pair[0].y) + (pair[1].z - pair[0].z);
            assert_eq!(delta, 1, "{pair:?}");
        }
    }

    #[test]
    fn hits_a_single_voxel_through_the_face_opposing_each_travel_direction() {
        let voxel = world(1, 2, 3);
        let grid = grid_with(world(0, 0, 0), (4, 5, 6), &[voxel]);
        let center = v(1.5, 2.5, 3.5);
        let skew = v(0.11, -0.23, 0.17);

        for (direction, face) in [
            (v(1.0, 0.0, 0.0), CubeFace::NegativeX),
            (v(-1.0, 0.0, 0.0), CubeFace::PositiveX),
            (v(0.0, 1.0, 0.0), CubeFace::NegativeY),
            (v(0.0, -1.0, 0.0), CubeFace::PositiveY),
            (v(0.0, 0.0, 1.0), CubeFace::NegativeZ),
            (v(0.0, 0.0, -1.0), CubeFace::PositiveZ),
        ] {
            let in_face_skew = skew - direction * skew.dot(direction);
            let ray = ray(center + in_face_skew - direction * 5.0, direction);
            let hit = assert_hit(first_hit(&grid, ray), ray, voxel, face, 4.5);

            assert_eq!(hit.block, BlockType::Cobblestone);
            assert_eq!(hit.material_id, materials().cobblestone);
        }
    }

    #[test]
    fn nearest_of_several_occupied_voxels_wins() {
        let blocks = [
            world(1, 0, 0),
            world(3, 0, 0),
            world(5, 0, 0),
            world(2, 1, 0),
            world(0, 0, 1),
        ];
        let grid = grid_with(world(-1, 0, 0), (8, 2, 2), &blocks);

        let forward = ray(v(-2.0, 0.5, 0.5), v(1.0, 0.0, 0.0));
        assert_hit(
            first_hit(&grid, forward),
            forward,
            world(1, 0, 0),
            CubeFace::NegativeX,
            3.0,
        );
        let backward = ray(v(8.0, 0.5, 0.5), v(-1.0, 0.0, 0.0));
        assert_hit(
            first_hit(&grid, backward),
            backward,
            world(5, 0, 0),
            CubeFace::PositiveX,
            2.0,
        );
        // An oblique ray rising over (1, 0, 0) meets (2, 1, 0) before (3, 0, 0).
        let oblique = ray(v(0.5, 0.9, 0.5), v(1.0, 0.3, 0.0));
        let hit = first_hit(&grid, oblique).unwrap();
        assert_eq!(hit.voxel, world(2, 1, 0));
        let expected = unit_aabb(world(2, 1, 0))
            .intersect(oblique, 0.0, INF)
            .unwrap();
        assert_eq!(hit.surface.t, expected.t);
        assert_eq!(hit.surface.face, expected.face);
    }

    // ---------------------------------------------------------------- starting inside

    #[test]
    fn ray_starting_in_an_empty_cell_steps_to_the_next_occupied_cell() {
        let grid = grid_with(world(0, 0, 0), (5, 1, 1), &[world(3, 0, 0)]);
        let ray = ray(v(0.5, 0.5, 0.5), v(1.0, 0.0, 0.0));

        assert_hit(
            first_hit(&grid, ray),
            ray,
            world(3, 0, 0),
            CubeFace::NegativeX,
            2.5,
        );
        assert_eq!(
            visited_from_zero(&grid, ray),
            [
                world(0, 0, 0),
                world(1, 0, 0),
                world(2, 0, 0),
                world(3, 0, 0)
            ]
        );
    }

    #[test]
    fn ray_strictly_inside_a_solid_voxel_exits_through_its_travel_face() {
        let lone = grid_with(world(-1, -1, -1), (3, 3, 3), &[world(0, 0, 0)]);
        let mut solid = empty_grid(world(-1, -1, -1), 3, 3, 3);
        solid.fill(Voxel::Block(BlockType::Obsidian));
        let origin = v(0.25, 0.5, 0.75);

        for (direction, face, t) in [
            (v(1.0, 0.0, 0.0), CubeFace::PositiveX, 0.75),
            (v(-1.0, 0.0, 0.0), CubeFace::NegativeX, 0.25),
            (v(0.0, 1.0, 0.0), CubeFace::PositiveY, 0.5),
            (v(0.0, -1.0, 0.0), CubeFace::NegativeY, 0.5),
            (v(0.0, 0.0, 1.0), CubeFace::PositiveZ, 0.25),
            (v(0.0, 0.0, -1.0), CubeFace::NegativeZ, 0.75),
        ] {
            let ray = ray(origin, direction);
            // Solid neighbors do not turn the exit into a neighbor's entry.
            for grid in [&lone, &solid] {
                let hit = assert_hit(first_hit(grid, ray), ray, world(0, 0, 0), face, t);
                assert!(hit.surface.t > 0.0);
                assert!(hit.surface.normal.dot(ray.direction()) > 0.0);
            }
        }
    }

    #[test]
    fn diagonal_ray_inside_a_solid_voxel_exits_through_the_first_face_reached() {
        let grid = grid_with(world(0, 0, 0), (2, 2, 2), &[world(0, 0, 0)]);
        let origin = v(0.8, 0.4, 0.3);

        // Distances to the exit planes along (1, 1, 1): x 0.2, y 0.6, z 0.7.
        let rising = ray(origin, v(1.0, 1.0, 1.0));
        assert_hit(
            first_hit(&grid, rising),
            rising,
            world(0, 0, 0),
            CubeFace::PositiveX,
            0.2 * 3.0_f32.sqrt(),
        );
        // Along (-1, 2, 0.5): x 0.8, y 0.3, z 1.4 parameter units before scaling.
        let steep = ray(origin, v(-1.0, 2.0, 0.5));
        assert_hit(
            first_hit(&grid, steep),
            steep,
            world(0, 0, 0),
            CubeFace::PositiveY,
            0.3 * 5.25_f32.sqrt(),
        );
    }

    #[test]
    fn t_min_beyond_the_exit_of_a_starting_solid_voxel_excludes_it() {
        let lone = grid_with(world(0, 0, 0), (4, 1, 1), &[world(0, 0, 0)]);
        let pair = grid_with(world(0, 0, 0), (4, 1, 1), &[world(0, 0, 0), world(1, 0, 0)]);
        let ray = ray(v(0.5, 0.5, 0.5), v(1.0, 0.0, 0.0));

        assert_hit(
            intersect(&lone, ray, 0.0, INF),
            ray,
            world(0, 0, 0),
            CubeFace::PositiveX,
            0.5,
        );
        assert_eq!(intersect(&lone, ray, 0.6, INF), None);
        // `t_min` inside the neighbor: like an `Aabb` whose entry is clipped, report its exit.
        assert_hit(
            intersect(&pair, ray, 0.6, INF),
            ray,
            world(1, 0, 0),
            CubeFace::PositiveX,
            1.5,
        );
        assert_eq!(
            unit_aabb(world(1, 0, 0))
                .intersect(ray, 0.6, INF)
                .unwrap()
                .t,
            1.5
        );
        // A finite `t_max` before the exit also excludes it.
        assert_eq!(intersect(&lone, ray, 0.0, 0.4), None);
    }

    // ---------------------------------------------------------------- exact boundaries

    #[test]
    fn origin_on_an_interior_plane_starts_in_the_cell_it_travels_into() {
        let grid = empty_grid(world(-2, 0, 0), 4, 1, 1);
        let at = |x: f32, direction: f32| ray(v(x, 0.5, 0.5), v(direction, 0.0, 0.0));

        assert_eq!(visited_from_zero(&grid, at(1.0, 1.0))[0], world(1, 0, 0));
        assert_eq!(visited_from_zero(&grid, at(1.0, -1.0))[0], world(0, 0, 0));
        assert_eq!(visited_from_zero(&grid, at(0.0, 1.0))[0], world(0, 0, 0));
        assert_eq!(visited_from_zero(&grid, at(0.0, -1.0))[0], world(-1, 0, 0));
        assert_eq!(visited_from_zero(&grid, at(-1.0, 1.0))[0], world(-1, 0, 0));
        assert_eq!(visited_from_zero(&grid, at(-1.0, -1.0))[0], world(-2, 0, 0));
    }

    #[test]
    fn origin_on_a_plane_never_hits_the_cell_it_is_leaving() {
        let behind_positive = grid_with(world(-2, 0, 0), (4, 1, 1), &[world(0, 0, 0)]);
        let behind_negative = grid_with(world(-2, 0, 0), (4, 1, 1), &[world(1, 0, 0)]);
        let negative_cells = grid_with(world(-2, 0, 0), (4, 1, 1), &[world(-1, 0, 0)]);

        // floor(1.0) == 1 must not matter: cell 0 is behind a +X ray at x = 1.
        let positive = ray(v(1.0, 0.5, 0.5), v(1.0, 0.0, 0.0));
        assert_eq!(first_hit(&behind_positive, positive), None);
        let negative = ray(v(1.0, 0.5, 0.5), v(-1.0, 0.0, 0.0));
        assert_eq!(first_hit(&behind_negative, negative), None);
        let at_minus_one = ray(v(-1.0, 0.5, 0.5), v(-1.0, 0.0, 0.0));
        assert_eq!(first_hit(&negative_cells, at_minus_one), None);
        // The `Aabb` closed-box test does report a t = 0 surface here; the DDA deliberately not.
        assert_eq!(
            unit_aabb(world(0, 0, 0))
                .intersect(positive, 0.0, INF)
                .map(|hit| hit.t),
            Some(0.0)
        );
    }

    #[test]
    fn origin_on_the_entry_face_of_an_occupied_cell_hits_it_at_zero() {
        let grid = grid_with(
            world(-2, 0, 0),
            (4, 1, 1),
            &[world(1, 0, 0), world(-2, 0, 0)],
        );

        // Deliberate t = 0: the ray starts on the face it crosses into a solid cell, matching
        // the slab test. An origin strictly inside would report the exit instead.
        let entering = ray(v(1.0, 0.5, 0.5), v(1.0, 0.0, 0.0));
        assert_hit(
            first_hit(&grid, entering),
            entering,
            world(1, 0, 0),
            CubeFace::NegativeX,
            0.0,
        );
        let entering_negative = ray(v(-1.0, 0.25, 0.75), v(-1.0, 0.0, 0.0));
        assert_hit(
            first_hit(&grid, entering_negative),
            entering_negative,
            world(-2, 0, 0),
            CubeFace::PositiveX,
            0.0,
        );
        // Requiring t > 0 skips the zero-distance entry and reports the cell's exit.
        assert_hit(
            intersect(&grid, entering, 1.0e-3, INF),
            entering,
            world(1, 0, 0),
            CubeFace::PositiveX,
            1.0,
        );
    }

    #[test]
    fn origin_on_an_edge_starts_in_the_cell_its_direction_enters() {
        let grid = empty_grid(world(0, 0, 0), 2, 2, 1);
        let origin = v(1.0, 1.0, 0.5);

        for (dx, dy, expected) in [
            (1.0, 1.0, world(1, 1, 0)),
            (-1.0, -1.0, world(0, 0, 0)),
            (1.0, -1.0, world(1, 0, 0)),
            (-1.0, 1.0, world(0, 1, 0)),
            (1.0, 0.0, world(1, 1, 0)),
            (0.0, -1.0, world(1, 0, 0)),
        ] {
            let ray = ray(origin, v(dx, dy, 0.0));
            assert_eq!(visited_from_zero(&grid, ray), [expected], "{dx} {dy}");
        }

        // Entering an occupied cell through its edge reports the X face first.
        let solid = grid_with(world(0, 0, 0), (2, 2, 1), &[world(1, 1, 0)]);
        let ray = ray(origin, v(1.0, 1.0, 0.0));
        assert_hit(
            first_hit(&solid, ray),
            ray,
            world(1, 1, 0),
            CubeFace::NegativeX,
            0.0,
        );
    }

    #[test]
    fn origin_on_a_corner_starts_in_the_cell_its_direction_enters() {
        let grid = empty_grid(world(0, 0, 0), 2, 2, 2);
        let origin = v(1.0, 1.0, 1.0);

        for sx in [-1.0_f32, 1.0] {
            for sy in [-1.0_f32, 1.0] {
                for sz in [-1.0_f32, 1.0] {
                    let ray = ray(origin, v(sx, sy, sz));
                    let cell = |s: f32| i32::from(s > 0.0);
                    assert_eq!(
                        visited_from_zero(&grid, ray),
                        [world(cell(sx), cell(sy), cell(sz))],
                        "{sx} {sy} {sz}"
                    );
                    assert_eq!(first_hit(&grid, ray), None);
                }
            }
        }
    }

    #[test]
    fn origin_on_the_outer_grid_boundary_enters_or_leaves_by_direction() {
        let empty = empty_grid(world(0, 0, 0), 3, 3, 3);
        let grid = grid_with(world(0, 0, 0), (3, 3, 3), &[world(0, 1, 1), world(2, 1, 1)]);
        let low = v(0.0, 1.5, 1.5);
        let high = v(3.0, 1.5, 1.5);
        let plus = v(1.0, 0.0, 0.0);
        let minus = v(-1.0, 0.0, 0.0);

        // Entering: the first cell inside, never the cell just outside.
        assert_eq!(visited_from_zero(&empty, ray(low, plus))[0], world(0, 1, 1));
        assert_eq!(
            visited_from_zero(&empty, ray(high, minus))[0],
            world(2, 1, 1)
        );
        assert_hit(
            first_hit(&grid, ray(low, plus)),
            ray(low, plus),
            world(0, 1, 1),
            CubeFace::NegativeX,
            0.0,
        );
        assert_hit(
            first_hit(&grid, ray(high, minus)),
            ray(high, minus),
            world(2, 1, 1),
            CubeFace::PositiveX,
            0.0,
        );

        // Leaving: nothing is visited and the boundary voxel is not reported.
        for leaving in [ray(high, plus), ray(low, minus)] {
            assert_eq!(first_hit(&grid, leaving), None);
            assert!(visited_from_zero(&grid, leaving).is_empty());
        }
        // Leaving through an outer corner.
        let corner = ray(v(3.0, 3.0, 3.0), v(1.0, 1.0, 1.0));
        assert!(visited_from_zero(&grid, corner).is_empty());
    }

    #[test]
    fn ray_entering_from_outside_reports_the_grid_entry_face() {
        let grid = grid_with(
            world(-1, -1, -1),
            (3, 3, 3),
            &[world(-1, 0, 0), world(0, 1, 0), world(0, 0, -1)],
        );

        let from_west = ray(v(-4.0, 0.5, 0.5), v(1.0, 0.0, 0.0));
        assert_hit(
            first_hit(&grid, from_west),
            from_west,
            world(-1, 0, 0),
            CubeFace::NegativeX,
            3.0,
        );
        let from_above = ray(v(0.5, 6.0, 0.5), v(0.0, -1.0, 0.0));
        assert_hit(
            first_hit(&grid, from_above),
            from_above,
            world(0, 1, 0),
            CubeFace::PositiveY,
            4.0,
        );
        let from_north = ray(v(0.25, 0.75, -3.0), v(0.0, 0.0, 1.0));
        assert_hit(
            first_hit(&grid, from_north),
            from_north,
            world(0, 0, -1),
            CubeFace::NegativeZ,
            2.0,
        );
    }

    // ---------------------------------------------------------------- zero components

    #[test]
    fn rays_with_one_zero_component_step_only_the_other_two_axes() {
        let grid_x = grid_with(world(0, 0, 0), (3, 3, 3), &[world(1, 2, 2)]);
        let no_x = ray(v(1.5, 0.25, 0.25), v(0.0, 0.6, 0.8));
        assert_eq!(
            visited_from_zero(&grid_x, no_x),
            [
                world(1, 0, 0),
                world(1, 0, 1),
                world(1, 1, 1),
                world(1, 1, 2),
                world(1, 2, 2)
            ]
        );
        assert_hit(
            first_hit(&grid_x, no_x),
            no_x,
            world(1, 2, 2),
            CubeFace::NegativeY,
            1.75 / 0.6,
        );

        let grid_y = grid_with(world(0, 0, 0), (3, 3, 3), &[world(2, 1, 0)]);
        let no_y = ray(v(0.25, 1.5, 2.75), v(0.8, 0.0, -0.6));
        assert_eq!(
            visited_from_zero(&grid_y, no_y),
            [
                world(0, 1, 2),
                world(1, 1, 2),
                world(1, 1, 1),
                world(2, 1, 1),
                world(2, 1, 0)
            ]
        );
        assert_hit(
            first_hit(&grid_y, no_y),
            no_y,
            world(2, 1, 0),
            CubeFace::PositiveZ,
            1.75 / 0.6,
        );

        let grid_z = grid_with(world(0, 0, 0), (3, 3, 3), &[world(0, 2, 1)]);
        let no_z = ray(v(2.75, 0.25, 1.5), v(-0.6, 0.8, 0.0));
        assert_eq!(
            visited_from_zero(&grid_z, no_z),
            [
                world(2, 0, 1),
                world(2, 1, 1),
                world(1, 1, 1),
                world(1, 2, 1),
                world(0, 2, 1)
            ]
        );
        assert_hit(
            first_hit(&grid_z, no_z),
            no_z,
            world(0, 2, 1),
            CubeFace::PositiveX,
            1.75 / 0.6,
        );
    }

    #[test]
    fn rays_with_two_zero_components_walk_a_single_row() {
        let grid = grid_with(world(0, 0, 0), (3, 3, 3), &[world(1, 1, 0)]);
        let ray = ray(v(1.5, 1.5, 5.0), v(0.0, 0.0, -1.0));

        assert_eq!(
            visited_from_zero(&grid, ray),
            [world(1, 1, 2), world(1, 1, 1), world(1, 1, 0)]
        );
        assert_hit(
            first_hit(&grid, ray),
            ray,
            world(1, 1, 0),
            CubeFace::PositiveZ,
            4.0,
        );
    }

    #[test]
    fn parallel_axes_use_half_open_ownership_and_never_step() {
        let grid = grid_with(world(0, 0, 0), (3, 3, 3), &[world(1, 0, 1)]);
        let along = |y: f32| ray(v(-1.0, y, 1.5), v(1.0, 0.0, 0.0));

        // On the grid's lower plane the ray is inside; on its upper plane it is outside.
        assert_hit(
            first_hit(&grid, along(0.0)),
            along(0.0),
            world(1, 0, 1),
            CubeFace::NegativeX,
            2.0,
        );
        assert_eq!(first_hit(&grid, along(3.0)), None);
        assert!(visited_from_zero(&grid, along(3.0)).is_empty());
        // On the interior plane y = 1 the ray belongs to row 1, so it slides past the block.
        assert_eq!(first_hit(&grid, along(1.0)), None);
        assert!(
            visited_from_zero(&grid, along(1.0))
                .iter()
                .all(|cell| cell.y == 1)
        );
    }

    #[test]
    fn tiny_direction_components_are_parallel_or_finite_never_nan() {
        let grid = grid_with(world(0, 0, 0), (2, 2, 3), &[world(0, 0, 2)]);

        // Below the shared slab epsilon the X component is treated as exactly parallel.
        let parallel = ray(v(0.5, 0.5, 0.5), v(1.0e-9, 0.0, 1.0));
        assert_eq!(
            visited_from_zero(&grid, parallel),
            [world(0, 0, 0), world(0, 0, 1), world(0, 0, 2)]
        );
        // Just above it the crossing parameter is huge but finite, so the ray still never
        // leaves column x = 0 inside this grid.
        let nearly_parallel = ray(v(0.5, 0.5, 0.5), v(1.0e-6, 0.0, 1.0));
        let hit = assert_hit(
            first_hit(&grid, nearly_parallel),
            nearly_parallel,
            world(0, 0, 2),
            CubeFace::NegativeZ,
            1.5,
        );
        assert!(hit.surface.position.is_finite());
    }

    // ---------------------------------------------------------------- ties

    #[test]
    fn xy_edge_tie_steps_diagonally_and_reports_the_x_face() {
        let empty = empty_grid(world(0, 0, 0), 4, 4, 1);
        let ray = ray(v(0.5, 0.5, 0.5), v(1.0, 1.0, 0.0));

        assert_eq!(
            visited_from_zero(&empty, ray),
            [
                world(0, 0, 0),
                world(1, 1, 0),
                world(2, 2, 0),
                world(3, 3, 0)
            ]
        );
        let grid = grid_with(world(0, 0, 0), (4, 4, 1), &[world(2, 2, 0)]);
        assert_hit(
            first_hit(&grid, ray),
            ray,
            world(2, 2, 0),
            CubeFace::NegativeX,
            1.5 * 2.0_f32.sqrt(),
        );
    }

    #[test]
    fn xz_and_yz_edge_ties_follow_the_axis_priority() {
        let xz = grid_with(world(0, 0, 0), (4, 1, 4), &[world(2, 0, 2)]);
        let xz_ray = ray(v(0.5, 0.5, 0.5), v(1.0, 0.0, 1.0));
        assert_eq!(
            visited_from_zero(&xz, xz_ray),
            [world(0, 0, 0), world(1, 0, 1), world(2, 0, 2)]
        );
        assert_hit(
            first_hit(&xz, xz_ray),
            xz_ray,
            world(2, 0, 2),
            CubeFace::NegativeX,
            1.5 * 2.0_f32.sqrt(),
        );

        let yz = grid_with(world(0, 0, 0), (1, 4, 4), &[world(0, 2, 2)]);
        let yz_ray = ray(v(0.5, 0.5, 0.5), v(0.0, 1.0, 1.0));
        assert_eq!(
            visited_from_zero(&yz, yz_ray),
            [world(0, 0, 0), world(0, 1, 1), world(0, 2, 2)]
        );
        assert_hit(
            first_hit(&yz, yz_ray),
            yz_ray,
            world(0, 2, 2),
            CubeFace::NegativeY,
            1.5 * 2.0_f32.sqrt(),
        );
    }

    #[test]
    fn xyz_corner_tie_steps_all_three_axes_and_reports_the_x_face() {
        let empty = empty_grid(world(0, 0, 0), 4, 4, 4);
        let ray = ray(v(0.5, 0.5, 0.5), v(1.0, 1.0, 1.0));

        assert_eq!(
            visited_from_zero(&empty, ray),
            (0..4).map(|i| world(i, i, i)).collect::<Vec<_>>()
        );
        let grid = grid_with(world(0, 0, 0), (4, 4, 4), &[world(2, 2, 2)]);
        assert_hit(
            first_hit(&grid, ray),
            ray,
            world(2, 2, 2),
            CubeFace::NegativeX,
            1.5 * 3.0_f32.sqrt(),
        );
    }

    #[test]
    fn negative_and_mixed_sign_ties_use_the_same_priority() {
        let negative = grid_with(world(0, 0, 0), (4, 4, 1), &[world(1, 1, 0)]);
        let down_left = ray(v(3.5, 3.5, 0.5), v(-1.0, -1.0, 0.0));
        assert_eq!(
            visited_from_zero(&negative, down_left),
            [world(3, 3, 0), world(2, 2, 0), world(1, 1, 0)]
        );
        assert_hit(
            first_hit(&negative, down_left),
            down_left,
            world(1, 1, 0),
            CubeFace::PositiveX,
            1.5 * 2.0_f32.sqrt(),
        );

        let mixed = grid_with(world(0, 0, 0), (4, 4, 1), &[world(2, 1, 0)]);
        let down_right = ray(v(0.5, 3.5, 0.5), v(1.0, -1.0, 0.0));
        assert_eq!(
            visited_from_zero(&mixed, down_right),
            [world(0, 3, 0), world(1, 2, 0), world(2, 1, 0)]
        );
        assert_hit(
            first_hit(&mixed, down_right),
            down_right,
            world(2, 1, 0),
            CubeFace::NegativeX,
            1.5 * 2.0_f32.sqrt(),
        );

        let mixed_corner = grid_with(world(0, 0, 0), (4, 4, 4), &[world(1, 2, 1)]);
        let ray = ray(v(3.5, 0.5, 3.5), v(-1.0, 1.0, -1.0));
        assert_hit(
            first_hit(&mixed_corner, ray),
            ray,
            world(1, 2, 1),
            CubeFace::PositiveX,
            1.5 * 3.0_f32.sqrt(),
        );
    }

    #[test]
    fn cells_touched_only_at_a_tied_edge_are_not_entered() {
        // Under half-open ownership a ray through an exact edge never occupies the two cells
        // sharing that edge with its path; the closed-box slab test does report a contact.
        let edge = ray(v(0.5, 0.5, 0.5), v(1.0, 1.0, 0.0));
        for side in [world(1, 0, 0), world(0, 1, 0)] {
            let grid = grid_with(world(0, 0, 0), (2, 2, 1), &[side]);
            assert_eq!(first_hit(&grid, edge), None, "{side:?}");
            let contact = unit_aabb(side).intersect(edge, 0.0, INF).unwrap();
            assert_eq!(contact.t, 0.5 * 2.0_f32.sqrt());
        }

        let corner = ray(v(0.5, 0.5, 0.5), v(1.0, 1.0, 1.0));
        for side in [
            world(1, 0, 0),
            world(0, 1, 0),
            world(0, 0, 1),
            world(1, 1, 0),
            world(1, 0, 1),
            world(0, 1, 1),
        ] {
            let grid = grid_with(world(0, 0, 0), (2, 2, 2), &[side]);
            assert_eq!(first_hit(&grid, corner), None, "{side:?}");
        }
    }

    #[test]
    fn exit_ties_from_inside_a_solid_voxel_use_the_axis_priority() {
        let grid = grid_with(world(0, 0, 0), (1, 1, 1), &[world(0, 0, 0)]);
        let origin = v(0.5, 0.5, 0.5);

        for (direction, face, t) in [
            (v(1.0, 1.0, 1.0), CubeFace::PositiveX, 0.5 * 3.0_f32.sqrt()),
            (
                v(0.0, -1.0, -1.0),
                CubeFace::NegativeY,
                0.5 * 2.0_f32.sqrt(),
            ),
            (v(-1.0, 0.0, 1.0), CubeFace::NegativeX, 0.5 * 2.0_f32.sqrt()),
        ] {
            let ray = ray(origin, direction);
            assert_hit(first_hit(&grid, ray), ray, world(0, 0, 0), face, t);
            assert_eq!(
                unit_aabb(world(0, 0, 0))
                    .intersect(ray, 0.0, INF)
                    .unwrap()
                    .face,
                face
            );
        }
    }

    // ---------------------------------------------------------------- grid origin

    #[test]
    fn negative_origin_grid_is_traversed_across_world_zero() {
        let blocks = [world(2, -1, -1), world(-8, -4, -8), world(0, 2, 0)];
        let grid = grid_with(world(-8, -4, -8), (16, 8, 16), &blocks);

        let east = ray(v(-6.5, -0.5, -0.5), v(1.0, 0.0, 0.0));
        assert_hit(
            first_hit(&grid, east),
            east,
            world(2, -1, -1),
            CubeFace::NegativeX,
            8.5,
        );
        assert_eq!(visited_from_zero(&grid, east).len(), 10);

        let from_far_east = ray(v(20.0, -3.5, -7.5), v(-1.0, 0.0, 0.0));
        assert_hit(
            first_hit(&grid, from_far_east),
            from_far_east,
            world(-8, -4, -8),
            CubeFace::PositiveX,
            27.0,
        );
        assert_eq!(visited_from_zero(&grid, from_far_east).len(), 16);

        let up = ray(v(0.5, -3.5, 0.5), v(0.0, 1.0, 0.0));
        assert_hit(
            first_hit(&grid, up),
            up,
            world(0, 2, 0),
            CubeFace::NegativeY,
            5.5,
        );
    }

    #[test]
    fn negative_origin_grid_handles_starts_inside_and_oblique_entries() {
        let block = world(-1, -1, -1);
        let grid = grid_with(world(-8, -4, -8), (16, 8, 16), &[block, world(0, -3, 0)]);

        // Inside the solid voxel just below world zero.
        let inside = ray(v(-0.5, -0.5, -0.5), v(0.0, -1.0, 0.0));
        assert_hit(
            first_hit(&grid, inside),
            inside,
            block,
            CubeFace::NegativeY,
            0.5,
        );
        // Inside an empty negative cell, stepping to a block across x = 0.
        let empty_start = ray(v(-3.2, -2.7, 0.4), v(1.0, 0.0, 0.0));
        assert_hit(
            first_hit(&grid, empty_start),
            empty_start,
            world(0, -3, 0),
            CubeFace::NegativeX,
            3.2,
        );
        // From outside the grid, obliquely, matching the equivalent unit AABB.
        let oblique = ray(v(-12.3, 6.1, -11.7), v(11.8, -6.9, 11.3));
        let hit = first_hit(&grid, oblique).unwrap();
        let expected = unit_aabb(block).intersect(oblique, 0.0, INF).unwrap();
        assert_eq!(hit.voxel, block);
        assert_eq!(hit.surface.t, expected.t);
        assert_eq!(hit.surface.face, expected.face);
        assert_eq!(hit.surface.uv, expected.uv);
    }

    #[test]
    fn positive_offset_grid_is_found_from_far_away() {
        let grid = grid_with(world(100, 50, -200), (4, 4, 4), &[world(102, 51, -199)]);
        let ray = ray(v(102.5, 51.5, -250.0), v(0.0, 0.0, 1.0));

        assert_hit(
            first_hit(&grid, ray),
            ray,
            world(102, 51, -199),
            CubeFace::NegativeZ,
            51.0,
        );
        assert_eq!(visited_from_zero(&grid, ray).len(), 2);
    }

    // ---------------------------------------------------------------- intervals

    #[test]
    fn interval_bounds_are_inclusive_and_respected() {
        let grid = grid_with(world(0, 0, 0), (8, 1, 1), &[world(2, 0, 0), world(5, 0, 0)]);
        let ray = ray(v(0.5, 0.5, 0.5), v(1.0, 0.0, 0.0));
        let near = world(2, 0, 0);
        let far = world(5, 0, 0);

        assert_hit(
            intersect(&grid, ray, 0.0, INF),
            ray,
            near,
            CubeFace::NegativeX,
            1.5,
        );
        assert_hit(
            intersect(&grid, ray, 1.5, INF),
            ray,
            near,
            CubeFace::NegativeX,
            1.5,
        );
        // `t_min` inside the near block: its exit, like a clipped `Aabb` entry.
        assert_hit(
            intersect(&grid, ray, 1.6, INF),
            ray,
            near,
            CubeFace::PositiveX,
            2.5,
        );
        // `t_min` exactly on the near block's exit plane: the ray is leaving it there.
        assert_hit(
            intersect(&grid, ray, 2.5, INF),
            ray,
            far,
            CubeFace::NegativeX,
            4.5,
        );
        assert_hit(
            intersect(&grid, ray, 2.6, INF),
            ray,
            far,
            CubeFace::NegativeX,
            4.5,
        );

        assert_hit(
            intersect(&grid, ray, 0.0, 1.5),
            ray,
            near,
            CubeFace::NegativeX,
            1.5,
        );
        assert_eq!(intersect(&grid, ray, 0.0, 1.499), None);
        assert_eq!(intersect(&grid, ray, 2.6, 4.0), None);
        assert_eq!(intersect(&grid, ray, 2.6, 4.4999), None);
    }

    #[test]
    fn finite_shadow_segments_ignore_blockers_beyond_the_light() {
        let grid = grid_with(world(0, 0, 0), (8, 1, 1), &[world(2, 0, 0), world(5, 0, 0)]);
        let from = v(3.5, 0.5, 0.5);
        let to_light = |light: Vec3| (ray(from, light - from), (light - from).length());

        // A light between the two blocks is unoccluded; one beyond the far block is occluded.
        let (open, distance) = to_light(v(4.5, 0.5, 0.5));
        assert_eq!(intersect(&grid, open, 0.0, distance), None);
        let (blocked, distance) = to_light(v(7.5, 0.5, 0.5));
        assert!(intersect(&grid, blocked, 0.0, distance).is_some());
        // Directional shadow rays use an infinite segment.
        let (sun, _) = to_light(v(-10.0, 0.5, 0.5));
        assert_eq!(
            intersect(&grid, sun, 0.0, INF).map(|hit| hit.voxel),
            Some(world(2, 0, 0))
        );
    }

    #[test]
    fn invalid_intervals_and_origins_return_none() {
        let mut grid = empty_grid(world(0, 0, 0), 2, 2, 2);
        grid.fill(Voxel::Block(BlockType::Cobblestone));
        let valid = ray(v(-1.0, 0.5, 0.5), v(1.0, 0.0, 0.0));

        assert!(intersect(&grid, valid, 0.0, INF).is_some());
        assert_eq!(intersect(&grid, valid, f32::NAN, INF), None);
        assert_eq!(intersect(&grid, valid, 0.0, f32::NAN), None);
        assert_eq!(intersect(&grid, valid, 2.0, 1.0), None);
        for origin in [v(f32::NAN, 0.5, 0.5), v(0.5, f32::INFINITY, 0.5)] {
            let invalid = ray(origin, v(1.0, 0.0, 0.0));
            assert_eq!(intersect(&grid, invalid, 0.0, INF), None);
        }
    }

    #[test]
    fn negative_t_min_searches_behind_the_origin_like_the_slab_test() {
        let grid = grid_with(world(0, 0, 0), (8, 1, 1), &[world(2, 0, 0)]);
        let ray = ray(v(6.5, 0.5, 0.5), v(1.0, 0.0, 0.0));

        assert_eq!(intersect(&grid, ray, 0.0, INF), None);
        assert_hit(
            intersect(&grid, ray, -10.0, INF),
            ray,
            world(2, 0, 0),
            CubeFace::NegativeX,
            -4.5,
        );
        assert_eq!(
            unit_aabb(world(2, 0, 0))
                .intersect(ray, -10.0, INF)
                .unwrap()
                .t,
            -4.5
        );
    }

    // ---------------------------------------------------------------- materials and blocks

    #[test]
    fn adjacent_glass_voxels_expose_their_shared_interface() {
        let mut grid = empty_grid(world(0, 0, 0), 4, 1, 1);
        for x in [1, 2] {
            grid.set_world(world(x, 0, 0), Voxel::Block(BlockType::Glass))
                .unwrap();
        }
        let direction = v(1.0, 0.0, 0.0);

        let primary = ray(v(-1.0, 0.5, 0.5), direction);
        let entry = assert_hit(
            first_hit(&grid, primary),
            primary,
            world(1, 0, 0),
            CubeFace::NegativeX,
            2.0,
        );
        assert_eq!(entry.block, BlockType::Glass);
        assert_eq!(entry.material_id, materials().glass);

        // A refracted ray biased inside the first voxel exits it at the internal interface,
        // not at the far side of the glass run: each glass cell is independent.
        let inside_first = ray(
            entry.surface.position - entry.surface.normal * ORIGIN_BIAS,
            direction,
        );
        let interface = assert_hit(
            first_hit(&grid, inside_first),
            inside_first,
            world(1, 0, 0),
            CubeFace::PositiveX,
            1.0 - ORIGIN_BIAS,
        );
        // Continuing past the interface enters the second voxel's interior and exits it.
        let inside_second = ray(
            interface.surface.position + interface.surface.normal * ORIGIN_BIAS,
            direction,
        );
        let exit = assert_hit(
            first_hit(&grid, inside_second),
            inside_second,
            world(2, 0, 0),
            CubeFace::PositiveX,
            1.0 - ORIGIN_BIAS,
        );
        let outgoing = ray(
            exit.surface.position + exit.surface.normal * ORIGIN_BIAS,
            direction,
        );
        assert_eq!(first_hit(&grid, outgoing), None);
    }

    #[test]
    fn lava_is_an_ordinary_occupied_voxel() {
        let mut grid = empty_grid(world(-1, -1, -1), 3, 3, 3);
        grid.set_world(world(0, 0, 0), Voxel::Block(BlockType::Lava))
            .unwrap();
        let ray = ray(v(0.5, 4.0, 0.5), v(0.0, -1.0, 0.0));

        let hit = assert_hit(
            first_hit(&grid, ray),
            ray,
            world(0, 0, 0),
            CubeFace::PositiveY,
            3.0,
        );
        assert_eq!(hit.block, BlockType::Lava);
        assert_eq!(hit.material_id, materials().lava);
    }

    #[test]
    fn every_block_type_resolves_its_material() {
        let materials = materials();
        for block in BlockType::ALL {
            let mut grid = empty_grid(world(0, 0, 0), 1, 1, 1);
            grid.set_world(world(0, 0, 0), Voxel::Block(block)).unwrap();
            let hit = grid
                .intersect(
                    ray(v(-1.0, 0.5, 0.5), v(1.0, 0.0, 0.0)),
                    &materials,
                    0.0,
                    INF,
                )
                .unwrap();
            assert_eq!(hit.block, block);
            assert_eq!(hit.material_id, materials.material(block));
        }
    }

    #[test]
    fn interior_voxels_are_stored_but_never_reached_from_outside() {
        let mut grid = empty_grid(world(-1, -1, -1), 3, 3, 3);
        grid.fill(Voxel::Block(BlockType::Cobblestone));
        grid.set_world(world(0, 0, 0), Voxel::Block(BlockType::DiamondOre))
            .unwrap();

        for face in CubeFace::ALL {
            let normal = face.normal();
            let ray = ray(v(0.5, 0.5, 0.5) + normal * 5.0, -normal);
            let hit = first_hit(&grid, ray).unwrap();
            assert_eq!(hit.block, BlockType::Cobblestone, "{face:?}");
            assert_eq!(hit.surface.face, face);
            assert_eq!(hit.surface.t, 3.5);
        }
        // A ray that starts inside the hidden voxel still finds its exit.
        let inside = ray(v(0.5, 0.5, 0.5), v(0.0, 1.0, 0.0));
        let hit = first_hit(&grid, inside).unwrap();
        assert_eq!(hit.block, BlockType::DiamondOre);
        assert_eq!(hit.surface.face, CubeFace::PositiveY);
    }

    // ---------------------------------------------------------------- biased secondary rays

    #[test]
    fn biased_secondary_origins_neither_self_hit_nor_tunnel() {
        let grid = grid_with(world(-2, -2, -2), (4, 4, 4), &[world(0, 0, 0)]);
        let primary = ray(v(0.3, 3.0, 0.6), v(0.1, -1.0, 0.05));
        let hit = assert_hit(
            first_hit(&grid, primary),
            primary,
            world(0, 0, 0),
            CubeFace::PositiveY,
            2.0 * primary.direction().length() / -primary.direction().y,
        );
        let position = hit.surface.position;
        let normal = hit.surface.normal;

        // Reflection and shadow rays start just outside the face and leave it.
        let reflected = ray(
            position + normal * ORIGIN_BIAS,
            primary.direction().reflect(normal),
        );
        assert_eq!(first_hit(&grid, reflected), None);
        assert_eq!(visited_from_zero(&grid, reflected)[0], world(0, 1, 0));
        let shadow = ray(position + normal * ORIGIN_BIAS, v(0.6, 1.0, 0.8));
        assert_eq!(first_hit(&grid, shadow), None);

        // A refracted ray starts just inside and exits through the opposite face.
        let refracted = ray(position - normal * ORIGIN_BIAS, v(0.05, -1.0, 0.02));
        let exit = first_hit(&grid, refracted).unwrap();
        assert_eq!(exit.voxel, world(0, 0, 0));
        assert_eq!(exit.surface.face, CubeFace::NegativeY);
        assert!(exit.surface.t > 0.9);
    }

    // ---------------------------------------------------------------- UV equivalence

    #[test]
    fn dda_surfaces_match_the_unit_aabb_on_every_face() {
        let voxel = world(-3, 5, 11);
        let grid = grid_with(world(-6, 2, 8), (7, 7, 7), &[voxel]);
        let center = voxel.min_corner() + v(0.5, 0.5, 0.5);
        let skews = [
            Vec3::ZERO,
            v(0.3, -0.2, 0.1),
            v(-0.4, 0.35, -0.25),
            v(0.21, 0.43, -0.37),
        ];

        for face in CubeFace::ALL {
            let normal = face.normal();
            for skew in skews {
                let skew = skew - normal * skew.dot(normal);
                let target = center + normal * 0.5 + skew * 0.6;
                let origin = target + normal * 2.0 + skew;
                let ray = ray(origin, target - origin);

                let aabb = unit_aabb(voxel).intersect(ray, 0.0, INF).unwrap();
                let hit = first_hit(&grid, ray).unwrap();
                assert_eq!(aabb.face, face);
                assert_same_surface(hit.surface, aabb, ray);
            }
        }
    }

    /// Bit-equal `t`, face, normal, and UV, and the same position up to the snapped coordinate.
    #[track_caller]
    fn assert_same_surface(dda: SurfaceHit, aabb: SurfaceHit, ray: Ray) {
        assert_eq!(dda.t, aabb.t, "{ray:?}");
        assert_eq!(dda.face, aabb.face, "{ray:?}");
        assert_eq!(dda.normal, aabb.normal, "{ray:?}");
        assert_eq!(dda.uv, aabb.uv, "{ray:?}");
        for (component, dda_value, aabb_value) in [
            (aabb.normal.x, dda.position.x, aabb.position.x),
            (aabb.normal.y, dda.position.y, aabb.position.y),
            (aabb.normal.z, dda.position.z, aabb.position.z),
        ] {
            if component == 0.0 {
                assert_eq!(dda_value, aabb_value, "{ray:?}");
            } else {
                assert!((dda_value - aabb_value).abs() <= EPSILON, "{ray:?}");
            }
        }
    }

    // ---------------------------------------------------------------- DDA vs AABB oracle

    /// The brute-force reference answer for one ray: test-only, never a production path.
    enum Reference {
        Miss,
        Hit(VoxelPosition, SurfaceHit),
        /// The nearest contact is an entry along an edge or corner, where the closed-box slab
        /// test and the DDA's half-open ownership legitimately disagree.
        Ambiguous,
    }

    fn occupied_voxels(grid: &VoxelGrid) -> Vec<(VoxelPosition, BlockType)> {
        let origin = grid.origin();
        let mut voxels = Vec::new();
        for y in 0..grid.height() as i32 {
            for z in 0..grid.depth() as i32 {
                for x in 0..grid.width() as i32 {
                    let position = world(origin.x + x, origin.y + y, origin.z + z);
                    if let Some(Voxel::Block(block)) = grid.get_world(position) {
                        voxels.push((position, block));
                    }
                }
            }
        }
        voxels
    }

    fn is_exit(ray: Ray, hit: SurfaceHit) -> bool {
        hit.normal.dot(ray.direction()) > 0.0
    }

    /// Whether a hit lies within `1e-4` of an edge of its face.
    fn near_edge(voxel: VoxelPosition, hit: SurfaceHit) -> bool {
        let offset = hit.position - voxel.min_corner();
        let near_boundary = |value: f32| value.abs() < 1.0e-4 || (value - 1.0).abs() < 1.0e-4;
        [
            (hit.normal.x, offset.x),
            (hit.normal.y, offset.y),
            (hit.normal.z, offset.z),
        ]
        .into_iter()
        .any(|(component, value)| component == 0.0 && near_boundary(value))
    }

    /// Nearest hit over one unit `Aabb` per occupied voxel.
    ///
    /// When a voxel's exit and a neighbor's entry share the nearest `t`, the voxel being left
    /// wins, as in the DDA; two entries at the same `t` are an edge or corner contact.
    fn reference_hit(
        voxels: &[(VoxelPosition, BlockType)],
        ray: Ray,
        t_min: f32,
        t_max: f32,
    ) -> Reference {
        let mut best: Option<(VoxelPosition, SurfaceHit)> = None;
        let mut tied_entries = false;
        for &(voxel, _) in voxels {
            let Some(hit) = unit_aabb(voxel).intersect(ray, t_min, t_max) else {
                continue;
            };
            match best {
                Some((_, current)) if hit.t > current.t => {}
                Some((_, current)) if hit.t == current.t => {
                    if is_exit(ray, hit) && !is_exit(ray, current) {
                        best = Some((voxel, hit));
                        tied_entries = false;
                    } else if !is_exit(ray, hit) && !is_exit(ray, current) {
                        tied_entries = true;
                    }
                }
                _ => {
                    best = Some((voxel, hit));
                    tied_entries = false;
                }
            }
        }

        match best {
            None => Reference::Miss,
            Some((voxel, hit)) if tied_entries || (!is_exit(ray, hit) && near_edge(voxel, hit)) => {
                Reference::Ambiguous
            }
            Some((voxel, hit)) => Reference::Hit(voxel, hit),
        }
    }

    /// A deterministic, irregular pattern of mixed blocks with gaps.
    fn oracle_grid() -> VoxelGrid {
        let origin = world(-2, -1, -3);
        let mut grid = empty_grid(origin, 5, 4, 6);
        for y in 0..4 {
            for z in 0..6 {
                for x in 0..5 {
                    let key = (x * 7 + y * 13 + z * 5 + x * z) % 9;
                    if key < 3 {
                        let block = BlockType::ALL[(x + 2 * y + 3 * z) as usize % 10];
                        let position = world(origin.x + x, origin.y + y, origin.z + z);
                        grid.set_world(position, Voxel::Block(block)).unwrap();
                    }
                }
            }
        }
        grid
    }

    /// A Fibonacci sphere plus axis-aligned and single-zero-component directions.
    fn oracle_directions() -> Vec<Vec3> {
        let count = 48;
        let golden_angle = PI * (3.0 - 5.0_f32.sqrt());
        let mut directions: Vec<Vec3> = (0..count)
            .map(|i| {
                let y = 1.0 - 2.0 * (i as f32 + 0.5) / count as f32;
                let radius = (1.0 - y * y).sqrt();
                let angle = golden_angle * i as f32;
                v(radius * angle.cos(), y, radius * angle.sin())
            })
            .collect();
        directions.extend(CubeFace::ALL.map(CubeFace::normal));
        directions.extend([
            v(0.0, 0.6, -0.8),
            v(0.0, -0.31, 0.95),
            v(0.7, 0.0, 0.3),
            v(-0.45, 0.0, -0.89),
            v(-0.45, 0.85, 0.0),
            v(0.92, -0.39, 0.0),
        ]);
        directions
    }

    /// Non-integer lattice origins around and inside the oracle grid.
    fn oracle_origins() -> Vec<Vec3> {
        let mut origins = Vec::new();
        for i in 0..8 {
            for j in 0..7 {
                for k in 0..8 {
                    origins.push(v(
                        -4.863 + 1.31 * i as f32,
                        -2.871 + 1.27 * j as f32,
                        -4.917 + 1.43 * k as f32,
                    ));
                }
            }
        }
        origins
    }

    #[test]
    fn dda_matches_the_brute_force_aabb_oracle() {
        let grid = oracle_grid();
        let voxels = occupied_voxels(&grid);
        let materials = materials();
        let (mut hits, mut misses, mut exits, mut ambiguous) = (0, 0, 0, 0);
        assert!(voxels.len() > 20 && voxels.len() < 80, "{}", voxels.len());

        for origin in oracle_origins() {
            for direction in oracle_directions() {
                let ray = ray(origin, direction);
                for (t_min, t_max) in [(0.0, INF), (0.7, INF), (0.0, 2.9), (1.6, 4.4)] {
                    let dda = grid.intersect(ray, &materials, t_min, t_max);
                    match (reference_hit(&voxels, ray, t_min, t_max), dda) {
                        (Reference::Ambiguous, _) => ambiguous += 1,
                        (Reference::Miss, None) => misses += 1,
                        (Reference::Hit(voxel, expected), Some(hit)) => {
                            assert_eq!(hit.voxel, voxel, "{ray:?} [{t_min}, {t_max}]");
                            assert_same_surface(hit.surface, expected, ray);
                            assert_eq!(hit.block, grid.get_world(voxel).unwrap().block().unwrap());
                            assert_eq!(hit.material_id, materials.material(hit.block));
                            assert!(t_min <= hit.surface.t && hit.surface.t <= t_max);
                            hits += 1;
                            exits += usize::from(is_exit(ray, expected));
                        }
                        (Reference::Miss, Some(hit)) => {
                            panic!(
                                "DDA hit {hit:?} where the oracle misses: {ray:?} [{t_min}, {t_max}]"
                            )
                        }
                        (Reference::Hit(voxel, expected), None) => {
                            panic!("DDA missed {voxel:?} {expected:?}: {ray:?} [{t_min}, {t_max}]")
                        }
                    }
                }
            }
        }

        // Every category is exercised, and edge/corner ambiguity is rare for generic rays.
        assert!(hits > 15_000, "{hits}");
        assert!(misses > 50_000, "{misses}");
        assert!(exits > 1_000, "{exits}");
        assert!(ambiguous * 200 < hits + misses, "{ambiguous}");
    }

    // ---------------------------------------------------------------- traversal cost

    #[test]
    fn visited_cells_scale_with_cells_crossed_not_with_grid_volume() {
        let (width, height, depth) = (64, 16, 64);
        let empty = empty_grid(world(-32, -8, -32), width, height, depth);
        let crossing = ray(v(-40.0, -7.3, -35.0), v(1.0, 0.2, 0.9));
        let cells = visited_from_zero(&empty, crossing);
        assert_eq!(first_hit(&empty, crossing), None);
        assert!(!cells.is_empty() && cells.len() <= width + height + depth);

        // A terrain-like half-filled grid: a naive scan would test every occupied voxel.
        let mut terrain = empty.clone();
        for y in -8..0 {
            for z in -32..32 {
                for x in -32..32 {
                    terrain
                        .set_world(world(x, y, z), Voxel::Block(BlockType::Dirt))
                        .unwrap();
                }
            }
        }
        let occupied = occupied_voxels(&terrain).len();
        let camera = ray(v(-30.3, 20.0, -29.6), v(1.0, -0.8, 0.9));
        let hit = first_hit(&terrain, camera).unwrap();
        let cells = visited_from_zero(&terrain, camera);

        assert_eq!(occupied, 32_768);
        assert_eq!(hit.surface.face, CubeFace::PositiveY);
        assert_eq!(cells.last(), Some(&hit.voxel));
        assert!(cells.len() < 64, "{}", cells.len());
    }
}
