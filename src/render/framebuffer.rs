use super::Color;

/// CPU-owned row-major pixels kept independent from any presentation API.
#[derive(Clone, Debug, PartialEq)]
pub struct Framebuffer {
    width: usize,
    height: usize,
    pixels: Vec<Color>,
}

impl Framebuffer {
    pub fn try_new(width: usize, height: usize) -> Option<Self> {
        let pixel_count = width.checked_mul(height)?;
        if pixel_count == 0 {
            return None;
        }

        let mut pixels = Vec::new();
        pixels.try_reserve_exact(pixel_count).ok()?;
        pixels.resize(pixel_count, Color::BLACK);

        Some(Self {
            width,
            height,
            pixels,
        })
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub fn pixels(&self) -> &[Color] {
        &self.pixels
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<Color> {
        self.index(x, y).map(|index| self.pixels[index])
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, color: Color) -> bool {
        let Some(index) = self.index(x, y) else {
            return false;
        };

        self.pixels[index] = color;
        true
    }

    pub fn fill(&mut self, color: Color) {
        self.pixels.fill(color);
    }

    fn index(&self, x: usize, y: usize) -> Option<usize> {
        (x < self.width && y < self.height).then_some(y * self.width + x)
    }
}

#[cfg(test)]
mod tests {
    use super::Framebuffer;
    use crate::render::Color;

    #[test]
    fn constructs_valid_black_framebuffer() {
        let framebuffer = Framebuffer::try_new(3, 2).unwrap();

        assert_eq!(framebuffer.width(), 3);
        assert_eq!(framebuffer.height(), 2);
        assert_eq!(framebuffer.pixels(), &[Color::BLACK; 6]);
    }

    #[test]
    fn rejects_zero_or_overflowing_dimensions() {
        assert!(Framebuffer::try_new(0, 2).is_none());
        assert!(Framebuffer::try_new(2, 0).is_none());
        assert!(Framebuffer::try_new(usize::MAX, 2).is_none());
    }

    #[test]
    fn sets_and_reads_pixels_in_row_major_order() {
        let mut framebuffer = Framebuffer::try_new(2, 2).unwrap();
        let red = Color::new(1.0, 0.0, 0.0);
        let green = Color::new(0.0, 1.0, 0.0);

        assert!(framebuffer.set_pixel(1, 0, red));
        assert!(framebuffer.set_pixel(0, 1, green));
        assert_eq!(framebuffer.pixel(1, 0), Some(red));
        assert_eq!(framebuffer.pixel(0, 1), Some(green));
        assert_eq!(
            framebuffer.pixels(),
            &[Color::BLACK, red, green, Color::BLACK]
        );
    }

    #[test]
    fn handles_out_of_bounds_access_without_panicking() {
        let mut framebuffer = Framebuffer::try_new(2, 2).unwrap();

        assert_eq!(framebuffer.pixel(2, 0), None);
        assert_eq!(framebuffer.pixel(0, 2), None);
        assert!(!framebuffer.set_pixel(2, 0, Color::WHITE));
        assert!(!framebuffer.set_pixel(0, 2, Color::WHITE));
    }

    #[test]
    fn fills_all_pixels() {
        let mut framebuffer = Framebuffer::try_new(2, 2).unwrap();
        framebuffer.fill(Color::WHITE);

        assert_eq!(framebuffer.pixels(), &[Color::WHITE; 4]);
    }
}
