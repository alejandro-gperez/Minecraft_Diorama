use super::{Color, tile::Tile};

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

    /// Copies a tile's row-major pixels to the tile's global position, one row slice at a time.
    ///
    /// Returns `false` and writes nothing when the tile does not fit the framebuffer or `pixels`
    /// does not hold exactly `width * height` colors.
    pub fn write_tile(&mut self, tile: Tile, pixels: &[Color]) -> bool {
        if tile.x_end() > self.width
            || tile.y_end() > self.height
            || pixels.len() != tile.width() * tile.height()
        {
            return false;
        }

        let rows = pixels.chunks_exact(tile.width());
        for (y, row) in (tile.y_start()..tile.y_end()).zip(rows) {
            let start = y * self.width + tile.x_start();
            self.pixels[start..start + tile.width()].copy_from_slice(row);
        }
        true
    }

    fn index(&self, x: usize, y: usize) -> Option<usize> {
        (x < self.width && y < self.height).then_some(y * self.width + x)
    }
}

#[cfg(test)]
mod tests {
    use super::Framebuffer;
    use crate::render::{Color, tile::TileGrid};

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

    #[test]
    fn writes_tiles_to_their_global_rows() {
        // 5x3 with 2x2 tiles: the last column and row are partial.
        let grid = TileGrid::try_new(5, 3, 2, 2).unwrap();
        let mut framebuffer = Framebuffer::try_new(5, 3).unwrap();

        for tile in grid.tiles() {
            let pixels: Vec<Color> = (tile.y_start()..tile.y_end())
                .flat_map(|y| (tile.x_start()..tile.x_end()).map(move |x| (x, y)))
                .map(|(x, y)| Color::new(x as f32, y as f32, 0.0))
                .collect();
            assert!(framebuffer.write_tile(tile, &pixels));
        }

        for y in 0..3 {
            for x in 0..5 {
                assert_eq!(
                    framebuffer.pixel(x, y),
                    Some(Color::new(x as f32, y as f32, 0.0))
                );
            }
        }
    }

    #[test]
    fn rejects_tiles_that_do_not_fit_without_writing() {
        let small = Framebuffer::try_new(4, 4).unwrap();
        let tile = TileGrid::try_new(8, 8, 8, 8).unwrap().tile(0).unwrap();
        let mut framebuffer = small.clone();

        assert!(!framebuffer.write_tile(tile, &[Color::WHITE; 64]));
        let fitting = TileGrid::try_new(4, 4, 2, 2).unwrap().tile(3).unwrap();
        assert!(!framebuffer.write_tile(fitting, &[Color::WHITE; 3]));
        assert!(!framebuffer.write_tile(fitting, &[Color::WHITE; 5]));
        assert_eq!(framebuffer, small);
    }
}
