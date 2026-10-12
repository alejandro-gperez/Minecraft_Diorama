use std::{
    error::Error,
    f32::consts::PI,
    path::{Path, PathBuf},
    time::Instant,
};

use app::run;
use camera::OrbitalCamera;
use cli::{SceneMode, USAGE};
use environment::Environment;
use geometry::Aabb;
use lighting::{AmbientLight, DirectionalLight, Lighting, PointLight};
use material::{AuxiliaryTextureIds, CanonicalTextureIds, PpmLoadError, TextureId, load_ppm};
use math::Vec3;
use render::{Color, Framebuffer};
use scene::{Scene, SceneObject};
use voxel::{BlockMaterials, BlockType, Voxel, VoxelGrid, VoxelPosition};
use world::{
    ArenaLayout, EggWarsArena, IslandConfig, IslandId, IslandStats, build_eggwars_arena,
    generate_island,
};

pub mod app;
pub mod camera;
pub mod cli;
pub mod color;
pub mod environment;
pub mod geometry;
pub mod lighting;
pub mod material;
pub mod math;
pub mod output;
pub mod ray;
pub mod render;
pub mod scene;
pub mod voxel;
pub mod world;

/// Internal CPU framebuffer; the window shows it at `app::app::WINDOW_SCALE`.
const WIDTH: usize = 640;
const HEIGHT: usize = 360;
const OUTPUT_PATH: &str = "output/phase1.ppm";
const VOXEL_DIAGNOSTIC_OUTPUT_PATH: &str = "output/voxel_diagnostic.ppm";
const VERTICAL_FOV_DEGREES: f32 = 50.0;

/// The procedural-island diagnostic: one standard 16×16 island whose flat surface level is
/// `y = 0`. The camera and grid derive from this config, so moving the island moves both.
const ISLAND_CONFIG: IslandConfig = IslandConfig::new(VoxelPosition::new(0, 0, 0), 12);
/// Initial orbit for the island: from the south-east, where the directional light falls, low
/// enough to see the rocky underside and high enough to see the grass surface.
const ISLAND_CAMERA_YAW: f32 = 1.05;
const ISLAND_CAMERA_PITCH: f32 = 0.25;
const ISLAND_CAMERA_RADIUS: f32 = 22.0;
/// Initial orbit for the EggWars arena, around the central island: from the lit south-east,
/// elevated enough to see all five grass tops and the void between them, low enough to keep the
/// rocky undersides in view, and just far enough to keep every island inside the frame.
const EGGWARS_CAMERA_YAW: f32 = 0.7;
const EGGWARS_CAMERA_PITCH: f32 = 0.35;
const EGGWARS_CAMERA_RADIUS: f32 = 68.0;

/// Position, color, intensity, and radius of the one representative light for the showcase lava.
///
/// It stands just outside the lava block's camera-facing (-Z) face (the block spans `x 1.7..2.9`,
/// `y 0..1.3`, `z -0.8..0.4`) so it is not shadowed by the block it represents. A future lava
/// region should likewise get one or a few such lights, never one per lava voxel.
const LAVA_LIGHT_POSITION: Vec3 = Vec3::new(2.3, 1.0, -1.15);
const LAVA_LIGHT_COLOR: Color = Color::new(1.0, 0.45, 0.12);
const LAVA_LIGHT_INTENSITY: f32 = 3.0;
const LAVA_LIGHT_RADIUS: f32 = 5.0;
/// Representative light for the voxel diagnostic's two-block lava pool (`x 1..3`, `y -1..0`,
/// `z 1..2`), half a block above the pool's center in an empty cell.
const VOXEL_LAVA_LIGHT_POSITION: Vec3 = Vec3::new(2.0, 0.5, 1.5);

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args_os().skip(1);
    let mode = match cli::parse_args(arguments.map(|a| a.to_string_lossy().into_owned())) {
        Ok(mode) => mode,
        Err(error) => {
            eprintln!("error: {error}\n{USAGE}");
            std::process::exit(2);
        }
    };

    let aspect_ratio = WIDTH as f32 / HEIGHT as f32;
    // The default showcase stays the Phase 3 five-block AABB scene so its render remains a
    // regression reference; the flags swap in the voxel diagnostic or the procedural island.
    let (camera, scene, output_path, overlay_label) = match mode {
        SceneMode::Showcase => (
            development_camera(aspect_ratio),
            phase2_test_scene()?,
            PathBuf::from(OUTPUT_PATH),
            None,
        ),
        SceneMode::VoxelDiagnostic => (
            development_camera(aspect_ratio),
            voxel_diagnostic_scene()?,
            PathBuf::from(VOXEL_DIAGNOSTIC_OUTPUT_PATH),
            None,
        ),
        SceneMode::ProceduralIsland { seed } => {
            let started = Instant::now();
            let (grid, stats) = procedural_island_grid(seed);
            let generation_ms = started.elapsed().as_secs_f64() * 1_000.0;
            println!(
                "Procedural island seed {seed}: {} blocks ({} ores) in a {}x{}x{} grid, \
                 generated in {generation_ms:.3} ms",
                stats.total(),
                stats.ores(),
                grid.width(),
                grid.height(),
                grid.depth(),
            );
            (
                procedural_island_camera(aspect_ratio),
                procedural_island_scene(grid)?,
                PathBuf::from(format!("output/procedural_island_seed_{seed}.ppm")),
                Some(format!(
                    "Procedural island | seed {seed} | {}x{} footprint",
                    ISLAND_CONFIG.footprint_width, ISLAND_CONFIG.footprint_depth,
                )),
            )
        }
        SceneMode::EggWars { seed } => {
            let started = Instant::now();
            let arena = eggwars_world(seed);
            let generation_ms = started.elapsed().as_secs_f64() * 1_000.0;
            log_arena(&arena, generation_ms);
            (
                eggwars_camera(aspect_ratio),
                eggwars_scene(arena.grid)?,
                PathBuf::from(format!("output/eggwars_seed_{seed}.ppm")),
                Some(format!(
                    "EggWars arena | seed {seed} | {} islands",
                    IslandId::ALL.len()
                )),
            )
        }
    };
    let lighting = phase2_lighting();
    let environment = Environment::sunset();
    let framebuffer =
        Framebuffer::try_new(WIDTH, HEIGHT).expect("development resolution must be valid");

    run(
        camera,
        scene,
        lighting,
        environment,
        framebuffer,
        &output_path,
        overlay_label.as_deref(),
    )?;
    Ok(())
}

/// The camera of the showcase and the voxel diagnostic.
fn development_camera(aspect_ratio: f32) -> OrbitalCamera {
    OrbitalCamera::try_new(
        Vec3::new(0.0, 0.7, 0.0),
        0.55 + PI,
        0.15,
        10.0,
        VERTICAL_FOV_DEGREES * PI / 180.0,
        aspect_ratio,
    )
    .expect("Phase 2 camera configuration must be valid")
}

fn phase2_lighting() -> Lighting {
    let ambient = AmbientLight::try_new(Color::new(0.30, 0.34, 0.46), 0.50)
        .expect("Phase 2 ambient-light configuration must be valid");
    let directional =
        DirectionalLight::try_new(Vec3::new(0.6, 1.0, 0.8), Color::new(1.0, 0.84, 0.70), 1.0)
            .expect("Phase 2 directional-light configuration must be valid");

    Lighting::new(ambient, directional)
}

