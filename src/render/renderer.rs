use crate::{
    camera::OrbitalCamera,
    lighting::{Lighting, shade_surface},
    math::Vec3,
    ray::Ray,
    scene::{Scene, SceneHit},
};

use super::{Color, Framebuffer};

const PRIMARY_RAY_T_MIN: f32 = 0.0;
const SHADOW_RAY_T_MIN: f32 = 0.0;
const SHADOW_RAY_T_MAX: f32 = f32::INFINITY;
/// Moves secondary rays just outside the hit surface to prevent floating-point self-intersection.
const RAY_ORIGIN_BIAS: f32 = 1.0e-4;
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
    ViewDirectionUnavailable,
    ShadowRayGenerationFailed,
}

pub struct Renderer;

impl Renderer {
    pub fn render(
        camera: &OrbitalCamera,
        scene: &Scene,
        lighting: &Lighting,
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
                    Some(hit) => shade_hit(scene, hit, camera.position(), *lighting)?,
                    None => background(v),
                };

                framebuffer.set_pixel(x, y, color);
            }
        }

        Ok(())
    }
}

fn shade_hit(
    scene: &Scene,
    hit: SceneHit,
    camera_position: Vec3,
    lighting: Lighting,
) -> Result<Color, RenderError> {
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

    let view_direction = (camera_position - hit.geometry.position)
        .try_normalized()
        .ok_or(RenderError::ViewDirectionUnavailable)?;
    let light = lighting.directional();
    let direct_light_visible =
        if light.intensity() > 0.0 && hit.geometry.normal.dot(light.direction_to_light()) > 0.0 {
            let shadow_origin = hit.geometry.position + hit.geometry.normal * RAY_ORIGIN_BIAS;
            let shadow_ray = Ray::try_new(shadow_origin, light.direction_to_light())
                .ok_or(RenderError::ShadowRayGenerationFailed)?;
            !scene.is_occluded(shadow_ray, SHADOW_RAY_T_MIN, SHADOW_RAY_T_MAX)
        } else {
            true
        };

    Ok(shade_surface(
        material.surface_color(texture_sample),
        hit.geometry.normal,
        view_direction,
        material.specular(),
        direct_light_visible,
        lighting,
    ))
}

fn pixel_center(x: usize, y: usize, width: usize, height: usize) -> (f32, f32) {
    (
        (x as f32 + 0.5) / width as f32,
        (y as f32 + 0.5) / height as f32,
    )
}

