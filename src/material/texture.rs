use crate::color::Color;

/// CPU-owned, row-major surface texels independent from the Raylib presentation texture.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    width: usize,
    height: usize,
    texels: Vec<Color>,
}

impl Texture {
    pub fn try_new(width: usize, height: usize, texels: Vec<Color>) -> Option<Self> {
        let expected_len = width.checked_mul(height)?;
        if width == 0 || height == 0 || texels.len() != expected_len {
            return None;
        }

        Some(Self {
            width,
            height,
            texels,
        })
    }

    pub fn solid(color: Color) -> Self {
        Self {
            width: 1,
            height: 1,
            texels: vec![color],
        }
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub fn texel(&self, x: usize, y: usize) -> Option<Color> {
        let index = y.checked_mul(self.width)?.checked_add(x)?;
        (x < self.width && y < self.height).then(|| self.texels[index])
    }

    /// Samples with nearest-neighbor filtering using left-to-right `u` and top-to-bottom `v`.
    ///
    /// Finite coordinates are clamped to `[0, 1]`; exact `1.0` selects the final row or column.
    /// Non-finite coordinates are rejected.
    pub fn sample_nearest(&self, u: f32, v: f32) -> Option<Color> {
        if !u.is_finite() || !v.is_finite() {
            return None;
        }

        let x = (u.clamp(0.0, 1.0) * (self.width - 1) as f32).round() as usize;
        let y = (v.clamp(0.0, 1.0) * (self.height - 1) as f32).round() as usize;
        self.texel(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::Texture;
    use crate::color::Color;

    const RED: Color = Color::new(1.0, 0.0, 0.0);
    const GREEN: Color = Color::new(0.0, 1.0, 0.0);
    const BLUE: Color = Color::new(0.0, 0.0, 1.0);
    const WHITE: Color = Color::WHITE;

    fn texture_2x2() -> Texture {
        Texture::try_new(2, 2, vec![RED, GREEN, BLUE, WHITE]).unwrap()
    }

    #[test]
    fn constructs_valid_texture_and_exposes_dimensions() {
        let texture = texture_2x2();

        assert_eq!(texture.width(), 2);
        assert_eq!(texture.height(), 2);
    }

    #[test]
    fn rejects_zero_dimensions() {
        assert_eq!(Texture::try_new(0, 1, Vec::new()), None);
        assert_eq!(Texture::try_new(1, 0, Vec::new()), None);
    }

    #[test]
    fn rejects_texel_count_mismatch() {
        assert_eq!(Texture::try_new(2, 2, vec![RED; 3]), None);
        assert_eq!(Texture::try_new(2, 2, vec![RED; 5]), None);
    }

    #[test]
    fn accesses_texels_in_row_major_order() {
        let texture = texture_2x2();

        assert_eq!(texture.texel(0, 0), Some(RED));
        assert_eq!(texture.texel(1, 0), Some(GREEN));
        assert_eq!(texture.texel(0, 1), Some(BLUE));
        assert_eq!(texture.texel(1, 1), Some(WHITE));
        assert_eq!(texture.texel(2, 0), None);
        assert_eq!(texture.texel(0, 2), None);
    }

    #[test]
    fn samples_nearest_texel() {
        let texture = texture_2x2();

        assert_eq!(texture.sample_nearest(0.1, 0.1), Some(RED));
        assert_eq!(texture.sample_nearest(0.9, 0.1), Some(GREEN));
        assert_eq!(texture.sample_nearest(0.1, 0.9), Some(BLUE));
        assert_eq!(texture.sample_nearest(0.9, 0.9), Some(WHITE));
    }

    #[test]
    fn samples_exact_normalized_edges_without_overflow() {
        let texture = texture_2x2();

        assert_eq!(texture.sample_nearest(1.0, 0.0), Some(GREEN));
        assert_eq!(texture.sample_nearest(0.0, 1.0), Some(BLUE));
        assert_eq!(texture.sample_nearest(1.0, 1.0), Some(WHITE));
    }

    #[test]
    fn clamps_out_of_range_coordinates() {
        let texture = texture_2x2();

        assert_eq!(texture.sample_nearest(-2.0, -1.0), Some(RED));
        assert_eq!(texture.sample_nearest(2.0, 3.0), Some(WHITE));
    }

    #[test]
    fn rejects_non_finite_coordinates() {
        let texture = texture_2x2();

        assert_eq!(texture.sample_nearest(f32::NAN, 0.5), None);
        assert_eq!(texture.sample_nearest(0.5, f32::INFINITY), None);
        assert_eq!(texture.sample_nearest(f32::NEG_INFINITY, 0.5), None);
    }

    #[test]
    fn creates_solid_single_texel_texture() {
        let texture = Texture::solid(GREEN);

        assert_eq!(texture.width(), 1);
        assert_eq!(texture.height(), 1);
        assert_eq!(texture.sample_nearest(0.0, 0.0), Some(GREEN));
        assert_eq!(texture.sample_nearest(1.0, 1.0), Some(GREEN));
    }
}