/// A scene with every texture and material loaded once and every block type mapped, but no
/// geometry yet.
fn scene_with_block_materials() -> Result<(Scene, BlockMaterials), PpmLoadError> {
    let mut scene = Scene::with_capacity(5);

    let texture_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/textures");
    let textures = CanonicalTextureIds {
        grass_top: register_texture(&mut scene, &texture_directory, "grass_top.ppm")?,
        grass_side: register_texture(&mut scene, &texture_directory, "grass_side.ppm")?,
        dirt: register_texture(&mut scene, &texture_directory, "dirt.ppm")?,
        cobblestone: register_texture(&mut scene, &texture_directory, "cobblestone.ppm")?,
        cobblestone_normal: register_texture(
            &mut scene,
            &texture_directory,
            "cobblestone_normal.ppm",
        )?,
        obsidian: register_texture(&mut scene, &texture_directory, "obsidian.ppm")?,
        glass: register_texture(&mut scene, &texture_directory, "glass.ppm")?,
        lava: register_texture(&mut scene, &texture_directory, "lava.ppm")?,
    };
    let materials = scene
        .add_canonical_materials(textures)
        .expect("canonical Phase 2 materials must be valid and fit MaterialId");

    // Dirt reuses the texture grass already uses for its bottom, so no texture loads twice.
    let auxiliary_textures = AuxiliaryTextureIds {
        dirt: textures.dirt,
        coal_ore: register_texture(&mut scene, &texture_directory, "coal_ore.ppm")?,
        iron_ore: register_texture(&mut scene, &texture_directory, "iron_ore.ppm")?,
        gold_ore: register_texture(&mut scene, &texture_directory, "gold_ore.ppm")?,
        diamond_ore: register_texture(&mut scene, &texture_directory, "diamond_ore.ppm")?,
    };
    let auxiliary = scene
        .add_auxiliary_materials(auxiliary_textures)
        .expect("auxiliary terrain materials must be valid and fit MaterialId");
    let block_materials = BlockMaterials::new(materials, auxiliary);
    scene
        .set_block_materials(block_materials)
        .expect("every block type must map to a registered material");

    Ok((scene, block_materials))
}

fn phase2_test_scene() -> Result<Scene, PpmLoadError> {
    let (mut scene, materials) = scene_with_block_materials()?;
    // The showcase stays the five AABB blocks, with no voxel grid, as the Phase 3 reference.

    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-4.0, -0.5, -3.0), Vec3::new(4.0, 0.0, 3.0)).unwrap(),
        materials.grass,
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-2.8, 0.0, -0.2), Vec3::new(-1.6, 1.4, 1.0)).unwrap(),
        materials.cobblestone,
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-1.3, 0.0, -0.6), Vec3::new(-0.1, 1.8, 0.6)).unwrap(),
        materials.obsidian,
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(0.2, 0.0, -0.2), Vec3::new(1.4, 1.6, 1.0)).unwrap(),
        materials.glass,
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(1.7, 0.0, -0.8), Vec3::new(2.9, 1.3, 0.4)).unwrap(),
        materials.lava,
    ));
    scene.add_point_light(
        PointLight::try_new(
            LAVA_LIGHT_POSITION,
            LAVA_LIGHT_COLOR,
            LAVA_LIGHT_INTENSITY,
            LAVA_LIGHT_RADIUS,
        )
        .expect("showcase lava light configuration must be valid"),
    );

    Ok(scene)
}

/// Small hybrid scene validating voxel rendering: every block type as voxels, plus two AABB
/// objects standing on the voxel ground, with no surface represented twice.
///
/// An 8 × 6 slab of grass over dirt with its top at `y = 0`, the four ores exposed on the
/// camera-facing (`-Z`) side of the dirt layer, a two-block lava pool set into the grass, and
/// cobblestone, obsidian, and glass blocks on top. This is a diagnostic, not the EggWars world.
fn voxel_diagnostic_scene() -> Result<Scene, PpmLoadError> {
    let (mut scene, materials) = scene_with_block_materials()?;
    let mut grid = VoxelGrid::try_new(VoxelPosition::new(-4, -2, -3), 8, 4, 6)
        .expect("voxel diagnostic grid dimensions must be valid");

    for z in -3..3 {
        for x in -4..4 {
            set_block(&mut grid, (x, -2, z), BlockType::Dirt);
            set_block(&mut grid, (x, -1, z), BlockType::Grass);
        }
    }
    for (x, ore) in [
        (-3, BlockType::CoalOre),
        (-1, BlockType::IronOre),
        (1, BlockType::GoldOre),
        (3, BlockType::DiamondOre),
    ] {
        set_block(&mut grid, (x, -2, -3), ore);
    }
    for (position, block) in [
        ((1, -1, 1), BlockType::Lava),
        ((2, -1, 1), BlockType::Lava),
        ((-3, 0, 0), BlockType::Cobblestone),
        ((-3, 1, 0), BlockType::Cobblestone),
        ((-2, 0, 0), BlockType::Cobblestone),
        ((-1, 0, -1), BlockType::Obsidian),
        ((-1, 1, -1), BlockType::Obsidian),
        ((1, 0, -1), BlockType::Glass),
    ] {
        set_block(&mut grid, position, block);
    }
    scene
        .set_voxel_grid(grid)
        .expect("block materials are registered before the grid");

    // Non-voxel geometry on the voxel ground: a thin obsidian post and a cobblestone half slab.
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(2.2, 0.0, -1.8), Vec3::new(2.8, 1.2, -1.2)).unwrap(),
        materials.obsidian,
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-1.9, 0.0, 1.1), Vec3::new(-1.1, 0.5, 1.9)).unwrap(),
        materials.cobblestone,
    ));
    scene.add_point_light(
        PointLight::try_new(
            VOXEL_LAVA_LIGHT_POSITION,
            LAVA_LIGHT_COLOR,
            LAVA_LIGHT_INTENSITY,
            LAVA_LIGHT_RADIUS,
        )
        .expect("voxel diagnostic lava light configuration must be valid"),
    );

    Ok(scene)
}

/// Generates the diagnostic island for `seed` into a grid sized exactly to the island's region.
///
/// This grid is the scene's one global grid. Later islands will share a larger one; for a single
/// island, a tight grid keeps every DDA step inside the island's own box.
fn procedural_island_grid(seed: u64) -> (VoxelGrid, IslandStats) {
    let bounds = ISLAND_CONFIG
        .bounds()
        .expect("procedural island configuration must be valid");
    let extent = |min: i32, max: i32| (max - min + 1) as usize;
    let mut grid = VoxelGrid::try_new(
        bounds.min,
        extent(bounds.min.x, bounds.max.x),
        extent(bounds.min.y, bounds.max.y),
        extent(bounds.min.z, bounds.max.z),
    )
    .expect("procedural island grid dimensions must be valid");
    let stats = generate_island(&mut grid, &ISLAND_CONFIG, seed)
        .expect("procedural island must fit the grid built from its bounds");
    (grid, stats)
}

/// The procedural-island diagnostic: one generated island and no AABB objects or lights beyond
/// the shared ambient and directional light. Terrain is generated before this, exactly once.
fn procedural_island_scene(grid: VoxelGrid) -> Result<Scene, PpmLoadError> {
    let (mut scene, _) = scene_with_block_materials()?;
    scene
        .set_voxel_grid(grid)
        .expect("block materials are registered before the grid");
    Ok(scene)
}

/// Orbits the island's visual center, a third of the way down from the surface toward the tip,
/// rather than the world origin, so the island stays framed wherever its config places it.
fn procedural_island_camera(aspect_ratio: f32) -> OrbitalCamera {
    let center = ISLAND_CONFIG.center;
    let surface_top = center.y as f32 + 1.0;
    let target = Vec3::new(
        center.x as f32,
        surface_top - ISLAND_CONFIG.thickness as f32 / 3.0,
        center.z as f32,
    );
    OrbitalCamera::try_new(
        target,
        ISLAND_CAMERA_YAW,
        ISLAND_CAMERA_PITCH,
        ISLAND_CAMERA_RADIUS,
        VERTICAL_FOV_DEGREES * PI / 180.0,
        aspect_ratio,
    )
    .expect("procedural island camera configuration must be valid")
}

/// The terrain stage alone: the five islands in a grid sized exactly to them (Mission 29). The
/// app renders `eggwars_world`; terrain-level scene tests use this.
#[cfg(test)]
fn eggwars_arena(seed: u64) -> world::Arena {
    world::generate_arena(&ArenaLayout::eggwars(), seed)
        .expect("the EggWars arena grid must be allocatable")
}

