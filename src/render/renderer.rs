use crate::{
    camera::OrbitalCamera,
    scene::{Scene, SceneHit},
};

use super::{Color, Framebuffer};

const PRIMARY_RAY_T_MIN: f32 = 0.0;
const ASPECT_RATIO_TOLERANCE: f32 = 1.0e-5;

const BACKGROUND_BOTTOM: Color = Color::new(0.06, 0.08, 0.16);
const BACKGROUND_TOP: Color = Color::new(0.48, 0.68, 0.92);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    AspectRatioMismatch,
    PrimaryRayGenerationFailed,
    MaterialNotFound,
    TextureNotFound,
    SurfaceUvUnavailable,
    TextureSamplingFailed,
}

pub struct Renderer;

impl Renderer {
    pub fn render(
        camera: &OrbitalCamera,
        scene: &Scene,
        framebuffer: &mut Framebuffer,
    ) -> Result<(), RenderError> {
        let width = framebuffer.width();
        let height = framebuffer.height();
        let framebuffer_aspect = width as f32 / height as f32;

        if (camera.aspect_ratio() - framebuffer_aspect).abs() > ASPECT_RATIO_TOLERANCE {
            return Err(RenderError::AspectRatioMismatch);
        }

        for y in 0..height {
            for x in 0..width {
                let (u, v) = pixel_center(x, y, width, height);
                let ray = camera
                    .ray_for_viewport(u, v)
                    .ok_or(RenderError::PrimaryRayGenerationFailed)?;
                let color = match scene.closest_hit(ray, PRIMARY_RAY_T_MIN, f32::INFINITY) {
                    Some(hit) => shade_hit(scene, hit)?,
                    None => background(v),
                };

                framebuffer.set_pixel(x, y, color);
            }
        }

        Ok(())
    }
}

fn shade_hit(scene: &Scene, hit: SceneHit) -> Result<Color, RenderError> {
    let material = scene
        .material(hit.material_id)
        .ok_or(RenderError::MaterialNotFound)?;
    let texture_id = material.textures().for_face(hit.geometry.face);
    let texture = scene
        .texture(texture_id)
        .ok_or(RenderError::TextureNotFound)?;
    let uv = hit.geometry.uv.ok_or(RenderError::SurfaceUvUnavailable)?;
    let texture_sample = texture
        .sample_nearest(uv.u, uv.v)
        .ok_or(RenderError::TextureSamplingFailed)?;

    Ok(debug_shade(
        material.surface_color(texture_sample),
        hit.geometry.normal,
    ))
}

fn pixel_center(x: usize, y: usize, width: usize, height: usize) -> (f32, f32) {
    (
        (x as f32 + 0.5) / width as f32,
        (y as f32 + 0.5) / height as f32,
    )
}

fn debug_shade(base_color: Color, normal: crate::math::Vec3) -> Color {
    let face_factor = if normal.y > 0.5 {
        1.0
    } else if normal.y < -0.5 {
        0.55
    } else {
        0.75
    };

    base_color.scale(face_factor)
}

