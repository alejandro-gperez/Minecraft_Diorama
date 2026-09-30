use crate::{
    camera::OrbitalCamera,
    environment::Environment,
    lighting::{Lighting, shade_surface},
    math::Vec3,
    ray::Ray,
    scene::{Scene, SceneHit},
};

use super::{Color, Framebuffer};

/// Recursion depth of a radiance ray, counted upward from the camera.
///
/// `0` is a primary camera ray, `1` is a ray spawned by a depth-0 hit, `2` is a ray spawned by a
/// depth-1 hit, and so on. Shadow visibility queries are not radiance rays and carry no depth.
type RayDepth = u32;

const PRIMARY_RAY_DEPTH: RayDepth = 0;
/// Deepest radiance ray the renderer may trace.
///
/// A ray at this depth is still intersected, locally shaded, and shadow-tested, and still samples
/// the environment on a miss; it only may not spawn a further secondary radiance ray.
const MAX_RAY_DEPTH: RayDepth = 3;

/// Secondary-ray origin handling is deliberately deferred to the missions that spawn such rays.
const RADIANCE_RAY_T_MIN: f32 = 0.0;
const SHADOW_RAY_T_MIN: f32 = 0.0;
const SHADOW_RAY_T_MAX: f32 = f32::INFINITY;
/// Fixed world-space offset for the current unit-scale AABB scene.
///
/// If a later scene spans substantially different world scales, this assumption should be
/// revisited together with the scene's numerical precision requirements.
const RAY_ORIGIN_BIAS: f32 = 1.0e-4;
const ASPECT_RATIO_TOLERANCE: f32 = 1.0e-5;

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
    RayDepthExceeded,
}

pub struct Renderer;

impl Renderer {
    pub fn render(
        camera: &OrbitalCamera,
        scene: &Scene,
        lighting: &Lighting,
        environment: &Environment,
        framebuffer: &mut Framebuffer,
    ) -> Result<(), RenderError> {
        let width = framebuffer.width();
        let height = framebuffer.height();
        let framebuffer_aspect = width as f32 / height as f32;

        if (camera.aspect_ratio() - framebuffer_aspect).abs() > ASPECT_RATIO_TOLERANCE {
            return Err(RenderError::AspectRatioMismatch);
        }

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
}

/// Shared, borrowed inputs for every radiance ray traced during one render.
struct Tracer<'a> {
    scene: &'a Scene,
    lighting: Lighting,
    environment: &'a Environment,
}

impl<'a> Tracer<'a> {
    const fn new(scene: &'a Scene, lighting: Lighting, environment: &'a Environment) -> Self {
        Self {
            scene,
            lighting,
            environment,
        }
    }

    /// Returns the radiance arriving along `ray`, traced at recursion `depth`.
    ///
    /// Every radiance ray, primary or secondary, shares one miss path: the world-space
    /// environment. The viewer for local specular shading is the ray origin, which is the camera
    /// position for primary rays and the spawning surface point for secondary rays.
    fn trace_ray(&self, ray: Ray, depth: RayDepth) -> Result<Color, RenderError> {
        if depth > MAX_RAY_DEPTH {
            return Err(RenderError::RayDepthExceeded);
        }

        let Some(hit) = self
            .scene
            .closest_hit(ray, RADIANCE_RAY_T_MIN, f32::INFINITY)
        else {
            return Ok(self.environment.sample(ray.direction()));
        };

        let local = shade_hit(self.scene, hit, ray.origin(), self.lighting)?;

        // Secondary radiance contributions belong here, each gated by
        // `can_spawn_secondary_ray(depth)` and traced with `depth + 1`. No material launches one
        // yet, so local shading is the complete result.
        Ok(local)
    }
}

/// Reports whether a ray traced at `depth` may spawn a radiance ray at `depth + 1`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no material launches secondary radiance rays yet")
)]
const fn can_spawn_secondary_ray(depth: RayDepth) -> bool {
    depth < MAX_RAY_DEPTH
}