/// Generates the five-island terrain for `seed` into the one global grid and builds the
/// designed bridges and structures on it, once, before the scene exists.
fn eggwars_world(seed: u64) -> EggWarsArena {
    build_eggwars_arena(seed).expect("the EggWars arena must generate and construct")
}

/// Startup summary: one line for the arena, one for construction, and one per island, never
/// per block.
fn log_arena(arena: &EggWarsArena, generation_ms: f64) {
    let info = &arena.terrain;
    let construction = &arena.construction;
    let (width, height, depth) = info.grid_dimensions;
    let origin = info.grid_origin;
    println!(
        "EggWars arena seed {}: {} islands, {} blocks ({} terrain, {} ores) in a \
         {width}x{height}x{depth} grid at origin ({}, {}, {}), separation {}, generated and \
         constructed in {generation_ms:.3} ms",
        info.global_seed,
        info.islands.len(),
        arena.occupied_voxels(),
        info.occupied_voxels,
        info.ores(),
        origin.x,
        origin.y,
        origin.z,
        info.separation,
    );
    println!(
        "  Construction: {} blocks ({} cobblestone, {} obsidian, {} glass)",
        construction.total(),
        construction.count(BlockType::Cobblestone),
        construction.count(BlockType::Obsidian),
        construction.count(BlockType::Glass),
    );
    for island in &info.islands {
        let config = island.config;
        println!(
            "  {:<6} {}x{} at ({}, {}, {}): island seed {:#018x}, {} blocks, {} ores",
            island.id.name(),
            config.footprint_width,
            config.footprint_depth,
            config.center.x,
            config.center.y,
            config.center.z,
            island.seed,
            island.stats.total(),
            island.stats.ores(),
        );
    }
    for (bridge, base) in construction.bridges.iter().zip(&construction.bases) {
        let intact = base.defense.iter().filter(|slot| slot.intact).count();
        println!(
            "  {:<6} bridge {:?} ({} missing), defense {:?} ({intact}/{} obsidian)",
            bridge.island.name(),
            bridge.state,
            bridge.missing_span,
            base.defense_state,
            base.defense.len(),
        );
    }
}

/// The EggWars arena: the generated grid and no AABB objects or lights beyond the shared ambient
/// and directional light. Terrain is generated before this, exactly once.
fn eggwars_scene(grid: VoxelGrid) -> Result<Scene, PpmLoadError> {
    procedural_island_scene(grid)
}

/// Orbits the central island's center line at a third of its thickness below the grass, so the
/// arena stays framed around its focal island.
fn eggwars_camera(aspect_ratio: f32) -> OrbitalCamera {
    let center = ArenaLayout::eggwars().config(IslandId::Center);
    let surface_top = center.center.y as f32 + 1.0;
    let target = Vec3::new(
        center.center.x as f32,
        surface_top - center.thickness as f32 / 3.0,
        center.center.z as f32,
    );
    OrbitalCamera::try_new(
        target,
        EGGWARS_CAMERA_YAW,
        EGGWARS_CAMERA_PITCH,
        EGGWARS_CAMERA_RADIUS,
        VERTICAL_FOV_DEGREES * PI / 180.0,
        aspect_ratio,
    )
    .expect("EggWars camera configuration must be valid")
}

fn set_block(grid: &mut VoxelGrid, (x, y, z): (i32, i32, i32), block: BlockType) {
    grid.set_world(VoxelPosition::new(x, y, z), Voxel::Block(block))
        .expect("voxel diagnostic blocks must lie inside its grid");
}

fn register_texture(
    scene: &mut Scene,
    directory: &Path,
    filename: &str,
) -> Result<TextureId, PpmLoadError> {
    let texture = load_ppm(&directory.join(filename))?;
    Ok(scene
        .add_texture(texture)
        .expect("prepared texture count must fit TextureId"))
}

#[cfg(test)]
mod tests {
    use std::{f32::consts::FRAC_PI_4, process};

    use super::{
        HEIGHT, ISLAND_CONFIG, WIDTH, eggwars_arena, eggwars_camera, eggwars_scene, eggwars_world,
        phase2_lighting, phase2_test_scene, procedural_island_camera, procedural_island_grid,
        procedural_island_scene, voxel_diagnostic_scene,
    };
    use crate::{
        app::app::{WINDOW_SCALE, window_size},
        camera::OrbitalCamera,
        environment::Environment,
        material::TextureSelection,
        math::Vec3,
        output::write_ppm,
        ray::Ray,
        render::{Framebuffer, Renderer, renderer::tile_tests::assert_tiling_matches_reference},
        scene::{HitSource, Scene},
        voxel::{BlockType, Voxel, VoxelGrid, VoxelPosition},
        world::{ArenaLayout, BridgeState, IslandId, construction::OUTER_ISLANDS},
    };

    #[test]
    fn showcase_registers_block_materials_once_without_a_voxel_grid() {
        let scene = phase2_test_scene().unwrap();
        let block_materials = scene.block_materials().unwrap();

        // Eight canonical textures plus four ores; five canonical plus five auxiliary materials.
        assert_eq!(scene.texture_count(), 12);
        assert_eq!(scene.material_count(), 10);
        assert_eq!(scene.len(), 5);
        assert!(scene.voxel_grid().is_none());
        for block in BlockType::ALL {
            assert!(scene.material(block_materials.material(block)).is_some());
        }

        let grass = scene
            .material(block_materials.material(BlockType::Grass))
            .unwrap();
        let dirt = scene
            .material(block_materials.material(BlockType::Dirt))
            .unwrap();
        let TextureSelection::TopSideBottom { bottom, .. } = grass.textures() else {
            panic!("canonical grass selects top, side, and bottom textures");
        };
        assert_eq!(dirt.textures(), TextureSelection::Uniform(bottom));
    }

    fn occupied_voxels(scene: &crate::scene::Scene) -> Vec<(VoxelPosition, BlockType)> {
        let grid = scene.voxel_grid().unwrap();
        let origin = grid.origin();
        let mut occupied = Vec::new();
        for y in 0..grid.height() as i32 {
            for z in 0..grid.depth() as i32 {
                for x in 0..grid.width() as i32 {
                    let position = VoxelPosition::new(origin.x + x, origin.y + y, origin.z + z);
                    if let Some(block) = grid.get_world(position).unwrap().block() {
                        occupied.push((position, block));
                    }
                }
            }
        }
        occupied
    }

    #[test]
    fn voxel_diagnostic_holds_every_block_type_without_double_geometry() {
        let scene = voxel_diagnostic_scene().unwrap();
        let occupied = occupied_voxels(&scene);

        for block in BlockType::ALL {
            assert!(occupied.iter().any(|&(_, b)| b == block), "{block:?}");
        }
        assert_eq!(scene.len(), 2);
        assert_eq!(scene.point_lights().len(), 1);
        // No AABB object shares volume with an occupied voxel: nothing is represented twice.
        let objects = [
            (Vec3::new(2.2, 0.0, -1.8), Vec3::new(2.8, 1.2, -1.2)),
            (Vec3::new(-1.9, 0.0, 1.1), Vec3::new(-1.1, 0.5, 1.9)),
        ];
        for (min, max) in objects {
            let probe = Ray::try_new(
                Vec3::new((min.x + max.x) * 0.5, 5.0, (min.z + max.z) * 0.5),
                Vec3::new(0.0, -1.0, 0.0),
            )
            .unwrap();
            let top = scene.closest_hit(probe, 0.0, f32::INFINITY).unwrap();
            assert_eq!(
                top.source,
                HitSource::Object,
                "objects must match the scene"
            );
            assert_eq!(top.geometry.position.y, max.y);

            for &(position, _) in &occupied {
                let voxel_min = position.min_corner();
                let voxel_max = voxel_min + Vec3::new(1.0, 1.0, 1.0);
                let overlaps = min.x < voxel_max.x
                    && max.x > voxel_min.x
                    && min.y < voxel_max.y
                    && max.y > voxel_min.y
                    && min.z < voxel_max.z
                    && max.z > voxel_min.z;
                assert!(!overlaps, "{min:?}..{max:?} overlaps {position:?}");
            }
        }
    }

