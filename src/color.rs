use std::ops::Mul;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Mul for Color {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self::new(self.r * rhs.r, self.g * rhs.g, self.b * rhs.b)
    }
}

impl Color {
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0);

    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    pub fn scale(self, factor: f32) -> Self {
        Self::new(self.r * factor, self.g * factor, self.b * factor)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
        )
    }

    pub fn to_rgb8(self) -> [u8; 3] {
        [
            channel_to_byte(self.r),
            channel_to_byte(self.g),
            channel_to_byte(self.b),
        ]
    }
}

fn channel_to_byte(value: f32) -> u8 {
    if !value.is_finite() {
        return 0;
    }

    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::Color;

    #[test]
    fn converts_black_and_white_to_rgb_bytes() {
        assert_eq!(Color::BLACK.to_rgb8(), [0, 0, 0]);
        assert_eq!(Color::WHITE.to_rgb8(), [255, 255, 255]);
    }

    #[test]
    fn clamps_channels_to_displayable_range() {
        assert_eq!(Color::new(-0.5, 0.5, 1.5).to_rgb8(), [0, 128, 255]);
    }

    #[test]
    fn converts_non_finite_channels_to_zero() {
        assert_eq!(
            Color::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY).to_rgb8(),
            [0, 0, 0]
        );
    }

    #[test]
    fn multiplies_channels_component_wise() {
        assert_eq!(
            Color::new(0.8, 0.5, 0.25) * Color::new(0.5, 0.4, 1.0),
            Color::new(0.4, 0.2, 0.25)
        );
    }
}
