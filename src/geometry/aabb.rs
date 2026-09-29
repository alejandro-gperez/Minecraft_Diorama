use crate::{math::Vec3, ray::Ray};

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
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
        ) || !interval.clip_axis(
            origin.y,
            direction.y,
            self.min.y,
            self.max.y,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ) || !interval.clip_axis(
            origin.z,
            direction.z,
            self.min.z,
            self.max.z,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
        ) {
            return None;
        }

        if self.strictly_contains(origin) {
            return hit_in_range(ray, interval.exit_t, interval.exit_normal, t_min, t_max);
        }

        if let Some(hit) = hit_in_range(ray, interval.entry_t, interval.entry_normal, t_min, t_max)
        {
            return Some(hit);
        }

        if interval.entry_t < t_min {
            return hit_in_range(ray, interval.exit_t, interval.exit_normal, t_min, t_max);
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
}

#[derive(Clone, Copy)]
struct SlabInterval {
    entry_t: f32,
    exit_t: f32,
    entry_normal: Vec3,
    exit_normal: Vec3,
}

impl SlabInterval {
    fn new() -> Self {
        Self {
            entry_t: f32::NEG_INFINITY,
            exit_t: f32::INFINITY,
            entry_normal: Vec3::ZERO,
            exit_normal: Vec3::ZERO,
        }
    }

    fn clip_axis(
        &mut self,
        origin: f32,
        direction: f32,
        slab_min: f32,
        slab_max: f32,
        min_normal: Vec3,
        max_normal: Vec3,
    ) -> bool {
        if direction.abs() <= PARALLEL_DIRECTION_EPSILON {
            return origin >= slab_min && origin <= slab_max;
        }

        let (near_t, far_t, near_normal, far_normal) = if direction > 0.0 {
            (
                (slab_min - origin) / direction,
                (slab_max - origin) / direction,
                min_normal,
                max_normal,
            )
        } else {
            (
                (slab_max - origin) / direction,
                (slab_min - origin) / direction,
                max_normal,
                min_normal,
            )
        };

        if near_t > self.entry_t {
            self.entry_t = near_t;
            self.entry_normal = near_normal;
        }

        if far_t < self.exit_t {
            self.exit_t = far_t;
            self.exit_normal = far_normal;
        }

        self.entry_t <= self.exit_t
    }
}

fn hit_in_range(ray: Ray, t: f32, normal: Vec3, t_min: f32, t_max: f32) -> Option<AabbHit> {
    (t.is_finite() && t >= t_min && t <= t_max).then(|| AabbHit {
        t,
        position: ray.at(t),
        normal,
    })
}

#[cfg(test)]
mod tests {
    use super::{Aabb, AabbHit};
    use crate::{math::Vec3, ray::Ray};

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