    #[test]
    fn voxel_diagnostic_primary_ray_hits_voxel_grass_from_the_default_camera() {
        let scene = voxel_diagnostic_scene().unwrap();
        let ray = Ray::try_new(Vec3::new(-2.5, 6.0, -2.5), Vec3::new(0.0, -1.0, 0.0)).unwrap();

        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

        assert_eq!(
            hit.source,
            HitSource::Voxel {
                position: VoxelPosition::new(-3, -1, -3),
                block: BlockType::Grass,
            }
        );
        assert_eq!(hit.geometry.position.y, 0.0);
    }

    #[test]
    fn voxel_diagnostic_renders_finite_colors_across_camera_poses() {
        let scene = voxel_diagnostic_scene().unwrap();
        let lighting = phase2_lighting();
        let environment = Environment::sunset();
        let mut framebuffer = Framebuffer::try_new(32, 18).unwrap();

        for target in [Vec3::new(0.0, 0.7, 0.0), Vec3::new(1.5, 0.2, 0.5)] {
            for yaw_step in 0..8 {
                for pitch in [-0.6, 0.15, 0.9, 1.4] {
                    for radius in [1.5, 4.0, 10.0, 25.0] {
                        let camera = OrbitalCamera::try_new(
                            target,
                            yaw_step as f32 * FRAC_PI_4 + 0.1,
                            pitch,
                            radius,
                            50.0_f32.to_radians(),
                            32.0 / 18.0,
                        )
                        .unwrap();

                        Renderer::render(
                            &camera,
                            &scene,
                            &lighting,
                            &environment,
                            &mut framebuffer,
                        )
                        .unwrap();
                        assert!(framebuffer.pixels().iter().all(|color| color.is_finite()));
                    }
                }
            }
        }
    }

    fn development_camera(width: usize, height: usize) -> OrbitalCamera {
        OrbitalCamera::try_new(
            Vec3::new(0.0, 0.7, 0.0),
            0.55 + std::f32::consts::PI,
            0.15,
            10.0,
            50.0_f32.to_radians(),
            width as f32 / height as f32,
        )
        .unwrap()
    }

    #[test]
    fn showcase_tiling_matches_the_sequential_reference() {
        let scene = phase2_test_scene().unwrap();
        let lighting = phase2_lighting();

        // Full development resolution once, then small and non-divisible frames for every size.
        for (width, height) in [(WIDTH, HEIGHT), (70, 41), (33, 19)] {
            let camera = development_camera(width, height);
            assert_tiling_matches_reference(&scene, &lighting, &camera, width, height);
        }
    }

    #[test]
    fn voxel_diagnostic_tiling_matches_the_sequential_reference() {
        let scene = voxel_diagnostic_scene().unwrap();
        let lighting = phase2_lighting();

        for (width, height) in [(WIDTH, HEIGHT), (70, 41), (33, 19)] {
            let camera = development_camera(width, height);
            assert_tiling_matches_reference(&scene, &lighting, &camera, width, height);
        }
    }

    const ISLAND_SEED: u64 = 1847;
    const ORES: [BlockType; 4] = [
        BlockType::CoalOre,
        BlockType::IronOre,
        BlockType::GoldOre,
        BlockType::DiamondOre,
    ];

    fn island_scene() -> Scene {
        procedural_island_scene(procedural_island_grid(ISLAND_SEED).0).unwrap()
    }

    fn voxel_center(position: VoxelPosition) -> Vec3 {
        position.min_corner() + Vec3::new(0.5, 0.5, 0.5)
    }

    #[test]
    fn procedural_island_attaches_one_generated_grid_with_every_material() {
        let (grid, stats) = procedural_island_grid(ISLAND_SEED);
        let scene = procedural_island_scene(grid.clone()).unwrap();
        let showcase = phase2_test_scene().unwrap();

        assert_eq!(scene.voxel_grid(), Some(&grid));
        assert_eq!(scene.len(), 0);
        assert!(scene.point_lights().is_empty());
        assert_eq!(scene.texture_count(), showcase.texture_count());
        assert_eq!(scene.material_count(), showcase.material_count());
        // The grid is exactly the island's region.
        let bounds = ISLAND_CONFIG.bounds().unwrap();
        assert_eq!(grid.origin(), bounds.min);
        assert_eq!((grid.width(), grid.height(), grid.depth()), (16, 13, 16));
        assert!(stats.total() > 500);

        let materials = scene.block_materials().unwrap();
        for block in BlockType::ALL {
            assert!(scene.material(materials.material(block)).is_some());
        }
    }

    #[test]
    fn island_grass_is_hit_by_dda_with_canonical_grass() {
        let scene = island_scene();
        let materials = scene.block_materials().unwrap();
        let ray = Ray::try_new(Vec3::new(0.5, 30.0, 0.5), Vec3::new(0.0, -1.0, 0.0)).unwrap();

        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

        assert_eq!(
            hit.source,
            HitSource::Voxel {
                position: VoxelPosition::new(0, 0, 0),
                block: BlockType::Grass,
            }
        );
        assert_eq!(hit.geometry.position.y, 1.0);
        assert_eq!(hit.material_id, materials.grass);
        let grass = scene.material(hit.material_id).unwrap();
        assert!(matches!(
            grass.textures(),
            TextureSelection::TopSideBottom { .. }
        ));
    }

    #[test]
    fn island_rock_and_ores_resolve_through_dda_to_their_materials() {
        let scene = island_scene();
        let grid = scene.voxel_grid().unwrap();
        let materials = scene.block_materials().unwrap();
        let bounds = ISLAND_CONFIG.bounds().unwrap();

        // Straight up into every column's lowest block: cobblestone there is normal mapped.
        let mut cobblestone_hits = 0;
        for z in bounds.min.z..=bounds.max.z {
            for x in bounds.min.x..=bounds.max.x {
                let origin = Vec3::new(x as f32 + 0.5, -40.0, z as f32 + 0.5);
                let ray = Ray::try_new(origin, Vec3::new(0.0, 1.0, 0.0)).unwrap();
                let Some(hit) = scene.closest_hit(ray, 0.0, f32::INFINITY) else {
                    continue;
                };
                let HitSource::Voxel { block, .. } = hit.source else {
                    panic!("the island scene has no AABB objects");
                };
                assert_eq!(hit.material_id, materials.material(block));
                if block == BlockType::Cobblestone {
                    cobblestone_hits += 1;
                    let material = scene.material(hit.material_id).unwrap();
                    assert!(material.normal_map().is_some());
                }
            }
        }
        assert!(cobblestone_hits > 50);

        // From the open cell beside every ore, toward the ore: the DDA reports that ore.
        let mut ore_hits = 0;
        for y in bounds.min.y..=bounds.max.y {
            for z in bounds.min.z..=bounds.max.z {
                for x in bounds.min.x..=bounds.max.x {
                    let position = VoxelPosition::new(x, y, z);
                    let Some(block) = grid.get_world(position).unwrap().block() else {
                        continue;
                    };
                    if !ORES.contains(&block) {
                        continue;
                    }
                    let open = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .into_iter()
                        .map(|(dx, dz)| VoxelPosition::new(x + dx, y, z + dz))
                        .find(|&n| !grid.is_occupied_world(n))
                        .expect("ores are only placed with an open side");
                    let start = voxel_center(open);
                    let ray = Ray::try_new(start, voxel_center(position) - start).unwrap();

                    let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

                    assert_eq!(hit.source, HitSource::Voxel { position, block });
                    assert_eq!(hit.geometry.t, 0.5);
                    assert_eq!(hit.material_id, materials.material(block));
                    let material = scene.material(hit.material_id).unwrap();
                    assert!(material.normal_map().is_none());
                    ore_hits += 1;
                }
            }
        }
        assert!(ore_hits > 0, "seed {ISLAND_SEED} must expose ore");
    }

