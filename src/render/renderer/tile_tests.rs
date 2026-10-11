//! Tile renderer equivalence: tiling reorganizes work but must never change a pixel.
//!
//! The reference here is the pre-tile renderer's plain row-major pixel loop, kept test-only. The
//! helpers are `pub(crate)` so the showcase and voxel-diagnostic scenes, which live in the binary
//! crate, can be checked against the same reference.

use super::{
    PRIMARY_RAY_DEPTH, RenderError, Renderer, TileGrid, TileGridError, Tracer, pixel_center,
    primary_ray, render_tile,
};
use crate::{
    camera::OrbitalCamera,
    environment::Environment,
    geometry::Aabb,
    lighting::{AmbientLight, DirectionalLight, Lighting, PointLight},
    material::{AuxiliaryTextureIds, CanonicalTextureIds, Texture},
    math::Vec3,
    render::{Color, Framebuffer},
    scene::{Scene, SceneObject},
    voxel::{BlockMaterials, BlockType, Voxel, VoxelGrid, VoxelPosition},
};

/// Tile sizes covering unit, square, wide, oversized, and non-dividing tiles.
pub(crate) const EQUIVALENCE_TILE_SIZES: [(usize, usize); 8] = [
    (1, 1),
    (8, 8),
    (16, 16),
    (32, 16),
    (32, 32),
    (5, 7),
    (7, 3),
    (4096, 4096),
];

/// The previous renderer: one row-major pass over every pixel, no tiles.
pub(crate) fn render_sequential_reference(
    camera: &OrbitalCamera,
    scene: &Scene,
    lighting: &Lighting,
    environment: &Environment,
    framebuffer: &mut Framebuffer,
) -> Result<(), RenderError> {
    let width = framebuffer.width();
    let height = framebuffer.height();
    let tracer = Tracer::new(scene, *lighting, environment);

    for y in 0..height {
        for x in 0..width {
            let (u, v) = pixel_center(x, y, width, height);
            let ray = camera
                .ray_for_viewport(u, v)
                .ok_or(RenderError::PrimaryRayGenerationFailed)?;
            let color = tracer.trace_ray(ray, PRIMARY_RAY_DEPTH)?;

            framebuffer.set_pixel(x, y, color);
        }
    }

    Ok(())
}

/// Renders the tiles of a `tile_width` × `tile_height` grid in the caller's index order.
pub(crate) fn render_tiles_in_order(
    camera: &OrbitalCamera,
    scene: &Scene,
    lighting: &Lighting,
    environment: &Environment,
    framebuffer: &mut Framebuffer,
    (tile_width, tile_height): (usize, usize),
    order: impl Fn(usize) -> Vec<usize>,
) {
    let grid = TileGrid::try_new(
        framebuffer.width(),
        framebuffer.height(),
        tile_width,
        tile_height,
    )
    .unwrap();
    let tracer = Tracer::new(scene, *lighting, environment);

    for index in order(grid.tile_count()) {
        let tile = grid.tile(index).unwrap();
        render_tile(camera, &tracer, framebuffer, tile).unwrap();
    }
}

/// Asserts that every tile size, and the default `Renderer::render`, reproduce the reference
/// image exactly for one scene and one resolution.
pub(crate) fn assert_tiling_matches_reference(
    scene: &Scene,
    lighting: &Lighting,
    camera: &OrbitalCamera,
    width: usize,
    height: usize,
) {
    let environment = Environment::sunset();
    let mut reference = Framebuffer::try_new(width, height).unwrap();
    render_sequential_reference(camera, scene, lighting, &environment, &mut reference).unwrap();

    let mut default = Framebuffer::try_new(width, height).unwrap();
    Renderer::render(camera, scene, lighting, &environment, &mut default).unwrap();
    assert_eq!(default, reference, "default tiles at {width}x{height}");

    for (tile_width, tile_height) in EQUIVALENCE_TILE_SIZES {
        let mut tiled = Framebuffer::try_new(width, height).unwrap();
        Renderer::render_with_tile_size(
            camera,
            scene,
            lighting,
            &environment,
            &mut tiled,
            tile_width,
            tile_height,
        )
        .unwrap();
        assert_eq!(
            tiled, reference,
            "{tile_width}x{tile_height} tiles at {width}x{height}"
        );
    }
}

fn camera(width: usize, height: usize) -> OrbitalCamera {
    OrbitalCamera::try_new(
        Vec3::new(0.0, 0.7, 0.0),
        0.55 + std::f32::consts::PI,
        0.3,
        9.0,
        50.0_f32.to_radians(),
        width as f32 / height as f32,
    )
    .unwrap()
}

