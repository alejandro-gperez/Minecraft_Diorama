//! Dynamic tile scheduling over scoped CPU worker threads.
//!
//! One render creates a fixed set of workers inside `std::thread::scope`; all of them are joined
//! before the render returns. There is no persistent pool.
//!
//! Protocol:
//! - Work assignment: workers claim row-major tile indices from one shared `AtomicUsize` with
//!   `fetch_add`, so every index is handed out exactly once. The counter only assigns work; it
//!   never publishes pixels, so `Ordering::Relaxed` suffices.
//! - Pixel ownership: the one framebuffer sits behind a `Mutex`. Each worker owns a single
//!   tile-sized buffer, allocated once and reused for every tile it claims. It traces a tile into
//!   that buffer without holding any lock, then locks the framebuffer only long enough for
//!   `Framebuffer::write_tile` to copy the finished tile into its global position. Tiles never
//!   overlap and nothing is blended, so the order in which workers take the lock cannot change the
//!   image.
//! - Failure: a worker whose tile fails stops and returns that tile's index and error; the shared
//!   stop flag keeps the others from claiming further tiles. Claims are handed out in increasing
//!   order and a claimed tile is always finished, so every tile before a failed one is still
//!   traced. After the join, the lowest-index failure is returned: the same error a sequential
//!   render would return. A panicking worker is re-raised on the calling thread after the join,
//!   so a panic never turns into `Ok`.

use std::{
    io,
    num::NonZeroUsize,
    panic,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use super::{
    Color, Framebuffer, RenderError,
    tile::{Tile, TileGrid},
};

/// Most worker threads one render may use, whatever is requested.
///
/// The development frame has 920 tiles; more threads than this only add creation cost, and an
/// unbounded request must never spawn thousands of threads.
pub const MAX_WORKERS: usize = 64;

/// Worker threads for an automatic render: the system's available parallelism, or `1` when it
/// cannot be determined.
pub fn default_worker_count() -> usize {
    worker_count_or_fallback(thread::available_parallelism())
}

fn worker_count_or_fallback(parallelism: io::Result<NonZeroUsize>) -> usize {
    parallelism.map_or(1, NonZeroUsize::get)
}

/// Threads actually used for `requested` workers over `tile_count` tiles: never more than there
/// are tiles or than `MAX_WORKERS`. Zero workers is a configuration error.
pub(crate) fn effective_worker_count(
    requested: usize,
    tile_count: usize,
) -> Result<usize, RenderError> {
    if requested == 0 {
        return Err(RenderError::InvalidWorkerCount);
    }

    // A `TileGrid` always has at least one tile, so the result is at least one.
    Ok(requested.min(tile_count).min(MAX_WORKERS))
}

/// A tile whose tracing failed, identified by its row-major index.
type TileFailure = (usize, RenderError);

/// Shared, read-only state every worker of one render borrows.
struct WorkQueue<'a, F> {
    grid: TileGrid,
    next_tile: &'a AtomicUsize,
    stop: &'a AtomicBool,
    trace_tile: &'a F,
}

/// Renders every tile of `grid` into `framebuffer` on `workers` scoped threads.
///
/// `trace_tile` fills a tile-local, row-major buffer of exactly `tile.width() * tile.height()`
/// pixels for a tile given in global coordinates. `grid` must partition `framebuffer`, and
/// `workers` must already be bounded by `effective_worker_count`. Returns only after every worker
/// has been joined: `Ok` means every tile was traced and written exactly once.
pub(crate) fn render_tiles_parallel<F>(
    grid: TileGrid,
    workers: usize,
    framebuffer: &mut Framebuffer,
    trace_tile: F,
) -> Result<(), RenderError>
where
    F: Fn(Tile, &mut [Color]) -> Result<(), RenderError> + Sync,
{
    // Tile 0 is never clipped more than any other tile, so its area bounds every tile.
    let buffer_len = grid.tile(0).map_or(0, |tile| tile.width() * tile.height());
    let next_tile = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let queue = WorkQueue {
        grid,
        next_tile: &next_tile,
        stop: &stop,
        trace_tile: &trace_tile,
    };
    let framebuffer = Mutex::new(framebuffer);

    let failures: Vec<Option<TileFailure>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| scope.spawn(|| run_worker(&queue, &framebuffer, buffer_len)))
            .collect();

        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|payload| panic::resume_unwind(payload))
            })
            .collect()
    });

    // Each worker stops at its first failure, so the minimum over workers is the lowest index.
    match failures
        .into_iter()
        .flatten()
        .min_by_key(|&(index, _)| index)
    {
        Some((_, error)) => Err(error),
        None => Ok(()),
    }
}

