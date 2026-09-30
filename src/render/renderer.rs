use crate::{
    camera::OrbitalCamera,
    environment::Environment,
    lighting::{Lighting, shade_surface},
    material::AIR_IOR,
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

const RADIANCE_RAY_T_MIN: f32 = 0.0;
const SHADOW_RAY_T_MIN: f32 = 0.0;
const SHADOW_RAY_T_MAX: f32 = f32::INFINITY;
/// Fixed world-space offset for the current unit-scale AABB scene.
///
/// If a later scene spans substantially different world scales, this assumption should be
/// revisited together with the scene's numerical precision requirements.
const RAY_ORIGIN_BIAS: f32 = 1.0e-4;
/// Fixed world-space offset lifting a reflected ray off its exterior hit surface.
///
/// Numerically equal to `RAY_ORIGIN_BIAS` today, but kept separate: reflection and shadow rays
/// may need different offsets once the scene scale or refraction origin handling changes. The
/// same unit-scale assumption applies.
const REFLECTION_RAY_ORIGIN_BIAS: f32 = 1.0e-4;
/// Fixed world-space offset moving a refracted ray onto the transmitted side of its interface.
///
/// Numerically equal to the other biases for the same unit-scale reason, but its direction depends
/// on whether the ray enters or exits the medium, so it is a separate operation and constant.
const REFRACTION_RAY_ORIGIN_BIAS: f32 = 1.0e-4;
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
    ReflectionRayGenerationFailed,
    RefractionRayGenerationFailed,
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
    /// environment. Local specular shading views the surface back along the ray, `-direction`.
    /// That equals the direction toward the ray origin but stays well defined when a secondary
    /// ray hits a surface closer to its origin than the normalization epsilon, such as in the
    /// concave corner between a block and the ground.
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

        let viewer_position = hit.geometry.position - ray.direction();
        let local = shade_hit(self.scene, hit, viewer_position, self.lighting)?;
        if !can_spawn_secondary_ray(depth) {
            return Ok(local);
        }

        // Material lookup already succeeded inside `shade_hit`.
        let material = self
            .scene
            .material(hit.material_id)
            .ok_or(RenderError::MaterialNotFound)?;

        let reflectivity = material.reflectivity();
        let base_with_reflection = if reflectivity > 0.0 {
            let reflected = self.trace_ray(reflection_ray(ray, hit)?, depth + 1)?;
            blend_reflection(local, reflected, reflectivity)
        } else {
            local
        };

        let transparency = material.transparency();
        if transparency <= 0.0 {
            return Ok(base_with_reflection);
        }
        // Total internal reflection transmits nothing; the reflection-blended result stands.
        let Some(refracted_ray) = refraction_ray(ray, hit, material.ior())? else {
            return Ok(base_with_reflection);
        };
        let refracted = self.trace_ray(refracted_ray, depth + 1)?;

        Ok(blend_transmission(
            base_with_reflection,
            refracted,
            transparency,
        ))
    }
}

/// Reports whether a ray traced at `depth` may spawn a radiance ray at `depth + 1`.
const fn can_spawn_secondary_ray(depth: RayDepth) -> bool {
    depth < MAX_RAY_DEPTH
}

/// Mirror reflection of `ray` about the hit's outward geometric normal, `R = D - 2(D·N)N`,
/// starting slightly off the surface so it does not re-hit the face it leaves.
fn reflection_ray(ray: Ray, hit: SceneHit) -> Result<Ray, RenderError> {
    let normal = hit.geometry.normal;
    let origin = hit.geometry.position + normal * REFLECTION_RAY_ORIGIN_BIAS;

    Ray::try_new(origin, ray.direction().reflect(normal))
        .ok_or(RenderError::ReflectionRayGenerationFailed)
}

/// Energy-conserving blend of local shading and reflected radiance with a constant coefficient.
fn blend_reflection(local: Color, reflected: Color, reflectivity: f32) -> Color {
    local.lerp(reflected, reflectivity)
}

/// Whether a ray crosses a transmissive AABB surface into or out of its material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediumTransition {
    Entering,
    Exiting,
}

/// Optical description of one refraction event at an AABB surface.
///
/// `normal` is the interface normal oriented against the incident ray, used only for the Snell
/// calculation; the hit's outward geometric normal stays untouched for lighting, reflection,
/// face identity, and UVs.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RefractionInterface {
    transition: MediumTransition,
    eta_incident: f32,
    eta_transmitted: f32,
    normal: Vec3,
}

/// Classifies an incident direction against a hit's outward geometric normal.
///
/// Every transmissive AABB is assumed to be surrounded by air: a ray against the outward normal
/// enters from `AIR_IOR` into `material_ior`, and a ray along it exits back into air. No stack of
/// nested media is tracked.
fn refraction_interface(
    direction: Vec3,
    outward_normal: Vec3,
    material_ior: f32,
) -> RefractionInterface {
    if direction.dot(outward_normal) < 0.0 {
        RefractionInterface {
            transition: MediumTransition::Entering,
            eta_incident: AIR_IOR,
            eta_transmitted: material_ior,
            normal: outward_normal,
        }
    } else {
        RefractionInterface {
            transition: MediumTransition::Exiting,
            eta_incident: material_ior,
            eta_transmitted: AIR_IOR,
            normal: -outward_normal,
        }
    }
}

/// Snell refraction of `ray` through the hit surface, or `None` on total internal reflection.
///
/// The origin is pushed to the transmitted side, opposite the oriented normal: just inside the
/// AABB when entering (`position - N * bias`) and just outside when exiting
/// (`position + N * bias`), for outward normal `N`. An entering ray therefore starts inside the
/// box, where AABB intersection reports the exit face. The source object is never excluded.
fn refraction_ray(ray: Ray, hit: SceneHit, material_ior: f32) -> Result<Option<Ray>, RenderError> {
    let interface = refraction_interface(ray.direction(), hit.geometry.normal, material_ior);
    let eta = interface.eta_incident / interface.eta_transmitted;
    let Some(direction) = ray.direction().refract(interface.normal, eta) else {
        return Ok(None);
    };
    let origin = hit.geometry.position - interface.normal * REFRACTION_RAY_ORIGIN_BIAS;

    Ray::try_new(origin, direction)
        .map(Some)
        .ok_or(RenderError::RefractionRayGenerationFailed)
}