    #[test]
    fn island_camera_orbits_the_island_center() {
        let camera = procedural_island_camera(WIDTH as f32 / HEIGHT as f32);
        let center = ISLAND_CONFIG.center;
        let target = camera.target();

        assert_eq!((target.x, target.z), (center.x as f32, center.z as f32));
        assert!(target.y < center.y as f32 + 1.0);
        assert!(target.y > (center.y - ISLAND_CONFIG.thickness as i32) as f32);
        // Elevated above the surface, so the grass is visible as well as the underside.
        assert!(camera.position().y > center.y as f32 + 1.0);
    }

    #[test]
    fn orbit_and_zoom_render_the_same_unchanged_terrain() {
        let scene = island_scene();
        let generated = scene.voxel_grid().unwrap().clone();
        let lighting = phase2_lighting();
        let environment = Environment::sunset();
        let mut framebuffer = Framebuffer::try_new(32, 18).unwrap();
        let mut camera = procedural_island_camera(32.0 / 18.0);
        let mut renders = Vec::new();

        for _ in 0..6 {
            Renderer::render(&camera, &scene, &lighting, &environment, &mut framebuffer).unwrap();
            assert!(framebuffer.pixels().iter().all(|color| color.is_finite()));
            renders.push(framebuffer.pixels().to_vec());
            camera.orbit_yaw(1.1);
            camera.orbit_pitch(-0.2);
            camera.adjust_radius(-2.5);
        }

        // Every viewpoint differs, yet the scene's terrain never changed and still equals a fresh
        // generation of the same seed.
        assert!(renders.windows(2).all(|pair| pair[0] != pair[1]));
        assert_eq!(scene.voxel_grid(), Some(&generated));
        assert_eq!(
            &procedural_island_grid(ISLAND_SEED).0,
            scene.voxel_grid().unwrap()
        );
    }

    #[test]
    fn procedural_island_renders_from_assorted_viewpoints() {
        let scene = island_scene();
        let lighting = phase2_lighting();
        let environment = Environment::sunset();
        let mut framebuffer = Framebuffer::try_new(32, 18).unwrap();
        let target = procedural_island_camera(1.0).target();

        for yaw_step in 0..8 {
            for pitch in [-1.2, -0.3, 0.25, 1.3] {
                for radius in [3.0, 12.0, 40.0] {
                    let camera = OrbitalCamera::try_new(
                        target,
                        yaw_step as f32 * FRAC_PI_4 + 0.2,
                        pitch,
                        radius,
                        50.0_f32.to_radians(),
                        32.0 / 18.0,
                    )
                    .unwrap();
                    Renderer::render(&camera, &scene, &lighting, &environment, &mut framebuffer)
                        .unwrap();
                    assert!(framebuffer.pixels().iter().all(|color| color.is_finite()));
                }
            }
        }
    }

    #[test]
    fn procedural_island_tiling_matches_the_sequential_reference() {
        let scene = island_scene();
        let lighting = phase2_lighting();

        for (width, height) in [(WIDTH, HEIGHT), (70, 41)] {
            let camera = procedural_island_camera(width as f32 / height as f32);
            assert_tiling_matches_reference(&scene, &lighting, &camera, width, height);
        }
    }

    #[test]
    fn island_grid_holds_only_generated_terrain() {
        let (grid, stats): (VoxelGrid, _) = procedural_island_grid(ISLAND_SEED);
        let origin = grid.origin();
        let mut count = 0;
        for y in 0..grid.height() as i32 {
            for z in 0..grid.depth() as i32 {
                for x in 0..grid.width() as i32 {
                    let voxel = grid
                        .get_world(VoxelPosition::new(origin.x + x, origin.y + y, origin.z + z))
                        .unwrap();
                    if let Voxel::Block(block) = voxel {
                        assert!(!matches!(
                            block,
                            BlockType::Glass | BlockType::Lava | BlockType::Obsidian
                        ));
                        count += 1;
                    }
                }
            }
        }
        assert_eq!(count, stats.total());
    }

    const ARENA_SEED: u64 = 1847;

    fn arena_scene() -> Scene {
        eggwars_scene(eggwars_arena(ARENA_SEED).grid).unwrap()
    }

    /// The island whose region contains a hit voxel, if the hit is terrain.
    fn hit_island(layout: &ArenaLayout, source: HitSource) -> Option<IslandId> {
        let HitSource::Voxel { position, .. } = source else {
            return None;
        };
        IslandId::ALL
            .into_iter()
            .find(|&id| layout.config(id).bounds().unwrap().contains(position))
    }

    #[test]
    fn eggwars_scene_attaches_the_one_arena_grid_and_nothing_else() {
        let arena = eggwars_arena(ARENA_SEED);
        let scene = eggwars_scene(arena.grid.clone()).unwrap();
        let showcase = phase2_test_scene().unwrap();

        assert_eq!(scene.voxel_grid(), Some(&arena.grid));
        assert_eq!(scene.len(), 0);
        assert!(scene.point_lights().is_empty());
        assert_eq!(scene.texture_count(), showcase.texture_count());
        assert_eq!(scene.material_count(), showcase.material_count());
        assert_eq!(arena.info.global_seed, ARENA_SEED);
        assert_eq!(arena.info.grid_dimensions, (80, 16, 80));
    }

    #[test]
    fn dda_reaches_the_grass_of_every_island_in_the_global_grid() {
        let scene = arena_scene();
        let layout = ArenaLayout::eggwars();
        let materials = scene.block_materials().unwrap();

        for id in IslandId::ALL {
            let center = layout.config(id).center;
            let origin = Vec3::new(center.x as f32 + 0.5, 30.0, center.z as f32 + 0.5);
            let ray = Ray::try_new(origin, Vec3::new(0.0, -1.0, 0.0)).unwrap();

            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

            assert_eq!(
                hit.source,
                HitSource::Voxel {
                    position: center,
                    block: BlockType::Grass,
                },
                "{id:?}"
            );
            assert_eq!(hit.geometry.position.y, center.y as f32 + 1.0);
            assert_eq!(hit.material_id, materials.grass);
        }
    }

    #[test]
    fn dda_crosses_the_void_between_islands() {
        let scene = arena_scene();
        let layout = ArenaLayout::eggwars();
        let east = Vec3::new(1.0, 0.0, 0.0);
        let along = |origin: Vec3, direction: Vec3| {
            let ray = Ray::try_new(origin, direction).unwrap();
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            (hit_island(&layout, hit.source), hit.geometry.t)
        };

        // From outside the grid, through the west island first.
        let (island, _) = along(Vec3::new(-100.0, 0.5, 0.5), east);
        assert_eq!(island, Some(IslandId::West));
        // From the void between west and center, either way, across the empty cells.
        let gap = Vec3::new(-18.0, 0.5, 0.5);
        let (island, t) = along(gap, east);
        assert_eq!(island, Some(IslandId::Center));
        assert!(t >= 6.0, "{t}");
        let (island, t) = along(gap, -east);
        assert_eq!(island, Some(IslandId::West));
        assert!(t >= 6.0, "{t}");
        // Straight down through the void: nothing at all.
        let down = Ray::try_new(gap, Vec3::new(0.0, -1.0, 0.0)).unwrap();
        assert!(scene.closest_hit(down, 0.0, f32::INFINITY).is_none());
    }

    #[test]
    fn arena_ores_resolve_through_dda_to_their_materials() {
        let scene = arena_scene();
        let grid = scene.voxel_grid().unwrap();
        let materials = scene.block_materials().unwrap();
        let mut found = [0; 4];

        for (position, block) in occupied_voxels(&scene) {
            let Some(index) = ORES.iter().position(|&ore| ore == block) else {
                continue;
            };
            let open = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .into_iter()
                .map(|(dx, dz)| VoxelPosition::new(position.x + dx, position.y, position.z + dz))
                .find(|&n| !grid.is_occupied_world(n))
                .expect("ores are only placed with an open side");
            let start = voxel_center(open);
            let ray = Ray::try_new(start, voxel_center(position) - start).unwrap();

            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

            assert_eq!(hit.source, HitSource::Voxel { position, block });
            assert_eq!(hit.material_id, materials.material(block));
            found[index] += 1;
        }
        assert!(found.iter().all(|&count| count > 0), "{found:?}");
    }

