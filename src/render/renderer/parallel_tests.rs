//! Parallel renderer equivalence: worker threads change who traces a tile and when, never a pixel.
//!
//! References are the test-only row-major loop and the single-worker path, which is the Mission
//! 25 sequential tile renderer.

use std::thread;

use super::{
    RenderError, Renderer, TileGrid, TileGridError, Tracer, render_tile_into,
    tile_tests::{
        assert_identical_frames, camera, lighting, optics_scene, render_sequential_reference,
    },
};
use crate::{
    environment::Environment,
    geometry::Aabb,
    material::{Material, Texture, TextureId, TextureSelection},
    math::Vec3,
    render::{Color, Framebuffer, parallel::render_tiles_parallel},
    scene::{Scene, SceneObject},
};

fn sequential_reference(scene: &Scene, width: usize, height: usize) -> Framebuffer {
    let mut reference = Framebuffer::try_new(width, height).unwrap();
    render_sequential_reference(
        &camera(width, height),
        scene,
        &lighting(),
        &Environment::sunset(),
        &mut reference,
    )
    .unwrap();
    reference
}

fn render_with_workers(
    scene: &Scene,
    (width, height): (usize, usize),
    tile_size: (usize, usize),
    workers: usize,
) -> Result<Framebuffer, RenderError> {
    let mut framebuffer = Framebuffer::try_new(width, height).unwrap();
    Renderer::render_with_configuration(
        &camera(width, height),
        scene,
        &lighting(),
        &Environment::sunset(),
        &mut framebuffer,
        tile_size,
        workers,
    )?;
    Ok(framebuffer)
}

#[test]
fn every_worker_count_matches_the_sequential_renderer() {
    let scene = optics_scene();

    // 48x27 divides 16x16 in width only; 37x21 divides nothing.
    for (width, height) in [(48, 27), (37, 21), (64, 36)] {
        let reference = sequential_reference(&scene, width, height);
        let mut single = Framebuffer::try_new(width, height).unwrap();
        Renderer::render_with_worker_count(
            &camera(width, height),
            &scene,
            &lighting(),
            &Environment::sunset(),
            &mut single,
            1,
        )
        .unwrap();
        assert_identical_frames(&single, &reference, "one worker");

        for workers in [2, 3, 4, 5, 8, 16] {
            let mut parallel = Framebuffer::try_new(width, height).unwrap();
            Renderer::render_with_worker_count(
                &camera(width, height),
                &scene,
                &lighting(),
                &Environment::sunset(),
                &mut parallel,
                workers,
            )
            .unwrap();
            assert_identical_frames(
                &parallel,
                &single,
                &format!("{workers} workers at {width}x{height}"),
            );
        }
    }
}

#[test]
fn every_tile_size_matches_the_sequential_renderer_in_parallel() {
    let scene = optics_scene();
    let size = (70, 41);
    let reference = sequential_reference(&scene, size.0, size.1);

    for tile_size in [(8, 8), (16, 16), (32, 16), (32, 32), (5, 7)] {
        for workers in [2, 3, 4] {
            let frame = render_with_workers(&scene, size, tile_size, workers).unwrap();
            assert_identical_frames(
                &frame,
                &reference,
                &format!("{tile_size:?} tiles, {workers} workers"),
            );
        }
    }
}

#[test]
fn more_workers_than_tiles_matches_the_sequential_renderer() {
    let scene = optics_scene();

    // 20x10 has two 16x16 tiles; 1x1 has a single tile and renders sequentially.
    for (width, height) in [(20, 10), (1, 1), (3, 2)] {
        let reference = sequential_reference(&scene, width, height);
        for workers in [8, 64, 1_000, usize::MAX] {
            let frame = render_with_workers(&scene, (width, height), (16, 16), workers).unwrap();
            assert_identical_frames(
                &frame,
                &reference,
                &format!("{workers} workers at {width}x{height}"),
            );
        }
    }
}