/// Temporary Phase 3 transmission blend applied after the reflection blend.
///
/// `transparency` is a constant, angle-independent coefficient; Fresnel composition replaces this
/// in a later mission.
fn blend_transmission(base_with_reflection: Color, refracted: Color, transparency: f32) -> Color {
    base_with_reflection.lerp(refracted, transparency)
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
        MAX_RAY_DEPTH, MediumTransition, PRIMARY_RAY_DEPTH, RAY_ORIGIN_BIAS,
        REFLECTION_RAY_ORIGIN_BIAS, REFRACTION_RAY_ORIGIN_BIAS, RefractionInterface, RenderError,
        Renderer, Tracer, blend_reflection, blend_transmission, can_spawn_secondary_ray,
        pixel_center, reflection_ray, refraction_interface, refraction_ray, shade_hit,
    };
    use crate::{
        camera::OrbitalCamera,
        environment::Environment,
        geometry::{Aabb, CubeFace},
        lighting::{AmbientLight, DirectionalLight, Lighting, shade_surface},
        material::{
            AIR_IOR, CanonicalMaterials, CanonicalTextureIds, GLASS_IOR, Material, MaterialId,
            Texture, TextureId, TextureSelection,
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

    /// Diagonal ray striking the unit cube's top face at `(0.5, 1.0, 0.5)`.
    fn angled_top_ray() -> Ray {
        Ray::try_new(Vec3::new(-0.5, 2.0, 0.5), Vec3::new(1.0, -1.0, 0.0)).unwrap()
    }

    // Blocks placed where a mirror reflection or straight transmission of `angled_top_ray` would
    // arrive; neither blocks the incoming ray or its upward shadow ray.
    const REFLECTION_WITNESS: (Vec3, Vec3) = (Vec3::new(1.2, 2.0, 0.0), Vec3::new(2.5, 3.0, 1.0));
    const TRANSMISSION_WITNESS: (Vec3, Vec3) =
        (Vec3::new(1.5, -1.5, 0.0), Vec3::new(2.5, -0.5, 1.0));

    fn assert_color_approx_eq(actual: Color, expected: Color) {
        for (a, e) in [
            (actual.r, expected.r),
            (actual.g, expected.g),
            (actual.b, expected.b),
        ] {
            assert!((a - e).abs() <= 1.0e-5, "{actual:?} != {expected:?}");
        }
    }

    /// Solid-colored material with the given reflectivity and no other optical effects.
    fn add_reflective_material(scene: &mut Scene, color: Color, reflectivity: f32) -> MaterialId {
        let texture_id = scene.add_texture(Texture::solid(color)).unwrap();
        scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture_id),
                    Color::WHITE,
                    0.0,
                    0.0,
                    reflectivity,
                )
                .unwrap(),
            )
            .unwrap()
    }

    /// Unit cube of `reflectivity` plus a solid-`witness_color` block at `REFLECTION_WITNESS`,
    /// i.e. exactly where `angled_top_ray` reflects to.
    fn mirror_and_witness_scene(reflectivity: f32, witness_color: Color) -> Scene {
        let mut scene = Scene::new();
        let mirror = add_reflective_material(&mut scene, Color::new(0.2, 0.4, 0.6), reflectivity);
        let witness = add_reflective_material(&mut scene, witness_color, 0.0);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            mirror,
        ));
        let (min, max) = REFLECTION_WITNESS;
        scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), witness));
        scene
    }

    fn local_shading(scene: &Scene, ray: Ray, lighting: Lighting) -> Color {
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        shade_hit(scene, hit, ray.origin(), lighting).unwrap()
    }

    fn canonical_reflectivity(select: impl Fn(CanonicalMaterials) -> MaterialId) -> f32 {
        let (scene, materials) = canonical_scene();
        scene.material(select(materials)).unwrap().reflectivity()
    }

    const OPAQUE_CANONICAL_MATERIALS: [fn(CanonicalMaterials) -> MaterialId; 4] =
        [|m| m.grass, |m| m.cobblestone, |m| m.obsidian, |m| m.lava];

    fn unit_cube() -> Aabb {
        Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap()
    }

    /// Ray striking the unit cube's top face at `(0.5, 1.0, 0.5)`, about 16.7 degrees off the
    /// normal: inside glass it reaches the bottom face without total internal reflection.
    fn tilted_top_ray() -> Ray {
        Ray::try_new(Vec3::new(0.2, 2.0, 0.5), Vec3::new(0.3, -1.0, 0.0)).unwrap()
    }

    /// Solid-colored material with glass's index of refraction and the given optical weights.
    fn add_optical_material(
        scene: &mut Scene,
        color: Color,
        reflectivity: f32,
        transparency: f32,
    ) -> MaterialId {
        let texture_id = scene.add_texture(Texture::solid(color)).unwrap();
        scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture_id),
                    Color::WHITE,
                    0.0,
                    transparency,
                    reflectivity,
                )
                .unwrap()
                .with_ior(GLASS_IOR)
                .unwrap(),
            )
            .unwrap()
    }

    /// Unit cube of a solid `(0.2, 0.4, 0.6)` transmissive test material.
    fn transmissive_cube_scene(reflectivity: f32, transparency: f32) -> Scene {
        let mut scene = Scene::new();
        let material = add_optical_material(
            &mut scene,
            Color::new(0.2, 0.4, 0.6),
            reflectivity,
            transparency,
        );
        scene.add(SceneObject::new(unit_cube(), material));
        scene
    }

    /// Ray refracted into the unit cube by `tilted_top_ray`, with its bottom-face exit hit.
    fn inside_ray_and_exit_hit(scene: &Scene) -> (Ray, crate::scene::SceneHit) {
        let entry = scene
            .closest_hit(tilted_top_ray(), 0.0, f32::INFINITY)
            .unwrap();
        let inside = refraction_ray(tilted_top_ray(), entry, GLASS_IOR)
            .unwrap()
            .unwrap();
        (
            inside,
            scene.closest_hit(inside, 0.0, f32::INFINITY).unwrap(),
        )
    }

    /// Local shading exactly as `trace_ray` computes it, viewing back along the ray.
    fn traced_local(scene: &Scene, ray: Ray, lighting: Lighting) -> Color {
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        shade_hit(
            scene,
            hit,
            hit.geometry.position - ray.direction(),
            lighting,
        )
        .unwrap()
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
    fn reflection_ray_follows_mirror_formula_from_biased_origin() {
        let scene = unit_cube_scene();
        let ray = angled_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected = reflection_ray(ray, hit).unwrap();
        let expected = Vec3::new(1.0, 1.0, 0.0).try_normalized().unwrap();

        assert!((reflected.direction() - expected).length() < 1.0e-6);
        assert!((reflected.direction().length() - 1.0).abs() < 1.0e-6);
        assert!(reflected.direction().is_finite());
        assert_eq!(REFLECTION_RAY_ORIGIN_BIAS, 1.0e-4);
        assert_eq!(
            reflected.origin(),
            hit.geometry.position + hit.geometry.normal * REFLECTION_RAY_ORIGIN_BIAS
        );
    }

    #[test]
    fn perpendicular_reflection_returns_along_incident_path() {
        let scene = unit_cube_scene();
        let ray = downward_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

        let reflected = reflection_ray(ray, hit).unwrap();

        assert!((reflected.direction() - Vec3::new(0.0, 1.0, 0.0)).length() < 1.0e-6);
    }

    #[test]
    fn reflected_ray_does_not_hit_its_own_exterior_surface() {
        let scene = unit_cube_scene();

        for ray in [downward_ray(), angled_top_ray()] {
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let reflected = reflection_ray(ray, hit).unwrap();

            // The source object is still in the scene; only the origin bias prevents the re-hit.
            assert_eq!(scene.closest_hit(reflected, 0.0, f32::INFINITY), None);
        }
    }

    #[test]
    fn reflection_blend_is_exact_linear_interpolation() {
        let local = Color::new(0.2, 0.4, 0.6);
        let reflected = Color::new(1.0, 0.0, 0.5);

        assert_color_approx_eq(blend_reflection(local, reflected, 0.0), local);
        assert_color_approx_eq(blend_reflection(local, reflected, 1.0), reflected);
        assert_color_approx_eq(
            blend_reflection(local, reflected, 0.25),
            local.scale(0.75) + reflected.scale(0.25),
        );
    }

    #[test]
    fn zero_reflectivity_yields_local_shading_only() {
        let lighting = upward_light_with_ambient();
        let ray = angled_top_ray();
        let scene = mirror_and_witness_scene(0.0, Color::WHITE);
        let environment = environment();

        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            assert_eq!(
                Tracer::new(&scene, lighting, &environment).trace_ray(ray, depth),
                Ok(local_shading(&scene, ray, lighting))
            );
        }
    }

    #[test]
    fn full_reflectivity_yields_reflected_radiance_only() {
        let lighting = ambient_only();
        let ray = angled_top_ray();
        let scene = mirror_and_witness_scene(1.0, Color::new(0.9, 0.1, 0.3));
        let environment = environment();

        // The witness has albedo-1 ambient-only shading, so it returns its own texture color.
        assert_color_approx_eq(
            Tracer::new(&scene, lighting, &environment)
                .trace_ray(ray, PRIMARY_RAY_DEPTH)
                .unwrap(),
            Color::new(0.9, 0.1, 0.3),
        );
    }

    #[test]
    fn intermediate_reflectivity_blends_local_and_reflected_radiance() {
        let lighting = ambient_only();
        let ray = angled_top_ray();
        let witness_color = Color::new(0.9, 0.1, 0.3);
        let scene = mirror_and_witness_scene(0.35, witness_color);
        let environment = environment();

        let local = local_shading(&scene, ray, lighting);
        let traced = Tracer::new(&scene, lighting, &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        assert_color_approx_eq(traced, local.scale(0.65) + witness_color.scale(0.35));
    }

    #[test]
    fn reflectivity_is_constant_across_incidence_angles() {
        // No Fresnel: a flat-colored mirror blended with a known environment color uses the same
        // coefficient at grazing and head-on incidence.
        let lighting = ambient_only();
        let environment = environment();
        let mut scene = Scene::new();
        let mirror = add_reflective_material(&mut scene, Color::new(0.2, 0.4, 0.6), 0.35);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            mirror,
        ));

        for (origin, direction) in [
            (Vec3::new(0.5, 2.0, 0.5), Vec3::new(0.0, -1.0, 0.0)),
            (Vec3::new(-0.5, 2.0, 0.5), Vec3::new(1.0, -1.0, 0.0)),
            (Vec3::new(-1.0, 0.9, 0.3), Vec3::new(1.0, -0.05, 0.3)),
        ] {
            let ray = Ray::try_new(origin, direction).unwrap();
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let sky = environment.sample(reflection_ray(ray, hit).unwrap().direction());
            let local = local_shading(&scene, ray, lighting);

            assert_color_approx_eq(
                Tracer::new(&scene, lighting, &environment)
                    .trace_ray(ray, PRIMARY_RAY_DEPTH)
                    .unwrap(),
                local.scale(0.65) + sky.scale(0.35),
            );
        }
    }

    #[test]
    fn reflected_miss_samples_environment_through_common_miss_path() {
        let lighting = ambient_only();
        let environment = environment();
        let mut scene = Scene::new();
        let mirror = add_reflective_material(&mut scene, Color::new(0.2, 0.4, 0.6), 0.5);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            mirror,
        ));
        let ray = angled_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let sky = environment.sample(reflection_ray(ray, hit).unwrap().direction());
        let local = local_shading(&scene, ray, lighting);

        assert_color_approx_eq(
            Tracer::new(&scene, lighting, &environment)
                .trace_ray(ray, PRIMARY_RAY_DEPTH)
                .unwrap(),
            local.scale(0.5) + sky.scale(0.5),
        );
    }

    #[test]
    fn reflective_hit_below_maximum_depth_spawns_reflection() {
        let lighting = ambient_only();
        let ray = angled_top_ray();
        let plain = mirror_and_witness_scene(0.0, Color::WHITE);
        let mirror = mirror_and_witness_scene(0.5, Color::WHITE);
        let environment = environment();

        for depth in PRIMARY_RAY_DEPTH..MAX_RAY_DEPTH {
            assert_ne!(
                Tracer::new(&mirror, lighting, &environment).trace_ray(ray, depth),
                Tracer::new(&plain, lighting, &environment).trace_ray(ray, depth)
            );
        }
    }

    #[test]
    fn reflective_hit_at_maximum_depth_is_locally_shaded_only() {
        let lighting = upward_light_with_ambient();
        let ray = angled_top_ray();
        let scene = mirror_and_witness_scene(0.5, Color::WHITE);
        let environment = environment();

        assert_eq!(
            Tracer::new(&scene, lighting, &environment).trace_ray(ray, MAX_RAY_DEPTH),
            Ok(local_shading(&scene, ray, lighting))
        );
    }

    #[test]
    fn reflected_hit_receives_full_shading_including_hard_shadows() {
        // The witness underside is lit by a downward-pointing light; a blocker beneath it must
        // darken what the mirror shows, proving reflected hits use the ordinary shading and
        // shadow path.
        let lighting = Lighting::new(
            AmbientLight::try_new(Color::WHITE, 0.2).unwrap(),
            DirectionalLight::try_new(Vec3::new(0.0, -1.0, 0.0), Color::WHITE, 1.0).unwrap(),
        );
        let ray = angled_top_ray();
        let environment = environment();
        let lit = mirror_and_witness_scene(1.0, Color::WHITE);
        let mut shadowed = lit.clone();
        add_blocker(
            &mut shadowed,
            Vec3::new(1.3, 1.2, 0.0),
            Vec3::new(1.7, 1.6, 1.0),
        );

        let lit_color = Tracer::new(&lit, lighting, &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();
        let shadowed_color = Tracer::new(&shadowed, lighting, &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        assert!(lit_color.r > 1.0);
        assert_color_approx_eq(shadowed_color, Color::new(0.2, 0.2, 0.2));
    }

    #[test]
    fn facing_mirrors_terminate_at_maximum_depth() {
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let mut scene = Scene::new();
        let mirror = add_reflective_material(&mut scene, Color::WHITE, 1.0);
        for (min, max) in [
            (Vec3::new(-5.0, -1.0, -5.0), Vec3::new(5.0, 0.0, 5.0)),
            (Vec3::new(-5.0, 2.0, -5.0), Vec3::new(5.0, 3.0, 5.0)),
        ] {
            scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), mirror));
        }
        let ray = Ray::try_new(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.2, -1.0, 0.1)).unwrap();

        let color = Tracer::new(&scene, lighting, &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        assert!(color.is_finite());
    }

    #[test]
    fn shadow_rays_do_not_consume_recursive_depth_with_reflection_active() {
        let lighting = upward_light_with_ambient();
        let ray = angled_top_ray();
        let environment = environment();
        let mut scene = mirror_and_witness_scene(0.5, Color::WHITE);
        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.5, 0.25),
            Vec3::new(0.75, 1.9, 0.75),
        );
        let local = local_shading(&scene, ray, lighting);

        assert_color_approx_eq(local, Color::new(0.04, 0.08, 0.12));
        // At maximum depth no reflection is spawned, yet the shadow still darkens local shading.
        assert_eq!(
            Tracer::new(&scene, lighting, &environment).trace_ray(ray, MAX_RAY_DEPTH),
            Ok(local)
        );
    }

    #[test]
    fn reflected_ray_hitting_adjacent_surface_at_tiny_distance_still_renders() {
        // A ground slab meets a block at a concave corner. A reflection spawned off the ground
        // just beside the block starts within 1e-6 of the block's face and hits it almost
        // immediately; this used to fail with ViewDirectionUnavailable.
        let mut scene = Scene::new();
        let ground = add_reflective_material(&mut scene, Color::new(0.3, 0.6, 0.3), 0.5);
        let block = add_reflective_material(&mut scene, Color::new(0.5, 0.5, 0.5), 0.5);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-4.0, -0.5, -4.0), Vec3::new(4.0, 0.0, 4.0)).unwrap(),
            ground,
        ));
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(-1.3, 0.0, -0.6), Vec3::new(-0.1, 1.8, 0.6)).unwrap(),
            block,
        ));
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let tracer = Tracer::new(&scene, lighting, &environment);
        // Ground hit just outside the block's +x face; the mirror ray heads into that face.
        let x = -0.1 + 2.0e-7;
        let ray = Ray::try_new(
            Vec3::new(x + 1.0, 1.0, 0.4265921),
            Vec3::new(-1.0, -1.0, 0.0),
        )
        .unwrap();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected = reflection_ray(ray, hit).unwrap();
        let near = scene.closest_hit(reflected, 0.0, f32::INFINITY).unwrap();
        assert!(near.geometry.t < 1.0e-6, "t = {}", near.geometry.t);

        let color = tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap();

        assert!(color.is_finite());
    }

    #[test]
    fn canonical_materials_keep_their_stored_reflectivity() {
        assert_eq!(canonical_reflectivity(|m| m.grass), 0.02);
        assert_eq!(canonical_reflectivity(|m| m.cobblestone), 0.03);
        assert_eq!(canonical_reflectivity(|m| m.obsidian), 0.35);
        assert_eq!(canonical_reflectivity(|m| m.glass), 0.15);
        assert_eq!(canonical_reflectivity(|m| m.lava), 0.05);
    }

    #[test]
    fn opaque_canonical_materials_reflect_their_stored_fraction_of_the_environment() {
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let ray = angled_top_ray();

        // Glass also transmits; its full composition is covered by its own test.
        for select in OPAQUE_CANONICAL_MATERIALS {
            let (mut scene, materials) = canonical_scene();
            let id = select(materials);
            let reflectivity = scene.material(id).unwrap().reflectivity();
            scene.add(SceneObject::new(
                Aabb::try_new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(),
                id,
            ));
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let sky = environment.sample(reflection_ray(ray, hit).unwrap().direction());
            let local = local_shading(&scene, ray, lighting);

            assert_color_approx_eq(
                Tracer::new(&scene, lighting, &environment)
                    .trace_ray(ray, PRIMARY_RAY_DEPTH)
                    .unwrap(),
                local.scale(1.0 - reflectivity) + sky.scale(reflectivity),
            );
        }
    }

    #[test]
    fn exterior_ray_entering_glass_is_classified_as_entering() {
        let scene = transmissive_cube_scene(0.0, 1.0);
        let ray = tilted_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

        assert!(ray.direction().dot(hit.geometry.normal) < 0.0);
        assert_eq!(
            refraction_interface(ray.direction(), hit.geometry.normal, GLASS_IOR),
            RefractionInterface {
                transition: MediumTransition::Entering,
                eta_incident: AIR_IOR,
                eta_transmitted: GLASS_IOR,
                normal: hit.geometry.normal,
            }
        );
    }

    #[test]
    fn entering_refracted_ray_starts_just_inside_and_bends_toward_the_normal() {
        let scene = transmissive_cube_scene(0.0, 1.0);
        let ray = tilted_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

        let refracted = refraction_ray(ray, hit, GLASS_IOR).unwrap().unwrap();

        assert_eq!(REFRACTION_RAY_ORIGIN_BIAS, 1.0e-4);
        assert_eq!(
            refracted.origin(),
            hit.geometry.position - hit.geometry.normal * REFRACTION_RAY_ORIGIN_BIAS
        );
        assert!(refracted.origin().y < 1.0 && refracted.origin().y > 0.99);
        // The incident plane is XY and the normal is +Y, so |x| is the sine to the normal.
        let (sin_i, sin_t) = (ray.direction().x, refracted.direction().x);
        assert!(sin_t > 0.0 && sin_t < sin_i);
        assert!((AIR_IOR * sin_i - GLASS_IOR * sin_t).abs() < 1.0e-6);
        assert!((refracted.direction().length() - 1.0).abs() < 1.0e-6);
        // The stored geometric normal is not replaced by the oriented interface normal.
        assert_eq!(hit.geometry.normal, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn entering_refracted_ray_hits_the_exit_face_not_its_entry_face() {
        let scene = transmissive_cube_scene(0.0, 1.0);
        let ray = tilted_top_ray();
        let entry = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let refracted = refraction_ray(ray, entry, GLASS_IOR).unwrap().unwrap();

        // The origin is inside the box, so intersection reports the exit surface.
        let exit = scene.closest_hit(refracted, 0.0, f32::INFINITY).unwrap();

        assert_eq!(entry.geometry.face, CubeFace::PositiveY);
        assert_eq!(exit.geometry.face, CubeFace::NegativeY);
        assert!(exit.geometry.t > 1.0);
        assert!(refracted.direction().dot(exit.geometry.normal) > 0.0);
        let tan_t = refracted.direction().x / -refracted.direction().y;
        assert!((exit.geometry.position.x - (0.5 + tan_t * (1.0 - 1.0e-4))).abs() < 1.0e-5);
    }

    #[test]
    fn inside_ray_reaching_a_face_is_classified_as_exiting() {
        let scene = transmissive_cube_scene(0.0, 1.0);
        let (inside, exit) = inside_ray_and_exit_hit(&scene);

        assert!(inside.direction().dot(exit.geometry.normal) > 0.0);
        assert_eq!(
            refraction_interface(inside.direction(), exit.geometry.normal, GLASS_IOR),
            RefractionInterface {
                transition: MediumTransition::Exiting,
                eta_incident: GLASS_IOR,
                eta_transmitted: AIR_IOR,
                normal: -exit.geometry.normal,
            }
        );
    }

    #[test]
    fn exiting_refracted_ray_starts_just_outside_and_restores_incident_direction() {
        let scene = transmissive_cube_scene(0.0, 1.0);
        let (inside, exit) = inside_ray_and_exit_hit(&scene);

        let outgoing = refraction_ray(inside, exit, GLASS_IOR).unwrap().unwrap();

        assert_eq!(
            outgoing.origin(),
            exit.geometry.position + exit.geometry.normal * REFRACTION_RAY_ORIGIN_BIAS
        );
        assert!(outgoing.origin().y < 0.0);
        // Parallel entry and exit faces restore the original direction ...
        assert!((outgoing.direction() - tilted_top_ray().direction()).length() < 1.0e-5);
        // ... displaced sideways: the unrefracted line would have crossed y = 0 at x = 0.8.
        assert!((exit.geometry.position.x - 0.8).abs() > 0.1);
        // The source object is still present; only the bias prevents re-hitting it.
        assert_eq!(scene.closest_hit(outgoing, 0.0, f32::INFINITY), None);
    }

    #[test]
    fn total_internal_reflection_spawns_no_refracted_ray() {
        let scene = transmissive_cube_scene(0.0, 1.0);
        // From inside, 45 degrees onto the +X face exceeds glass's ~41.8 degree critical angle.
        let ray = Ray::try_new(Vec3::new(0.5, 0.9, 0.5), Vec3::new(1.0, -1.0, 0.0)).unwrap();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        assert_eq!(hit.geometry.face, CubeFace::PositiveX);

        assert_eq!(refraction_ray(ray, hit, GLASS_IOR), Ok(None));
        // A lower index clears the critical angle, so the geometry alone is not the cause.
        assert!(refraction_ray(ray, hit, 1.2).unwrap().is_some());

        // With no reflectivity, a fully transparent surface under TIR shows its local shading.
        let lighting = upward_light_with_ambient();
        let environment = environment();
        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            assert_color_approx_eq(
                Tracer::new(&scene, lighting, &environment)
                    .trace_ray(ray, depth)
                    .unwrap(),
                traced_local(&scene, ray, lighting),
            );
        }
    }

    #[test]
    fn transparent_hit_below_maximum_depth_traces_refraction_at_next_depth() {
        // A distinct bottom texture makes the exit face seen through the top differ from the top,
        // so every depth below the maximum visibly blends in the refracted contribution.
        let lighting = ambient_only();
        let environment = environment();
        let mut scene = Scene::new();
        let top = scene
            .add_texture(Texture::solid(Color::new(0.2, 0.4, 0.6)))
            .unwrap();
        let bottom = scene
            .add_texture(Texture::solid(Color::new(0.9, 0.1, 0.3)))
            .unwrap();
        let material = scene
            .add_material(
                Material::try_new(
                    TextureSelection::TopSideBottom {
                        top,
                        side: top,
                        bottom,
                    },
                    Color::WHITE,
                    0.0,
                    0.5,
                    0.0,
                )
                .unwrap()
                .with_ior(GLASS_IOR)
                .unwrap(),
            )
            .unwrap();
        scene.add(SceneObject::new(unit_cube(), material));
        let tracer = Tracer::new(&scene, lighting, &environment);
        let ray = tilted_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let refracted_ray = refraction_ray(ray, hit, GLASS_IOR).unwrap().unwrap();
        let local = traced_local(&scene, ray, lighting);

        for depth in PRIMARY_RAY_DEPTH..MAX_RAY_DEPTH {
            let refracted = tracer.trace_ray(refracted_ray, depth + 1).unwrap();
            let expected = blend_transmission(local, refracted, 0.5);

            assert_ne!(expected, local);
            assert_color_approx_eq(tracer.trace_ray(ray, depth).unwrap(), expected);
        }
    }

    #[test]
    fn transparent_hit_at_maximum_depth_is_locally_shaded_only() {
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.glass));
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let ray = tilted_top_ray();
        let local = traced_local(&scene, ray, lighting);
        let without_witness = Tracer::new(&scene, lighting, &environment)
            .trace_ray(ray, MAX_RAY_DEPTH)
            .unwrap();

        let white = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(0.0, -2.0, 0.0), Vec3::new(2.0, -1.0, 1.0)).unwrap(),
            white,
        ));

        assert_eq!(without_witness, local);
        assert_eq!(
            Tracer::new(&scene, lighting, &environment).trace_ray(ray, MAX_RAY_DEPTH),
            Ok(local)
        );
    }

    #[test]
    fn refracted_miss_samples_environment_through_common_miss_path() {
        let environment = environment();
        let scene = transmissive_cube_scene(0.0, 1.0);
        let ray = tilted_top_ray();
        let entry = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let inside = refraction_ray(ray, entry, GLASS_IOR).unwrap().unwrap();
        let exit = scene.closest_hit(inside, 0.0, f32::INFINITY).unwrap();
        let outgoing = refraction_ray(inside, exit, GLASS_IOR).unwrap().unwrap();
        assert_eq!(scene.closest_hit(outgoing, 0.0, f32::INFINITY), None);

        assert_color_approx_eq(
            Tracer::new(&scene, ambient_only(), &environment)
                .trace_ray(ray, PRIMARY_RAY_DEPTH)
                .unwrap(),
            environment.sample(outgoing.direction()),
        );
    }

    #[test]
    fn refraction_displaces_geometry_seen_through_glass() {
        // Entering the 3-unit-thick slab at (0.5, 1, 0.5), `tilted_top_ray` leaves its bottom at
        // x ≈ 1.086 and reaches y = -3 at x ≈ 1.386. Unrefracted, it would reach x = 1.7 there.
        let witness_color = Color::new(0.9, 0.1, 0.3);
        let slab_scene = |witness_min_x: f32, witness_max_x: f32| {
            let mut scene = Scene::new();
            let glass = add_optical_material(&mut scene, Color::new(0.2, 0.4, 0.6), 0.0, 1.0);
            let witness = add_solid_material(&mut scene, witness_color);
            scene.add(SceneObject::new(
                Aabb::try_new(Vec3::new(-2.0, -2.0, 0.0), Vec3::new(3.0, 1.0, 1.0)).unwrap(),
                glass,
            ));
            scene.add(SceneObject::new(
                Aabb::try_new(
                    Vec3::new(witness_min_x, -3.5, 0.0),
                    Vec3::new(witness_max_x, -3.0, 1.0),
                )
                .unwrap(),
                witness,
            ));
            scene
        };
        let environment = environment();
        let ray = tilted_top_ray();

        let at_refracted_path = slab_scene(1.2, 1.55);
        let at_straight_path = slab_scene(1.55, 1.85);

        assert_color_approx_eq(
            Tracer::new(&at_refracted_path, ambient_only(), &environment)
                .trace_ray(ray, PRIMARY_RAY_DEPTH)
                .unwrap(),
            witness_color,
        );
        assert_color_approx_eq(
            Tracer::new(&at_straight_path, ambient_only(), &environment)
                .trace_ray(ray, PRIMARY_RAY_DEPTH)
                .unwrap(),
            environment.sample(ray.direction()),
        );
    }

    #[test]
    fn bounded_glass_configurations_terminate_with_finite_radiance() {
        let (mut scene, materials) = canonical_scene();
        for (min, max) in [
            // Touching blocks, an overlapping block, and two facing slabs.
            (Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)),
            (Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0)),
            (Vec3::new(0.5, 0.5, 0.5), Vec3::new(1.5, 1.5, 1.5)),
            (Vec3::new(-5.0, -2.0, -5.0), Vec3::new(5.0, -1.5, 5.0)),
            (Vec3::new(-5.0, 3.0, -5.0), Vec3::new(5.0, 3.5, 5.0)),
        ] {
            scene.add(SceneObject::new(
                Aabb::try_new(min, max).unwrap(),
                materials.glass,
            ));
        }
        let environment = environment();
        let tracer = Tracer::new(&scene, upward_light_with_ambient(), &environment);

        for origin in [
            Vec3::new(0.3, 2.5, 0.4),
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(-3.0, 0.2, 0.7),
        ] {
            for direction in [
                Vec3::new(0.2, -1.0, 0.1),
                Vec3::new(1.0, -1.0, 0.0),
                Vec3::new(-0.7, 0.4, 0.2),
                Vec3::new(1.0, 0.05, 0.3),
                Vec3::new(0.0, 1.0, 0.0),
            ] {
                let ray = Ray::try_new(origin, direction).unwrap();
                assert!(
                    tracer
                        .trace_ray(ray, PRIMARY_RAY_DEPTH)
                        .unwrap()
                        .is_finite()
                );
            }
        }
    }

    #[test]
    fn recursive_tracing_state_is_plain_copy_data() {
        // Recursion passes rays, hits, and colors by value on the stack; nothing is boxed.
        fn stack_value<T: Copy>() {}
        stack_value::<Ray>();
        stack_value::<crate::scene::SceneHit>();
        stack_value::<RefractionInterface>();
        stack_value::<Color>();
    }

    #[test]
    fn shadow_rays_do_not_consume_recursive_depth_with_refraction_active() {
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let mut scene = transmissive_cube_scene(0.0, 0.5);
        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.5, 0.25),
            Vec3::new(0.75, 1.9, 0.75),
        );
        let ray = angled_top_ray();

        assert_color_approx_eq(
            Tracer::new(&scene, lighting, &environment)
                .trace_ray(ray, MAX_RAY_DEPTH)
                .unwrap(),
            Color::new(0.04, 0.08, 0.12),
        );
    }

    #[test]
    fn transmission_blend_is_exact_linear_interpolation() {
        let base = Color::new(0.2, 0.4, 0.6);
        let refracted = Color::new(1.0, 0.0, 0.5);

        assert_color_approx_eq(blend_transmission(base, refracted, 0.0), base);
        assert_color_approx_eq(blend_transmission(base, refracted, 1.0), refracted);
        assert_color_approx_eq(
            blend_transmission(base, refracted, 0.85),
            base.scale(0.15) + refracted.scale(0.85),
        );
    }

    #[test]
    fn zero_transparency_preserves_reflection_only_result() {
        // A high index is irrelevant without transparency: nothing behind the cube shows.
        let lighting = ambient_only();
        let environment = environment();
        let mut scene = transmissive_cube_scene(0.35, 0.0);
        let ray = angled_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let sky = environment.sample(reflection_ray(ray, hit).unwrap().direction());
        let expected = traced_local(&scene, ray, lighting).lerp(sky, 0.35);
        let white = add_solid_material(&mut scene, Color::WHITE);
        let (min, max) = TRANSMISSION_WITNESS;
        scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), white));

        assert_color_approx_eq(
            Tracer::new(&scene, lighting, &environment)
                .trace_ray(ray, PRIMARY_RAY_DEPTH)
                .unwrap(),
            expected,
        );
    }

    #[test]
    fn full_transparency_selects_refracted_radiance() {
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let scene = transmissive_cube_scene(0.0, 1.0);
        let tracer = Tracer::new(&scene, lighting, &environment);
        let ray = tilted_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let refracted_ray = refraction_ray(ray, hit, GLASS_IOR).unwrap().unwrap();

        assert_color_approx_eq(
            tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap(),
            tracer.trace_ray(refracted_ray, 1).unwrap(),
        );
    }

    #[test]
    fn composition_blends_reflection_then_transmission_with_constant_coefficients() {
        // No Fresnel: head-on, oblique, and TIR-inducing incidences use the same coefficients.
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let scene = transmissive_cube_scene(0.25, 0.4);
        let tracer = Tracer::new(&scene, lighting, &environment);

        for ray in [downward_ray(), tilted_top_ray(), angled_top_ray()] {
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let reflected = tracer
                .trace_ray(reflection_ray(ray, hit).unwrap(), 1)
                .unwrap();
            let refracted = tracer
                .trace_ray(refraction_ray(ray, hit, GLASS_IOR).unwrap().unwrap(), 1)
                .unwrap();
            let base = traced_local(&scene, ray, lighting).lerp(reflected, 0.25);

            assert_color_approx_eq(
                tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap(),
                base.lerp(refracted, 0.4),
            );
        }
    }

    #[test]
    fn canonical_glass_composes_local_reflection_and_refraction() {
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.glass));
        let glass = scene.material(materials.glass).unwrap();
        assert_eq!(glass.reflectivity(), 0.15);
        assert_eq!(glass.transparency(), 0.85);
        assert_eq!(glass.ior(), GLASS_IOR);
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let tracer = Tracer::new(&scene, lighting, &environment);
        let ray = angled_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected = tracer
            .trace_ray(reflection_ray(ray, hit).unwrap(), 1)
            .unwrap();
        let refracted = tracer
            .trace_ray(refraction_ray(ray, hit, GLASS_IOR).unwrap().unwrap(), 1)
            .unwrap();
        let local = traced_local(&scene, ray, lighting);

        let traced = tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap();

        assert_color_approx_eq(traced, local.lerp(reflected, 0.15).lerp(refracted, 0.85));
        assert_ne!(traced, local.lerp(reflected, 0.15));
    }

    #[test]
    fn canonical_glass_refracts_what_lies_behind_it() {
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.glass));
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let ray = tilted_top_ray();
        let baseline = Tracer::new(&scene, lighting, &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        // A bright block below the cube where the transmitted ray arrives becomes visible.
        let white = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(0.0, -2.0, 0.0), Vec3::new(2.0, -1.0, 1.0)).unwrap(),
            white,
        ));

        assert_ne!(
            Tracer::new(&scene, lighting, &environment).trace_ray(ray, PRIMARY_RAY_DEPTH),
            Ok(baseline)
        );
    }

    #[test]
    fn glass_remains_an_opaque_shadow_blocker() {
        let (mut scene, materials) = canonical_scene();
        let white = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(unit_cube(), white));
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(0.25, 1.5, 0.25), Vec3::new(0.75, 1.9, 0.75)).unwrap(),
            materials.glass,
        ));
        // The diagonal view ray passes beside the glass; the upward shadow ray passes through it.
        let ray = angled_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        assert!((hit.geometry.position - Vec3::new(0.5, 1.0, 0.5)).length() < 1.0e-6);

        assert!(scene.is_occluded(
            Ray::try_new(Vec3::new(0.5, 1.1, 0.5), Vec3::new(0.0, 1.0, 0.0)).unwrap(),
            0.0,
            f32::INFINITY,
        ));
        assert_eq!(
            shade_hit(&scene, hit, ray.origin(), upward_light_with_ambient()),
            Ok(Color::new(0.2, 0.2, 0.2))
        );
    }

    #[test]
    fn non_transmissive_canonical_materials_do_not_refract() {
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let ray = angled_top_ray();

        for select in OPAQUE_CANONICAL_MATERIALS {
            let (mut scene, materials) = canonical_scene();
            let id = select(materials);
            assert_eq!(scene.material(id).unwrap().transparency(), 0.0);
            assert_eq!(scene.material(id).unwrap().ior(), AIR_IOR);
            scene.add(SceneObject::new(unit_cube(), id));
            let baseline = Tracer::new(&scene, lighting, &environment)
                .trace_ray(ray, PRIMARY_RAY_DEPTH)
                .unwrap();

            let white = add_solid_material(&mut scene, Color::WHITE);
            let (min, max) = TRANSMISSION_WITNESS;
            scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), white));

            assert_eq!(
                Tracer::new(&scene, lighting, &environment).trace_ray(ray, PRIMARY_RAY_DEPTH),
                Ok(baseline)
            );
        }

        // Obsidian still reflects even though it does not refract.
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.obsidian));
        assert_ne!(
            Tracer::new(&scene, lighting, &environment).trace_ray(ray, PRIMARY_RAY_DEPTH),
            Ok(traced_local(&scene, ray, lighting))
        );
    }

    #[test]
    fn cobblestone_still_shades_with_its_geometric_normal() {
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.cobblestone));
        let lighting = directional(Vec3::new(0.6, 1.0, 0.8));
        let cobblestone = scene.material(materials.cobblestone).unwrap();

        for origin in [Vec3::new(0.2, 2.0, 0.3), Vec3::new(0.7, 2.0, 0.8)] {
            let ray = Ray::try_new(origin, Vec3::new(0.0, -1.0, 0.0)).unwrap();
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            assert_eq!(hit.geometry.normal, Vec3::new(0.0, 1.0, 0.0));

            assert_eq!(
                shade_hit(&scene, hit, origin, lighting),
                Ok(shade_surface(
                    cobblestone.surface_color(Color::new(0.6, 0.5, 0.4)),
                    hit.geometry.normal,
                    Vec3::new(0.0, 1.0, 0.0),
                    cobblestone.specular(),
                    true,
                    lighting,
                ))
            );
        }
    }

    #[test]
    fn showcase_layout_renders_finite_colors_across_camera_poses() {
        // The five-block diagnostic layout from `main.rs`, swept over orbit, elevation, and zoom.
        let (mut scene, materials) = canonical_scene();
        for (min, max, material) in [
            (
                Vec3::new(-4.0, -0.5, -3.0),
                Vec3::new(4.0, 0.0, 3.0),
                materials.grass,
            ),
            (
                Vec3::new(-2.8, 0.0, -0.2),
                Vec3::new(-1.6, 1.4, 1.0),
                materials.cobblestone,
            ),
            (
                Vec3::new(-1.3, 0.0, -0.6),
                Vec3::new(-0.1, 1.8, 0.6),
                materials.obsidian,
            ),
            (
                Vec3::new(0.2, 0.0, -0.2),
                Vec3::new(1.4, 1.6, 1.0),
                materials.glass,
            ),
            (
                Vec3::new(1.7, 0.0, -0.8),
                Vec3::new(2.9, 1.3, 0.4),
                materials.lava,
            ),
        ] {
            scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), material));
        }
        let lighting = Lighting::new(
            AmbientLight::try_new(Color::new(0.30, 0.34, 0.46), 0.50).unwrap(),
            DirectionalLight::try_new(Vec3::new(0.6, 1.0, 0.8), Color::new(1.0, 0.84, 0.70), 1.0)
                .unwrap(),
        );
        let environment = environment();
        let mut framebuffer = Framebuffer::try_new(32, 18).unwrap();

        for yaw_step in 0..8 {
            for pitch in [-0.6, 0.15, 0.9] {
                for radius in [3.0, 10.0] {
                    let camera = OrbitalCamera::try_new(
                        Vec3::new(0.0, 0.7, 0.0),
                        yaw_step as f32 * std::f32::consts::FRAC_PI_4 + 0.1,
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
        let lava_reflectivity = scene.material(materials.lava).unwrap().reflectivity();
        let reflected_sky = environment
            .sample(Vec3::new(0.0, 1.0, 0.0))
            .scale(lava_reflectivity);

        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            // Under zero light, lava shows only its small stored reflection, never emitted light.
            let expected = if can_spawn_secondary_ray(depth) {
                reflected_sky
            } else {
                Color::BLACK
            };
            assert_color_approx_eq(tracer.trace_ray(onto_lava, depth).unwrap(), expected);
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