    #[test]
    fn arena_terrain_casts_shadows_and_the_void_does_not() {
        let scene = arena_scene();
        let layout = ArenaLayout::eggwars();
        let up = Vec3::new(0.0, 1.0, 0.0);
        let sun = Vec3::new(0.6, 1.0, 0.8);

        for id in IslandId::ALL {
            let center = layout.config(id).center;
            let below = Vec3::new(center.x as f32 + 0.5, -30.0, center.z as f32 + 0.5);
            assert!(
                scene.is_occluded(Ray::try_new(below, up).unwrap(), 0.0, f32::INFINITY),
                "{id:?}"
            );
        }
        // Below the central island, on the line from its center grass away from the directional
        // light: in open air, yet shaded by the island toward the light.
        let toward_sun = Ray::try_new(Vec3::ZERO, sun).unwrap().direction();
        let shaded = Vec3::new(0.5, 0.5, 0.5) - toward_sun * 20.0;
        let cell = VoxelPosition::new(
            shaded.x.floor() as i32,
            shaded.y.floor() as i32,
            shaded.z.floor() as i32,
        );
        assert!(!scene.voxel_grid().unwrap().is_occupied_world(cell));
        assert!(scene.is_occluded(Ray::try_new(shaded, sun).unwrap(), 0.0, f32::INFINITY));
        // In the gaps the sky is open.
        for gap in [Vec3::new(-18.0, -30.0, 0.5), Vec3::new(0.5, -30.0, 18.0)] {
            assert!(!scene.is_occluded(Ray::try_new(gap, up).unwrap(), 0.0, f32::INFINITY));
        }
    }

    #[test]
    fn initial_eggwars_camera_frames_all_five_islands_without_clipping() {
        let scene = arena_scene();
        let layout = ArenaLayout::eggwars();
        let camera = eggwars_camera(WIDTH as f32 / HEIGHT as f32);
        let (columns, rows) = (160, 90);
        let mut seen = [0; 5];

        for row in 0..rows {
            for column in 0..columns {
                let u = column as f32 / (columns - 1) as f32;
                let v = row as f32 / (rows - 1) as f32;
                let ray = camera.ray_for_viewport(u, v).unwrap();
                let Some(hit) = scene.closest_hit(ray, 0.0, f32::INFINITY) else {
                    continue;
                };
                let island = hit_island(&layout, hit.source).expect("only terrain in the arena");
                seen[island as usize] += 1;
                let border = row == 0 || column == 0 || row == rows - 1 || column == columns - 1;
                assert!(
                    !border,
                    "{island:?} touches the frame edge at ({column}, {row})"
                );
            }
        }
        // Every island is visible, and the central island covers more of the frame than the far
        // outer islands behind it.
        assert!(seen.iter().all(|&pixels| pixels > 50), "{seen:?}");
        assert!(seen[IslandId::Center as usize] > seen[IslandId::North as usize]);
        assert!(seen[IslandId::Center as usize] > seen[IslandId::West as usize]);

        let target = camera.target();
        assert_eq!((target.x, target.z), (0.0, 0.0));
        assert!(
            camera.position().y > 1.0,
            "the camera looks down onto the grass"
        );
    }

    #[test]
    fn eggwars_orbit_and_zoom_render_the_same_unchanged_arena() {
        let scene = arena_scene();
        let generated = scene.voxel_grid().unwrap().clone();
        let lighting = phase2_lighting();
        let environment = Environment::sunset();
        let mut framebuffer = Framebuffer::try_new(32, 18).unwrap();
        let mut camera = eggwars_camera(32.0 / 18.0);
        let initial = camera;
        let mut renders = Vec::new();

        for _ in 0..6 {
            Renderer::render(&camera, &scene, &lighting, &environment, &mut framebuffer).unwrap();
            assert!(framebuffer.pixels().iter().all(|color| color.is_finite()));
            renders.push(framebuffer.pixels().to_vec());
            camera.orbit_yaw(1.1);
            camera.orbit_pitch(-0.1);
            camera.adjust_radius(-9.0);
        }

        assert_ne!(camera, initial);
        assert!(camera.radius() < initial.radius());
        assert!(renders.windows(2).all(|pair| pair[0] != pair[1]));
        assert_eq!(scene.voxel_grid(), Some(&generated));
        assert_eq!(&eggwars_arena(ARENA_SEED).grid, scene.voxel_grid().unwrap());
    }

    #[test]
    fn eggwars_arena_renders_from_assorted_viewpoints() {
        let scene = arena_scene();
        let lighting = phase2_lighting();
        let environment = Environment::sunset();
        let mut framebuffer = Framebuffer::try_new(32, 18).unwrap();
        let target = eggwars_camera(1.0).target();

        for yaw_step in 0..8 {
            for pitch in [-1.2, -0.3, 0.35, 1.3] {
                // Inside the central island, beside it, across the arena, and far outside.
                for radius in [2.0, 15.0, 45.0, 120.0] {
                    let camera = OrbitalCamera::try_new(
                        target,
                        yaw_step as f32 * FRAC_PI_4 + 0.2,
                        pitch,
                        radius,
                        50.0_f32.to_radians(),
                        32.0 / 18.0,
                    )
                    .unwrap();
                    Renderer::render(&camera, &scene, &lighting, &environment, &mut framebuffer)
                        .unwrap();
                    assert!(framebuffer.pixels().iter().all(|color| color.is_finite()));
                }
            }
        }
    }

    #[test]
    fn eggwars_tiling_matches_the_sequential_reference() {
        let scene = arena_scene();
        let lighting = phase2_lighting();

        for (width, height) in [(96, 54), (70, 41)] {
            let camera = eggwars_camera(width as f32 / height as f32);
            assert_tiling_matches_reference(&scene, &lighting, &camera, width, height);
        }
    }

    fn constructed_scene() -> Scene {
        eggwars_scene(eggwars_world(ARENA_SEED).grid).unwrap()
    }

    fn down_onto(position: VoxelPosition) -> Ray {
        let origin = Vec3::new(position.x as f32 + 0.5, 30.0, position.z as f32 + 0.5);
        Ray::try_new(origin, Vec3::new(0.0, -1.0, 0.0)).unwrap()
    }

    #[test]
    fn constructed_eggwars_scene_attaches_one_taller_grid_and_nothing_else() {
        let world = eggwars_world(ARENA_SEED);
        let scene = eggwars_scene(world.grid.clone()).unwrap();

        assert_eq!(scene.voxel_grid(), Some(&world.grid));
        assert_eq!(scene.len(), 0, "no block is also an AABB object");
        assert!(scene.point_lights().is_empty());
        assert_eq!(world.terrain.grid_dimensions, (80, 20, 80));
        assert_eq!(
            world.terrain.islands,
            eggwars_arena(ARENA_SEED).info.islands
        );
    }

    #[test]
    fn bridges_render_through_the_dda_with_normal_mapped_cobblestone() {
        let world = eggwars_world(ARENA_SEED);
        let scene = eggwars_scene(world.grid.clone()).unwrap();
        let materials = scene.block_materials().unwrap();

        for bridge in world.construction.bridges {
            let (dx, dz) = bridge.toward_center;
            for lane in bridge.lanes {
                // Straight down onto each lane's first deck block, then along the deck to the
                // first missing cell, through which the ray falls into the void.
                let hit = scene.closest_hit(down_onto(lane.outer_end), 0.0, f32::INFINITY);
                let hit = hit.unwrap();
                assert_eq!(
                    hit.source,
                    HitSource::Voxel {
                        position: lane.outer_end,
                        block: BlockType::Cobblestone,
                    }
                );
                assert_eq!(hit.material_id, materials.cobblestone);
                let material = scene.material(hit.material_id).unwrap();
                assert!(material.normal_map().is_some());

                let mut cell = lane.outer_end;
                while cell != lane.center_end && world.grid.is_occupied_world(cell) {
                    cell = VoxelPosition::new(cell.x + dx, cell.y, cell.z + dz);
                }
                let gap = scene.closest_hit(down_onto(cell), 0.0, f32::INFINITY);
                match bridge.state {
                    BridgeState::Complete => {
                        assert!(gap.is_some(), "the complete bridge has no gap")
                    }
                    _ => assert!(gap.is_none(), "{:?} gap at {cell:?}", bridge.island),
                }
            }
        }
    }

