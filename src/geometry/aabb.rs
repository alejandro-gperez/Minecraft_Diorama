use crate::{math::Vec3, ray::Ray};

use super::{CubeFace, Uv};

// Avoid unstable reciprocal distances for directions effectively parallel to a slab.
const PARALLEL_DIRECTION_EPSILON: f32 = 1.0e-8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    min: Vec3,
    max: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AabbHit {
    pub t: f32,
    pub position: Vec3,
    pub normal: Vec3,
    pub face: CubeFace,
    /// Absent only when a dimension required by the selected face is exactly degenerate.
    pub uv: Option<Uv>,
}

impl Aabb {
    pub fn try_new(min: Vec3, max: Vec3) -> Option<Self> {
        let bounds_are_ordered = min.x <= max.x && min.y <= max.y && min.z <= max.z;

        (min.is_finite() && max.is_finite() && bounds_are_ordered).then_some(Self { min, max })
    }

    pub const fn min(self) -> Vec3 {
        self.min
    }

    pub const fn max(self) -> Vec3 {
        self.max
    }

    /// Finds the nearest valid surface in the requested interval using slab clipping.
    ///
    /// A ray whose origin is strictly inside the box reports the exit surface, so callers
    /// always receive a forward-facing boundary hit rather than an artificial entry point.
    pub fn intersect(&self, ray: Ray, t_min: f32, t_max: f32) -> Option<AabbHit> {
        let (t, face) = self.surface_in_range(ray, t_min, t_max)?;
        let position = ray.at(t);

        Some(AabbHit {
            t,
            position,
            normal: face.normal(),
            face,
            uv: self.uv_for_face(position, face),
        })
    }

    /// Reports whether any box surface falls in the requested ray interval.
    ///
    /// This avoids constructing positions, normals, and UVs for visibility-only queries.
    pub fn intersects(&self, ray: Ray, t_min: f32, t_max: f32) -> bool {
        self.surface_in_range(ray, t_min, t_max).is_some()
    }

    fn surface_in_range(&self, ray: Ray, t_min: f32, t_max: f32) -> Option<(f32, CubeFace)> {
        if t_min.is_nan() || t_max.is_nan() || t_min > t_max || !ray.origin().is_finite() {
            return None;
        }

        let mut interval = SlabInterval::new();
        let origin = ray.origin();
        let direction = ray.direction();

        if !interval.clip_axis(
            origin.x,
            direction.x,
            self.min.x,
            self.max.x,
            CubeFace::NegativeX,
            CubeFace::PositiveX,
        ) || !interval.clip_axis(
            origin.y,
            direction.y,
            self.min.y,
            self.max.y,
            CubeFace::NegativeY,
            CubeFace::PositiveY,
        ) || !interval.clip_axis(
            origin.z,
            direction.z,
            self.min.z,
            self.max.z,
            CubeFace::NegativeZ,
            CubeFace::PositiveZ,
        ) {
            return None;
        }

        if self.strictly_contains(origin) {
            return surface_in_range(interval.exit_t, interval.exit_face, t_min, t_max);
        }

        if let Some(surface) = surface_in_range(interval.entry_t, interval.entry_face, t_min, t_max)
        {
            return Some(surface);
        }

        if interval.entry_t < t_min {
            return surface_in_range(interval.exit_t, interval.exit_face, t_min, t_max);
        }

        None
    }

    fn strictly_contains(&self, point: Vec3) -> bool {
        point.x > self.min.x
            && point.x < self.max.x
            && point.y > self.min.y
            && point.y < self.max.y
            && point.z > self.min.z
            && point.z < self.max.z
    }

    fn uv_for_face(&self, position: Vec3, face: CubeFace) -> Option<Uv> {
        let local_x = || normalized_coordinate(position.x, self.min.x, self.max.x);
        let local_y = || normalized_coordinate(position.y, self.min.y, self.max.y);
        let local_z = || normalized_coordinate(position.z, self.min.z, self.max.z);

        let uv = match face {
            CubeFace::PositiveX => Uv::new(1.0 - local_z()?, 1.0 - local_y()?),
            CubeFace::NegativeX => Uv::new(local_z()?, 1.0 - local_y()?),
            CubeFace::PositiveY => Uv::new(local_x()?, local_z()?),
            CubeFace::NegativeY => Uv::new(local_x()?, 1.0 - local_z()?),
            CubeFace::PositiveZ => Uv::new(local_x()?, 1.0 - local_y()?),
            CubeFace::NegativeZ => Uv::new(1.0 - local_x()?, 1.0 - local_y()?),
        };

        Some(uv)
    }
}

