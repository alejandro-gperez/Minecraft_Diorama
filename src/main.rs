use std::{f32::consts::PI, io, path::Path, time::Instant};

use camera::OrbitalCamera;
use geometry::Aabb;
use math::Vec3;
use output::write_ppm;
use render::{Color, Framebuffer, Renderer};
use scene::{Scene, SceneObject};

pub mod camera;
pub mod color;
pub mod geometry;
pub mod math;
pub mod output;
pub mod ray;
pub mod render;
pub mod scene;

const WIDTH: usize = 320;
const HEIGHT: usize = 180;
const OUTPUT_PATH: &str = "output/phase1.ppm";

fn main() -> io::Result<()> {
    let aspect_ratio = WIDTH as f32 / HEIGHT as f32;
    let camera = OrbitalCamera::try_new(
        Vec3::new(0.0, 0.7, 0.0),
        0.55,
        0.35,
        10.0,
        50.0 * PI / 180.0,
        aspect_ratio,
    )
    .expect("Phase 1 camera configuration must be valid");
    let scene = phase1_test_scene();
    let mut framebuffer =
        Framebuffer::try_new(WIDTH, HEIGHT).expect("development resolution must be valid");

    let started = Instant::now();
    Renderer::render(&camera, &scene, &mut framebuffer)
        .expect("camera and framebuffer aspect ratios must match");
    let render_time = started.elapsed();

    write_ppm(Path::new(OUTPUT_PATH), &framebuffer)?;
    println!(
        "Rendered {OUTPUT_PATH} ({WIDTH}x{HEIGHT}) in {:.2?}",
        render_time
    );

    Ok(())
}

fn phase1_test_scene() -> Scene {
    let mut scene = Scene::with_capacity(5);

    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-4.0, -0.5, -3.0), Vec3::new(4.0, 0.0, 3.0)).unwrap(),
        Color::new(0.32, 0.42, 0.30),
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-0.8, 0.0, -0.8), Vec3::new(0.8, 1.6, 0.8)).unwrap(),
        Color::new(0.82, 0.24, 0.18),
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-2.3, 0.0, -0.2), Vec3::new(-1.2, 1.0, 0.9)).unwrap(),
        Color::new(0.20, 0.48, 0.84),
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(1.2, 0.0, -1.4), Vec3::new(2.2, 2.2, -0.4)).unwrap(),
        Color::new(0.92, 0.66, 0.16),
    ));
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-0.3, 0.0, -2.4), Vec3::new(0.7, 2.0, -1.4)).unwrap(),
        Color::new(0.55, 0.28, 0.72),
    ));

    scene
}
