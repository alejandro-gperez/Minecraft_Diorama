use crate::{camera::OrbitalCamera, scene::Scene};

use super::{Color, Framebuffer};

const PRIMARY_RAY_T_MIN: f32 = 0.0;
const ASPECT_RATIO_TOLERANCE: f32 = 1.0e-5;

const BACKGROUND_BOTTOM: Color = Color::new(0.06, 0.08, 0.16);
const BACKGROUND_TOP: Color = Color::new(0.48, 0.68, 0.92);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    AspectRatioMismatch,
    PrimaryRayGenerationFailed,
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
                    Some(hit) => debug_shade(hit.color, hit.geometry.normal),
                    None => background(v),
                };

                framebuffer.set_pixel(x, y, color);
            }
        }

        Ok(())
    }
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

    use super::{RenderError, Renderer, pixel_center};
    use crate::{
        camera::OrbitalCamera,
        geometry::Aabb,
        math::Vec3,
        render::{Color, Framebuffer},
        scene::{Scene, SceneObject},
    };

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
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5)).unwrap(),
            Color::new(1.0, 0.0, 0.0),
        ));
        let mut framebuffer = Framebuffer::try_new(3, 3).unwrap();

        Renderer::render(&camera, &scene, &mut framebuffer).unwrap();

        assert_eq!(framebuffer.pixel(1, 1), Some(Color::new(0.75, 0.0, 0.0)));
        assert_ne!(framebuffer.pixel(0, 0), framebuffer.pixel(1, 1));
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
}
