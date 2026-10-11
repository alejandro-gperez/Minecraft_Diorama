//! Dynamic tile scheduling over scoped CPU worker threads.
//!
//! One render creates a fixed set of workers inside `std::thread::scope`; all of them are joined
//! before the render returns. There is no persistent pool.
//!
//! Protocol:
//! - Work assignment: workers claim row-major tile indices from one shared `AtomicUsize` with
//!   `fetch_add`, so every index is handed out exactly once. The counter only assigns work; it
//!   never publishes pixels, so `Ordering::Relaxed` suffices.
//! - Pixel ownership: the calling thread owns the framebuffer and is its only writer. Each worker
//!   owns `BUFFERS_PER_WORKER` reusable tile-sized buffers, traces a claimed tile into one of them
//!   without holding any lock, and sends the buffer through a channel to the owner. The owner
//!   copies the tile into its global position and sends the buffer back through that worker's
//!   own channel. Channel transfer moves the `Vec`, never its pixels, and synchronizes once per
//!   tile.
//! - Failure: a worker that fails reports the error and stops; the shared stop flag keeps the
//!   others from claiming further tiles. Because indices are claimed in increasing order, every
//!   tile before the failed one is still finished, so the owner reports the lowest-index failure:
//!   the same error a sequential render would return. A panicking worker panics the whole render
//!   through `std::thread::scope`.

use std::{
    io,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use super::{
    Color, Framebuffer, RenderError,
    tile::{Tile, TileGrid},
};

/// Most worker threads one render may use, whatever is requested.
///
/// The development frame has 240 tiles; more threads than this only add creation cost, and an
/// unbounded request must never spawn thousands of threads.
pub const MAX_WORKERS: usize = 64;

/// Tile buffers owned by each worker. Two let a worker trace its next tile while the owner is
/// still copying the previous one; with one, every tile would wait for the owner to wake up.
const BUFFERS_PER_WORKER: usize = 2;

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

/// A traced tile travelling from its worker to the framebuffer owner.
struct CompletedTile {
    worker: usize,
    tile: Tile,
    /// The worker's buffer; the tile's pixels are its first `width * height` entries, row-major.
    pixels: Vec<Color>,
}

enum WorkerMessage {
    Completed(CompletedTile),
    Failed {
        tile_index: usize,
        error: RenderError,
    },
}

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

    thread::scope(|scope| {
        let (completed_sender, completed) = mpsc::channel();
        let mut buffer_returns = Vec::with_capacity(workers);
        for worker in 0..workers {
            let (buffer_return, free_buffers) = mpsc::channel();
            for _ in 0..BUFFERS_PER_WORKER {
                buffer_return
                    .send(vec![Color::BLACK; buffer_len])
                    .expect("the worker's buffer receiver is still owned here");
            }
            buffer_returns.push(buffer_return);

            let completed_sender = completed_sender.clone();
            let queue = &queue;
            scope.spawn(move || run_worker(worker, queue, &free_buffers, &completed_sender));
        }
        // Only workers hold senders now, so the loop below ends once every worker has exited.
        drop(completed_sender);

        integrate_tiles(grid, framebuffer, &completed, &buffer_returns)
    })
}

/// One worker: claim a tile, trace it into a free buffer, hand it to the owner, repeat.
fn run_worker<F>(
    worker: usize,
    queue: &WorkQueue<'_, F>,
    free_buffers: &Receiver<Vec<Color>>,
    completed: &Sender<WorkerMessage>,
) where
    F: Fn(Tile, &mut [Color]) -> Result<(), RenderError>,
{
    while !queue.stop.load(Ordering::Relaxed) {
        let tile_index = queue.next_tile.fetch_add(1, Ordering::Relaxed);
        let Some(tile) = queue.grid.tile(tile_index) else {
            return;
        };
        // Blocks only while both buffers wait at the owner. An error means the owner is gone,
        // which happens only while it unwinds.
        let Ok(mut pixels) = free_buffers.recv() else {
            return;
        };

        let area = tile.width() * tile.height();
        let message = match (queue.trace_tile)(tile, &mut pixels[..area]) {
            Ok(()) => WorkerMessage::Completed(CompletedTile {
                worker,
                tile,
                pixels,
            }),
            Err(error) => {
                queue.stop.store(true, Ordering::Relaxed);
                WorkerMessage::Failed { tile_index, error }
            }
        };
        let failed = matches!(message, WorkerMessage::Failed { .. });
        if completed.send(message).is_err() || failed {
            return;
        }
    }
}

/// The owner's side: copy every completed tile into place and recycle its buffer.
///
/// Returns the lowest-index failure if any tile failed.
fn integrate_tiles(
    grid: TileGrid,
    framebuffer: &mut Framebuffer,
    completed: &Receiver<WorkerMessage>,
    buffer_returns: &[Sender<Vec<Color>>],
) -> Result<(), RenderError> {
    let mut integrated = 0;
    let mut first_failure: Option<(usize, RenderError)> = None;

    for message in completed {
        match message {
            WorkerMessage::Completed(CompletedTile {
                worker,
                tile,
                pixels,
            }) => {
                let area = tile.width() * tile.height();
                assert!(
                    framebuffer.write_tile(tile, &pixels[..area]),
                    "a scheduled tile must lie inside the framebuffer it partitions",
                );
                integrated += 1;
                // A worker that has already stopped no longer needs its buffer.
                let _ = buffer_returns[worker].send(pixels);
            }
            WorkerMessage::Failed { tile_index, error } => {
                if first_failure.is_none_or(|(first, _)| tile_index < first) {
                    first_failure = Some((tile_index, error));
                }
            }
        }
    }

    if let Some((_, error)) = first_failure {
        return Err(error);
    }
    // Without a reported failure, a missing tile means a worker panicked; `thread::scope`
    // propagates that panic, and this keeps an incomplete frame from ever returning `Ok`.
    assert_eq!(
        integrated,
        grid.tile_count(),
        "every tile must be integrated exactly once",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        num::NonZeroUsize,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
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
}
