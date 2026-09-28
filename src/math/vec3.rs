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
