use std::{
    error::Error,
    f32::consts::PI,
    path::{Path, PathBuf},
};

use app::run;
use camera::OrbitalCamera;
use environment::Environment;
use geometry::Aabb;
use lighting::{AmbientLight, DirectionalLight, Lighting};
use material::{CanonicalTextureIds, PpmLoadError, TextureId, load_ppm};
use math::Vec3;
use render::{Color, Framebuffer};
use scene::{Scene, SceneObject};

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

const WIDTH: usize = 320;
const HEIGHT: usize = 180;
const OUTPUT_PATH: &str = "output/phase1.ppm";

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
    let scene = phase2_test_scene()?;
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
        Path::new(OUTPUT_PATH),
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

fn phase2_test_scene() -> Result<Scene, PpmLoadError> {
    let mut scene = Scene::with_capacity(5);

    let texture_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/textures");
    let textures = CanonicalTextureIds {
        grass_top: register_texture(&mut scene, &texture_directory, "grass_top.ppm")?,
        grass_side: register_texture(&mut scene, &texture_directory, "grass_side.ppm")?,
        dirt: register_texture(&mut scene, &texture_directory, "dirt.ppm")?,
        cobblestone: register_texture(&mut scene, &texture_directory, "cobblestone.ppm")?,
        obsidian: register_texture(&mut scene, &texture_directory, "obsidian.ppm")?,
        glass: register_texture(&mut scene, &texture_directory, "glass.ppm")?,
        lava: register_texture(&mut scene, &texture_directory, "lava.ppm")?,
    };
    let materials = scene
        .add_canonical_materials(textures)
        .expect("canonical Phase 2 materials must be valid and fit MaterialId");

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