    #[test]
    fn defenses_resolve_to_canonical_reflective_obsidian() {
        let world = eggwars_world(ARENA_SEED);
        let scene = eggwars_scene(world.grid.clone()).unwrap();
        let materials = scene.block_materials().unwrap();

        for base in world.construction.bases {
            for slot in base.defense {
                let hit = scene
                    .closest_hit(down_onto(slot.position), 0.0, f32::INFINITY)
                    .unwrap();
                let HitSource::Voxel { position, block } = hit.source else {
                    panic!("the arena has no AABB objects");
                };
                if slot.intact {
                    assert_eq!((position, block), (slot.position, BlockType::Obsidian));
                    assert_eq!(hit.material_id, materials.obsidian);
                    let obsidian = scene.material(hit.material_id).unwrap();
                    assert_eq!(obsidian.reflectivity(), 0.35);
                    assert_eq!(obsidian.transparency(), 0.0);
                } else {
                    // A broken slot shows the floor under the egg level: the ring stood directly
                    // on it, and a broken cap lets the ray fall through the empty egg cell.
                    assert_eq!(position.y, base.egg.y - 1, "{:?}", base.island);
                    assert_ne!(block, BlockType::Obsidian);
                }
            }
        }
    }

    #[test]
    fn central_windows_refract_through_canonical_glass() {
        let world = eggwars_world(ARENA_SEED);
        let scene = eggwars_scene(world.grid.clone()).unwrap();
        let materials = scene.block_materials().unwrap();
        let window = VoxelPosition::new(-2, 3, -4);

        // From inside the ruin, straight at the pane.
        let origin = Vec3::new(-1.5, 3.5, -2.5);
        let ray = Ray::try_new(origin, Vec3::new(0.0, 0.0, -1.0)).unwrap();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        assert_eq!(
            hit.source,
            HitSource::Voxel {
                position: window,
                block: BlockType::Glass,
            }
        );
        assert_eq!(hit.material_id, materials.glass);
        let glass = scene.material(hit.material_id).unwrap();
        assert!(glass.transparency() > 0.0);
        assert_eq!(glass.ior(), 1.5);
        assert_eq!(glass.reflectivity(), 0.15);

        // Looking at the wall, the panes change the image: light passes through them.
        let lighting = phase2_lighting();
        let environment = Environment::sunset();
        let camera = OrbitalCamera::try_new(
            Vec3::new(-1.0, 3.5, -4.0),
            0.2,
            0.1,
            6.0,
            50.0_f32.to_radians(),
            32.0 / 18.0,
        )
        .unwrap();
        let mut with_glass = Framebuffer::try_new(32, 18).unwrap();
        Renderer::render(&camera, &scene, &lighting, &environment, &mut with_glass).unwrap();
        let mut grid = world.grid.clone();
        for pane in [window, VoxelPosition::new(0, 3, -4)] {
            grid.set_world(pane, Voxel::Block(BlockType::Cobblestone))
                .unwrap();
        }
        let solid = eggwars_scene(grid).unwrap();
        let mut without = Framebuffer::try_new(32, 18).unwrap();
        Renderer::render(&camera, &solid, &lighting, &environment, &mut without).unwrap();
        assert!(with_glass.pixels().iter().all(|color| color.is_finite()));
        assert_ne!(with_glass.pixels(), without.pixels());
    }

    #[test]
    fn construction_casts_shadows_and_bridge_gaps_do_not() {
        let world = eggwars_world(ARENA_SEED);
        let scene = eggwars_scene(world.grid.clone()).unwrap();
        let up = Vec3::new(0.0, 1.0, 0.0);
        let below = |p: VoxelPosition| Vec3::new(p.x as f32 + 0.5, -20.0, p.z as f32 + 0.5);

        let north = world.construction.bridge(IslandId::North).unwrap();
        let (dx, dz) = north.toward_center;
        let start = north.lanes[0].outer_end;
        // Under the middle of the complete bridge, in the void: the deck shades it.
        let middle = VoxelPosition::new(start.x + dx * 6, 0, start.z + dz * 6);
        assert!(scene.is_occluded(Ray::try_new(below(middle), up).unwrap(), 0.0, f32::INFINITY));

        let east = world.construction.bridge(IslandId::East).unwrap();
        let (dx, dz) = east.toward_center;
        let mut cell = east.lanes[0].outer_end;
        while world.grid.is_occupied_world(cell) {
            cell = VoxelPosition::new(cell.x + dx, cell.y, cell.z + dz);
        }
        assert!(!scene.is_occluded(Ray::try_new(below(cell), up).unwrap(), 0.0, f32::INFINITY));

        // The intact front-right defense block shades the empty egg cell from the light.
        let sun = Vec3::new(0.6, 1.0, 0.8);
        let base = world.construction.base(IslandId::North).unwrap();
        let slot = base.defense[2].position;
        let shaded = slot.min_corner() + Vec3::new(0.5, 0.05, 0.5) - Vec3::new(0.6, 0.0, 0.8);
        assert!(!world.grid.is_occupied_world(VoxelPosition::new(
            shaded.x.floor() as i32,
            shaded.y.floor() as i32,
            shaded.z.floor() as i32
        )));
        assert_eq!(
            VoxelPosition::new(
                shaded.x.floor() as i32,
                shaded.y.floor() as i32,
                shaded.z.floor() as i32
            ),
            base.egg
        );
        assert!(scene.is_occluded(Ray::try_new(shaded, sun).unwrap(), 0.0, f32::INFINITY));
    }

    #[test]
    fn initial_eggwars_camera_frames_the_constructed_arena_without_clipping() {
        let world = eggwars_world(ARENA_SEED);
        let terrain = eggwars_arena(ARENA_SEED).grid;
        let scene = eggwars_scene(world.grid.clone()).unwrap();
        let layout = ArenaLayout::eggwars();
        let camera = eggwars_camera(WIDTH as f32 / HEIGHT as f32);
        let (columns, rows) = (160, 90);
        let mut seen = [0; 5];
        let mut construction = 0;

        for row in 0..rows {
            for column in 0..columns {
                let u = column as f32 / (columns - 1) as f32;
                let v = row as f32 / (rows - 1) as f32;
                let ray = camera.ray_for_viewport(u, v).unwrap();
                let Some(hit) = scene.closest_hit(ray, 0.0, f32::INFINITY) else {
                    continue;
                };
                let border = row == 0 || column == 0 || row == rows - 1 || column == columns - 1;
                assert!(
                    !border,
                    "the arena touches the frame edge at ({column}, {row})"
                );
                let HitSource::Voxel { position, .. } = hit.source else {
                    panic!("the arena has no AABB objects");
                };
                if !terrain.is_occupied_world(position) {
                    construction += 1;
                }
                if let Some(island) = hit_island(&layout, hit.source) {
                    seen[island as usize] += 1;
                }
            }
        }
        assert!(seen.iter().all(|&pixels| pixels > 50), "{seen:?}");
        assert!(seen[IslandId::Center as usize] > seen[IslandId::North as usize]);
        assert!(
            construction > 100,
            "bridges and structures are in view: {construction}"
        );
    }