fn surface_in_range(t: f32, face: CubeFace, t_min: f32, t_max: f32) -> Option<(f32, CubeFace)> {
    (t.is_finite() && t >= t_min && t <= t_max).then_some((t, face))
}

#[derive(Clone, Copy)]
struct SlabInterval {
    entry_t: f32,
    exit_t: f32,
    entry_face: CubeFace,
    exit_face: CubeFace,
}

impl SlabInterval {
    fn new() -> Self {
        Self {
            entry_t: f32::NEG_INFINITY,
            exit_t: f32::INFINITY,
            entry_face: CubeFace::NegativeX,
            exit_face: CubeFace::PositiveX,
        }
    }

    fn clip_axis(
        &mut self,
        origin: f32,
        direction: f32,
        slab_min: f32,
        slab_max: f32,
        min_face: CubeFace,
        max_face: CubeFace,
    ) -> bool {
        if direction.abs() <= PARALLEL_DIRECTION_EPSILON {
            return origin >= slab_min && origin <= slab_max;
        }

        let (near_t, far_t, near_face, far_face) = if direction > 0.0 {
            (
                (slab_min - origin) / direction,
                (slab_max - origin) / direction,
                min_face,
                max_face,
            )
        } else {
            (
                (slab_max - origin) / direction,
                (slab_min - origin) / direction,
                max_face,
                min_face,
            )
        };

        if near_t > self.entry_t {
            self.entry_t = near_t;
            self.entry_face = near_face;
        }

        if far_t < self.exit_t {
            self.exit_t = far_t;
            self.exit_face = far_face;
        }

        self.entry_t <= self.exit_t
    }
}

