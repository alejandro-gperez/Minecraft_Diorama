use crate::math::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    origin: Vec3,
    direction: Vec3,
}

impl Ray {
    pub fn try_new(origin: Vec3, direction: Vec3) -> Option<Self> {
        direction
            .try_normalized()
            .map(|direction| Self { origin, direction })
    }

    pub const fn origin(self) -> Vec3 {
        self.origin
    }

    pub const fn direction(self) -> Vec3 {
        self.direction
    }

    pub fn at(self, t: f32) -> Vec3 {
        self.origin + self.direction * t
    }
}

#[cfg(test)]
mod tests {
    use super::Ray;
    use crate::math::Vec3;

    const EPSILON: f32 = 1.0e-6;

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

    #[test]
    fn constructs_a_ray_with_accessible_origin_and_normalized_direction() {
        let origin = Vec3::new(1.0, 2.0, 3.0);
        let ray = Ray::try_new(origin, Vec3::new(0.0, 3.0, 4.0)).unwrap();

        assert_eq!(ray.origin(), origin);
        assert_vec_approx_eq(ray.direction(), Vec3::new(0.0, 0.6, 0.8));
        assert_approx_eq(ray.direction().length(), 1.0);
    }

    #[test]
    fn rejects_zero_direction() {
        assert_eq!(Ray::try_new(Vec3::ZERO, Vec3::ZERO), None);
    }

    #[test]
    fn rejects_near_zero_direction() {
        assert_eq!(Ray::try_new(Vec3::ZERO, Vec3::new(1.0e-7, 0.0, 0.0)), None);
    }

    #[test]
    fn rejects_non_finite_direction() {
        assert_eq!(
            Ray::try_new(Vec3::ZERO, Vec3::new(f32::INFINITY, 0.0, 0.0)),
            None
        );
        assert_eq!(
            Ray::try_new(Vec3::ZERO, Vec3::new(f32::NAN, 0.0, 0.0)),
            None
        );
    }

    #[test]
    fn evaluates_origin_at_zero() {
        let origin = Vec3::new(2.0, -1.0, 4.0);
        let ray = Ray::try_new(origin, Vec3::new(1.0, 0.0, 0.0)).unwrap();

        assert_eq!(ray.at(0.0), origin);
    }

    #[test]
    fn evaluates_positive_parameter() {
        let ray = Ray::try_new(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 3.0, 4.0)).unwrap();

        assert_vec_approx_eq(ray.at(5.0), Vec3::new(1.0, 5.0, 7.0));
    }

    #[test]
    fn evaluates_negative_parameter_without_clamping() {
        let ray = Ray::try_new(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 3.0, 4.0)).unwrap();

        assert_vec_approx_eq(ray.at(-5.0), Vec3::new(1.0, -1.0, -1.0));
    }
}
