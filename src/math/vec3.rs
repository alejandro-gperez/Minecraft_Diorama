use std::ops::{Add, Div, Mul, Neg, Sub};

const NORMALIZATION_EPSILON: f32 = 1.0e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns a unit vector, or `None` for non-finite and numerically degenerate input.
    ///
    /// The explicit failure keeps zero-length directions from turning into NaNs in camera,
    /// ray, and intersection calculations.
    pub fn try_normalized(self) -> Option<Self> {
        let length_squared = self.length_squared();
        let minimum_length_squared = NORMALIZATION_EPSILON * NORMALIZATION_EPSILON;

        if !length_squared.is_finite() || length_squared <= minimum_length_squared {
            return None;
        }

        let inverse_length = length_squared.sqrt().recip();
        let normalized = self * inverse_length;

        normalized.is_finite().then_some(normalized)
    }

    /// Mirrors `self` about the surface described by the unit `normal`: `self - 2(self·n)n`.
    ///
    /// For a unit `self` and unit `normal` the result is unit length up to rounding; callers that
    /// need the project's normalization guarantee should pass it through `Ray::try_new`.
    pub fn reflect(self, normal: Self) -> Self {
        self - normal * (2.0 * self.dot(normal))
    }

    /// Bends the unit direction `self` through an interface by Snell's law.
    ///
    /// `normal` is the unit interface normal oriented against `self` (`self·normal <= 0`), and
    /// `eta = eta_i / eta_t` is the ratio of the incident to the transmitted index of refraction.
    /// With `cos_i = -self·normal` and `sin²_t = eta²(1 - cos_i²)`, the transmitted direction is
    /// `eta * self + (eta * cos_i - cos_t) * normal`.
    ///
    /// Returns `None` on total internal reflection (`sin²_t > 1`), for a normal facing along
    /// `self`, for a non-finite or non-positive `eta`, or when the result cannot be normalized.
    pub fn refract(self, normal: Self, eta: f32) -> Option<Self> {
        if !eta.is_finite() || eta <= 0.0 {
            return None;
        }

        let cos_i = -self.dot(normal);
        if !cos_i.is_finite() || cos_i < 0.0 {
            return None;
        }

        let sin_squared_t = eta * eta * (1.0 - cos_i * cos_i);
        if sin_squared_t > 1.0 {
            // Total internal reflection: no transmitted direction exists.
            return None;
        }

        let cos_t = (1.0 - sin_squared_t).sqrt();
        (self * eta + normal * (eta * cos_i - cos_t)).try_normalized()
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Neg for Vec3 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl Div<f32> for Vec3 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        assert!(
            rhs.is_finite() && rhs != 0.0,
            "Vec3 division requires a finite, non-zero scalar"
        );
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::Vec3;

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
    fn reflect_inverts_perpendicular_incidence() {
        let normal = Vec3::new(0.0, 1.0, 0.0);

        assert_vec_approx_eq(Vec3::new(0.0, -1.0, 0.0).reflect(normal), normal);
    }

    #[test]
    fn reflect_mirrors_angled_incidence_about_the_normal() {
        let incident = Vec3::new(1.0, -1.0, 0.0).try_normalized().unwrap();
        let reflected = incident.reflect(Vec3::new(0.0, 1.0, 0.0));
        let expected = Vec3::new(1.0, 1.0, 0.0).try_normalized().unwrap();

        assert_vec_approx_eq(reflected, expected);
        assert_approx_eq(reflected.length(), 1.0);
        assert!(reflected.is_finite());
    }

    #[test]
    fn reflect_is_symmetric_under_normal_sign_and_involutive() {
        let incident = Vec3::new(0.3, -0.8, 0.5).try_normalized().unwrap();
        let normal = Vec3::new(0.0, 1.0, 0.0);
        let reflected = incident.reflect(normal);

        assert_vec_approx_eq(reflected, incident.reflect(-normal));
        assert_vec_approx_eq(reflected.reflect(normal), incident);
        // The tangential component is preserved and the normal component flips.
        assert_approx_eq(reflected.x, incident.x);
        assert_approx_eq(reflected.z, incident.z);
        assert_approx_eq(reflected.y, -incident.y);
    }

    const AIR_TO_GLASS: f32 = 1.0 / 1.5;
    const GLASS_TO_AIR: f32 = 1.5;
    const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);

    /// Sine of the angle between a unit direction and the unit normal of a surface.
    fn sine_to_normal(direction: Vec3, normal: Vec3) -> f32 {
        direction.cross(normal).length()
    }

    fn assert_unit_and_finite(vector: Vec3) {
        assert!(vector.is_finite());
        assert_approx_eq(vector.length(), 1.0);
    }

    #[test]
    fn refract_passes_normal_incidence_straight_through_in_both_directions() {
        let down = Vec3::new(0.0, -1.0, 0.0);

        for eta in [AIR_TO_GLASS, GLASS_TO_AIR] {
            let transmitted = down.refract(UP, eta).unwrap();
            assert_vec_approx_eq(transmitted, down);
            assert_unit_and_finite(transmitted);
        }
    }

    #[test]
    fn refract_bends_toward_the_normal_entering_denser_medium() {
        // 45 degrees in air: sin_t = sin(45°) / 1.5 = 0.4714045.
        let incident = Vec3::new(1.0, -1.0, 0.0).try_normalized().unwrap();
        let transmitted = incident.refract(UP, AIR_TO_GLASS).unwrap();
        let sin_t = std::f32::consts::FRAC_1_SQRT_2 / 1.5;

        assert_vec_approx_eq(
            transmitted,
            Vec3::new(sin_t, -(1.0 - sin_t * sin_t).sqrt(), 0.0),
        );
        assert!(sine_to_normal(transmitted, UP) < sine_to_normal(incident, UP));
        assert_unit_and_finite(transmitted);
    }

    #[test]
    fn refract_bends_away_from_the_normal_leaving_denser_medium() {
        // sin_i = 0.4 inside glass leaves into air at sin_t = 1.5 * 0.4 = 0.6, i.e. (0.6, -0.8).
        let incident = Vec3::new(0.4, -(0.84_f32).sqrt(), 0.0);
        let transmitted = incident.refract(UP, GLASS_TO_AIR).unwrap();

        assert_vec_approx_eq(transmitted, Vec3::new(0.6, -0.8, 0.0));
        assert!(sine_to_normal(transmitted, UP) > sine_to_normal(incident, UP));
        assert_unit_and_finite(transmitted);
    }

    #[test]
    fn refract_satisfies_snells_law_in_the_plane_of_incidence() {
        let normal = Vec3::new(1.0, 2.0, 3.0).try_normalized().unwrap();
        let tangent = Vec3::new(3.0, 0.0, -1.0).try_normalized().unwrap();
        assert_approx_eq(tangent.dot(normal), 0.0);

        for (n_i, n_t) in [(1.0, 1.5), (1.5, 1.0), (1.0, 1.33), (1.2, 1.2)] {
            for sin_i in [0.0_f32, 0.1, 0.35, 0.6] {
                let cos_i = (1.0 - sin_i * sin_i).sqrt();
                let incident = tangent * sin_i - normal * cos_i;
                let transmitted = incident.refract(normal, n_i / n_t).unwrap();

                assert_unit_and_finite(transmitted);
                assert!(
                    (n_i * sine_to_normal(incident, normal)
                        - n_t * sine_to_normal(transmitted, normal))
                    .abs()
                        <= 1.0e-5
                );
                // Coplanar with the incident ray and normal, still crossing the interface, and
                // with its tangential component on the same side.
                assert_approx_eq(incident.cross(normal).dot(transmitted), 0.0);
                assert!(transmitted.dot(normal) < 0.0);
                assert!(transmitted.dot(tangent) >= -EPSILON);
            }
        }
    }

    #[test]
    fn refract_reports_total_internal_reflection_beyond_critical_angle() {
        // Glass to air: the critical angle has sin_c = 1 / 1.5.
        let incident = Vec3::new(1.0, -1.0, 0.0).try_normalized().unwrap();
        assert_eq!(incident.refract(UP, GLASS_TO_AIR), None);

        let at = |sin_i: f32| Vec3::new(sin_i, -(1.0 - sin_i * sin_i).sqrt(), 0.0);
        let critical = 1.0 / 1.5;
        assert!(at(critical - 1.0e-3).refract(UP, GLASS_TO_AIR).is_some());
        assert_eq!(at(critical + 1.0e-3).refract(UP, GLASS_TO_AIR), None);
        // Grazing incidence from the dense side is always totally reflected.
        assert_eq!(Vec3::new(1.0, 0.0, 0.0).refract(UP, GLASS_TO_AIR), None);
    }

    #[test]
    fn refract_with_matched_indices_is_the_identity() {
        let incident = Vec3::new(0.3, -0.8, 0.5).try_normalized().unwrap();

        assert_vec_approx_eq(incident.refract(UP, 1.0).unwrap(), incident);
    }

    #[test]
    fn refract_rejects_invalid_input() {
        let down = Vec3::new(0.0, -1.0, 0.0);

        // The normal must be oriented against the incident direction.
        assert_eq!(down.refract(-UP, AIR_TO_GLASS), None);
        for eta in [0.0, -1.5, f32::NAN, f32::INFINITY] {
            assert_eq!(down.refract(UP, eta), None);
        }
        assert_eq!(
            Vec3::new(f32::NAN, -1.0, 0.0).refract(UP, AIR_TO_GLASS),
            None
        );
    }

    #[test]
    fn constructs_with_public_components() {
        let vector = Vec3::new(1.0, -2.5, 3.25);

        assert_eq!(vector.x, 1.0);
        assert_eq!(vector.y, -2.5);
        assert_eq!(vector.z, 3.25);
        assert_eq!(Vec3::ZERO, Vec3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn supports_basic_vector_operators() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);

        assert_eq!(a + b, Vec3::new(5.0, 7.0, 9.0));
        assert_eq!(b - a, Vec3::new(3.0, 3.0, 3.0));
        assert_eq!(-a, Vec3::new(-1.0, -2.0, -3.0));
        assert_eq!(a * 2.0, Vec3::new(2.0, 4.0, 6.0));
        assert_eq!(b / 2.0, Vec3::new(2.0, 2.5, 3.0));
    }

    #[test]
    fn calculates_dot_product() {
        assert_eq!(
            Vec3::new(1.0, 2.0, 3.0).dot(Vec3::new(4.0, -5.0, 6.0)),
            12.0
        );
    }

    #[test]
    fn calculates_cross_product_orientation() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);

        assert_eq!(x.cross(y), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(y.cross(x), Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn calculates_squared_length_and_length() {
        let vector = Vec3::new(2.0, 3.0, 6.0);

        assert_eq!(vector.length_squared(), 49.0);
        assert_eq!(vector.length(), 7.0);
    }

    #[test]
    fn normalizes_non_degenerate_vectors() {
        let normalized = Vec3::new(3.0, 0.0, 4.0).try_normalized().unwrap();

        assert_vec_approx_eq(normalized, Vec3::new(0.6, 0.0, 0.8));
        assert_approx_eq(normalized.length(), 1.0);
    }

    #[test]
    fn rejects_zero_and_vectors_below_normalization_threshold() {
        assert_eq!(Vec3::ZERO.try_normalized(), None);
        assert_eq!(Vec3::new(1.0e-7, 0.0, 0.0).try_normalized(), None);
    }

    #[test]
    fn normalizes_vectors_above_normalization_threshold() {
        assert_eq!(
            Vec3::new(1.0e-4, 0.0, 0.0).try_normalized(),
            Some(Vec3::new(1.0, 0.0, 0.0))
        );
    }

    #[test]
    fn rejects_non_finite_normalization_input() {
        assert_eq!(Vec3::new(f32::INFINITY, 0.0, 0.0).try_normalized(), None);
        assert_eq!(Vec3::new(f32::NAN, 0.0, 0.0).try_normalized(), None);
    }
}