fn normalized_coordinate(value: f32, min: f32, max: f32) -> Option<f32> {
    let extent = max - min;
    if extent == 0.0 || !extent.is_finite() {
        return None;
    }

    let local = (value - min) / extent;
    local.is_finite().then(|| local.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::{Aabb, AabbHit};
    use crate::{
        geometry::{CubeFace, Uv},
        math::Vec3,
        ray::Ray,
    };

    const EPSILON: f32 = 1.0e-6;

    fn unit_box() -> Aabb {
        Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap()
    }

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray::try_new(origin, direction).unwrap()
    }

    fn assert_approx_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= EPSILON,
            "{actual} != {expected}"
        );
    }

    fn assert_vec_approx_eq(actual: Vec3, expected: Vec3) {
        assert_approx_eq(actual.x, expected.x);
        assert_approx_eq(actual.y, expected.y);
        assert_approx_eq(actual.z, expected.z);
    }

    fn assert_hit(hit: AabbHit, expected_t: f32, expected_position: Vec3, expected_normal: Vec3) {
        assert_approx_eq(hit.t, expected_t);
        assert_vec_approx_eq(hit.position, expected_position);
        assert_eq!(hit.normal, expected_normal);
        assert_eq!(hit.face.normal(), expected_normal);
    }

    fn assert_uv(actual: Option<Uv>, expected_u: f32, expected_v: f32) {
        let actual = actual.expect("face should have valid UV coordinates");
        assert_approx_eq(actual.u, expected_u);
        assert_approx_eq(actual.v, expected_v);
    }

    #[test]
    fn constructs_valid_box_and_exposes_bounds() {
        let min = Vec3::new(-1.0, -2.0, -3.0);
        let max = Vec3::new(1.0, 2.0, 3.0);
        let bounds = Aabb::try_new(min, max).unwrap();

        assert_eq!(bounds.min(), min);
        assert_eq!(bounds.max(), max);
    }

    #[test]
    fn rejects_reversed_bounds() {
        assert_eq!(Aabb::try_new(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO), None);
        assert_eq!(Aabb::try_new(Vec3::new(0.0, 1.0, 0.0), Vec3::ZERO), None);
        assert_eq!(Aabb::try_new(Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO), None);
    }

    #[test]
    fn rejects_non_finite_bounds() {
        assert_eq!(
            Aabb::try_new(
                Vec3::new(f32::NEG_INFINITY, 0.0, 0.0),
                Vec3::new(1.0, 1.0, 1.0)
            ),
            None
        );
        assert_eq!(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, f32::NAN, 1.0)),
            None
        );
    }

    #[test]
    fn hits_negative_x_face_with_correct_result() {
        let hit = unit_box()
            .intersect(
                ray(Vec3::new(-2.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_hit(
            hit,
            2.0,
            Vec3::new(0.0, 0.5, 0.5),
            Vec3::new(-1.0, 0.0, 0.0),
        );
    }

    #[test]
    fn hits_positive_x_face_with_correct_normal() {
        let hit = unit_box()
            .intersect(
                ray(Vec3::new(3.0, 0.5, 0.5), Vec3::new(-1.0, 0.0, 0.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_hit(hit, 2.0, Vec3::new(1.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn hits_negative_y_face() {
        let hit = unit_box()
            .intersect(
                ray(Vec3::new(0.5, -2.0, 0.5), Vec3::new(0.0, 1.0, 0.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_hit(
            hit,
            2.0,
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(0.0, -1.0, 0.0),
        );
    }

    #[test]
    fn hits_positive_z_face() {
        let hit = unit_box()
            .intersect(
                ray(Vec3::new(0.5, 0.5, 3.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_hit(hit, 2.0, Vec3::new(0.5, 0.5, 1.0), Vec3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn misses_when_ray_does_not_cross_box() {
        let hit = unit_box().intersect(
            ray(Vec3::new(-2.0, 2.0, 0.5), Vec3::new(1.0, 1.0, 0.0)),
            0.0,
            f32::INFINITY,
        );

        assert_eq!(hit, None);
    }

    #[test]
    fn parallel_ray_outside_slab_misses() {
        let hit = unit_box().intersect(
            ray(Vec3::new(-2.0, 2.0, 0.5), Vec3::new(1.0, 0.0, 0.0)),
            0.0,
            f32::INFINITY,
        );

        assert_eq!(hit, None);
    }

    #[test]
    fn parallel_ray_inside_slab_can_hit() {
        let hit = unit_box()
            .intersect(
                ray(Vec3::new(-2.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_approx_eq(hit.t, 2.0);
    }

    #[test]
    fn nearly_parallel_ray_inside_slab_can_hit() {
        let hit = unit_box()
            .intersect(
                ray(Vec3::new(0.5, -2.0, 0.5), Vec3::new(1.0e-9, 1.0, 0.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_approx_eq(hit.t, 2.0);
        assert_eq!(hit.normal, Vec3::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn ray_starting_inside_returns_exit_with_outward_normal() {
        let hit = unit_box()
            .intersect(
                ray(Vec3::new(0.5, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_hit(hit, 0.5, Vec3::new(1.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn clipped_entry_can_return_exit_surface_in_interval() {
        let hit = unit_box()
            .intersect(
                ray(Vec3::new(-2.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
                2.5,
                3.5,
            )
            .unwrap();

        assert_hit(hit, 3.0, Vec3::new(1.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn rejects_surfaces_before_t_min() {
        let hit = unit_box().intersect(
            ray(Vec3::new(-2.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
            3.5,
            f32::INFINITY,
        );

        assert_eq!(hit, None);
    }

    #[test]
    fn rejects_intersection_after_t_max() {
        let hit = unit_box().intersect(
            ray(Vec3::new(-2.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
            0.0,
            1.5,
        );

        assert_eq!(hit, None);
    }

    #[test]
    fn rejects_invalid_or_nan_interval() {
        let ray = ray(Vec3::new(-2.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0));

        assert_eq!(unit_box().intersect(ray, 2.0, 1.0), None);
        assert_eq!(unit_box().intersect(ray, f32::NAN, 3.0), None);
        assert_eq!(unit_box().intersect(ray, 0.0, f32::NAN), None);
    }

    #[test]
    fn intersects_zero_thickness_box_without_nan() {
        let bounds = Aabb::try_new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 1.0)).unwrap();
        let hit = bounds
            .intersect(
                ray(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_hit(
            hit,
            1.0,
            Vec3::new(0.0, 0.5, 0.5),
            Vec3::new(-1.0, 0.0, 0.0),
        );
        assert_uv(hit.uv, 0.5, 0.5);
    }

    #[test]
    fn reports_all_six_faces_with_matching_outward_normals() {
        let cases = [
            (
                Vec3::new(-1.0, 0.25, 0.75),
                Vec3::new(1.0, 0.0, 0.0),
                CubeFace::NegativeX,
            ),
            (
                Vec3::new(2.0, 0.25, 0.75),
                Vec3::new(-1.0, 0.0, 0.0),
                CubeFace::PositiveX,
            ),
            (
                Vec3::new(0.25, -1.0, 0.75),
                Vec3::new(0.0, 1.0, 0.0),
                CubeFace::NegativeY,
            ),
            (
                Vec3::new(0.25, 2.0, 0.75),
                Vec3::new(0.0, -1.0, 0.0),
                CubeFace::PositiveY,
            ),
            (
                Vec3::new(0.25, 0.75, -1.0),
                Vec3::new(0.0, 0.0, 1.0),
                CubeFace::NegativeZ,
            ),
            (
                Vec3::new(0.25, 0.75, 2.0),
                Vec3::new(0.0, 0.0, -1.0),
                CubeFace::PositiveZ,
            ),
        ];

        for (origin, direction, expected_face) in cases {
            let hit = unit_box()
                .intersect(ray(origin, direction), 0.0, f32::INFINITY)
                .unwrap();

            assert_eq!(hit.face, expected_face);
            assert_eq!(hit.normal, expected_face.normal());
        }
    }

    #[test]
    fn maps_all_faces_using_translated_non_unit_local_coordinates() {
        let bounds =
            Aabb::try_new(Vec3::new(10.0, 20.0, 30.0), Vec3::new(14.0, 26.0, 38.0)).unwrap();
        let cases = [
            (
                Vec3::new(9.0, 21.5, 36.0),
                Vec3::new(1.0, 0.0, 0.0),
                CubeFace::NegativeX,
                (0.75, 0.75),
            ),
            (
                Vec3::new(15.0, 21.5, 36.0),
                Vec3::new(-1.0, 0.0, 0.0),
                CubeFace::PositiveX,
                (0.25, 0.75),
            ),
            (
                Vec3::new(11.0, 19.0, 36.0),
                Vec3::new(0.0, 1.0, 0.0),
                CubeFace::NegativeY,
                (0.25, 0.25),
            ),
            (
                Vec3::new(11.0, 27.0, 36.0),
                Vec3::new(0.0, -1.0, 0.0),
                CubeFace::PositiveY,
                (0.25, 0.75),
            ),
            (
                Vec3::new(11.0, 21.5, 29.0),
                Vec3::new(0.0, 0.0, 1.0),
                CubeFace::NegativeZ,
                (0.75, 0.75),
            ),
            (
                Vec3::new(11.0, 21.5, 39.0),
                Vec3::new(0.0, 0.0, -1.0),
                CubeFace::PositiveZ,
                (0.25, 0.75),
            ),
        ];

        for (origin, direction, expected_face, (expected_u, expected_v)) in cases {
            let hit = bounds
                .intersect(ray(origin, direction), 0.0, f32::INFINITY)
                .unwrap();

            assert_eq!(hit.face, expected_face);
            assert_uv(hit.uv, expected_u, expected_v);
        }
    }

    #[test]
    fn uv_mapping_is_translation_invariant() {
        let original = Aabb::try_new(Vec3::ZERO, Vec3::new(2.0, 4.0, 6.0)).unwrap();
        let translated =
            Aabb::try_new(Vec3::new(10.0, -7.0, 3.0), Vec3::new(12.0, -3.0, 9.0)).unwrap();

        let original_hit = original
            .intersect(
                ray(Vec3::new(0.5, 1.0, 7.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();
        let translated_hit = translated
            .intersect(
                ray(Vec3::new(10.5, -6.0, 10.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_uv(original_hit.uv, 0.25, 0.75);
        assert_eq!(original_hit.uv, translated_hit.uv);
    }

    #[test]
    fn degenerate_required_dimension_returns_no_uv_without_losing_hit() {
        let bounds = Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 0.0, 1.0)).unwrap();
        let hit = bounds
            .intersect(
                ray(Vec3::new(2.0, 0.0, 0.5), Vec3::new(-1.0, 0.0, 0.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_eq!(hit.face, CubeFace::PositiveX);
        assert_eq!(hit.normal, CubeFace::PositiveX.normal());
        assert_eq!(hit.uv, None);
    }

    #[test]
    fn rejects_ray_with_non_finite_origin() {
        let invalid_origin_ray = ray(Vec3::new(f32::NAN, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0));

        assert_eq!(
            unit_box().intersect(invalid_origin_ray, 0.0, f32::INFINITY),
            None
        );
    }
}