#[test]
fn uneven_tile_durations_do_not_change_the_rendered_image() {
    // Early tiles are delayed most, so workers complete tiles far out of index order.
    let scene = optics_scene();
    let (width, height) = (37, 21);
    let reference = sequential_reference(&scene, width, height);
    let environment = Environment::sunset();
    let camera = camera(width, height);
    let tracer = Tracer::new(&scene, lighting(), &environment);
    let grid = TileGrid::try_new(width, height, 8, 8).unwrap();

    for workers in [2, 4] {
        let mut framebuffer = Framebuffer::try_new(width, height).unwrap();
        render_tiles_parallel(grid, workers, &mut framebuffer, |tile, pixels| {
            let index = grid.tiles().position(|other| other == tile).unwrap();
            for _ in 0..(grid.tile_count() - index) * 100 {
                thread::yield_now();
            }
            render_tile_into(&camera, &tracer, (width, height), tile, pixels)
        })
        .unwrap();

        assert_identical_frames(&framebuffer, &reference, &format!("{workers} workers"));
    }
}

#[test]
fn zero_workers_is_rejected_before_any_tile_work() {
    let scene = optics_scene();
    let mut framebuffer = Framebuffer::try_new(16, 9).unwrap();
    framebuffer.fill(Color::WHITE);

    let result = Renderer::render_with_worker_count(
        &camera(16, 9),
        &scene,
        &lighting(),
        &Environment::sunset(),
        &mut framebuffer,
        0,
    );

    assert_eq!(result, Err(RenderError::InvalidWorkerCount));
    assert!(
        framebuffer
            .pixels()
            .iter()
            .all(|&color| color == Color::WHITE)
    );
}

#[test]
fn invalid_tile_sizes_are_rejected_for_every_worker_count() {
    let scene = optics_scene();

    for workers in [1, 2, 4] {
        assert_eq!(
            render_with_workers(&scene, (16, 9), (0, 16), workers),
            Err(RenderError::InvalidTileConfiguration(
                TileGridError::ZeroTileWidth
            ))
        );
        assert_eq!(
            render_with_workers(&scene, (16, 9), (16, 0), workers),
            Err(RenderError::InvalidTileConfiguration(
                TileGridError::ZeroTileHeight
            ))
        );
    }
}

/// A unit cube whose material names a normal map that was never registered: every pixel that
/// sees it fails shading with `TextureNotFound`, while the sky around it renders normally.
fn broken_normal_map_scene() -> Scene {
    let mut scene = Scene::new();
    let texture = scene.add_texture(Texture::solid(Color::WHITE)).unwrap();
    let material = scene
        .add_material(
            Material::try_new(
                TextureSelection::Uniform(texture),
                Color::WHITE,
                0.0,
                0.0,
                0.0,
            )
            .unwrap()
            .with_normal_map(TextureId::new(99)),
        )
        .unwrap();
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-0.5, 0.2, -0.5), Vec3::new(0.5, 1.2, 0.5)).unwrap(),
        material,
    ));
    scene
}

#[test]
fn a_render_error_inside_a_worker_reaches_the_caller() {
    let scene = broken_normal_map_scene();
    let size = (64, 36);

    assert_eq!(
        render_with_workers(&scene, size, (16, 16), 1),
        Err(RenderError::TextureNotFound)
    );
    for tile_size in [(16, 16), (8, 8), (1, 1)] {
        for workers in [2, 3, 4, 8, 64] {
            assert_eq!(
                render_with_workers(&scene, size, tile_size, workers),
                Err(RenderError::TextureNotFound),
                "{tile_size:?} tiles, {workers} workers"
            );
        }
    }

    // The failed renders joined their workers; the next render is complete and exact.
    let scene = optics_scene();
    let frame = render_with_workers(&scene, size, (16, 16), 4).unwrap();
    assert_identical_frames(
        &frame,
        &sequential_reference(&scene, 64, 36),
        "after failure",
    );
}