fn lighting() -> Lighting {
    Lighting::new(
        AmbientLight::try_new(Color::new(0.30, 0.34, 0.46), 0.50).unwrap(),
        DirectionalLight::try_new(Vec3::new(0.6, 1.0, 0.8), Color::new(1.0, 0.84, 0.70), 1.0)
            .unwrap(),
    )
}

/// Reflective obsidian, refractive glass, emissive lava with a point light, and a voxel ground:
/// every secondary-ray path crosses tile boundaries.
fn optics_scene() -> Scene {
    let mut scene = Scene::new();
    let mut texture = |r: f32, g: f32, b: f32| {
        scene
            .add_texture(Texture::solid(Color::new(r, g, b)))
            .unwrap()
    };
    let grass_top = texture(0.20, 0.70, 0.15);
    let grass_side = texture(0.45, 0.55, 0.20);
    let dirt = texture(0.50, 0.35, 0.20);
    let cobblestone = texture(0.55, 0.55, 0.55);
    let obsidian = texture(0.15, 0.10, 0.25);
    let glass = texture(0.70, 0.85, 0.90);
    let lava = texture(0.95, 0.45, 0.10);
    let ore = texture(0.75, 0.60, 0.50);
    let flat_normal = texture(0.5, 0.5, 1.0);

    let canonical = scene
        .add_canonical_materials(CanonicalTextureIds {
            grass_top,
            grass_side,
            dirt,
            cobblestone,
            cobblestone_normal: flat_normal,
            obsidian,
            glass,
            lava,
        })
        .unwrap();
    let auxiliary = scene
        .add_auxiliary_materials(AuxiliaryTextureIds {
            dirt,
            coal_ore: ore,
            iron_ore: ore,
            gold_ore: ore,
            diamond_ore: ore,
        })
        .unwrap();
    scene
        .set_block_materials(BlockMaterials::new(canonical, auxiliary))
        .unwrap();

    let mut grid = VoxelGrid::try_new(VoxelPosition::new(-4, -2, -3), 8, 3, 6).unwrap();
    for z in -3..3 {
        for x in -4..4 {
            let block = if (x + z) % 3 == 0 {
                BlockType::Dirt
            } else {
                BlockType::Grass
            };
            grid.set_world(VoxelPosition::new(x, -1, z), Voxel::Block(block))
                .unwrap();
        }
    }
    for (position, block) in [
        (VoxelPosition::new(-3, 0, 0), BlockType::Cobblestone),
        (VoxelPosition::new(1, 0, -1), BlockType::Glass),
        (VoxelPosition::new(2, -1, 1), BlockType::Lava),
    ] {
        grid.set_world(position, Voxel::Block(block)).unwrap();
    }
    scene.set_voxel_grid(grid).unwrap();

    for (min, max, material) in [
        (
            Vec3::new(-1.3, 0.0, -0.6),
            Vec3::new(-0.1, 1.8, 0.6),
            canonical.obsidian,
        ),
        (
            Vec3::new(0.2, 0.0, 0.2),
            Vec3::new(1.4, 1.6, 1.4),
            canonical.glass,
        ),
        (
            Vec3::new(2.2, 0.0, -1.8),
            Vec3::new(2.8, 1.2, -1.2),
            canonical.lava,
        ),
    ] {
        scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), material));
    }
    scene.add_point_light(
        PointLight::try_new(
            Vec3::new(2.5, 1.0, -1.0),
            Color::new(1.0, 0.45, 0.12),
            3.0,
            5.0,
        )
        .unwrap(),
    );

    scene
}

#[test]
fn tile_pixels_generate_the_sequential_primary_rays() {
    let (width, height) = (37, 21);
    let camera = camera(width, height);
    let grid = TileGrid::try_new(width, height, 8, 8).unwrap();

    let mut checked = 0;
    for tile in grid.tiles() {
        // Corners of every tile (boundary pixels, partial edge tiles) plus its center.
        let last_x = tile.x_end() - 1;
        let last_y = tile.y_end() - 1;
        for (x, y) in [
            (tile.x_start(), tile.y_start()),
            (last_x, tile.y_start()),
            (tile.x_start(), last_y),
            (last_x, last_y),
            (
                tile.x_start() + tile.width() / 2,
                tile.y_start() + tile.height() / 2,
            ),
        ] {
            let expected = camera
                .ray_for_viewport(
                    (x as f32 + 0.5) / width as f32,
                    (y as f32 + 0.5) / height as f32,
                )
                .unwrap();
            let actual = primary_ray(&camera, x, y, width, height).unwrap();

            assert_eq!(actual.origin(), expected.origin(), "origin at ({x}, {y})");
            assert_eq!(
                actual.direction(),
                expected.direction(),
                "direction at ({x}, {y})"
            );
            checked += 1;
        }
    }

    assert_eq!(checked, 5 * grid.tile_count());
    // The first and last framebuffer pixels are tile corners of the first and last tiles.
    assert_eq!(grid.tile(0).unwrap().x_start(), 0);
    assert_eq!(grid.tile(grid.tile_count() - 1).unwrap().x_end(), width);
    assert_eq!(grid.tile(grid.tile_count() - 1).unwrap().y_end(), height);
}