/// One worker: claim a tile, trace it into the worker's own buffer, copy it into the framebuffer,
/// repeat. Returns the failure that made it stop, if any.
fn run_worker<F>(
    queue: &WorkQueue<'_, F>,
    framebuffer: &Mutex<&mut Framebuffer>,
    buffer_len: usize,
) -> Option<TileFailure>
where
    F: Fn(Tile, &mut [Color]) -> Result<(), RenderError>,
{
    let mut pixels = vec![Color::BLACK; buffer_len];

    while !queue.stop.load(Ordering::Relaxed) {
        let tile_index = queue.next_tile.fetch_add(1, Ordering::Relaxed);
        let tile = queue.grid.tile(tile_index)?;
        let area = tile.width() * tile.height();

        if let Err(error) = (queue.trace_tile)(tile, &mut pixels[..area]) {
            queue.stop.store(true, Ordering::Relaxed);
            return Some((tile_index, error));
        }

        // The lock covers only the copy. A poisoned lock means another worker panicked, and that
        // panic is re-raised by the join; this worker has nothing useful left to do.
        let written = framebuffer
            .lock()
            .expect("the framebuffer lock is poisoned by a panicking worker")
            .write_tile(tile, &pixels[..area]);
        assert!(
            written,
            "a scheduled tile must lie inside the framebuffer it partitions",
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        num::NonZeroUsize,
        panic::{self, AssertUnwindSafe},
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        thread::{self, ThreadId},
    };

    use super::{
        MAX_WORKERS, default_worker_count, effective_worker_count, render_tiles_parallel,
        worker_count_or_fallback,
    };
    use crate::render::{
        Color, Framebuffer, RenderError,
        tile::{Tile, TileGrid},
    };

    /// Frame and tile sizes with full tiles, partial last rows and columns, and both at once.
    const GRIDS: [(usize, usize, usize, usize); 5] = [
        (320, 180, 16, 16),
        (37, 21, 8, 8),
        (17, 19, 16, 16),
        (64, 36, 32, 16),
        (23, 29, 5, 7),
    ];
    const WORKER_COUNTS: [usize; 6] = [2, 3, 4, 7, 16, MAX_WORKERS];

    /// A color unique to the global pixel, so a tile copied to the wrong place is visible.
    fn coordinate_color(x: usize, y: usize) -> Color {
        Color::new(x as f32, y as f32, 0.5)
    }

    fn fill_with_coordinates(tile: Tile, pixels: &mut [Color]) {
        let rows = pixels.chunks_exact_mut(tile.width());
        for (y, row) in (tile.y_start()..tile.y_end()).zip(rows) {
            for (x, pixel) in (tile.x_start()..tile.x_end()).zip(row) {
                *pixel = coordinate_color(x, y);
            }
        }
    }

    fn assert_holds_coordinates(framebuffer: &Framebuffer) {
        for y in 0..framebuffer.height() {
            for x in 0..framebuffer.width() {
                assert_eq!(framebuffer.pixel(x, y), Some(coordinate_color(x, y)));
            }
        }
    }

    fn setup(
        width: usize,
        height: usize,
        tile_width: usize,
        tile_height: usize,
    ) -> (TileGrid, Framebuffer) {
        (
            TileGrid::try_new(width, height, tile_width, tile_height).unwrap(),
            Framebuffer::try_new(width, height).unwrap(),
        )
    }

    #[test]
    fn requested_worker_counts_within_the_tile_count_are_used_unchanged() {
        for workers in [1, 2, 4, 7] {
            assert_eq!(effective_worker_count(workers, 240), Ok(workers));
        }
    }

    #[test]
    fn zero_workers_is_rejected() {
        assert_eq!(
            effective_worker_count(0, 240),
            Err(RenderError::InvalidWorkerCount)
        );
    }

    #[test]
    fn worker_count_is_bounded_by_tile_count_and_maximum() {
        // More workers than tiles: one worker per tile, and a single tile renders sequentially.
        assert_eq!(effective_worker_count(8, 2), Ok(2));
        assert_eq!(effective_worker_count(8, 1), Ok(1));
        assert_eq!(effective_worker_count(usize::MAX, 240), Ok(MAX_WORKERS));
        assert_eq!(
            effective_worker_count(MAX_WORKERS + 1, 10_000),
            Ok(MAX_WORKERS)
        );
    }

    #[test]
    fn unavailable_parallelism_falls_back_to_one_worker() {
        assert_eq!(
            worker_count_or_fallback(Err(io::Error::other("unknown"))),
            1
        );
        assert_eq!(
            worker_count_or_fallback(Ok(NonZeroUsize::new(6).unwrap())),
            6
        );
        assert!(default_worker_count() >= 1);
    }

    #[test]
    fn every_tile_is_claimed_exactly_once_and_written_to_its_global_position() {
        for (width, height, tile_width, tile_height) in GRIDS {
            for workers in WORKER_COUNTS {
                let (grid, mut framebuffer) = setup(width, height, tile_width, tile_height);
                let workers = effective_worker_count(workers, grid.tile_count()).unwrap();
                let claims: Vec<AtomicUsize> = (0..grid.tile_count())
                    .map(|_| AtomicUsize::new(0))
                    .collect();

                render_tiles_parallel(grid, workers, &mut framebuffer, |tile, pixels| {
                    assert_eq!(pixels.len(), tile.width() * tile.height());
                    let index = grid
                        .tiles()
                        .position(|candidate| candidate == tile)
                        .unwrap();
                    claims[index].fetch_add(1, Ordering::Relaxed);
                    fill_with_coordinates(tile, pixels);
                    Ok(())
                })
                .unwrap();

                let case =
                    format!("{width}x{height}, {tile_width}x{tile_height}, {workers} workers");
                assert!(
                    claims
                        .iter()
                        .all(|count| count.load(Ordering::Relaxed) == 1),
                    "{case}"
                );
                assert_holds_coordinates(&framebuffer);
            }
        }
    }

    #[test]
    fn tiles_are_traced_on_worker_threads_only() {
        let (grid, mut framebuffer) = setup(64, 36, 8, 8);
        let caller = thread::current().id();
        let tracing_threads = Mutex::new(Vec::new());

        render_tiles_parallel(grid, 4, &mut framebuffer, |tile, pixels| {
            let mut threads = tracing_threads.lock().unwrap();
            if !threads.contains(&thread::current().id()) {
                threads.push(thread::current().id());
            }
            drop(threads);
            fill_with_coordinates(tile, pixels);
            Ok(())
        })
        .unwrap();

        let threads = tracing_threads.into_inner().unwrap();
        assert!(!threads.contains(&caller));
        assert!((1..=4).contains(&threads.len()));
    }

    #[test]
    fn uneven_tile_durations_do_not_change_the_image() {
        // Early tiles take longest, so workers finish tiles far out of index order.
        let (grid, mut framebuffer) = setup(37, 21, 8, 8);
        let count = grid.tile_count();

        render_tiles_parallel(grid, 4, &mut framebuffer, |tile, pixels| {
            let index = grid
                .tiles()
                .position(|candidate| candidate == tile)
                .unwrap();
            for _ in 0..(count - index) * 50 {
                thread::yield_now();
            }
            fill_with_coordinates(tile, pixels);
            Ok(())
        })
        .unwrap();

        assert_holds_coordinates(&framebuffer);
    }

    #[test]
    fn a_worker_error_reaches_the_caller() {
        for failing_tile in [0, 7, 239] {
            for workers in WORKER_COUNTS {
                let (grid, mut framebuffer) = setup(320, 180, 16, 16);
                let failing = grid.tile(failing_tile).unwrap();

                let result =
                    render_tiles_parallel(grid, workers, &mut framebuffer, |tile, pixels| {
                        if tile == failing {
                            return Err(RenderError::TextureNotFound);
                        }
                        fill_with_coordinates(tile, pixels);
                        Ok(())
                    });

                assert_eq!(
                    result,
                    Err(RenderError::TextureNotFound),
                    "tile {failing_tile}, {workers} workers"
                );
            }
        }
    }

    #[test]
    fn the_lowest_index_failure_is_reported_like_a_sequential_render() {
        // Tile 9 may fail first in time, but tile 5 was claimed earlier and is always finished.
        let (grid, _) = setup(320, 180, 16, 16);
        let (early, late) = (grid.tile(5).unwrap(), grid.tile(9).unwrap());
        for workers in WORKER_COUNTS {
            for _ in 0..20 {
                let mut framebuffer = Framebuffer::try_new(320, 180).unwrap();
                let result = render_tiles_parallel(grid, workers, &mut framebuffer, |tile, _| {
                    if tile == early {
                        thread::yield_now();
                        return Err(RenderError::MaterialNotFound);
                    }
                    if tile == late {
                        return Err(RenderError::TextureNotFound);
                    }
                    Ok(())
                });

                assert_eq!(
                    result,
                    Err(RenderError::MaterialNotFound),
                    "{workers} workers"
                );
            }
        }
    }

    #[test]
    fn failures_in_every_tile_terminate_without_deadlock() {
        for workers in WORKER_COUNTS {
            let (grid, mut framebuffer) = setup(37, 21, 1, 1);
            let result = render_tiles_parallel(grid, workers, &mut framebuffer, |_, _| {
                Err(RenderError::SurfaceUvUnavailable)
            });

            assert_eq!(result, Err(RenderError::SurfaceUvUnavailable));
        }
    }

    #[test]
    fn every_pixel_is_traced_and_written_exactly_once() {
        for (width, height, tile_width, tile_height) in GRIDS {
            for workers in WORKER_COUNTS {
                let (grid, mut framebuffer) = setup(width, height, tile_width, tile_height);
                let workers = effective_worker_count(workers, grid.tile_count()).unwrap();
                let traced_pixels = AtomicUsize::new(0);

                // A second write over a coordinate color would be invisible, so count the work:
                // the claimed tiles must add up to exactly the frame, with no tile claimed twice.
                render_tiles_parallel(grid, workers, &mut framebuffer, |tile, pixels| {
                    traced_pixels.fetch_add(pixels.len(), Ordering::Relaxed);
                    fill_with_coordinates(tile, pixels);
                    Ok(())
                })
                .unwrap();

                assert_eq!(
                    traced_pixels.load(Ordering::Relaxed),
                    width * height,
                    "{width}x{height}, {tile_width}x{tile_height}, {workers} workers"
                );
                assert_holds_coordinates(&framebuffer);
            }
        }
    }

    #[test]
    fn each_worker_reuses_one_buffer_for_all_its_tiles() {
        // 17x19 with 16x16 tiles has full and clipped tiles, so the buffer outlasts size changes.
        let (grid, mut framebuffer) = setup(17, 19, 16, 16);
        let buffers: Mutex<Vec<(ThreadId, usize)>> = Mutex::new(Vec::new());

        render_tiles_parallel(grid, 1, &mut framebuffer, |tile, pixels| {
            buffers
                .lock()
                .unwrap()
                .push((thread::current().id(), pixels.as_ptr() as usize));
            fill_with_coordinates(tile, pixels);
            Ok(())
        })
        .unwrap();

        let buffers = buffers.into_inner().unwrap();
        assert_eq!(buffers.len(), grid.tile_count());
        assert!(buffers.iter().all(|&entry| entry == buffers[0]));
        assert_holds_coordinates(&framebuffer);
    }

    #[test]
    fn workers_keep_distinct_buffers() {
        let (grid, mut framebuffer) = setup(64, 36, 8, 8);
        let buffers: Mutex<Vec<(ThreadId, usize)>> = Mutex::new(Vec::new());

        render_tiles_parallel(grid, 4, &mut framebuffer, |tile, pixels| {
            let entry = (thread::current().id(), pixels.as_ptr() as usize);
            let mut buffers = buffers.lock().unwrap();
            if !buffers.contains(&entry) {
                buffers.push(entry);
            }
            drop(buffers);
            fill_with_coordinates(tile, pixels);
            Ok(())
        })
        .unwrap();

        // One buffer per thread, and no two threads share one.
        let buffers = buffers.into_inner().unwrap();
        for (index, &(thread, pointer)) in buffers.iter().enumerate() {
            for &(other_thread, other_pointer) in &buffers[index + 1..] {
                assert_ne!(thread, other_thread, "a worker used two buffers");
                assert_ne!(pointer, other_pointer, "two workers shared a buffer");
            }
        }
    }

    #[test]
    fn a_failed_render_leaves_the_framebuffer_usable() {
        let (grid, mut framebuffer) = setup(64, 36, 8, 8);
        let result = render_tiles_parallel(grid, 4, &mut framebuffer, |_, _| {
            Err(RenderError::TextureNotFound)
        });
        assert_eq!(result, Err(RenderError::TextureNotFound));

        // The lock lives only inside one render, so the next render starts from a clean state.
        render_tiles_parallel(grid, 4, &mut framebuffer, |tile, pixels| {
            fill_with_coordinates(tile, pixels);
            Ok(())
        })
        .unwrap();
        assert_holds_coordinates(&framebuffer);
    }

    #[test]
    #[should_panic]
    fn a_worker_panic_propagates_to_the_caller() {
        let (grid, mut framebuffer) = setup(64, 36, 8, 8);
        let panicking = grid.tile(3).unwrap();

        let _ = render_tiles_parallel(grid, 4, &mut framebuffer, |tile, pixels| {
            assert_ne!(tile, panicking, "injected worker panic");
            fill_with_coordinates(tile, pixels);
            Ok(())
        });
    }

    #[test]
    fn a_worker_panic_never_yields_a_result_and_keeps_its_message() {
        for workers in [2, 4, 16] {
            let (grid, mut framebuffer) = setup(64, 36, 8, 8);
            let panicking = grid.tile(3).unwrap();

            let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                render_tiles_parallel(grid, workers, &mut framebuffer, |tile, pixels| {
                    assert_ne!(tile, panicking, "injected worker panic");
                    fill_with_coordinates(tile, pixels);
                    Ok(())
                })
            }));

            let payload = outcome.expect_err("a worker panic must not return a result");
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap();
            assert!(message.contains("injected worker panic"), "{message}");
        }
    }

    #[test]
    fn a_panic_in_every_tile_terminates_without_deadlock() {
        for workers in WORKER_COUNTS {
            let (grid, mut framebuffer) = setup(37, 21, 4, 4);

            let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                render_tiles_parallel(grid, workers, &mut framebuffer, |_, _| {
                    panic!("injected worker panic")
                })
            }));

            assert!(outcome.is_err(), "{workers} workers");
        }
    }

    #[test]
    fn a_tile_outside_the_framebuffer_panics_instead_of_returning_ok() {
        // The grid describes a larger frame than the framebuffer, which breaks the partition
        // contract; the rejected write must surface rather than leave a silently incomplete frame.
        let grid = TileGrid::try_new(64, 36, 8, 8).unwrap();
        let mut framebuffer = Framebuffer::try_new(32, 18).unwrap();

        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            render_tiles_parallel(grid, 4, &mut framebuffer, |tile, pixels| {
                fill_with_coordinates(tile, pixels);
                Ok(())
            })
        }));

        assert!(outcome.is_err());
    }
}
