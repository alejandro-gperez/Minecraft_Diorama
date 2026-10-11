//! Rectangular framebuffer work units for the tile renderer.
//!
//! A [`Tile`] only describes a region; it owns no pixels and borrows no framebuffer. A
//! [`TileGrid`] derives every tile from the framebuffer and tile dimensions by arithmetic, so no
//! tile list is ever stored.
//!
//! Conventions:
//! - A tile is half-open: it covers `x_start <= x < x_end` and `y_start <= y < y_end`, in global
//!   framebuffer pixel coordinates.
//! - Tile indices are row-major: `tile_x = index % columns`, `tile_y = index / columns`. Index `0`
//!   is the upper-left tile; indices advance left to right, then top to bottom. Dynamic work
//!   distribution can hand out these indices unchanged.
//! - Tiles on the last column and row are clipped to the framebuffer, so dimensions need not be
//!   multiples of the tile size.

/// Default tile width in pixels.
pub const TILE_WIDTH: usize = 16;
/// Default tile height in pixels.
pub const TILE_HEIGHT: usize = 16;

/// A half-open rectangular region of the framebuffer. Small, `Copy`, and free of pixel storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    x_start: usize,
    y_start: usize,
    x_end: usize,
    y_end: usize,
}

impl Tile {
    pub const fn x_start(self) -> usize {
        self.x_start
    }

    pub const fn y_start(self) -> usize {
        self.y_start
    }

    /// First column past the tile.
    pub const fn x_end(self) -> usize {
        self.x_end
    }

    /// First row past the tile.
    pub const fn y_end(self) -> usize {
        self.y_end
    }

    pub const fn width(self) -> usize {
        self.x_end - self.x_start
    }

    pub const fn height(self) -> usize {
        self.y_end - self.y_start
    }
}

/// Why a [`TileGrid`] could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileGridError {
    ZeroFramebufferWidth,
    ZeroFramebufferHeight,
    ZeroTileWidth,
    ZeroTileHeight,
    /// The tile count does not fit `usize`.
    TileCountOverflow,
}

/// The row-major partition of a framebuffer into tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileGrid {
    width: usize,
    height: usize,
    tile_width: usize,
    tile_height: usize,
    columns: usize,
    rows: usize,
    count: usize,
}

impl TileGrid {
    pub fn try_new(
        width: usize,
        height: usize,
        tile_width: usize,
        tile_height: usize,
    ) -> Result<Self, TileGridError> {
        if width == 0 {
            return Err(TileGridError::ZeroFramebufferWidth);
        }
        if height == 0 {
            return Err(TileGridError::ZeroFramebufferHeight);
        }
        if tile_width == 0 {
            return Err(TileGridError::ZeroTileWidth);
        }
        if tile_height == 0 {
            return Err(TileGridError::ZeroTileHeight);
        }

        let columns = width.div_ceil(tile_width);
        let rows = height.div_ceil(tile_height);
        let count = columns
            .checked_mul(rows)
            .ok_or(TileGridError::TileCountOverflow)?;

        Ok(Self {
            width,
            height,
            tile_width,
            tile_height,
            columns,
            rows,
            count,
        })
    }

    pub const fn columns(&self) -> usize {
        self.columns
    }

    pub const fn rows(&self) -> usize {
        self.rows
    }

    pub const fn tile_count(&self) -> usize {
        self.count
    }

    /// The tile at row-major `index`, or `None` when `index >= tile_count()`.
    pub const fn tile(&self, index: usize) -> Option<Tile> {
        if index >= self.count {
            return None;
        }

        let x_start = (index % self.columns) * self.tile_width;
        let y_start = (index / self.columns) * self.tile_height;
        // `x_start < width`, so these differences are positive and the sums cannot overflow even
        // when the tile size exceeds the framebuffer.
        let x_end = x_start + min(self.width - x_start, self.tile_width);
        let y_end = y_start + min(self.height - y_start, self.tile_height);

        Some(Tile {
            x_start,
            y_start,
            x_end,
            y_end,
        })
    }

    /// Every tile in index order, without allocating.
    pub const fn tiles(&self) -> Tiles {
        Tiles {
            grid: *self,
            next: 0,
        }
    }
}

/// Iterator over a [`TileGrid`] in row-major index order.
#[derive(Clone, Debug)]
pub struct Tiles {
    grid: TileGrid,
    next: usize,
}

impl Iterator for Tiles {
    type Item = Tile;