#[test]
fn tile_render_matches_the_sequential_reference_for_every_tile_size() {
    let scene = optics_scene();
    let lighting = lighting();

    // 48x27 divides 16x16 in width only; 37x21 divides nothing; 1x1 is a degenerate frame.
    for (width, height) in [(48, 27), (37, 21), (17, 19), (64, 36)] {
        assert_tiling_matches_reference(&scene, &lighting, &camera(width, height), width, height);
    }
}

#[test]
fn tile_render_handles_degenerate_framebuffers() {
    let scene = optics_scene();
    let lighting = lighting();

    for (width, height) in [(1, 1), (3, 2), (2, 33)] {
        assert_tiling_matches_reference(&scene, &lighting, &camera(width, height), width, height);
    }
}

#[test]
fn tile_order_does_not_change_the_image() {
    let scene = optics_scene();
    let lighting = lighting();
    let environment = Environment::sunset();
    let (width, height) = (37, 21);
    let camera = camera(width, height);

    let mut reference = Framebuffer::try_new(width, height).unwrap();
    render_sequential_reference(&camera, &scene, &lighting, &environment, &mut reference).unwrap();

    type Order = fn(usize) -> Vec<usize>;
    let orders: [(&str, Order); 3] = [
        ("row-major", |count| (0..count).collect()),
        ("reverse", |count| (0..count).rev().collect()),
        // Deterministic shuffle: stride coprime with every tested tile count.
        ("strided", |count| {
            let stride = (1..)
                .find(|stride| gcd(*stride, count) == 1 && *stride > 1)
                .unwrap_or(1);
            (0..count).map(|i| (i * stride) % count).collect()
        }),
    ];
    for tile_size in [(8, 8), (16, 16), (5, 7)] {
        for (name, order) in orders {
            let mut framebuffer = Framebuffer::try_new(width, height).unwrap();
            render_tiles_in_order(
                &camera,
                &scene,
                &lighting,
                &environment,
                &mut framebuffer,
                tile_size,
                order,
            );

            assert_eq!(framebuffer, reference, "{name} order, tiles {tile_size:?}");
        }
    }
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[test]
fn rendering_overwrites_every_pixel_exactly_once() {
    let scene = optics_scene();
    let lighting = lighting();
    let environment = Environment::sunset();
    let (width, height) = (37, 21);

    // A sentinel no radiance can produce shows any pixel a tile pass failed to write.
    let sentinel = Color::new(-1.0, -1.0, -1.0);
    let mut framebuffer = Framebuffer::try_new(width, height).unwrap();
    framebuffer.fill(sentinel);

    Renderer::render(
        &camera(width, height),
        &scene,
        &lighting,
        &environment,
        &mut framebuffer,
    )
    .unwrap();

    assert!(framebuffer.pixels().iter().all(|&color| color != sentinel));
    assert!(framebuffer.pixels().iter().all(|color| color.is_finite()));
}

#[test]
fn invalid_tile_sizes_are_rejected_without_panicking() {
    let scene = optics_scene();
    let lighting = lighting();
    let environment = Environment::sunset();
    let camera = camera(16, 9);
    let mut framebuffer = Framebuffer::try_new(16, 9).unwrap();

    let render = |framebuffer: &mut Framebuffer, tile_width, tile_height| {
        Renderer::render_with_tile_size(
            &camera,
            &scene,
            &lighting,
            &environment,
            framebuffer,
            tile_width,
            tile_height,
        )
    };

    assert_eq!(
        render(&mut framebuffer, 0, 16),
        Err(RenderError::InvalidTileConfiguration(
            TileGridError::ZeroTileWidth
        ))
    );
    assert_eq!(
        render(&mut framebuffer, 16, 0),
        Err(RenderError::InvalidTileConfiguration(
            TileGridError::ZeroTileHeight
        ))
    );
}

#[test]
fn aspect_ratio_mismatch_is_still_reported_before_any_tile_work() {
    let scene = optics_scene();
    let environment = Environment::sunset();
    let mut framebuffer = Framebuffer::try_new(16, 9).unwrap();
    framebuffer.fill(Color::WHITE);

    let result = Renderer::render(
        &camera(10, 10),
        &scene,
        &lighting(),
        &environment,
        &mut framebuffer,
    );

    assert_eq!(result, Err(RenderError::AspectRatioMismatch));
    assert!(
        framebuffer
            .pixels()
            .iter()
            .all(|&color| color == Color::WHITE)
    );
}