    #[test]
    fn constructed_arena_renders_from_assorted_viewpoints_without_rebuilding() {
        let scene = constructed_scene();
        let lighting = phase2_lighting();
        let environment = Environment::sunset();
        let mut framebuffer = Framebuffer::try_new(32, 18).unwrap();
        let world = eggwars_world(ARENA_SEED);
        let mut targets = vec![eggwars_camera(1.0).target(), Vec3::new(0.0, 3.0, 0.0)];
        targets.extend(OUTER_ISLANDS.map(|id| {
            let egg = world.construction.base(id).unwrap().egg;
            egg.min_corner() + Vec3::new(0.5, 0.5, 0.5)
        }));

        for target in targets {
            for yaw_step in 0..8 {
                for pitch in [-0.9, 0.2, 0.7, 1.4] {
                    // Inside structures, beside them, across the arena, and far outside.
                    for radius in [1.0, 6.0, 30.0, 110.0] {
                        let camera = OrbitalCamera::try_new(
                            target,
                            yaw_step as f32 * FRAC_PI_4 + 0.2,
                            pitch,
                            radius,
                            50.0_f32.to_radians(),
                            32.0 / 18.0,
                        )
                        .unwrap();
                        Renderer::render(
                            &camera,
                            &scene,
                            &lighting,
                            &environment,
                            &mut framebuffer,
                        )
                        .unwrap();
                        assert!(framebuffer.pixels().iter().all(|color| color.is_finite()));
                    }
                }
            }
        }
        // Camera movement never touches the world: it is still exactly a fresh build.
        assert_eq!(scene.voxel_grid(), Some(&world.grid));
    }

    #[test]
    fn constructed_eggwars_tiling_matches_the_sequential_reference() {
        let scene = constructed_scene();
        let lighting = phase2_lighting();

        for (width, height) in [(96, 54), (70, 41)] {
            let camera = eggwars_camera(width as f32 / height as f32);
            assert_tiling_matches_reference(&scene, &lighting, &camera, width, height);
        }
    }

    #[test]
    fn rendering_has_no_arena_specific_paths() {
        // The arena is ordinary voxel data: nothing in the renderer, scene queries, voxel
        // traversal, or lighting knows about islands, arenas, or EggWars.
        let sources = [
            include_str!("render/renderer.rs"),
            include_str!("render/parallel.rs"),
            include_str!("render/tile.rs"),
            include_str!("scene/scene.rs"),
            include_str!("voxel/traversal.rs"),
            include_str!("voxel/grid.rs"),
            include_str!("lighting/lighting.rs"),
            include_str!("environment/environment.rs"),
        ];
        for source in sources {
            let source = source.to_lowercase();
            for word in ["eggwars", "arena", "islandid", "island_seed"] {
                assert!(!source.contains(word), "renderer source mentions {word}");
            }
        }
    }

    // ---- 640x360 development resolution ----

    #[test]
    fn internal_resolution_is_640x360_shown_at_an_exact_2x_in_a_1280x720_window() {
        assert_eq!((WIDTH, HEIGHT), (640, 360));
        assert_eq!(WIDTH * HEIGHT, 230_400);
        assert_eq!(WIDTH * 9, HEIGHT * 16, "16:9");
        assert_eq!(WINDOW_SCALE, 2);
        let (window_width, window_height) = window_size(WIDTH as i32, HEIGHT as i32);
        assert_eq!((window_width, window_height), (1280, 720));
        assert_eq!(window_width % WIDTH as i32, 0);
        assert_eq!(window_height / HEIGHT as i32, WINDOW_SCALE);
        // The framebuffer the app allocates and the RGBA upload buffer derived from it.
        let framebuffer = Framebuffer::try_new(WIDTH, HEIGHT).unwrap();
        assert_eq!(framebuffer.pixels().len(), 230_400);
        assert_eq!(framebuffer.pixels().len() * 4, 921_600);
    }

    #[test]
    fn cameras_keep_their_composition_at_the_new_aspect_ratio() {
        let aspect = WIDTH as f32 / HEIGHT as f32;
        assert_eq!(aspect, 16.0 / 9.0);
        // Same 16:9 aspect as the old 320x180 frame, so FOV, yaw, pitch, radius, and target give
        // the identical camera and the initial composition does not move.
        assert_eq!(eggwars_camera(aspect), eggwars_camera(320.0 / 180.0));
        assert_eq!(
            procedural_island_camera(aspect),
            procedural_island_camera(320.0 / 180.0)
        );
        // The framebuffer's own corners and center map to the camera's viewport corners.
        let camera = eggwars_camera(aspect);
        let ray = |column: usize, row: usize| {
            camera
                .ray_for_viewport(
                    (column as f32 + 0.5) / WIDTH as f32,
                    (row as f32 + 0.5) / HEIGHT as f32,
                )
                .unwrap()
        };
        assert_eq!(
            ray(0, 0).direction().x.signum(),
            -ray(WIDTH - 1, HEIGHT - 1).direction().x.signum()
        );
    }

    #[test]
    fn ppm_export_has_the_640x360_header_and_exact_payload() {
        let path =
            std::env::temp_dir().join(format!("minecraft_diorama_640_{}.ppm", process::id()));
        let framebuffer = Framebuffer::try_new(WIDTH, HEIGHT).unwrap();

        write_ppm(&path, &framebuffer).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        let header = b"P6\n640 360\n255\n";
        assert_eq!(&bytes[..header.len()], header);
        assert_eq!(bytes.len(), header.len() + 640 * 360 * 3);
    }

    #[test]
    fn every_scene_mode_renders_finite_pixels_at_the_new_resolution() {
        let lighting = phase2_lighting();
        let environment = Environment::sunset();
        let aspect = WIDTH as f32 / HEIGHT as f32;
        let world = eggwars_world(ARENA_SEED);
        let scenes = [
            (
                phase2_test_scene().unwrap(),
                development_camera(WIDTH, HEIGHT),
            ),
            (
                voxel_diagnostic_scene().unwrap(),
                development_camera(WIDTH, HEIGHT),
            ),
            (island_scene(), procedural_island_camera(aspect)),
            (
                eggwars_scene(world.grid.clone()).unwrap(),
                eggwars_camera(aspect),
            ),
        ];
        let mut framebuffer = Framebuffer::try_new(WIDTH, HEIGHT).unwrap();
        for (scene, camera) in &scenes {
            Renderer::render(camera, scene, &lighting, &environment, &mut framebuffer).unwrap();
            assert_eq!(framebuffer.pixels().len(), 230_400);
            assert!(framebuffer.pixels().iter().all(|color| color.is_finite()));
            // Not a blank frame: the sky and the scene differ.
            assert!(
                framebuffer
                    .pixels()
                    .windows(2)
                    .any(|pair| pair[0] != pair[1])
            );
        }
    }

    #[test]
    fn eggwars_640x360_output_ignores_workers_tile_size_and_repetition() {
        let world = eggwars_world(ARENA_SEED);
        let scene = eggwars_scene(world.grid.clone()).unwrap();
        let lighting = phase2_lighting();
        let environment = Environment::sunset();
        let camera = eggwars_camera(WIDTH as f32 / HEIGHT as f32);
        let render = |workers: Option<usize>, tile: usize| {
            let mut framebuffer = Framebuffer::try_new(WIDTH, HEIGHT).unwrap();
            match (workers, tile) {
                (Some(workers), _) => Renderer::render_with_worker_count(
                    &camera,
                    &scene,
                    &lighting,
                    &environment,
                    &mut framebuffer,
                    workers,
                ),
                (None, 16) => {
                    Renderer::render(&camera, &scene, &lighting, &environment, &mut framebuffer)
                }
                (None, tile) => Renderer::render_with_tile_size(
                    &camera,
                    &scene,
                    &lighting,
                    &environment,
                    &mut framebuffer,
                    tile,
                    tile,
                ),
            }
            .unwrap();
            framebuffer
        };

        let sequential = render(Some(1), 16);
        assert_eq!(sequential, render(Some(1), 16), "repeated render");
        assert_eq!(sequential, render(Some(3), 16), "3 workers");
        assert_eq!(sequential, render(None, 16), "automatic workers");
        assert_eq!(sequential, render(None, 8), "8x8 tiles");
        assert_eq!(sequential, render(None, 64), "64x64 tiles");
        // Moving the camera never touches the generated world, and the world is a pure function
        // of the seed.
        assert_eq!(scene.voxel_grid(), Some(&eggwars_world(ARENA_SEED).grid));
    }
}