    fn next(&mut self) -> Option<Tile> {
        let tile = self.grid.tile(self.next)?;
        self.next += 1;
        Some(tile)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.grid.count - self.next.min(self.grid.count);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Tiles {}

const fn min(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

#[cfg(test)]
mod tests {
    use super::{TILE_HEIGHT, TILE_WIDTH, Tile, TileGrid, TileGridError};

    fn grid(width: usize, height: usize, tile_width: usize, tile_height: usize) -> TileGrid {
        TileGrid::try_new(width, height, tile_width, tile_height).unwrap()
    }

    /// Counts how many tiles cover each pixel of a `width` × `height` framebuffer.
    fn coverage(grid: &TileGrid, width: usize, height: usize) -> Vec<u32> {
        let mut counts = vec![0_u32; width * height];
        for tile in grid.tiles() {
            assert!(tile.x_start() < tile.x_end() && tile.x_end() <= width);
            assert!(tile.y_start() < tile.y_end() && tile.y_end() <= height);
            for y in tile.y_start()..tile.y_end() {
                for x in tile.x_start()..tile.x_end() {
                    counts[y * width + x] += 1;
                }
            }
        }
        counts
    }

    #[test]
    fn accepts_valid_tile_dimensions() {
        let grid = grid(320, 180, 16, 16);

        assert_eq!(grid.columns(), 20);
        assert_eq!(grid.rows(), 12);
    }

    #[test]
    fn rejects_zero_tile_dimensions() {
        assert_eq!(
            TileGrid::try_new(320, 180, 0, 16),
            Err(TileGridError::ZeroTileWidth)
        );
        assert_eq!(
            TileGrid::try_new(320, 180, 16, 0),
            Err(TileGridError::ZeroTileHeight)
        );
    }

    #[test]
    fn rejects_zero_framebuffer_dimensions() {
        assert_eq!(
            TileGrid::try_new(0, 180, 16, 16),
            Err(TileGridError::ZeroFramebufferWidth)
        );
        assert_eq!(
            TileGrid::try_new(320, 0, 16, 16),
            Err(TileGridError::ZeroFramebufferHeight)
        );
    }

    #[test]
    fn rejects_out_of_range_tile_indices() {
        let grid = grid(320, 180, 16, 16);

        assert!(grid.tile(239).is_some());
        assert_eq!(grid.tile(240), None);
        assert_eq!(grid.tile(usize::MAX), None);
    }

    #[test]
    fn tiles_use_half_open_boundaries() {
        let grid = grid(40, 20, 16, 16);

        assert_eq!(
            grid.tile(0),
            Some(Tile {
                x_start: 0,
                y_start: 0,
                x_end: 16,
                y_end: 16
            })
        );
        assert_eq!(
            grid.tile(1),
            Some(Tile {
                x_start: 16,
                y_start: 0,
                x_end: 32,
                y_end: 16
            })
        );
        // The next tile starts exactly where the previous one ends.
        assert_eq!(
            grid.tile(0).unwrap().x_end(),
            grid.tile(1).unwrap().x_start()
        );
        assert_eq!(
            grid.tile(0).unwrap().y_end(),
            grid.tile(3).unwrap().y_start()
        );
    }

    #[test]
    fn construction_does_not_overflow() {
        // A tile larger than the framebuffer clips instead of overflowing.
        let huge = grid(5, 7, usize::MAX, usize::MAX);
        assert_eq!(huge.tile_count(), 1);
        assert_eq!(huge.tile(0).unwrap().width(), 5);
        assert_eq!(huge.tile(0).unwrap().height(), 7);

        assert_eq!(
            TileGrid::try_new(usize::MAX, usize::MAX, 1, 1),
            Err(TileGridError::TileCountOverflow)
        );
        // A single column of the maximum height still fits.
        let tall = grid(1, usize::MAX, 1, 1);
        assert_eq!(tall.tile_count(), usize::MAX);
        assert_eq!(tall.tile(usize::MAX - 1).unwrap().y_end(), usize::MAX);
    }

    #[test]
    fn tile_order_is_row_major_from_the_upper_left() {
        let grid = grid(50, 40, 16, 16);
        let order: Vec<(usize, usize)> = grid
            .tiles()
            .map(|tile| (tile.x_start(), tile.y_start()))
            .collect();

        assert_eq!(grid.columns(), 4);
        assert_eq!(order.len(), 12);
        assert_eq!(order[0], (0, 0));
        assert_eq!(order[1], (16, 0));
        assert_eq!(order[3], (48, 0));
        assert_eq!(order[4], (0, 16));
        assert_eq!(order[11], (48, 32));
        // Iteration agrees with arithmetic indexing and is repeatable.
        for (index, tile) in grid.tiles().enumerate() {
            assert_eq!(Some(tile), grid.tile(index));
        }
        assert!(grid.tiles().eq(grid.tiles()));
        assert_eq!(grid.tiles().len(), 12);
    }

    #[test]
    fn development_resolution_yields_240_tiles_with_partial_last_row() {
        let grid = grid(320, 180, TILE_WIDTH, TILE_HEIGHT);

        assert_eq!((TILE_WIDTH, TILE_HEIGHT), (16, 16));
        assert_eq!(grid.columns(), 20);
        assert_eq!(grid.rows(), 12);
        assert_eq!(grid.tile_count(), 240);
        // 180 = 11 * 16 + 4: the final row is four pixels high, the columns divide evenly.
        let last_row_first = grid.tile(220).unwrap();
        assert_eq!(
            (last_row_first.y_start(), last_row_first.y_end()),
            (176, 180)
        );
        assert_eq!(last_row_first.height(), 4);
        assert_eq!(grid.tile(239).unwrap().width(), 16);
        assert_eq!(grid.tile(0).unwrap().height(), 16);
    }

    #[test]
    fn partial_last_column_is_clipped() {
        let grid = grid(17, 19, 16, 16);

        assert_eq!(grid.tile_count(), 4);
        assert_eq!(grid.tile(1).unwrap().width(), 1);
        assert_eq!(grid.tile(2).unwrap().height(), 3);
        let corner = grid.tile(3).unwrap();
        assert_eq!((corner.width(), corner.height()), (1, 3));
    }

    #[test]
    fn every_pixel_is_covered_exactly_once() {
        let cases = [
            (320, 180, 16, 16),
            (16, 16, 16, 16),
            (1, 1, 16, 16),
            (5, 3, 16, 16),
            (33, 32, 16, 16),
            (32, 33, 16, 16),
            (17, 19, 16, 16),
            (17, 19, 1, 1),
            (17, 19, 5, 7),
            (17, 19, 32, 8),
            (64, 48, 32, 16),
        ];

        for (width, height, tile_width, tile_height) in cases {
            let grid = grid(width, height, tile_width, tile_height);
            assert!(
                coverage(&grid, width, height)
                    .iter()
                    .all(|&count| count == 1),
                "{width}x{height} with {tile_width}x{tile_height} tiles",
            );
        }
    }
}