/// Local surface shading: texture, albedo, and direct lighting with hard-shadow visibility.
///
/// The shadow ray is an any-hit visibility query rather than a radiance ray, so it takes no
/// recursion depth and is cast identically at every traced depth.
fn shade_hit(
    scene: &Scene,
    hit: SceneHit,
    viewer_position: Vec3,
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

    let view_direction = (viewer_position - hit.geometry.position)
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

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::{
        MAX_RAY_DEPTH, PRIMARY_RAY_DEPTH, RAY_ORIGIN_BIAS, RenderError, Renderer, Tracer,
        can_spawn_secondary_ray, pixel_center, shade_hit,
    };
    use crate::{
        camera::OrbitalCamera,
        environment::Environment,
        geometry::Aabb,
        lighting::{AmbientLight, DirectionalLight, Lighting},
        material::{
            CanonicalMaterials, CanonicalTextureIds, Material, MaterialId, Texture, TextureId,
            TextureSelection,
        },
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

    fn environment() -> Environment {
        Environment::sunset()
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

    fn darkness() -> Lighting {
        Lighting::new(
            AmbientLight::try_new(Color::BLACK, 0.0).unwrap(),
            DirectionalLight::try_new(Vec3::new(0.0, 1.0, 0.0), Color::WHITE, 0.0).unwrap(),
        )
    }

    fn unit_cube_scene() -> Scene {
        let mut scene = Scene::new();
        let material_id = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            material_id,
        ));
        scene
    }

    fn downward_ray() -> Ray {
        Ray::try_new(Vec3::new(0.5, 2.0, 0.5), Vec3::new(0.0, -1.0, 0.0)).unwrap()
    }

    fn miss_directions() -> [Vec3; 4] {
        [
            Vec3::new(0.6, 0.2, 0.8),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-0.3, 0.05, 1.0),
            Vec3::new(0.2, -1.0, 0.1),
        ]
    }

    /// Registers the canonical materials over one shared solid test texture.
    fn canonical_scene() -> (Scene, CanonicalMaterials) {
        let mut scene = Scene::new();
        let texture = scene
            .add_texture(Texture::solid(Color::new(0.6, 0.5, 0.4)))
            .unwrap();
        let materials = scene
            .add_canonical_materials(CanonicalTextureIds {
                grass_top: texture,
                grass_side: texture,
                dirt: texture,
                cobblestone: texture,
                obsidian: texture,
                glass: texture,
                lava: texture,
            })
            .unwrap();
        (scene, materials)
    }

    fn all_canonical(materials: CanonicalMaterials) -> [MaterialId; 5] {
        [
            materials.grass,
            materials.cobblestone,
            materials.obsidian,
            materials.glass,
            materials.lava,
        ]
    }

    /// Diagonal ray striking the unit cube's top face at `(0.5, 1.0, 0.5)`.
    fn angled_top_ray() -> Ray {
        Ray::try_new(Vec3::new(-0.5, 2.0, 0.5), Vec3::new(1.0, -1.0, 0.0)).unwrap()
    }

    // Bright blocks placed where a mirror reflection or straight transmission of
    // `angled_top_ray` would arrive; neither blocks the incoming ray or its upward shadow ray.
    const REFLECTION_WITNESS: (Vec3, Vec3) = (Vec3::new(1.5, 2.0, 0.0), Vec3::new(2.5, 3.0, 1.0));
    const TRANSMISSION_WITNESS: (Vec3, Vec3) =
        (Vec3::new(1.5, -1.5, 0.0), Vec3::new(2.5, -0.5, 1.0));

    /// Traces `angled_top_ray` onto a unit cube of the selected canonical material at every valid
    /// depth, with and without the witness blocks, and expects exactly its local shading.
    fn assert_only_local_shading(select: impl Fn(CanonicalMaterials) -> MaterialId) {
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let ray = angled_top_ray();
        let (mut scene, materials) = canonical_scene();
        let material_id = select(materials);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            material_id,
        ));
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        assert_eq!(hit.material_id, material_id);
        let local = shade_hit(&scene, hit, ray.origin(), lighting).unwrap();

        let mut witnessed = scene.clone();
        let white = add_solid_material(&mut witnessed, Color::WHITE);
        for (min, max) in [REFLECTION_WITNESS, TRANSMISSION_WITNESS] {
            witnessed.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), white));
        }

        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            for candidate in [&scene, &witnessed] {
                assert_eq!(
                    Tracer::new(candidate, lighting, &environment).trace_ray(ray, depth),
                    Ok(local)
                );
            }
        }
    }

    #[test]
    fn primary_rays_enter_tracing_at_depth_zero() {
        assert_eq!(PRIMARY_RAY_DEPTH, 0);

        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let mut scene = Scene::new();
        let red = add_solid_material(&mut scene, Color::new(1.0, 0.0, 0.0));
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5)).unwrap(),
            red,
        ));
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let tracer = Tracer::new(&scene, lighting, &environment);
        let mut framebuffer = Framebuffer::try_new(3, 3).unwrap();

        Renderer::render(&camera, &scene, &lighting, &environment, &mut framebuffer).unwrap();

        for y in 0..3 {
            for x in 0..3 {
                let (u, v) = pixel_center(x, y, 3, 3);
                let ray = camera.ray_for_viewport(u, v).unwrap();
                assert_eq!(
                    framebuffer.pixel(x, y),
                    Some(tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap())
                );
            }
        }
    }

    #[test]
    fn every_valid_depth_receives_ordinary_local_hit_shading() {
        let scene = unit_cube_scene();
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let tracer = Tracer::new(&scene, lighting, &environment);
        let ray = downward_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let local = shade_hit(&scene, hit, ray.origin(), lighting).unwrap();

        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            assert_eq!(tracer.trace_ray(ray, depth), Ok(local));
        }
    }

    #[test]
    fn miss_at_primary_depth_samples_environment() {
        let scene = Scene::new();
        let environment = environment();
        let tracer = Tracer::new(&scene, ambient_only(), &environment);

        for direction in miss_directions() {
            let ray = Ray::try_new(Vec3::new(0.0, 0.5, 0.0), direction).unwrap();
            assert_eq!(
                tracer.trace_ray(ray, PRIMARY_RAY_DEPTH),
                Ok(environment.sample(ray.direction()))
            );
        }
    }

    #[test]
    fn miss_at_maximum_depth_samples_environment() {
        let scene = unit_cube_scene();
        let environment = environment();
        let tracer = Tracer::new(&scene, ambient_only(), &environment);

        for direction in miss_directions() {
            let ray = Ray::try_new(Vec3::new(3.0, 0.5, 3.0), direction).unwrap();
            assert_eq!(scene.closest_hit(ray, 0.0, f32::INFINITY), None);
            assert_eq!(
                tracer.trace_ray(ray, MAX_RAY_DEPTH),
                Ok(environment.sample(ray.direction()))
            );
        }
    }

    #[test]
    fn hit_at_maximum_depth_receives_local_surface_shading() {
        let scene = unit_cube_scene();
        let environment = environment();
        let ray = downward_ray();

        let lit = Tracer::new(&scene, upward_light_with_ambient(), &environment)
            .trace_ray(ray, MAX_RAY_DEPTH)
            .unwrap();
        let ambient = Tracer::new(&scene, ambient_only(), &environment)
            .trace_ray(ray, MAX_RAY_DEPTH)
            .unwrap();

        assert!(lit.r > 1.0 && lit.g > 1.0 && lit.b > 1.0);
        assert_eq!(ambient, Color::WHITE);
    }

    #[test]
    fn depth_policy_forbids_spawning_beyond_maximum() {
        for depth in PRIMARY_RAY_DEPTH..MAX_RAY_DEPTH {
            let child_depth = depth + 1;
            assert!(can_spawn_secondary_ray(depth));
            assert!(child_depth <= MAX_RAY_DEPTH);
        }

        assert!(!can_spawn_secondary_ray(MAX_RAY_DEPTH));
        assert!(!can_spawn_secondary_ray(MAX_RAY_DEPTH + 1));
        assert!(!can_spawn_secondary_ray(u32::MAX));
    }

    #[test]
    fn tracing_beyond_maximum_depth_is_rejected() {
        let hit_scene = unit_cube_scene();
        let empty_scene = Scene::new();
        let environment = environment();

        for scene in [&hit_scene, &empty_scene] {
            let tracer = Tracer::new(scene, ambient_only(), &environment);
            for depth in [MAX_RAY_DEPTH + 1, u32::MAX] {
                assert_eq!(
                    tracer.trace_ray(downward_ray(), depth),
                    Err(RenderError::RayDepthExceeded)
                );
            }
        }
    }

    #[test]
    fn shadow_queries_do_not_consume_recursive_depth() {
        let mut scene = unit_cube_scene();
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let ray = angled_top_ray();
        let unblocked = Tracer::new(&scene, lighting, &environment)
            .trace_ray(ray, MAX_RAY_DEPTH)
            .unwrap();

        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.5, 0.25),
            Vec3::new(0.75, 1.9, 0.75),
        );
        let tracer = Tracer::new(&scene, lighting, &environment);

        // A depth-limited ray may spawn no radiance ray, yet its shadow query still runs.
        assert!(!can_spawn_secondary_ray(MAX_RAY_DEPTH));
        assert!(unblocked.r > 1.0);
        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            assert_eq!(tracer.trace_ray(ray, depth), Ok(Color::new(0.2, 0.2, 0.2)));
        }
    }

    #[test]
    fn trace_resolves_face_specific_textures() {
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
        let environment = environment();
        let tracer = Tracer::new(&scene, ambient_only(), &environment);

        for (origin, direction, expected) in [
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
        ] {
            let ray = Ray::try_new(origin, direction).unwrap();
            assert_eq!(tracer.trace_ray(ray, PRIMARY_RAY_DEPTH), Ok(expected));
        }
    }

    #[test]
    fn trace_preserves_directional_lighting_and_hard_shadows() {
        let mut scene = unit_cube_scene();
        let environment = environment();
        let ray = angled_top_ray();
        let lit = Tracer::new(&scene, upward_light_with_ambient(), &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();
        let unlit = Tracer::new(&scene, darkness(), &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.5, 0.25),
            Vec3::new(0.75, 1.9, 0.75),
        );
        let shadowed = Tracer::new(&scene, upward_light_with_ambient(), &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        assert_eq!(unlit, Color::BLACK);
        assert!(lit.r > shadowed.r);
        assert_eq!(shadowed, Color::new(0.2, 0.2, 0.2));
    }

    #[test]
    fn canonical_materials_produce_no_recursive_contribution() {
        for index in 0..5 {
            assert_only_local_shading(|materials| all_canonical(materials)[index]);
        }
    }

    #[test]
    fn glass_remains_opaque() {
        let (scene, materials) = canonical_scene();
        assert!(scene.material(materials.glass).unwrap().transparency() > 0.0);

        assert_only_local_shading(|m| m.glass);
    }

    #[test]
    fn obsidian_does_not_reflect_yet() {
        let (scene, materials) = canonical_scene();
        assert!(scene.material(materials.obsidian).unwrap().reflectivity() > 0.0);

        assert_only_local_shading(|m| m.obsidian);
    }

    #[test]
    fn lava_does_not_emit_yet() {
        let (mut scene, materials) = canonical_scene();
        let white = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            materials.lava,
        ));
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0)).unwrap(),
            white,
        ));
        let environment = environment();
        let tracer = Tracer::new(&scene, darkness(), &environment);
        let onto_lava = downward_ray();
        let onto_neighbor =
            Ray::try_new(Vec3::new(1.5, 2.0, 0.5), Vec3::new(0.0, -1.0, 0.0)).unwrap();

        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            assert_eq!(tracer.trace_ray(onto_lava, depth), Ok(Color::BLACK));
            assert_eq!(tracer.trace_ray(onto_neighbor, depth), Ok(Color::BLACK));
        }
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
    fn scene_miss_uses_world_space_environment_sample() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let scene = Scene::new();
        let environment = environment();
        let mut framebuffer = Framebuffer::try_new(1, 1).unwrap();
        let center_direction = camera.ray_for_viewport(0.5, 0.5).unwrap().direction();

        Renderer::render(
            &camera,
            &scene,
            &ambient_only(),
            &environment,
            &mut framebuffer,
        )
        .unwrap();

        assert_eq!(
            framebuffer.pixel(0, 0),
            Some(environment.sample(center_direction))
        );
    }

    #[test]
    fn scene_hit_still_uses_material_and_lighting_pipeline() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap();
        let mut scene = Scene::new();
        let red = add_solid_material(&mut scene, Color::new(1.0, 0.0, 0.0));
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5)).unwrap(),
            red,
        ));
        let mut framebuffer = Framebuffer::try_new(3, 3).unwrap();

        Renderer::render(
            &camera,
            &scene,
            &ambient_only(),
            &environment(),
            &mut framebuffer,
        )
        .unwrap();

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

        Renderer::render(
            &camera,
            &scene,
            &ambient_only(),
            &environment(),
            &mut framebuffer,
        )
        .unwrap();

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

        Renderer::render(
            &camera,
            &scene,
            &ambient_only(),
            &environment(),
            &mut framebuffer,
        )
        .unwrap();

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
            Renderer::render(
                &camera,
                &scene,
                &ambient_only(),
                &environment(),
                &mut framebuffer,
            ),
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
            Renderer::render(
                &camera,
                &scene,
                &ambient_only(),
                &environment(),
                &mut framebuffer,
            ),
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
            Renderer::render(
                &camera,
                &scene,
                &ambient_only(),
                &environment(),
                &mut framebuffer,
            ),
            Err(RenderError::TextureNotFound)
        );
    }
}