fn background(v: f32) -> Color {
    BACKGROUND_BOTTOM.lerp(BACKGROUND_TOP, 1.0 - v)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::{RenderError, Renderer, pixel_center, shade_hit};
    use crate::{
        camera::OrbitalCamera,
        geometry::Aabb,
        material::{Material, MaterialId, Texture, TextureId, TextureSelection},
        math::Vec3,
        ray::Ray,
        render::{Color, Framebuffer},
        scene::{Scene, SceneObject},
    };

    fn add_solid_material(scene: &mut Scene, color: Color) -> MaterialId {
        let texture_id = scene.add_texture(Texture::solid(color)).unwrap();
        scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture_id),
                    Color::WHITE,
                    0.0,
                    0.0,
                    0.0,
                )
                .unwrap(),
            )
            .unwrap()
    }

    #[test]
    fn pixel_centers_stay_inside_normalized_viewport() {
        let top_left = pixel_center(0, 0, 4, 2);
        let bottom_right = pixel_center(3, 1, 4, 2);

        assert!(top_left.0 > 0.0 && top_left.0 < 1.0);
        assert!(top_left.1 > 0.0 && top_left.1 < 1.0);
        assert!(bottom_right.0 > 0.0 && bottom_right.0 < 1.0);
        assert!(bottom_right.1 > 0.0 && bottom_right.1 < 1.0);
    }

    #[test]
    fn tiny_render_contains_hit_and_background_pixels() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let mut scene = Scene::new();
        let red = add_solid_material(&mut scene, Color::new(1.0, 0.0, 0.0));
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5)).unwrap(),
            red,
        ));
        let mut framebuffer = Framebuffer::try_new(3, 3).unwrap();

        Renderer::render(&camera, &scene, &mut framebuffer).unwrap();

        assert_eq!(framebuffer.pixel(1, 1), Some(Color::new(0.75, 0.0, 0.0)));
        assert_ne!(framebuffer.pixel(0, 0), framebuffer.pixel(1, 1));
    }

    #[test]
    fn material_lookup_keeps_object_colors_distinguishable() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let mut scene = Scene::new();
        let red = add_solid_material(&mut scene, Color::new(1.0, 0.0, 0.0));
        let blue = add_solid_material(&mut scene, Color::new(0.0, 0.0, 1.0));
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-2.5, -0.5, -0.5), Vec3::new(-1.5, 0.5, 0.5)).unwrap(),
            red,
        ));
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(1.5, -0.5, -0.5), Vec3::new(2.5, 0.5, 0.5)).unwrap(),
            blue,
        ));
        let mut framebuffer = Framebuffer::try_new(5, 5).unwrap();

        Renderer::render(&camera, &scene, &mut framebuffer).unwrap();

        assert_eq!(framebuffer.pixel(1, 2), Some(Color::new(0.75, 0.0, 0.0)));
        assert_eq!(framebuffer.pixel(3, 2), Some(Color::new(0.0, 0.0, 0.75)));
    }

    #[test]
    fn renderer_samples_distinct_uv_regions_of_asymmetric_texture() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let texture = Texture::try_new(
            2,
            2,
            vec![
                Color::new(1.0, 0.0, 0.0),
                Color::new(0.0, 1.0, 0.0),
                Color::new(0.0, 0.0, 1.0),
                Color::WHITE,
            ],
        )
        .unwrap();
        let mut scene = Scene::new();
        let texture_id = scene.add_texture(texture).unwrap();
        let material_id = scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture_id),
                    Color::WHITE,
                    0.0,
                    0.0,
                    0.0,
                )
                .unwrap(),
            )
            .unwrap();
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-3.0, -3.0, -0.5), Vec3::new(3.0, 3.0, 0.5)).unwrap(),
            material_id,
        ));
        let mut framebuffer = Framebuffer::try_new(2, 2).unwrap();

        Renderer::render(&camera, &scene, &mut framebuffer).unwrap();

        assert_eq!(framebuffer.pixel(0, 0), Some(Color::new(0.75, 0.0, 0.0)));
        assert_eq!(framebuffer.pixel(1, 0), Some(Color::new(0.0, 0.75, 0.0)));
        assert_eq!(framebuffer.pixel(0, 1), Some(Color::new(0.0, 0.0, 0.75)));
        assert_eq!(framebuffer.pixel(1, 1), Some(Color::new(0.75, 0.75, 0.75)));
    }

    #[test]
    fn renderer_selects_top_side_and_bottom_textures_from_hit_face() {
        let mut scene = Scene::new();
        let top = scene
            .add_texture(Texture::solid(Color::new(1.0, 0.0, 0.0)))
            .unwrap();
        let side = scene
            .add_texture(Texture::solid(Color::new(0.0, 1.0, 0.0)))
            .unwrap();
        let bottom = scene
            .add_texture(Texture::solid(Color::new(0.0, 0.0, 1.0)))
            .unwrap();
        let material_id = scene
            .add_material(
                Material::try_new(
                    TextureSelection::TopSideBottom { top, side, bottom },
                    Color::WHITE,
                    0.0,
                    0.0,
                    0.0,
                )
                .unwrap(),
            )
            .unwrap();
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            material_id,
        ));

        let cases = [
            (
                Vec3::new(0.5, 2.0, 0.5),
                Vec3::new(0.0, -1.0, 0.0),
                Color::new(1.0, 0.0, 0.0),
            ),
            (
                Vec3::new(0.5, 0.5, 2.0),
                Vec3::new(0.0, 0.0, -1.0),
                Color::new(0.0, 0.75, 0.0),
            ),
            (
                Vec3::new(0.5, -1.0, 0.5),
                Vec3::new(0.0, 1.0, 0.0),
                Color::new(0.0, 0.0, 0.55),
            ),
        ];

        for (origin, direction, expected) in cases {
            let ray = Ray::try_new(origin, direction).unwrap();
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            assert_eq!(shade_hit(&scene, hit), Ok(expected));
        }
    }

    #[test]
    fn rejects_camera_framebuffer_aspect_mismatch() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let scene = Scene::new();
        let mut framebuffer = Framebuffer::try_new(4, 2).unwrap();

        assert_eq!(
            Renderer::render(&camera, &scene, &mut framebuffer),
            Err(RenderError::AspectRatioMismatch)
        );
    }

    #[test]
    fn rejects_scene_object_with_unknown_material() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let mut scene = Scene::new();
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5)).unwrap(),
            MaterialId::new(0),
        ));
        let mut framebuffer = Framebuffer::try_new(3, 3).unwrap();

        assert_eq!(
            Renderer::render(&camera, &scene, &mut framebuffer),
            Err(RenderError::MaterialNotFound)
        );
    }

    #[test]
    fn rejects_material_with_unknown_texture() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let mut scene = Scene::new();
        let material_id = scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(TextureId::new(0)),
                    Color::WHITE,
                    0.0,
                    0.0,
                    0.0,
                )
                .unwrap(),
            )
            .unwrap();
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5)).unwrap(),
            material_id,
        ));
        let mut framebuffer = Framebuffer::try_new(3, 3).unwrap();

        assert_eq!(
            Renderer::render(&camera, &scene, &mut framebuffer),
            Err(RenderError::TextureNotFound)
        );
    }
}