fn background(v: f32) -> Color {
    BACKGROUND_BOTTOM.lerp(BACKGROUND_TOP, 1.0 - v)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::{RAY_ORIGIN_BIAS, RenderError, Renderer, pixel_center, shade_hit};
    use crate::{
        camera::OrbitalCamera,
        geometry::Aabb,
        lighting::{AmbientLight, DirectionalLight, Lighting},
        material::{Material, MaterialId, Texture, TextureId, TextureSelection},
        math::Vec3,
        ray::Ray,
        render::{Color, Framebuffer},
        scene::{Scene, SceneObject},
    };

    fn ambient_only() -> Lighting {
        Lighting::new(
            AmbientLight::try_new(Color::WHITE, 1.0).unwrap(),
            DirectionalLight::try_new(Vec3::new(0.0, 1.0, 0.0), Color::WHITE, 0.0).unwrap(),
        )
    }

    fn directional(direction_to_light: Vec3) -> Lighting {
        Lighting::new(
            AmbientLight::try_new(Color::BLACK, 0.0).unwrap(),
            DirectionalLight::try_new(direction_to_light, Color::WHITE, 1.0).unwrap(),
        )
    }

    fn upward_light_with_ambient() -> Lighting {
        Lighting::new(
            AmbientLight::try_new(Color::WHITE, 0.2).unwrap(),
            DirectionalLight::try_new(Vec3::new(0.0, 1.0, 0.0), Color::WHITE, 1.0).unwrap(),
        )
    }

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

    fn top_hit(scene: &Scene) -> (crate::scene::SceneHit, Vec3) {
        let camera_position = Vec3::new(0.5, 2.0, 0.5);
        let ray = Ray::try_new(camera_position, Vec3::new(0.0, -1.0, 0.0)).unwrap();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        (hit, camera_position)
    }

    fn add_blocker(scene: &mut Scene, min: Vec3, max: Vec3) {
        scene.add(SceneObject::new(
            Aabb::try_new(min, max).unwrap(),
            MaterialId::new(u32::MAX),
        ));
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

        Renderer::render(&camera, &scene, &ambient_only(), &mut framebuffer).unwrap();

        assert_eq!(framebuffer.pixel(1, 1), Some(Color::new(1.0, 0.0, 0.0)));
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

        Renderer::render(&camera, &scene, &ambient_only(), &mut framebuffer).unwrap();

        assert_eq!(framebuffer.pixel(1, 2), Some(Color::new(1.0, 0.0, 0.0)));
        assert_eq!(framebuffer.pixel(3, 2), Some(Color::new(0.0, 0.0, 1.0)));
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

        Renderer::render(&camera, &scene, &ambient_only(), &mut framebuffer).unwrap();

        assert_eq!(framebuffer.pixel(0, 0), Some(Color::new(1.0, 0.0, 0.0)));
        assert_eq!(framebuffer.pixel(1, 0), Some(Color::new(0.0, 1.0, 0.0)));
        assert_eq!(framebuffer.pixel(0, 1), Some(Color::new(0.0, 0.0, 1.0)));
        assert_eq!(framebuffer.pixel(1, 1), Some(Color::WHITE));
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
                Color::new(0.0, 1.0, 0.0),
            ),
            (
                Vec3::new(0.5, -1.0, 0.5),
                Vec3::new(0.0, 1.0, 0.0),
                Color::new(0.0, 0.0, 1.0),
            ),
        ];

        for (origin, direction, expected) in cases {
            let ray = Ray::try_new(origin, direction).unwrap();
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            assert_eq!(shade_hit(&scene, hit, origin, ambient_only()), Ok(expected));
        }
    }

    #[test]
    fn texture_and_albedo_both_affect_lit_output() {
        let mut scene = Scene::new();
        let texture_id = scene
            .add_texture(Texture::solid(Color::new(0.8, 0.6, 0.4)))
            .unwrap();
        let material_id = scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture_id),
                    Color::new(0.5, 0.25, 1.0),
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
        let origin = Vec3::new(0.5, 0.5, 2.0);
        let ray = Ray::try_new(origin, Vec3::new(0.0, 0.0, -1.0)).unwrap();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

        assert_eq!(
            shade_hit(&scene, hit, origin, ambient_only()),
            Ok(Color::new(0.4, 0.15, 0.4))
        );
    }

    #[test]
    fn geometric_normals_create_directional_lighting_difference() {
        let mut scene = Scene::new();
        let material_id = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            material_id,
        ));
        let light = directional(Vec3::new(2.0, 1.0, 0.0));
        let side_origin = Vec3::new(2.0, 0.5, 0.5);
        let top_origin = Vec3::new(0.5, 2.0, 0.5);
        let side_hit = scene
            .closest_hit(
                Ray::try_new(side_origin, Vec3::new(-1.0, 0.0, 0.0)).unwrap(),
                0.0,
                f32::INFINITY,
            )
            .unwrap();
        let top_hit = scene
            .closest_hit(
                Ray::try_new(top_origin, Vec3::new(0.0, -1.0, 0.0)).unwrap(),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        let side = shade_hit(&scene, side_hit, side_origin, light).unwrap();
        let top = shade_hit(&scene, top_hit, top_origin, light).unwrap();

        assert!(side.r > top.r);
    }

    #[test]
    fn ambient_only_has_no_legacy_face_factors() {
        let mut scene = Scene::new();
        let material_id = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            material_id,
        ));

        for (origin, direction) in [
            (Vec3::new(0.5, 2.0, 0.5), Vec3::new(0.0, -1.0, 0.0)),
            (Vec3::new(0.5, -1.0, 0.5), Vec3::new(0.0, 1.0, 0.0)),
            (Vec3::new(2.0, 0.5, 0.5), Vec3::new(-1.0, 0.0, 0.0)),
        ] {
            let ray = Ray::try_new(origin, direction).unwrap();
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            assert_eq!(
                shade_hit(&scene, hit, origin, ambient_only()),
                Ok(Color::WHITE)
            );
        }
    }

    #[test]
    fn camera_position_changes_specular_result() {
        let mut scene = Scene::new();
        let texture_id = scene.add_texture(Texture::solid(Color::BLACK)).unwrap();
        let material_id = scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture_id),
                    Color::WHITE,
                    1.0,
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
        let hit_origin = Vec3::new(0.5, 0.5, 2.0);
        let hit = scene
            .closest_hit(
                Ray::try_new(hit_origin, Vec3::new(0.0, 0.0, -1.0)).unwrap(),
                0.0,
                f32::INFINITY,
            )
            .unwrap();
        let light = directional(Vec3::new(0.0, 0.0, 1.0));

        let aligned = shade_hit(&scene, hit, hit_origin, light).unwrap();
        let off_axis = shade_hit(&scene, hit, Vec3::new(2.5, 0.5, 1.0), light).unwrap();

        assert!(aligned.r > off_axis.r);
    }

    #[test]
    fn directly_lit_surface_does_not_shadow_itself() {
        let mut scene = Scene::new();
        let material_id = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            material_id,
        ));
        let (hit, camera_position) = top_hit(&scene);

        let lit = shade_hit(&scene, hit, camera_position, upward_light_with_ambient()).unwrap();

        assert!(lit.r > 1.0);
        assert!(lit.g > 1.0);
        assert!(lit.b > 1.0);
    }

    #[test]
    fn blocker_removes_diffuse_but_preserves_ambient() {
        let mut scene = Scene::new();
        let material_id = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            material_id,
        ));
        let (hit, camera_position) = top_hit(&scene);
        let lighting = upward_light_with_ambient();
        let visible = shade_hit(&scene, hit, camera_position, lighting).unwrap();

        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.5, 0.25),
            Vec3::new(0.75, 2.0, 0.75),
        );
        let blocked = shade_hit(&scene, hit, camera_position, lighting).unwrap();

        assert!(visible.r > blocked.r);
        assert_eq!(blocked, Color::new(0.2, 0.2, 0.2));
    }

    #[test]
    fn blocker_removes_specular_highlight() {
        let mut scene = Scene::new();
        let texture_id = scene.add_texture(Texture::solid(Color::BLACK)).unwrap();
        let material_id = scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture_id),
                    Color::WHITE,
                    1.0,
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
        let (hit, camera_position) = top_hit(&scene);
        let lighting = upward_light_with_ambient();
        let visible = shade_hit(&scene, hit, camera_position, lighting).unwrap();

        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.5, 0.25),
            Vec3::new(0.75, 2.0, 0.75),
        );
        let blocked = shade_hit(&scene, hit, camera_position, lighting).unwrap();

        assert!(visible.r > 0.0);
        assert_eq!(blocked, Color::BLACK);
    }

    #[test]
    fn blockers_outside_light_direction_or_behind_surface_do_not_shadow() {
        let mut scene = Scene::new();
        let material_id = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            material_id,
        ));
        let (hit, camera_position) = top_hit(&scene);
        let lighting = upward_light_with_ambient();
        let baseline = shade_hit(&scene, hit, camera_position, lighting).unwrap();

        add_blocker(
            &mut scene,
            Vec3::new(2.0, 1.5, 2.0),
            Vec3::new(3.0, 2.5, 3.0),
        );
        add_blocker(
            &mut scene,
            Vec3::new(0.25, -2.0, 0.25),
            Vec3::new(0.75, -1.5, 0.75),
        );

        assert_eq!(
            shade_hit(&scene, hit, camera_position, lighting),
            Ok(baseline)
        );
    }

    #[test]
    fn nearby_legitimate_blocker_is_not_skipped_by_bias() {
        let mut scene = Scene::new();
        let material_id = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            material_id,
        ));
        let (hit, camera_position) = top_hit(&scene);
        let blocker_min_y = 1.0 + RAY_ORIGIN_BIAS * 1.5;
        add_blocker(
            &mut scene,
            Vec3::new(0.25, blocker_min_y, 0.25),
            Vec3::new(0.75, blocker_min_y + RAY_ORIGIN_BIAS, 0.75),
        );

        let blocked = shade_hit(&scene, hit, camera_position, upward_light_with_ambient()).unwrap();

        assert_eq!(blocked, Color::new(0.2, 0.2, 0.2));
    }

    #[test]
    fn rejects_camera_framebuffer_aspect_mismatch() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let scene = Scene::new();
        let mut framebuffer = Framebuffer::try_new(4, 2).unwrap();

        assert_eq!(
            Renderer::render(&camera, &scene, &ambient_only(), &mut framebuffer),
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
            Renderer::render(&camera, &scene, &ambient_only(), &mut framebuffer),
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
            Renderer::render(&camera, &scene, &ambient_only(), &mut framebuffer),
            Err(RenderError::TextureNotFound)
        );
    }
}
