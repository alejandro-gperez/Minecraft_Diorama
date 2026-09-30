use std::{
    error::Error,
    f32::consts::PI,
    path::{Path, PathBuf},
};

use app::run;
use camera::OrbitalCamera;
use geometry::Aabb;
use material::{Material, MaterialId, PpmLoadError, TextureId, TextureSelection, load_ppm};
use math::Vec3;
use render::{Color, Framebuffer};
use scene::{Scene, SceneObject};

pub mod app;
pub mod camera;
pub mod color;
pub mod geometry;
pub mod material;
pub mod math;
pub mod output;
pub mod ray;
pub mod render;
pub mod scene;

const WIDTH: usize = 320;
const HEIGHT: usize = 180;
const OUTPUT_PATH: &str = "output/phase1.ppm";

fn main() -> Result<(), Box<dyn Error>> {
    let aspect_ratio = WIDTH as f32 / HEIGHT as f32;
    let camera = OrbitalCamera::try_new(
        Vec3::new(0.0, 0.7, 0.0),
        0.55,
        0.35,
        10.0,
        50.0 * PI / 180.0,
        aspect_ratio,
    )
    .expect("Phase 2 camera configuration must be valid");
    let scene = phase2_test_scene()?;
    let framebuffer =
        Framebuffer::try_new(WIDTH, HEIGHT).expect("development resolution must be valid");

    run(camera, scene, framebuffer, Path::new(OUTPUT_PATH))?;
    Ok(())
}

fn phase2_test_scene() -> Result<Scene, PpmLoadError> {
    let mut scene = Scene::with_capacity(5);

    let texture_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/textures");
    let grass_top = register_texture(&mut scene, &texture_directory, "grass_top.ppm")?;
    let grass_side = register_texture(&mut scene, &texture_directory, "grass_side.ppm")?;
    let dirt = register_texture(&mut scene, &texture_directory, "dirt.ppm")?;
    let cobblestone = register_texture(&mut scene, &texture_directory, "cobblestone.ppm")?;
    let obsidian = register_texture(&mut scene, &texture_directory, "obsidian.ppm")?;
    let _glass = register_texture(&mut scene, &texture_directory, "glass.ppm")?;
    let _lava = register_texture(&mut scene, &texture_directory, "lava.ppm")?;
    let _coal = register_texture(&mut scene, &texture_directory, "coal_ore.ppm")?;
    let _iron = register_texture(&mut scene, &texture_directory, "iron_ore.ppm")?;
    let gold = register_texture(&mut scene, &texture_directory, "gold_ore.ppm")?;
    let diamond = register_texture(&mut scene, &texture_directory, "diamond_ore.ppm")?;

    let ground = add_material(
        &mut scene,
        TextureSelection::TopSideBottom {
            top: grass_top,
            side: grass_side,
            bottom: dirt,
        },
    );
    let cobblestone = add_material(&mut scene, TextureSelection::Uniform(cobblestone));
    let diamond = add_material(&mut scene, TextureSelection::Uniform(diamond));
    let gold = add_material(&mut scene, TextureSelection::Uniform(gold));
    let obsidian = add_material(&mut scene, TextureSelection::Uniform(obsidian));

    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-4.0, -0.5, -3.0), Vec3::new(4.0, 0.0, 3.0)).unwrap(),
        ground,
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-0.8, 0.0, -0.8), Vec3::new(0.8, 1.6, 0.8)).unwrap(),
        cobblestone,
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-2.3, 0.0, -0.2), Vec3::new(-1.2, 1.0, 0.9)).unwrap(),
        diamond,
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(1.2, 0.0, -1.4), Vec3::new(2.2, 2.2, -0.4)).unwrap(),
        gold,
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-0.3, 0.0, -2.4), Vec3::new(0.7, 2.0, -1.4)).unwrap(),
        obsidian,
    ));

    Ok(scene)
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

fn add_material(scene: &mut Scene, textures: TextureSelection) -> MaterialId {
    scene
        .add_material(
            Material::try_new(textures, Color::WHITE, 0.0, 0.0, 0.0)
                .expect("test-scene material must be valid"),
        )
        .expect("test-scene material count must fit MaterialId")
}
