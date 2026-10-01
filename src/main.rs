use std::{
    error::Error,
    f32::consts::PI,
    path::{Path, PathBuf},
};

use app::run;
use camera::OrbitalCamera;
use environment::Environment;
use geometry::Aabb;
use lighting::{AmbientLight, DirectionalLight, Lighting, PointLight};
use material::{AuxiliaryTextureIds, CanonicalTextureIds, PpmLoadError, TextureId, load_ppm};
use math::Vec3;
use render::{Color, Framebuffer};
use scene::{Scene, SceneObject};
use voxel::{BlockMaterials, BlockType, Voxel, VoxelGrid, VoxelPosition};

pub mod app;
pub mod camera;
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

const WIDTH: usize = 320;
const HEIGHT: usize = 180;
const OUTPUT_PATH: &str = "output/phase1.ppm";
const VOXEL_DIAGNOSTIC_FLAG: &str = "--voxel-diagnostic";
const VOXEL_DIAGNOSTIC_OUTPUT_PATH: &str = "output/voxel_diagnostic.ppm";

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
    let aspect_ratio = WIDTH as f32 / HEIGHT as f32;
    let camera = OrbitalCamera::try_new(
        Vec3::new(0.0, 0.7, 0.0),
        0.55 + PI,
        0.15,
        10.0,
        50.0 * PI / 180.0,
        aspect_ratio,
    )
    .expect("Phase 2 camera configuration must be valid");
    // The default showcase stays the Phase 3 five-block AABB scene so its render remains a
    // regression reference; the flag swaps in the small hybrid voxel scene instead.
    let voxel_diagnostic = std::env::args().any(|argument| argument == VOXEL_DIAGNOSTIC_FLAG);
    let (scene, output_path) = if voxel_diagnostic {
        (voxel_diagnostic_scene()?, VOXEL_DIAGNOSTIC_OUTPUT_PATH)
    } else {
        (phase2_test_scene()?, OUTPUT_PATH)
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
        Path::new(output_path),
    )?;
    Ok(())
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
    use std::f32::consts::FRAC_PI_4;

    use super::{phase2_lighting, phase2_test_scene, voxel_diagnostic_scene};
    use crate::{
        camera::OrbitalCamera,
        environment::Environment,
        material::TextureSelection,
        math::Vec3,
        ray::Ray,
        render::{Framebuffer, Renderer},
        scene::HitSource,
        voxel::{BlockType, VoxelPosition},
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
}
