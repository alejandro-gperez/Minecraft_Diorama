use crate::{
    camera::OrbitalCamera,
    environment::Environment,
    lighting::{Lighting, PointLight, shade_point_light, shade_surface},
    material::{AIR_IOR, shading_normal},
    math::Vec3,
    ray::Ray,
    scene::{Scene, SceneHit},
};

use super::{Color, Framebuffer, fresnel::schlick_reflectance};

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
/// Fixed world-space offset along the geometric normal for point-light shadow rays.
///
/// Numerically equal to `RAY_ORIGIN_BIAS` today, but kept separate because point-light shadow rays
/// have a finite interval and aim at a position rather than along a direction. Same unit-scale
/// assumption.
const POINT_LIGHT_SHADOW_BIAS: f32 = 1.0e-4;
/// Fixed world-space offset moving a reflected ray onto the incident side of its hit surface.
///
/// Numerically equal to `RAY_ORIGIN_BIAS` today, but kept separate: reflection and shadow rays
/// may need different offsets once the scene scale changes, and a reflection's offset direction
/// depends on which side the incident ray arrived from. The same unit-scale assumption applies.
const REFLECTION_RAY_ORIGIN_BIAS: f32 = 1.0e-4;
/// Fixed world-space offset moving a refracted ray onto the transmitted side of its interface.
///
/// Numerically equal to the other biases for the same unit-scale reason, but its direction depends
/// on whether the ray enters or exits the medium, so it is a separate operation and constant.
const REFRACTION_RAY_ORIGIN_BIAS: f32 = 1.0e-4;
/// Fresnel reflectance under total internal reflection: no transmitted direction exists, so the
/// whole transmissive optical portion reflects.
const TOTAL_INTERNAL_REFLECTANCE: f32 = 1.0;
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
    FresnelReflectanceUnavailable,
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
            // Terminal hit: its local shading stands in, unweighted, for the optical contribution
            // it may no longer trace. This holds for transmissive materials too.
            return Ok(local);
        }

        // Material lookup already succeeded inside `shade_hit`.
        let material = self
            .scene
            .material(hit.material_id)
            .ok_or(RenderError::MaterialNotFound)?;

        let transparency = material.transparency();
        if transparency > 0.0 {
            // Fresnel alone splits the transmissive portion between reflection and refraction;
            // the stored reflectivity is not applied on top, which would count reflection twice.
            let optical = self.trace_fresnel_optics(ray, hit, material.ior(), depth)?;
            return Ok(blend_transmissive(local, optical, transparency));
        }

        let reflectivity = material.reflectivity();
        if reflectivity > 0.0 {
            let reflected = self.trace_ray(reflection_ray(ray, hit)?, depth + 1)?;
            Ok(blend_reflection(local, reflected, reflectivity))
        } else {
            Ok(local)
        }
    }

    /// Radiance leaving a transmissive interface back along `ray`, split by Fresnel reflectance:
    /// `reflected * F + refracted * (1 - F)`.
    ///
    /// `F` comes from Schlick's approximation at the incidence cosine against the oriented
    /// interface normal, or is exactly `1` under total internal reflection, where no refracted ray
    /// exists. Each branch is traced at `depth + 1` only when its weight is non-zero: distinct
    /// indices keep `F >= R0 > 0`, so reflection is traced, while refraction is skipped under
    /// total internal reflection and at exactly grazing incidence.
    fn trace_fresnel_optics(
        &self,
        ray: Ray,
        hit: SceneHit,
        material_ior: f32,
        depth: RayDepth,
    ) -> Result<Color, RenderError> {
        let refracted_ray = refraction_ray(ray, hit, material_ior)?;
        let reflectance = if refracted_ray.is_some() {
            optical_interface(ray.direction(), hit.geometry.normal, material_ior)
                .fresnel_reflectance(ray.direction())
                .ok_or(RenderError::FresnelReflectanceUnavailable)?
        } else {
            TOTAL_INTERNAL_REFLECTANCE
        };

        let reflected = if reflectance > 0.0 {
            self.trace_ray(reflection_ray(ray, hit)?, depth + 1)?
        } else {
            Color::BLACK
        };
        let refracted = match refracted_ray {
            Some(refracted_ray) if reflectance < 1.0 => self.trace_ray(refracted_ray, depth + 1)?,
            _ => Color::BLACK,
        };

        Ok(compose_fresnel(reflected, refracted, reflectance))
    }
}

/// Reports whether a ray traced at `depth` may spawn a radiance ray at `depth + 1`.
const fn can_spawn_secondary_ray(depth: RayDepth) -> bool {
    depth < MAX_RAY_DEPTH
}

/// Mirror reflection of `ray` about the hit's outward geometric normal, `R = D - 2(D·N)N`.
///
/// A reflected ray stays in the medium the incident ray travelled through, so its origin is
/// offset onto the incident side: `position + N * bias` for a ray arriving from outside and
/// `position - N * bias` for one arriving from inside the AABB. The inside case keeps a ray
/// reflecting within glass from being pushed out and immediately re-entering the same face. The
/// source object is never excluded.
fn reflection_ray(ray: Ray, hit: SceneHit) -> Result<Ray, RenderError> {
    let normal = hit.geometry.normal;
    let incident_side = MediumTransition::classify(ray.direction(), normal).oriented_normal(normal);
    let origin = hit.geometry.position + incident_side * REFLECTION_RAY_ORIGIN_BIAS;

    Ray::try_new(origin, ray.direction().reflect(normal))
        .ok_or(RenderError::ReflectionRayGenerationFailed)
}

/// Energy-conserving blend of local shading and reflected radiance with a constant coefficient.
fn blend_reflection(local: Color, reflected: Color, reflectivity: f32) -> Color {
    local.lerp(reflected, reflectivity)
}

/// Whether a ray crosses an AABB surface into or out of its material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediumTransition {
    Entering,
    Exiting,
}

impl MediumTransition {
    /// Classifies an incident direction against a hit's outward geometric normal `N`.
    ///
    /// `D·N < 0` arrives from outside and enters; otherwise the ray arrives from inside and exits.
    /// This is the single source of interface orientation for reflection, refraction, and
    /// Fresnel; the camera position plays no part.
    fn classify(direction: Vec3, outward_normal: Vec3) -> Self {
        if direction.dot(outward_normal) < 0.0 {
            Self::Entering
        } else {
            Self::Exiting
        }
    }

    /// Interface normal on the incident side, opposing the incident ray: `N` when entering and
    /// `-N` when exiting.
    fn oriented_normal(self, outward_normal: Vec3) -> Vec3 {
        match self {
            Self::Entering => outward_normal,
            Self::Exiting => -outward_normal,
        }
    }
}

/// Optical description of one transmissive interface event at an AABB surface.
///
/// `normal` is the interface normal oriented against the incident ray, used only for the Snell
/// and Fresnel calculations; the hit's outward geometric normal stays untouched for lighting,
/// reflection, face identity, and UVs.
#[derive(Clone, Copy, Debug, PartialEq)]
struct OpticalInterface {
    transition: MediumTransition,
    eta_incident: f32,
    eta_transmitted: f32,
    normal: Vec3,
}

impl OpticalInterface {
    /// Incidence cosine `clamp(-D·n, 0, 1)` against the oriented normal `n`.
    fn incidence_cosine(self, direction: Vec3) -> f32 {
        (-direction.dot(self.normal)).clamp(0.0, 1.0)
    }

    /// Schlick reflectance for `direction` crossing this interface from `eta_incident` into
    /// `eta_transmitted`. Total internal reflection must be handled by the caller.
    fn fresnel_reflectance(self, direction: Vec3) -> Option<f32> {
        schlick_reflectance(
            self.incidence_cosine(direction),
            self.eta_incident,
            self.eta_transmitted,
        )
    }
}

/// Describes the interface an incident direction crosses at a transmissive AABB surface.
///
/// Every transmissive AABB is assumed to be surrounded by air: a ray against the outward normal
/// enters from `AIR_IOR` into `material_ior`, and a ray along it exits back into air. No stack of
/// nested media is tracked, so two touching glass boxes still form glass -> air -> glass.
fn optical_interface(direction: Vec3, outward_normal: Vec3, material_ior: f32) -> OpticalInterface {
    let transition = MediumTransition::classify(direction, outward_normal);
    let (eta_incident, eta_transmitted) = match transition {
        MediumTransition::Entering => (AIR_IOR, material_ior),
        MediumTransition::Exiting => (material_ior, AIR_IOR),
    };

    OpticalInterface {
        transition,
        eta_incident,
        eta_transmitted,
        normal: transition.oriented_normal(outward_normal),
    }
}

/// Snell refraction of `ray` through the hit surface, or `None` on total internal reflection.
///
/// The origin is pushed to the transmitted side, opposite the oriented normal: just inside the
/// AABB when entering (`position - N * bias`) and just outside when exiting
/// (`position + N * bias`), for outward normal `N`. An entering ray therefore starts inside the
/// box, where AABB intersection reports the exit face. The source object is never excluded.
fn refraction_ray(ray: Ray, hit: SceneHit, material_ior: f32) -> Result<Option<Ray>, RenderError> {
    let interface = optical_interface(ray.direction(), hit.geometry.normal, material_ior);
    let eta = interface.eta_incident / interface.eta_transmitted;
    let Some(direction) = ray.direction().refract(interface.normal, eta) else {
        return Ok(None);
    };
    let origin = hit.geometry.position - interface.normal * REFRACTION_RAY_ORIGIN_BIAS;

    Ray::try_new(origin, direction)
        .map(Some)
        .ok_or(RenderError::RefractionRayGenerationFailed)
}

/// Fresnel split of the transmissive optical portion: `reflected * F + refracted * (1 - F)`.
fn compose_fresnel(reflected: Color, refracted: Color, reflectance: f32) -> Color {
    reflected.scale(reflectance) + refracted.scale(1.0 - reflectance)
}

/// Transmissive-material composition: `local * (1 - transparency) + optical * transparency`.
///
/// `transparency` is the fraction of the surface that behaves as a clear optical interface; the
/// remainder keeps its textured, lit local appearance.
fn blend_transmissive(local: Color, optical: Color, transparency: f32) -> Color {
    local.scale(1.0 - transparency) + optical.scale(transparency)
}

/// Local surface shading: texture, albedo, direct lighting, point lights, and emission.
///
/// Lighting uses the *shading* normal: the geometric normal, or for a material with a normal map
/// the normal decoded at the hit's UV and rotated into the face's tangent basis. The geometric
/// normal still owns face identity, UVs, shadow-ray origins, and the side a light must be on.
/// Reflection and refraction never see the shading normal; they use the geometric interface.
///
/// ```text
/// local = ambient
///       + directional_visible * (directional_diffuse + directional_specular)
///       + sum(point_visible * (point_diffuse + point_specular))
///       + emission
/// ```
///
/// Emission is self-radiance added last: it is not scaled by any light term and is never
/// shadowed. Shadow rays are any-hit visibility queries rather than radiance rays, so they take no
/// recursion depth and are cast identically at every traced depth. Emission spawns no rays.
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

    let shading_normal = match material.normal_map() {
        Some(normal_map_id) => {
            let normal_map = scene
                .texture(normal_map_id)
                .ok_or(RenderError::TextureNotFound)?;
            let sample = normal_map
                .sample_nearest(uv.u, uv.v)
                .ok_or(RenderError::TextureSamplingFailed)?;
            shading_normal(hit.geometry.face, sample)
        }
        None => hit.geometry.normal,
    };

    let view_direction = (viewer_position - hit.geometry.position)
        .try_normalized()
        .ok_or(RenderError::ViewDirectionUnavailable)?;
    let light = lighting.directional();
    // The geometric normal decides whether the sun is on the visible side of the surface and
    // where the shadow ray starts. A bump facing the sun on a face turned away from it is under
    // the surface and must stay dark, so this gate also closes `shade_surface` below.
    let direct_light_visible = if light.intensity() > 0.0 {
        if hit.geometry.normal.dot(light.direction_to_light()) > 0.0 {
            let shadow_origin = hit.geometry.position + hit.geometry.normal * RAY_ORIGIN_BIAS;
            let shadow_ray = Ray::try_new(shadow_origin, light.direction_to_light())
                .ok_or(RenderError::ShadowRayGenerationFailed)?;
            !scene.is_occluded(shadow_ray, SHADOW_RAY_T_MIN, SHADOW_RAY_T_MAX)
        } else {
            false
        }
    } else {
        true
    };

    let base_color = material.surface_color(texture_sample);
    let mut color = shade_surface(
        base_color,
        shading_normal,
        view_direction,
        material.specular(),
        direct_light_visible,
        lighting,
    );
    for point_light in scene.point_lights() {
        color = color
            + point_light_contribution(
                scene,
                *point_light,
                hit,
                shading_normal,
                base_color,
                view_direction,
                material.specular(),
            )?;
    }
    if material.is_emissive() {
        color = color + material.emitted_radiance(texture_sample);
    }

    Ok(color)
}

/// Diffuse and specular contribution of one point light, or black when it cannot reach the hit.
///
/// Out-of-radius lights, inert lights, and surfaces facing away are rejected before any shadow ray
/// is traced. The shadow ray starts `POINT_LIGHT_SHADOW_BIAS` off the surface along the geometric
/// normal and aims at the light, with `t_max` equal to the remaining distance to the light. The
/// light is a point, not geometry, so a blocker beyond it cannot shadow it. Glass blocks the ray
/// like any other AABB.
///
/// The geometric normal admits the light and offsets the shadow ray; `shading_normal`, equal to
/// it unless the material has a normal map, drives diffuse and specular.
fn point_light_contribution(
    scene: &Scene,
    light: PointLight,
    hit: SceneHit,
    shading_normal: Vec3,
    base_color: Color,
    view_direction: Vec3,
    material_specular: f32,
) -> Result<Color, RenderError> {
    let Some(incidence) = light.incidence_at(hit.geometry.position, hit.geometry.normal) else {
        return Ok(Color::BLACK);
    };

    let shadow_origin = hit.geometry.position + hit.geometry.normal * POINT_LIGHT_SHADOW_BIAS;
    let to_light = light.position() - shadow_origin;
    // The biased origin can only coincide with the light when the light sits on the surface, where
    // it has no defined direction and, like `incidence_at`, contributes nothing. This must not be
    // a render error: a light placed against geometry would otherwise abort the whole frame.
    let Some(shadow_ray) = Ray::try_new(shadow_origin, to_light) else {
        return Ok(Color::BLACK);
    };
    if scene.is_occluded(shadow_ray, SHADOW_RAY_T_MIN, to_light.length()) {
        return Ok(Color::BLACK);
    }

    Ok(shade_point_light(
        incidence,
        base_color,
        shading_normal,
        view_direction,
        material_specular,
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
        MAX_RAY_DEPTH, MediumTransition, OpticalInterface, POINT_LIGHT_SHADOW_BIAS,
        PRIMARY_RAY_DEPTH, RAY_ORIGIN_BIAS, REFLECTION_RAY_ORIGIN_BIAS, REFRACTION_RAY_ORIGIN_BIAS,
        RenderError, Renderer, TOTAL_INTERNAL_REFLECTANCE, Tracer, blend_reflection,
        blend_transmissive, can_spawn_secondary_ray, compose_fresnel, optical_interface,
        pixel_center, reflection_ray, refraction_ray, shade_hit,
    };
    use crate::{
        camera::OrbitalCamera,
        environment::Environment,
        geometry::{Aabb, CubeFace},
        lighting::{
            AmbientLight, DirectionalLight, Lighting, PointLight, shade_point_light, shade_surface,
        },
        material::{
            AIR_IOR, CanonicalMaterials, CanonicalTextureIds, GLASS_IOR, LAVA_EMISSION_COLOR,
            LAVA_EMISSION_STRENGTH, Material, MaterialId, Texture, TextureId, TextureSelection,
        },
        math::Vec3,
        ray::Ray,
        render::{Color, Framebuffer, fresnel::schlick_reflectance},
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
        let flat_normal = scene
            .add_texture(Texture::solid(Color::new(0.5, 0.5, 1.0)))
            .unwrap();
        let materials = scene
            .add_canonical_materials(CanonicalTextureIds {
                grass_top: texture,
                grass_side: texture,
                dirt: texture,
                cobblestone: texture,
                cobblestone_normal: flat_normal,
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

    /// Unit cube of a glass-index material whose `(0.9, 0.1, 0.3)` bottom face is distinguishable
    /// from its `(0.2, 0.4, 0.6)` top and sides, so paths ending on the bottom are observable.
    fn two_tone_transmissive_cube_scene(transparency: f32) -> Scene {
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
                    transparency,
                    0.0,
                )
                .unwrap()
                .with_ior(GLASS_IOR)
                .unwrap(),
            )
            .unwrap();
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

    /// Schlick reflectance of `ray` at its closest hit on a glass-index surface.
    fn glass_reflectance(scene: &Scene, ray: Ray) -> f32 {
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        optical_interface(ray.direction(), hit.geometry.normal, GLASS_IOR)
            .fresnel_reflectance(ray.direction())
            .unwrap()
    }

    /// The Mission 18 transmissive composition written out independently of the tracer:
    /// `local(1 - t) + (reflected F + refracted (1 - F)) t`, with children traced at `depth + 1`.
    fn expected_transmissive(
        tracer: &Tracer,
        scene: &Scene,
        ray: Ray,
        depth: u32,
        transparency: f32,
    ) -> Color {
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let local = traced_local(scene, ray, tracer.lighting);
        let reflected = tracer
            .trace_ray(reflection_ray(ray, hit).unwrap(), depth + 1)
            .unwrap();
        let (refracted, reflectance) = match refraction_ray(ray, hit, GLASS_IOR).unwrap() {
            Some(refracted_ray) => (
                tracer.trace_ray(refracted_ray, depth + 1).unwrap(),
                glass_reflectance(scene, ray),
            ),
            None => (Color::BLACK, 1.0),
        };
        let optical = reflected.scale(reflectance) + refracted.scale(1.0 - reflectance);
        local.scale(1.0 - transparency) + optical.scale(transparency)
    }

    /// Ray from inside the unit cube striking its +X face 45 degrees off the normal, beyond
    /// glass's ~41.8 degree critical angle.
    fn internal_tir_ray() -> Ray {
        Ray::try_new(Vec3::new(0.5, 0.9, 0.5), Vec3::new(1.0, -1.0, 0.0)).unwrap()
    }

    /// Ray striking the unit cube's top face about 87 degrees off the normal.
    fn grazing_top_ray() -> Ray {
        Ray::try_new(Vec3::new(-1.5, 1.1, 0.5), Vec3::new(1.0, -0.05, 0.0)).unwrap()
    }

    fn max_channel_difference(a: Color, b: Color) -> f32 {
        (a.r - b.r)
            .abs()
            .max((a.g - b.g).abs())
            .max((a.b - b.b).abs())
    }

    fn assert_strictly_inside_unit_cube(point: Vec3) {
        for c in [point.x, point.y, point.z] {
            assert!(c > 0.0 && c < 1.0, "{point:?} is not inside the unit cube");
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
            optical_interface(ray.direction(), hit.geometry.normal, GLASS_IOR),
            OpticalInterface {
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
            optical_interface(inside.direction(), exit.geometry.normal, GLASS_IOR),
            OpticalInterface {
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
        let ray = internal_tir_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        assert_eq!(hit.geometry.face, CubeFace::PositiveX);

        assert_eq!(refraction_ray(ray, hit, GLASS_IOR), Ok(None));
        // A lower index clears the critical angle, so the geometry alone is not the cause.
        assert!(refraction_ray(ray, hit, 1.2).unwrap().is_some());
    }

    #[test]
    fn total_internal_reflection_routes_the_optical_portion_to_reflection() {
        // The +X face is struck from inside and mirrors onto the distinct bottom face.
        let lighting = ambient_only();
        let environment = environment();
        let ray = internal_tir_ray();
        let mut scene = two_tone_transmissive_cube_scene(0.85);
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected_ray = reflection_ray(ray, hit).unwrap();
        assert!(reflected_ray.direction().is_finite() && reflected_ray.origin().is_finite());
        assert_eq!(TOTAL_INTERNAL_REFLECTANCE, 1.0);
        let before_witness = Tracer::new(&scene, lighting, &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        // A witness where the ray would transmit with a lower index stays invisible: no refracted
        // ray is traced.
        let white = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(1.5, -1.5, 0.0), Vec3::new(2.5, 0.0, 1.0)).unwrap(),
            white,
        ));
        let tracer = Tracer::new(&scene, lighting, &environment);
        let local = traced_local(&scene, ray, lighting);

        for depth in PRIMARY_RAY_DEPTH..MAX_RAY_DEPTH {
            let reflected = tracer.trace_ray(reflected_ray, depth + 1).unwrap();
            let traced = tracer.trace_ray(ray, depth).unwrap();

            assert!(traced.is_finite());
            // The local interface keeps exactly its `1 - transparency` share.
            assert_color_approx_eq(traced, local.scale(0.15) + reflected.scale(0.85));
            assert_color_approx_eq(
                traced,
                expected_transmissive(&tracer, &scene, ray, depth, 0.85),
            );
            // Mission 17 fell back to the local color for the whole transmissive portion.
            assert!(
                max_channel_difference(traced, local) > 0.05,
                "{traced:?} vs {local:?}"
            );
        }
        assert_eq!(tracer.trace_ray(ray, PRIMARY_RAY_DEPTH), Ok(before_witness));
    }

    #[test]
    fn canonical_glass_under_total_internal_reflection_no_longer_shows_its_local_fallback() {
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.glass));
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let tracer = Tracer::new(&scene, lighting, &environment);
        let ray = internal_tir_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected = tracer
            .trace_ray(reflection_ray(ray, hit).unwrap(), 1)
            .unwrap();
        let local = traced_local(&scene, ray, lighting);

        let traced = tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap();

        assert_color_approx_eq(traced, local.scale(0.15) + reflected.scale(0.85));
        // The Mission 17 result was lerp(local, reflected, reflectivity): mostly local color,
        // which made total-internal-reflection edges glow with the lit interface texture.
        let mission_17_fallback = local.lerp(reflected, 0.15);
        assert!((traced.r - mission_17_fallback.r).abs() > 0.1);
    }

    #[test]
    fn interfaces_choose_indices_and_a_normal_opposing_the_incident_ray() {
        let scene = transmissive_cube_scene(0.0, 1.0);
        let (inside, _) = inside_ray_and_exit_hit(&scene);
        let cases = [
            (downward_ray(), MediumTransition::Entering),
            (tilted_top_ray(), MediumTransition::Entering),
            (angled_top_ray(), MediumTransition::Entering),
            (grazing_top_ray(), MediumTransition::Entering),
            (inside, MediumTransition::Exiting),
            (internal_tir_ray(), MediumTransition::Exiting),
        ];

        for (ray, transition) in cases {
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let outward = hit.geometry.normal;
            let interface = optical_interface(ray.direction(), outward, GLASS_IOR);
            let expected_etas = match transition {
                MediumTransition::Entering => (AIR_IOR, GLASS_IOR),
                MediumTransition::Exiting => (GLASS_IOR, AIR_IOR),
            };

            assert_eq!(
                MediumTransition::classify(ray.direction(), outward),
                transition
            );
            assert_eq!(interface.transition, transition);
            assert_eq!(
                (interface.eta_incident, interface.eta_transmitted),
                expected_etas
            );
            assert_eq!(interface.normal, transition.oriented_normal(outward));
            assert!(ray.direction().dot(interface.normal) < 0.0);
            let cosine = interface.incidence_cosine(ray.direction());
            assert!((cosine - ray.direction().dot(outward).abs()).abs() < 1.0e-6);
            assert!((0.0..=1.0).contains(&cosine));
            // The oriented normal is derived; the stored normal stays the outward face normal.
            assert_eq!(scene.closest_hit(ray, 0.0, f32::INFINITY), Some(hit));
            assert_eq!(outward, hit.geometry.face.normal());
        }
    }

    #[test]
    fn exterior_reflection_origin_lies_outside_the_object() {
        let scene = transmissive_cube_scene(0.0, 1.0);

        for ray in [
            downward_ray(),
            tilted_top_ray(),
            angled_top_ray(),
            grazing_top_ray(),
        ] {
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let reflected = reflection_ray(ray, hit).unwrap();

            assert_eq!(
                reflected.origin(),
                hit.geometry.position + hit.geometry.normal * REFLECTION_RAY_ORIGIN_BIAS
            );
            assert!(reflected.origin().y > 1.0);
            assert_eq!(scene.closest_hit(reflected, 0.0, f32::INFINITY), None);
        }
    }

    #[test]
    fn interior_reflection_origin_stays_inside_the_glass() {
        let scene = transmissive_cube_scene(0.0, 1.0);
        let ray = internal_tir_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        assert_eq!(hit.geometry.face, CubeFace::PositiveX);

        let reflected = reflection_ray(ray, hit).unwrap();

        assert_eq!(REFLECTION_RAY_ORIGIN_BIAS, 1.0e-4);
        assert_eq!(
            reflected.origin(),
            hit.geometry.position - hit.geometry.normal * REFLECTION_RAY_ORIGIN_BIAS
        );
        assert_strictly_inside_unit_cube(reflected.origin());
        // Still inside, the mirrored ray crosses the interior to the bottom face, where it is
        // again an exiting ray, instead of re-entering the face it reflected from.
        let next = scene.closest_hit(reflected, 0.0, f32::INFINITY).unwrap();
        assert_eq!(next.geometry.face, CubeFace::NegativeY);
        assert!((next.geometry.t - 0.4 * std::f32::consts::SQRT_2).abs() < 1.0e-3);
        assert_eq!(
            MediumTransition::classify(reflected.direction(), next.geometry.normal),
            MediumTransition::Exiting
        );

        // The Mission 17 exterior origin put the same ray just outside, re-entering at once.
        let pushed_outside = Ray::try_new(
            hit.geometry.position + hit.geometry.normal * REFLECTION_RAY_ORIGIN_BIAS,
            reflected.direction(),
        )
        .unwrap();
        let reentry = scene
            .closest_hit(pushed_outside, 0.0, f32::INFINITY)
            .unwrap();
        assert_eq!(reentry.geometry.face, CubeFace::PositiveX);
        assert!(reentry.geometry.t < 1.0e-3);
    }

    #[test]
    fn interior_partial_reflection_continues_through_the_interior() {
        let scene = transmissive_cube_scene(0.0, 1.0);
        let (inside, exit) = inside_ray_and_exit_hit(&scene);

        let reflected = reflection_ray(inside, exit).unwrap();

        assert_strictly_inside_unit_cube(reflected.origin());
        let next = scene.closest_hit(reflected, 0.0, f32::INFINITY).unwrap();
        assert_eq!(next.geometry.face, CubeFace::PositiveY);
        assert!(next.geometry.t > 0.9);
    }

    #[test]
    fn transparent_hit_below_maximum_depth_traces_refraction_at_next_depth() {
        // A distinct bottom texture makes the exit face seen through the top differ from the top,
        // so every depth below the maximum visibly blends in the refracted contribution.
        let lighting = ambient_only();
        let environment = environment();
        let scene = two_tone_transmissive_cube_scene(0.5);
        let tracer = Tracer::new(&scene, lighting, &environment);
        let ray = tilted_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected_ray = reflection_ray(ray, hit).unwrap();
        let refracted_ray = refraction_ray(ray, hit, GLASS_IOR).unwrap().unwrap();
        let reflectance = glass_reflectance(&scene, ray);
        let local = traced_local(&scene, ray, lighting);

        for depth in PRIMARY_RAY_DEPTH..MAX_RAY_DEPTH {
            // Both children are traced exactly one level deeper than their parent.
            let reflected = tracer.trace_ray(reflected_ray, depth + 1).unwrap();
            let refracted = tracer.trace_ray(refracted_ray, depth + 1).unwrap();
            let expected = blend_transmissive(
                local,
                compose_fresnel(reflected, refracted, reflectance),
                0.5,
            );

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

        // Witnesses on both the transmitted and the mirrored path stay invisible: neither
        // branch is traced, yet the terminal hit still receives its full local shading.
        let white = add_solid_material(&mut scene, Color::WHITE);
        for (min, max) in [
            (Vec3::new(0.0, -2.0, 0.0), Vec3::new(2.0, -1.0, 1.0)),
            (Vec3::new(0.6, 2.3, 0.0), Vec3::new(1.3, 2.8, 1.0)),
        ] {
            scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), white));
        }
        let tracer = Tracer::new(&scene, lighting, &environment);

        assert_eq!(without_witness, local);
        assert_ne!(local, Color::BLACK);
        assert_eq!(tracer.trace_ray(ray, MAX_RAY_DEPTH), Ok(local));
        assert_ne!(tracer.trace_ray(ray, MAX_RAY_DEPTH - 1), Ok(local));
    }

    #[test]
    fn refracted_miss_samples_environment_through_common_miss_path() {
        let environment = environment();
        let scene = transmissive_cube_scene(0.0, 1.0);
        let tracer = Tracer::new(&scene, ambient_only(), &environment);
        let (inside, exit) = inside_ray_and_exit_hit(&scene);
        let outgoing = refraction_ray(inside, exit, GLASS_IOR).unwrap().unwrap();
        let internal = reflection_ray(inside, exit).unwrap();
        assert_eq!(scene.closest_hit(outgoing, 0.0, f32::INFINITY), None);
        let sky = environment.sample(outgoing.direction());
        assert_eq!(tracer.trace_ray(outgoing, MAX_RAY_DEPTH), Ok(sky));

        // One level above the limit, the exiting ray's refracted child is that environment sample.
        let reflectance = glass_reflectance(&scene, inside);
        let expected = compose_fresnel(
            tracer.trace_ray(internal, MAX_RAY_DEPTH).unwrap(),
            sky,
            reflectance,
        );

        assert!(reflectance < 0.05);
        assert_color_approx_eq(
            tracer.trace_ray(inside, MAX_RAY_DEPTH - 1).unwrap(),
            expected,
        );
    }

    #[test]
    fn glass_reflected_miss_samples_environment_through_common_miss_path() {
        let environment = environment();
        let scene = transmissive_cube_scene(0.0, 1.0);
        let tracer = Tracer::new(&scene, ambient_only(), &environment);
        let ray = tilted_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected = reflection_ray(ray, hit).unwrap();
        let refracted = refraction_ray(ray, hit, GLASS_IOR).unwrap().unwrap();
        assert_eq!(scene.closest_hit(reflected, 0.0, f32::INFINITY), None);
        let sky = environment.sample(reflected.direction());
        assert_eq!(tracer.trace_ray(reflected, MAX_RAY_DEPTH), Ok(sky));

        assert_color_approx_eq(
            tracer.trace_ray(ray, MAX_RAY_DEPTH - 1).unwrap(),
            compose_fresnel(
                sky,
                tracer.trace_ray(refracted, MAX_RAY_DEPTH).unwrap(),
                glass_reflectance(&scene, ray),
            ),
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
        let trace = |scene: &Scene| {
            Tracer::new(scene, ambient_only(), &environment)
                .trace_ray(ray, PRIMARY_RAY_DEPTH)
                .unwrap()
        };

        let unobstructed = trace(&slab_scene(10.0, 11.0));
        let at_refracted_path = trace(&slab_scene(1.2, 1.55));
        let at_straight_path = trace(&slab_scene(1.55, 1.85));

        // Both near-normal interfaces transmit about 96%, so the witness dominates the pixel.
        assert!(at_refracted_path.r - unobstructed.r > 0.5);
        assert_eq!(at_straight_path, unobstructed);
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
        stack_value::<OpticalInterface>();
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
    fn fresnel_composition_splits_reflected_and_refracted_radiance_exactly() {
        let reflected = Color::new(0.9, 0.1, 0.3);
        let refracted = Color::new(0.2, 0.4, 0.6);

        assert_eq!(compose_fresnel(reflected, refracted, 0.0), refracted);
        assert_eq!(compose_fresnel(reflected, refracted, 1.0), reflected);
        assert_color_approx_eq(
            compose_fresnel(reflected, refracted, 0.04),
            reflected.scale(0.04) + refracted.scale(0.96),
        );
        assert_color_approx_eq(
            compose_fresnel(reflected, refracted, 0.7),
            Color::new(0.69, 0.19, 0.39),
        );
    }

    #[test]
    fn transmissive_blend_weights_local_by_one_minus_transparency() {
        let local = Color::new(0.2, 0.4, 0.6);
        let optical = Color::new(1.0, 0.0, 0.5);

        assert_eq!(blend_transmissive(local, optical, 0.0), local);
        assert_eq!(blend_transmissive(local, optical, 1.0), optical);
        assert_color_approx_eq(
            blend_transmissive(local, optical, 0.85),
            local.scale(0.15) + optical.scale(0.85),
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
    fn full_transparency_yields_only_the_fresnel_optical_composition() {
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let scene = transmissive_cube_scene(0.0, 1.0);
        let tracer = Tracer::new(&scene, lighting, &environment);
        let ray = tilted_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected = tracer
            .trace_ray(reflection_ray(ray, hit).unwrap(), 1)
            .unwrap();
        let refracted = tracer
            .trace_ray(refraction_ray(ray, hit, GLASS_IOR).unwrap().unwrap(), 1)
            .unwrap();
        let reflectance = glass_reflectance(&scene, ray);

        assert_color_approx_eq(
            tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap(),
            reflected.scale(reflectance) + refracted.scale(1.0 - reflectance),
        );
    }

    #[test]
    fn zero_transparency_transmissive_material_yields_local_shading_only() {
        // Glass index and no reflectivity, but nothing transmits: only the local surface shows.
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let scene = transmissive_cube_scene(0.0, 0.0);
        let tracer = Tracer::new(&scene, lighting, &environment);

        for ray in [downward_ray(), tilted_top_ray(), grazing_top_ray()] {
            for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
                assert_eq!(
                    tracer.trace_ray(ray, depth),
                    Ok(traced_local(&scene, ray, lighting))
                );
            }
        }
    }

    #[test]
    fn intermediate_transparency_matches_the_exact_fresnel_equation() {
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let scene = transmissive_cube_scene(0.25, 0.4);
        let tracer = Tracer::new(&scene, lighting, &environment);

        for ray in [
            downward_ray(),
            tilted_top_ray(),
            angled_top_ray(),
            grazing_top_ray(),
            internal_tir_ray(),
        ] {
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let local = traced_local(&scene, ray, lighting);
            let reflected = tracer
                .trace_ray(reflection_ray(ray, hit).unwrap(), 1)
                .unwrap();
            // Every exterior ray here enters from air; the cosine is |D·N| by construction.
            let (refracted, reflectance) = match refraction_ray(ray, hit, GLASS_IOR).unwrap() {
                Some(refracted_ray) => (
                    tracer.trace_ray(refracted_ray, 1).unwrap(),
                    schlick_reflectance(
                        ray.direction().dot(hit.geometry.normal).abs(),
                        AIR_IOR,
                        GLASS_IOR,
                    )
                    .unwrap(),
                ),
                None => (Color::BLACK, 1.0),
            };
            let optical = reflected.scale(reflectance) + refracted.scale(1.0 - reflectance);

            assert_color_approx_eq(
                tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap(),
                local.scale(0.6) + optical.scale(0.4),
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
        let reflectance = glass_reflectance(&scene, ray);

        let traced = tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap();

        // The stored 0.15 reflectivity plays no part: Fresnel alone splits the 85% optical share.
        assert_color_approx_eq(
            traced,
            local.scale(0.15)
                + (reflected.scale(reflectance) + refracted.scale(1.0 - reflectance)).scale(0.85),
        );
        assert_color_approx_eq(traced, expected_transmissive(&tracer, &scene, ray, 0, 0.85));
        let mission_17 = local.lerp(reflected, 0.15).lerp(refracted, 0.85);
        assert!(max_channel_difference(traced, mission_17) > 1.0e-3);
    }

    #[test]
    fn normal_incidence_glass_is_mostly_refraction() {
        let lighting = ambient_only();
        let environment = environment();
        let scene = transmissive_cube_scene(0.0, 1.0);
        let tracer = Tracer::new(&scene, lighting, &environment);
        let ray = downward_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected = tracer
            .trace_ray(reflection_ray(ray, hit).unwrap(), 1)
            .unwrap();
        let refracted = tracer
            .trace_ray(refraction_ray(ray, hit, GLASS_IOR).unwrap().unwrap(), 1)
            .unwrap();
        let reflectance = glass_reflectance(&scene, ray);

        assert!((reflectance - 0.04).abs() < 1.0e-6);
        assert_color_approx_eq(
            tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap(),
            reflected.scale(0.04) + refracted.scale(0.96),
        );
    }

    #[test]
    fn grazing_glass_reflects_more_than_normal_incidence() {
        let lighting = ambient_only();
        let environment = environment();
        let scene = transmissive_cube_scene(0.0, 1.0);
        let tracer = Tracer::new(&scene, lighting, &environment);

        let normal = glass_reflectance(&scene, downward_ray());
        let oblique = glass_reflectance(&scene, angled_top_ray());
        let grazing = glass_reflectance(&scene, grazing_top_ray());

        assert!(normal < oblique && oblique < grazing);
        assert!(normal < 0.05 && oblique < 0.1 && grazing > 0.75);
        for ray in [downward_ray(), angled_top_ray(), grazing_top_ray()] {
            assert_color_approx_eq(
                tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap(),
                expected_transmissive(&tracer, &scene, ray, PRIMARY_RAY_DEPTH, 1.0),
            );
        }
    }

    #[test]
    fn stored_reflectivity_is_not_double_counted_for_transmissive_materials() {
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let unreflective = transmissive_cube_scene(0.0, 0.85);
        let reflective = transmissive_cube_scene(0.9, 0.85);

        for ray in [
            downward_ray(),
            tilted_top_ray(),
            grazing_top_ray(),
            internal_tir_ray(),
        ] {
            for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
                assert_eq!(
                    Tracer::new(&reflective, lighting, &environment).trace_ray(ray, depth),
                    Tracer::new(&unreflective, lighting, &environment).trace_ray(ray, depth)
                );
            }
        }
    }

    #[test]
    fn obsidian_reflection_stays_constant_and_non_fresnel() {
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.obsidian));
        let obsidian = scene.material(materials.obsidian).unwrap();
        assert_eq!(obsidian.reflectivity(), 0.35);
        assert_eq!(obsidian.transparency(), 0.0);
        let lighting = upward_light_with_ambient();
        let environment = environment();
        let tracer = Tracer::new(&scene, lighting, &environment);

        for ray in [downward_ray(), angled_top_ray(), grazing_top_ray()] {
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let reflected = tracer
                .trace_ray(reflection_ray(ray, hit).unwrap(), 1)
                .unwrap();

            assert_color_approx_eq(
                tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap(),
                traced_local(&scene, ray, lighting).scale(0.65) + reflected.scale(0.35),
            );
        }
    }

    #[test]
    fn glass_below_maximum_depth_shows_geometry_in_both_branches() {
        // `tilted_top_ray` mirrors up through the first witness and, refracted twice, leaves the
        // cube's bottom toward the second.
        let reflection_witness = (Vec3::new(0.6, 2.3, 0.0), Vec3::new(1.3, 2.8, 1.0));
        let refraction_witness = (Vec3::new(0.8, -2.0, 0.0), Vec3::new(1.6, -1.2, 1.0));
        let scene_with = |witnesses: &[(Vec3, Vec3)]| {
            let mut scene = transmissive_cube_scene(0.0, 1.0);
            let witness = add_solid_material(&mut scene, Color::new(0.9, 0.1, 0.3));
            for &(min, max) in witnesses {
                scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), witness));
            }
            scene
        };
        let environment = environment();
        let ray = tilted_top_ray();
        let bare = scene_with(&[]);
        let bare_color = Tracer::new(&bare, ambient_only(), &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        for witnesses in [
            &[reflection_witness][..],
            &[refraction_witness][..],
            &[reflection_witness, refraction_witness][..],
        ] {
            let scene = scene_with(witnesses);
            let tracer = Tracer::new(&scene, ambient_only(), &environment);
            let traced = tracer.trace_ray(ray, PRIMARY_RAY_DEPTH).unwrap();

            assert!(max_channel_difference(traced, bare_color) > 1.0e-3);
            for depth in PRIMARY_RAY_DEPTH..MAX_RAY_DEPTH {
                assert_color_approx_eq(
                    tracer.trace_ray(ray, depth).unwrap(),
                    expected_transmissive(&tracer, &scene, ray, depth, 1.0),
                );
            }
        }
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

    /// Showcase lava light position; mirrors `main.rs`.
    const SHOWCASE_LAVA_LIGHT: Vec3 = Vec3::new(2.3, 1.0, -1.15);

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
        scene.add_point_light(
            PointLight::try_new(SHOWCASE_LAVA_LIGHT, Color::new(1.0, 0.45, 0.12), 3.0, 5.0)
                .unwrap(),
        );
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

    /// Opaque, non-reflective material that emits `emission_color * strength` through its texture.
    fn add_emissive_material(
        scene: &mut Scene,
        texture_color: Color,
        emission_color: Color,
        strength: f32,
    ) -> MaterialId {
        let texture_id = scene.add_texture(Texture::solid(texture_color)).unwrap();
        scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture_id),
                    Color::WHITE,
                    0.0,
                    0.0,
                    0.0,
                )
                .unwrap()
                .with_emission(emission_color, strength)
                .unwrap(),
            )
            .unwrap()
    }

    /// Unit cube emitting `(0.5, 0.25, 0.1)`: white texture, emission color `(1, 0.5, 0.2)` at 0.5.
    fn emissive_cube_scene() -> Scene {
        let mut scene = Scene::new();
        let material =
            add_emissive_material(&mut scene, Color::WHITE, Color::new(1.0, 0.5, 0.2), 0.5);
        scene.add(SceneObject::new(unit_cube(), material));
        scene
    }

    const CUBE_EMISSION: Color = Color::new(0.5, 0.25, 0.1);

    /// Light straight below a surface that faces up: zero Lambert diffuse and no shadow ray.
    fn downward_facing_light() -> Lighting {
        Lighting::new(
            AmbientLight::try_new(Color::BLACK, 0.0).unwrap(),
            DirectionalLight::try_new(Vec3::new(0.0, -1.0, 0.0), Color::WHITE, 1.0).unwrap(),
        )
    }

    #[test]
    fn emissive_surface_shows_its_emission_with_no_light_at_all() {
        let scene = emissive_cube_scene();
        let (hit, camera_position) = top_hit(&scene);

        assert_color_approx_eq(
            shade_hit(&scene, hit, camera_position, darkness()).unwrap(),
            CUBE_EMISSION,
        );
    }

    #[test]
    fn emission_remains_when_directional_diffuse_is_zero() {
        let scene = emissive_cube_scene();
        let (hit, camera_position) = top_hit(&scene);

        // The top face looks away from a light below it: no Lambert term and no ambient.
        assert_color_approx_eq(
            shade_hit(&scene, hit, camera_position, downward_facing_light()).unwrap(),
            CUBE_EMISSION,
        );
    }

    #[test]
    fn emission_remains_when_the_directional_light_is_occluded() {
        let mut scene = emissive_cube_scene();
        let (hit, camera_position) = top_hit(&scene);
        let lighting = upward_light_with_ambient();
        let unblocked = shade_hit(&scene, hit, camera_position, lighting).unwrap();

        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.5, 0.25),
            Vec3::new(0.75, 1.9, 0.75),
        );
        let blocked = shade_hit(&scene, hit, camera_position, lighting).unwrap();

        assert!(unblocked.r > blocked.r);
        // Ambient (0.2) plus the untouched emission.
        assert_color_approx_eq(blocked, Color::new(0.2, 0.2, 0.2) + CUBE_EMISSION);
    }

    #[test]
    fn emission_is_added_to_lighting_rather_than_scaled_by_it() {
        let emissive = emissive_cube_scene();
        let plain = unit_cube_scene();
        let (hit, camera_position) = top_hit(&emissive);
        let lighting = upward_light_with_ambient();

        let lit_emissive = shade_hit(&emissive, hit, camera_position, lighting).unwrap();
        let lit_plain = shade_hit(&plain, hit, camera_position, lighting).unwrap();

        assert!(lit_plain.r > 1.0);
        assert_color_approx_eq(lit_emissive, lit_plain + CUBE_EMISSION);
    }

    #[test]
    fn emission_follows_the_texture_and_ignores_albedo() {
        let mut scene = Scene::new();
        let texture = scene
            .add_texture(Texture::solid(Color::new(0.8, 0.4, 0.2)))
            .unwrap();
        let material = scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture),
                    Color::BLACK,
                    0.0,
                    0.0,
                    0.0,
                )
                .unwrap()
                .with_emission(Color::WHITE, 2.0)
                .unwrap(),
            )
            .unwrap();
        scene.add(SceneObject::new(unit_cube(), material));
        let (hit, camera_position) = top_hit(&scene);

        assert_color_approx_eq(
            shade_hit(&scene, hit, camera_position, darkness()).unwrap(),
            Color::new(1.6, 0.8, 0.4),
        );
    }

    #[test]
    fn canonical_lava_emits_under_zero_light() {
        let (mut scene, materials) = canonical_scene();
        let white = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(unit_cube(), materials.lava));
        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0)).unwrap(),
            white,
        ));
        let environment = environment();
        let tracer = Tracer::new(&scene, darkness(), &environment);
        let onto_lava = downward_ray();
        let onto_neighbor =
            Ray::try_new(Vec3::new(1.5, 2.0, 0.5), Vec3::new(0.0, -1.0, 0.0)).unwrap();
        let texture = Color::new(0.6, 0.5, 0.4);
        let emission = texture * LAVA_EMISSION_COLOR.scale(LAVA_EMISSION_STRENGTH);
        let lava_reflectivity = scene.material(materials.lava).unwrap().reflectivity();
        let reflected_sky = environment.sample(Vec3::new(0.0, 1.0, 0.0));

        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            // Emission is part of the local result; lava's small stored reflection blends with it
            // below the maximum depth, and only the reflected fraction is ever environment light.
            let expected = if can_spawn_secondary_ray(depth) {
                blend_reflection(emission, reflected_sky, lava_reflectivity)
            } else {
                emission
            };
            assert_color_approx_eq(tracer.trace_ray(onto_lava, depth).unwrap(), expected);
            assert_eq!(tracer.trace_ray(onto_neighbor, depth), Ok(Color::BLACK));
        }
    }

    #[test]
    fn non_lava_canonical_materials_stay_dark_under_zero_light() {
        let environment = environment();

        let selectors: [fn(CanonicalMaterials) -> MaterialId; 2] = [|m| m.grass, |m| m.cobblestone];
        for select in selectors {
            let (mut scene, materials) = canonical_scene();
            scene.add(SceneObject::new(unit_cube(), select(materials)));
            let tracer = Tracer::new(&scene, darkness(), &environment);

            for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
                let traced = tracer.trace_ray(downward_ray(), depth).unwrap();
                // Only the stored reflection of the environment may remain, never self-light.
                assert!(traced.r <= 0.05 * environment.sample(Vec3::new(0.0, 1.0, 0.0)).r + 1.0e-5);
            }
        }

        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.glass));
        assert_eq!(
            Tracer::new(&scene, darkness(), &environment).trace_ray(downward_ray(), MAX_RAY_DEPTH),
            Ok(Color::BLACK)
        );
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.obsidian));
        assert_eq!(
            Tracer::new(&scene, darkness(), &environment).trace_ray(downward_ray(), MAX_RAY_DEPTH),
            Ok(Color::BLACK)
        );
    }

    #[test]
    fn emission_spawns_no_secondary_radiance_rays() {
        // Opaque, non-reflective emissive material: the ordinary depth policy spawns nothing, so
        // the traced result equals local shading at every depth, including the terminal one.
        let scene = emissive_cube_scene();
        let environment = environment();
        let lighting = upward_light_with_ambient();
        let tracer = Tracer::new(&scene, lighting, &environment);
        let ray = downward_ray();
        let local = traced_local(&scene, ray, lighting);

        assert!(local.r > CUBE_EMISSION.r);
        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            assert_eq!(tracer.trace_ray(ray, depth), Ok(local));
        }
    }

    #[test]
    fn obsidian_reflection_shows_emissive_lava() {
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.obsidian));
        let (min, max) = REFLECTION_WITNESS;
        scene.add(SceneObject::new(
            Aabb::try_new(min, max).unwrap(),
            materials.lava,
        ));
        let environment = environment();
        let ray = angled_top_ray();
        let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
        let reflected_ray = reflection_ray(ray, hit).unwrap();
        let lava_hit = scene
            .closest_hit(reflected_ray, 0.0, f32::INFINITY)
            .unwrap();
        assert_eq!(lava_hit.material_id, materials.lava);

        let traced = Tracer::new(&scene, darkness(), &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        // Obsidian is black under zero light, so its whole output is its reflectivity times the
        // radiance of the lava ray reached through the ordinary `trace_ray` path.
        let obsidian_reflectivity = scene.material(materials.obsidian).unwrap().reflectivity();
        let lava_radiance = Tracer::new(&scene, darkness(), &environment)
            .trace_ray(reflected_ray, PRIMARY_RAY_DEPTH + 1)
            .unwrap();
        assert!(lava_radiance.r > 0.5);
        assert_color_approx_eq(traced, lava_radiance.scale(obsidian_reflectivity));
    }

    #[test]
    fn glass_refraction_shows_emissive_lava() {
        let environment = environment();
        let build = |emissive: bool| {
            let (mut scene, materials) = canonical_scene();
            let dark = add_solid_material(&mut scene, Color::new(0.05, 0.05, 0.05));
            scene.add(SceneObject::new(unit_cube(), materials.glass));
            // Directly below the glass cube along the vertical view ray.
            scene.add(SceneObject::new(
                Aabb::try_new(Vec3::new(0.0, -2.0, 0.0), Vec3::new(1.0, -1.0, 1.0)).unwrap(),
                if emissive { materials.lava } else { dark },
            ));
            scene
        };

        let with_lava = Tracer::new(&build(true), darkness(), &environment)
            .trace_ray(downward_ray(), PRIMARY_RAY_DEPTH)
            .unwrap();
        let with_dark = Tracer::new(&build(false), darkness(), &environment)
            .trace_ray(downward_ray(), PRIMARY_RAY_DEPTH)
            .unwrap();

        assert!(with_lava.r > with_dark.r + 0.05);
        assert!(with_lava.g > with_dark.g);
        assert!(with_lava.is_finite());
    }

    #[test]
    fn recursion_through_emissive_and_point_lit_materials_stays_bounded() {
        let (mut scene, materials) = canonical_scene();
        for (min, max, material) in [
            (
                Vec3::new(-4.0, -0.5, -3.0),
                Vec3::new(4.0, 0.0, 3.0),
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
        scene.add_point_light(
            PointLight::try_new(
                Vec3::new(1.55, 0.9, 0.7),
                Color::new(1.0, 0.45, 0.12),
                2.0,
                4.0,
            )
            .unwrap(),
        );
        let environment = environment();
        let tracer = Tracer::new(&scene, upward_light_with_ambient(), &environment);

        for (origin, direction) in [
            (Vec3::new(0.8, 3.0, 0.4), Vec3::new(0.3, -1.0, 0.1)),
            (Vec3::new(-3.0, 1.0, 0.4), Vec3::new(1.0, -0.1, 0.0)),
            (Vec3::new(0.8, 0.8, 4.0), Vec3::new(0.2, -0.05, -1.0)),
        ] {
            let ray = Ray::try_new(origin, direction).unwrap();
            assert!(
                tracer
                    .trace_ray(ray, PRIMARY_RAY_DEPTH)
                    .unwrap()
                    .is_finite()
            );
            assert_eq!(
                tracer.trace_ray(ray, MAX_RAY_DEPTH + 1),
                Err(RenderError::RayDepthExceeded)
            );
        }
    }

    /// Unit-intensity white point light above the unit cube's top-face center, radius 4.
    const LIGHT_ABOVE: Vec3 = Vec3::new(0.5, 2.0, 0.5);

    fn add_light_above(scene: &mut Scene) {
        scene.add_point_light(PointLight::try_new(LIGHT_ABOVE, Color::WHITE, 1.0, 4.0).unwrap());
    }

    /// Point-light diffuse at distance 1 of radius 4 on a white, upward-facing surface.
    const POINT_DIFFUSE_AT_ONE: f32 = 0.5625;

    /// Shades the unit cube's top-face center using the cube hit found in a blocker-free scene, so
    /// blockers added afterwards cannot replace the shaded hit. The cube must be material `0`.
    fn point_lit_top(scene: &Scene, lighting: Lighting) -> Color {
        let (hit, camera_position) = top_hit(&unit_cube_scene());
        shade_hit(scene, hit, camera_position, lighting).unwrap()
    }

    #[test]
    fn unobstructed_point_light_illuminates_the_surface() {
        let mut scene = unit_cube_scene();
        add_light_above(&mut scene);

        assert_color_approx_eq(
            point_lit_top(&scene, darkness()),
            Color::new(
                POINT_DIFFUSE_AT_ONE,
                POINT_DIFFUSE_AT_ONE,
                POINT_DIFFUSE_AT_ONE,
            ),
        );
    }

    #[test]
    fn blocker_between_surface_and_point_light_removes_its_contribution() {
        let mut scene = unit_cube_scene();
        add_light_above(&mut scene);
        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.3, 0.25),
            Vec3::new(0.75, 1.6, 0.75),
        );

        assert_eq!(point_lit_top(&scene, darkness()), Color::BLACK);
    }

    #[test]
    fn point_light_shadow_removes_specular_as_well_as_diffuse() {
        let mut scene = Scene::new();
        let texture_id = scene.add_texture(Texture::solid(Color::BLACK)).unwrap();
        let shiny = scene
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
        scene.add(SceneObject::new(unit_cube(), shiny));
        add_light_above(&mut scene);
        let (hit, camera_position) = top_hit(&scene);
        let visible = shade_hit(&scene, hit, camera_position, darkness()).unwrap();

        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.3, 0.25),
            Vec3::new(0.75, 1.6, 0.75),
        );
        let blocked = shade_hit(&scene, hit, camera_position, darkness()).unwrap();

        assert!(visible.r > 0.0);
        assert_eq!(blocked, Color::BLACK);
    }

    #[test]
    fn blocker_beyond_the_point_light_does_not_shadow_it() {
        let mut scene = unit_cube_scene();
        add_light_above(&mut scene);
        let baseline = point_lit_top(&scene, darkness());

        // Directly behind the light, on the same line from the surface.
        add_blocker(
            &mut scene,
            Vec3::new(0.25, 2.2, 0.25),
            Vec3::new(0.75, 2.8, 0.75),
        );

        assert!(baseline.r > 0.0);
        assert_eq!(point_lit_top(&scene, darkness()), baseline);
    }

    #[test]
    fn blocker_behind_the_surface_or_off_the_path_does_not_shadow_the_point_light() {
        let mut scene = unit_cube_scene();
        add_light_above(&mut scene);
        let baseline = point_lit_top(&scene, darkness());

        add_blocker(
            &mut scene,
            Vec3::new(0.25, -2.0, 0.25),
            Vec3::new(0.75, -1.5, 0.75),
        );
        add_blocker(
            &mut scene,
            Vec3::new(2.0, 1.2, 2.0),
            Vec3::new(3.0, 2.2, 3.0),
        );

        assert_eq!(point_lit_top(&scene, darkness()), baseline);
    }

    #[test]
    fn nearby_legitimate_blocker_is_not_skipped_by_the_point_light_bias() {
        let mut scene = unit_cube_scene();
        add_light_above(&mut scene);
        let min_y = 1.0 + POINT_LIGHT_SHADOW_BIAS * 1.5;
        add_blocker(
            &mut scene,
            Vec3::new(0.25, min_y, 0.25),
            Vec3::new(0.75, min_y + POINT_LIGHT_SHADOW_BIAS, 0.75),
        );

        assert_eq!(point_lit_top(&scene, darkness()), Color::BLACK);
    }

    #[test]
    fn ambient_remains_when_a_point_light_is_blocked() {
        let mut scene = unit_cube_scene();
        add_light_above(&mut scene);
        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.3, 0.25),
            Vec3::new(0.75, 1.6, 0.75),
        );

        assert_color_approx_eq(
            point_lit_top(&scene, ambient_only()),
            Color::new(1.0, 1.0, 1.0),
        );
    }

    #[test]
    fn emission_remains_when_a_point_light_is_blocked() {
        let mut scene = emissive_cube_scene();
        add_light_above(&mut scene);
        add_blocker(
            &mut scene,
            Vec3::new(0.25, 1.3, 0.25),
            Vec3::new(0.75, 1.6, 0.75),
        );

        assert_color_approx_eq(point_lit_top(&scene, darkness()), CUBE_EMISSION);
    }

    #[test]
    fn point_light_bias_prevents_self_shadowing_on_every_face() {
        let center = Vec3::new(0.5, 0.5, 0.5);
        let faces = [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
        ];

        for outward in faces {
            let mut scene = unit_cube_scene();
            scene.add_point_light(
                PointLight::try_new(center + outward * 1.0, Color::WHITE, 1.0, 4.0).unwrap(),
            );
            let ray = Ray::try_new(center + outward * 3.0, -outward).unwrap();
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let lit = shade_hit(&scene, hit, ray.origin(), darkness()).unwrap();

            assert!(lit.r > 0.3, "face {outward:?} was self-shadowed: {lit:?}");
        }
    }

    #[test]
    fn point_light_just_above_the_surface_is_not_self_shadowed() {
        let mut scene = unit_cube_scene();
        scene.add_point_light(
            PointLight::try_new(Vec3::new(0.5, 1.01, 0.5), Color::WHITE, 1.0, 4.0).unwrap(),
        );

        assert!(point_lit_top(&scene, darkness()).r > 0.9);
    }

    #[test]
    fn point_light_on_or_inside_the_bias_of_the_surface_is_not_a_render_error() {
        for height in [1.0, 1.0 + POINT_LIGHT_SHADOW_BIAS * 0.5, 1.0 + 1.0e-7] {
            let mut scene = unit_cube_scene();
            scene.add_point_light(
                PointLight::try_new(Vec3::new(0.5, height, 0.5), Color::WHITE, 1.0, 4.0).unwrap(),
            );
            let (hit, camera_position) = top_hit(&scene);

            let color = shade_hit(&scene, hit, camera_position, darkness()).unwrap();

            assert!(color.is_finite());
        }
    }

    #[test]
    fn glass_blocks_point_light_like_any_other_aabb() {
        let (mut scene, materials) = canonical_scene();
        let white = add_solid_material(&mut scene, Color::WHITE);
        scene.add(SceneObject::new(unit_cube(), white));
        add_light_above(&mut scene);
        let (hit, camera_position) = top_hit(&scene);
        let open = shade_hit(&scene, hit, camera_position, darkness()).unwrap();

        scene.add(SceneObject::new(
            Aabb::try_new(Vec3::new(0.25, 1.3, 0.25), Vec3::new(0.75, 1.6, 0.75)).unwrap(),
            materials.glass,
        ));

        assert!(open.r > 0.0);
        assert_eq!(
            shade_hit(&scene, hit, camera_position, darkness()),
            Ok(Color::BLACK)
        );
    }

    #[test]
    fn point_light_outside_its_radius_adds_nothing() {
        let mut scene = unit_cube_scene();
        scene.add_point_light(PointLight::try_new(LIGHT_ABOVE, Color::WHITE, 1.0, 1.0).unwrap());

        // The hit is exactly one radius away.
        assert_eq!(point_lit_top(&scene, darkness()), Color::BLACK);

        let mut far = unit_cube_scene();
        far.add_point_light(
            PointLight::try_new(Vec3::new(0.5, 20.0, 0.5), Color::WHITE, 100.0, 4.0).unwrap(),
        );
        assert_eq!(point_lit_top(&far, darkness()), Color::BLACK);
    }

    #[test]
    fn point_light_behind_a_surface_adds_nothing() {
        let mut scene = unit_cube_scene();
        scene.add_point_light(
            PointLight::try_new(Vec3::new(0.5, 0.5, 0.5), Color::WHITE, 1.0, 4.0).unwrap(),
        );

        assert_eq!(point_lit_top(&scene, darkness()), Color::BLACK);
    }

    #[test]
    fn point_lights_add_to_each_other_and_to_the_directional_light() {
        let lighting = upward_light_with_ambient();
        let mut first_only = unit_cube_scene();
        first_only.add_point_light(
            PointLight::try_new(
                Vec3::new(0.5, 2.0, 0.5),
                Color::new(1.0, 0.5, 0.2),
                1.0,
                4.0,
            )
            .unwrap(),
        );
        let mut second_only = unit_cube_scene();
        second_only.add_point_light(
            PointLight::try_new(
                Vec3::new(0.3, 1.5, 0.6),
                Color::new(0.2, 0.4, 1.0),
                0.5,
                3.0,
            )
            .unwrap(),
        );
        let mut both = unit_cube_scene();
        for scene in [&first_only, &second_only] {
            for light in scene.point_lights() {
                both.add_point_light(*light);
            }
        }
        let without = point_lit_top(&unit_cube_scene(), lighting);

        let first = point_lit_top(&first_only, lighting);
        let second = point_lit_top(&second_only, lighting);

        assert!(first.r > without.r);
        assert!(second.b > without.b);
        assert_color_approx_eq(
            point_lit_top(&both, lighting),
            first + second + without.scale(-1.0),
        );
    }

    #[test]
    fn point_light_color_tints_the_lit_surface() {
        let mut scene = unit_cube_scene();
        scene.add_point_light(
            PointLight::try_new(LIGHT_ABOVE, Color::new(1.0, 0.5, 0.0), 1.0, 4.0).unwrap(),
        );

        let lit = point_lit_top(&scene, darkness());

        assert_color_approx_eq(
            lit,
            Color::new(POINT_DIFFUSE_AT_ONE, POINT_DIFFUSE_AT_ONE * 0.5, 0.0),
        );
    }

    #[test]
    fn point_lights_do_not_change_environment_misses() {
        let mut scene = unit_cube_scene();
        add_light_above(&mut scene);
        let environment = environment();
        let tracer = Tracer::new(&scene, darkness(), &environment);

        for direction in miss_directions() {
            let ray = Ray::try_new(Vec3::new(3.0, 0.5, 3.0), direction).unwrap();
            assert_eq!(
                tracer.trace_ray(ray, PRIMARY_RAY_DEPTH),
                Ok(environment.sample(ray.direction()))
            );
        }
    }

    #[test]
    fn terminal_depth_hits_still_receive_point_light_shading() {
        let mut scene = unit_cube_scene();
        add_light_above(&mut scene);
        let environment = environment();
        let tracer = Tracer::new(&scene, darkness(), &environment);

        for depth in PRIMARY_RAY_DEPTH..=MAX_RAY_DEPTH {
            assert_color_approx_eq(
                tracer.trace_ray(downward_ray(), depth).unwrap(),
                Color::new(
                    POINT_DIFFUSE_AT_ONE,
                    POINT_DIFFUSE_AT_ONE,
                    POINT_DIFFUSE_AT_ONE,
                ),
            );
        }
    }

    #[test]
    fn canonical_obsidian_shows_a_point_light_highlight() {
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.obsidian));
        let warm = Color::new(1.0, 0.45, 0.12);
        scene.add_point_light(PointLight::try_new(LIGHT_ABOVE, warm, 1.0, 4.0).unwrap());
        let (hit, camera_position) = top_hit(&scene);

        let lit = shade_hit(&scene, hit, camera_position, darkness()).unwrap();

        // Aligned view and light: diffuse of the tinted texture plus a warm specular highlight.
        assert!(lit.r > lit.g && lit.g > lit.b);
        let diffuse_only = Color::new(0.6, 0.5, 0.4)
            * Color::new(0.90, 0.90, 1.00)
            * warm.scale(POINT_DIFFUSE_AT_ONE);
        let specular = scene.material(materials.obsidian).unwrap().specular();
        assert_color_approx_eq(
            lit,
            diffuse_only + warm.scale(POINT_DIFFUSE_AT_ONE * specular),
        );
    }

    #[test]
    fn reflected_rays_see_point_lit_surfaces() {
        let ray = angled_top_ray();
        let witness = Color::new(0.9, 0.1, 0.3);
        let unlit = mirror_and_witness_scene(1.0, witness);
        let mut lit = mirror_and_witness_scene(1.0, witness);
        // Just below the witness's underside, where the mirror reflection lands.
        lit.add_point_light(
            PointLight::try_new(Vec3::new(1.5, 1.7, 0.5), Color::WHITE, 1.0, 2.0).unwrap(),
        );
        let environment = environment();

        let without = Tracer::new(&unlit, darkness(), &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();
        let with = Tracer::new(&lit, darkness(), &environment)
            .trace_ray(ray, PRIMARY_RAY_DEPTH)
            .unwrap();

        assert_eq!(without, Color::BLACK);
        assert!(with.r > 0.1);
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

    // --- Normal mapping -------------------------------------------------------------------------

    const ALL_FACES: [CubeFace; 6] = [
        CubeFace::NegativeX,
        CubeFace::PositiveX,
        CubeFace::NegativeY,
        CubeFace::PositiveY,
        CubeFace::NegativeZ,
        CubeFace::PositiveZ,
    ];

    /// Tangent-space normal tilted by `angle` radians toward tangent-space +X (increasing `u`).
    fn lean_u(angle: f32) -> Vec3 {
        Vec3::new(angle.sin(), 0.0, angle.cos())
    }

    fn encode_normal(normal: Vec3) -> Color {
        Color::new(
            normal.x * 0.5 + 0.5,
            normal.y * 0.5 + 0.5,
            normal.z * 0.5 + 0.5,
        )
    }

    /// One-texel normal map holding `tangent_normal`.
    fn uniform_normal_map(tangent_normal: Vec3) -> Texture {
        Texture::solid(encode_normal(tangent_normal))
    }

    /// Material with an optional normal map and the given optical weights, over a solid `color`.
    fn add_mapped_material(
        scene: &mut Scene,
        color: Color,
        specular: f32,
        transparency: f32,
        reflectivity: f32,
        normal_map: Option<Texture>,
    ) -> MaterialId {
        let texture_id = scene.add_texture(Texture::solid(color)).unwrap();
        let mut material = Material::try_new(
            TextureSelection::Uniform(texture_id),
            Color::WHITE,
            specular,
            transparency,
            reflectivity,
        )
        .unwrap();
        if transparency > 0.0 {
            material = material.with_ior(GLASS_IOR).unwrap();
        }
        if let Some(normal_map) = normal_map {
            material = material.with_normal_map(scene.add_texture(normal_map).unwrap());
        }
        scene.add_material(material).unwrap()
    }

    fn mapped_cube_scene(
        color: Color,
        specular: f32,
        transparency: f32,
        reflectivity: f32,
        normal_map: Option<Texture>,
    ) -> Scene {
        let mut scene = Scene::new();
        let material = add_mapped_material(
            &mut scene,
            color,
            specular,
            transparency,
            reflectivity,
            normal_map,
        );
        scene.add(SceneObject::new(unit_cube(), material));
        scene
    }

    /// Lit only by a white directional light of intensity 1 from `direction_to_light`.
    fn sun_only(direction_to_light: Vec3) -> Lighting {
        directional(direction_to_light)
    }

    fn assert_f32_approx_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1.0e-5,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn cobblestone_with_a_flat_normal_map_shades_like_its_geometric_normal() {
        let (mut scene, materials) = canonical_scene();
        scene.add(SceneObject::new(unit_cube(), materials.cobblestone));
        let lighting = directional(Vec3::new(0.6, 1.0, 0.8));
        let cobblestone = scene.material(materials.cobblestone).unwrap();
        assert!(cobblestone.normal_map().is_some());

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
    fn materials_without_a_normal_map_shade_with_their_geometric_normal() {
        let lighting = directional(Vec3::new(0.6, 1.0, 0.8));
        let origin = Vec3::new(0.5, 2.0, 0.5);
        let texture_color = Color::new(0.6, 0.5, 0.4);

        let selectors: [fn(CanonicalMaterials) -> MaterialId; 4] =
            [|m| m.grass, |m| m.obsidian, |m| m.glass, |m| m.lava];
        for select in selectors {
            let (mut scene, materials) = canonical_scene();
            let material_id = select(materials);
            scene.add(SceneObject::new(unit_cube(), material_id));
            let material = scene.material(material_id).unwrap();
            assert_eq!(material.normal_map(), None);

            let ray = Ray::try_new(origin, Vec3::new(0.0, -1.0, 0.0)).unwrap();
            let hit = scene.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let mut expected = shade_surface(
                material.surface_color(texture_color),
                hit.geometry.normal,
                Vec3::new(0.0, 1.0, 0.0),
                material.specular(),
                true,
                lighting,
            );
            if material.is_emissive() {
                expected = expected + material.emitted_radiance(texture_color);
            }
            assert_color_approx_eq(shade_hit(&scene, hit, origin, lighting).unwrap(), expected);
        }
    }

    #[test]
    fn normal_map_changes_directional_lighting_to_the_shading_normal() {
        let lean = lean_u(30.0_f32.to_radians());
        let mapped = mapped_cube_scene(Color::WHITE, 0.0, 0.0, 0.0, Some(uniform_normal_map(lean)));
        let flat = mapped_cube_scene(Color::WHITE, 0.0, 0.0, 0.0, None);
        let lighting = sun_only(Vec3::new(1.0, 1.0, 0.0));

        let mapped_color = local_shading(&mapped, downward_ray(), lighting);
        let flat_color = local_shading(&flat, downward_ray(), lighting);

        // On the +Y face tangent-space +X is world +X: the normal becomes (0.5, 0.866, 0).
        let expected = shade_surface(
            Color::WHITE,
            Vec3::new(0.5, 0.75_f32.sqrt(), 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.0,
            true,
            lighting,
        );
        assert_color_approx_eq(mapped_color, expected);
        assert_ne!(mapped_color, flat_color);
        assert_f32_approx_eq(flat_color.r, 0.5_f32.sqrt());
        assert_f32_approx_eq(mapped_color.r, (0.5 + 0.75_f32.sqrt()) / 2.0_f32.sqrt());
    }

    #[test]
    fn diffuse_response_follows_the_normal_map_lean_monotonically() {
        let lighting = sun_only(Vec3::new(1.0, 1.0, 0.0));
        let brightness = |angle_degrees: f32| {
            let scene = mapped_cube_scene(
                Color::WHITE,
                0.0,
                0.0,
                0.0,
                Some(uniform_normal_map(lean_u(angle_degrees.to_radians()))),
            );
            local_shading(&scene, downward_ray(), lighting).r
        };

        // The light sits at +X, so leaning toward +U brightens and leaning away darkens, up to
        // the point where the normal faces the light directly (45 degrees).
        let samples = [-40.0, -20.0, 0.0, 20.0, 40.0].map(brightness);
        assert!(
            samples.windows(2).all(|pair| pair[0] < pair[1]),
            "{samples:?}"
        );
        assert_f32_approx_eq(brightness(0.0), 0.5_f32.sqrt());
        assert!(brightness(45.0) > 0.999);
    }

    #[test]
    fn specular_highlight_uses_the_shading_normal() {
        // Black surface isolates specular. Light at 45 degrees and viewer overhead put the
        // half-vector 22.5 degrees from vertical toward +X, where only a leaning normal points.
        let lighting = sun_only(Vec3::new(1.0, 1.0, 0.0));
        let half_angle = 22.5_f32.to_radians();
        let mapped = mapped_cube_scene(
            Color::BLACK,
            1.0,
            0.0,
            0.0,
            Some(uniform_normal_map(lean_u(half_angle))),
        );
        let flat = mapped_cube_scene(Color::BLACK, 1.0, 0.0, 0.0, None);

        let mapped_highlight = local_shading(&mapped, downward_ray(), lighting).r;
        let flat_highlight = local_shading(&flat, downward_ray(), lighting).r;

        assert!(mapped_highlight > 0.99, "{mapped_highlight}");
        assert_f32_approx_eq(flat_highlight, half_angle.cos().powf(32.0));
        assert!(flat_highlight < 0.1);
    }

    #[test]
    fn point_light_diffuse_and_specular_use_the_shading_normal() {
        let light = PointLight::try_new(Vec3::new(2.0, 2.0, 0.5), Color::WHITE, 1.0, 10.0).unwrap();
        let surface = Vec3::new(0.5, 1.0, 0.5);
        let up = Vec3::new(0.0, 1.0, 0.0);
        let lean = lean_u(30.0_f32.to_radians());
        let expected_normal = Vec3::new(0.5, 0.75_f32.sqrt(), 0.0);

        for specular in [0.0, 0.6] {
            let shade = |normal_map: Option<Texture>| {
                let mut scene =
                    mapped_cube_scene(Color::new(0.8, 0.8, 0.8), specular, 0.0, 0.0, normal_map);
                scene.add_point_light(light);
                local_shading(&scene, downward_ray(), darkness())
            };
            let incidence = light.incidence_at(surface, up).unwrap();

            assert_color_approx_eq(
                shade(Some(uniform_normal_map(lean))),
                shade_point_light(
                    incidence,
                    Color::new(0.8, 0.8, 0.8),
                    expected_normal,
                    up,
                    specular,
                ),
            );
            assert_ne!(shade(Some(uniform_normal_map(lean))), shade(None));
        }

        // Leaning toward the light at +X brightens the flat result; leaning away darkens it.
        let shade = |normal: Vec3| {
            let mut scene = mapped_cube_scene(
                Color::WHITE,
                0.0,
                0.0,
                0.0,
                Some(uniform_normal_map(normal)),
            );
            scene.add_point_light(light);
            local_shading(&scene, downward_ray(), darkness()).r
        };
        let flat = shade(Vec3::new(0.0, 0.0, 1.0));
        assert!(shade(lean) > flat);
        assert!(shade(lean_u(-30.0_f32.to_radians())) < flat);
    }

    #[test]
    fn a_bump_cannot_be_lit_through_a_face_turned_away_from_the_light() {
        // The sun is slightly below the top face's plane, so the geometric normal faces away.
        // A strong lean toward the sun makes the shading normal face it, but the light would be
        // arriving through the surface, so only ambient may remain.
        let lean = lean_u(53.0_f32.to_radians());
        let under_sun = Vec3::new(1.0, -0.1, 0.0);
        let lighting = Lighting::new(
            AmbientLight::try_new(Color::WHITE, 0.2).unwrap(),
            DirectionalLight::try_new(under_sun, Color::WHITE, 1.0).unwrap(),
        );
        let mapped = mapped_cube_scene(Color::WHITE, 1.0, 0.0, 0.0, Some(uniform_normal_map(lean)));
        let flat = mapped_cube_scene(Color::WHITE, 1.0, 0.0, 0.0, None);
        assert!(lean.x > 0.79);

        let mapped_color = local_shading(&mapped, downward_ray(), lighting);
        assert_color_approx_eq(mapped_color, Color::new(0.2, 0.2, 0.2));
        assert_eq!(mapped_color, local_shading(&flat, downward_ray(), lighting));

        let mut with_point_light = mapped;
        with_point_light.add_point_light(
            PointLight::try_new(Vec3::new(3.0, 0.9, 0.5), Color::WHITE, 5.0, 10.0).unwrap(),
        );
        assert_eq!(
            local_shading(&with_point_light, downward_ray(), darkness()),
            Color::BLACK
        );
    }

    #[test]
    fn shadows_remain_geometric_for_normal_mapped_surfaces() {
        let lean = lean_u(30.0_f32.to_radians());
        let lighting = Lighting::new(
            AmbientLight::try_new(Color::WHITE, 0.2).unwrap(),
            DirectionalLight::try_new(Vec3::new(0.0, 1.0, 0.0), Color::WHITE, 1.0).unwrap(),
        );
        let ray = Ray::try_new(Vec3::new(0.5, 1.5, 0.5), Vec3::new(0.0, -1.0, 0.0)).unwrap();
        let mut mapped =
            mapped_cube_scene(Color::WHITE, 0.5, 0.0, 0.0, Some(uniform_normal_map(lean)));

        let lit = local_shading(&mapped, ray, lighting);
        assert!(lit.r > 0.3);

        // A slab above the hit, behind the ray origin, blocks the sun: the bump is shadowed
        // exactly like the flat face, leaving only ambient.
        add_blocker(
            &mut mapped,
            Vec3::new(0.0, 1.6, 0.0),
            Vec3::new(1.0, 2.0, 1.0),
        );
        assert_color_approx_eq(
            local_shading(&mapped, ray, lighting),
            Color::new(0.2, 0.2, 0.2),
        );
    }

    #[test]
    fn invalid_normal_map_texels_fall_back_to_the_geometric_normal() {
        let lighting = sun_only(Vec3::new(1.0, 1.0, 0.0));
        let flat = mapped_cube_scene(Color::WHITE, 0.4, 0.0, 0.0, None);
        let expected = local_shading(&flat, downward_ray(), lighting);

        for corrupt in [
            Color::new(0.5, 0.5, 0.5),
            Color::new(0.5, 0.5, 0.0),
            Color::new(1.0, 0.5, 0.45),
            Color::new(f32::NAN, 0.5, 1.0),
        ] {
            let scene =
                mapped_cube_scene(Color::WHITE, 0.4, 0.0, 0.0, Some(Texture::solid(corrupt)));
            let color = local_shading(&scene, downward_ray(), lighting);

            assert!(color.is_finite());
            assert_eq!(color, expected, "{corrupt:?}");
        }
    }

    #[test]
    fn missing_normal_map_texture_is_a_render_error() {
        let mut scene = Scene::new();
        let texture = scene.add_texture(Texture::solid(Color::WHITE)).unwrap();
        let material = scene
            .add_material(
                Material::try_new(
                    TextureSelection::Uniform(texture),
                    Color::WHITE,
                    0.0,
                    0.0,
                    0.0,
                )
                .unwrap()
                .with_normal_map(TextureId::new(99)),
            )
            .unwrap();
        scene.add(SceneObject::new(unit_cube(), material));
        let hit = scene
            .closest_hit(downward_ray(), 0.0, f32::INFINITY)
            .unwrap();

        assert_eq!(
            shade_hit(&scene, hit, downward_ray().origin(), ambient_only()),
            Err(RenderError::TextureNotFound)
        );
    }

    #[test]
    fn normal_map_is_sampled_at_the_same_uv_as_the_color_texture() {
        // A 2x2 normal map with a different lean per texel. Each hit's expected normal comes from
        // sampling the map at that hit's own UV, which also selects the color texel.
        let leans = [-50.0_f32, 50.0, 0.0, 25.0].map(|degrees| lean_u(degrees.to_radians()));
        let normal_map = Texture::try_new(2, 2, leans.map(encode_normal).to_vec()).unwrap();
        let mapped = mapped_cube_scene(Color::WHITE, 0.0, 0.0, 0.0, Some(normal_map.clone()));
        let lighting = sun_only(Vec3::new(1.0, 1.0, 0.0));
        let mut distinct = Vec::new();

        for (x, z) in [(0.1, 0.1), (0.9, 0.1), (0.1, 0.9), (0.9, 0.9)] {
            let origin = Vec3::new(x, 2.0, z);
            let ray = Ray::try_new(origin, Vec3::new(0.0, -1.0, 0.0)).unwrap();
            let hit = mapped.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let uv = hit.geometry.uv.unwrap();
            let tangent_normal = crate::material::decode_tangent_normal(
                normal_map.sample_nearest(uv.u, uv.v).unwrap(),
            )
            .unwrap();
            let world_normal = Vec3::new(tangent_normal.x, tangent_normal.z, tangent_normal.y);

            let actual = shade_hit(&mapped, hit, origin, lighting).unwrap();
            assert_color_approx_eq(
                actual,
                shade_surface(
                    Color::WHITE,
                    world_normal,
                    Vec3::new(0.0, 1.0, 0.0),
                    0.0,
                    true,
                    lighting,
                ),
            );
            distinct.push(actual.r);
        }
        // The four corners land on four different texels.
        distinct.sort_by(f32::total_cmp);
        distinct.dedup_by(|a, b| (*a - *b).abs() < 1.0e-4);
        assert!(distinct.len() >= 3, "{distinct:?}");
    }

    #[test]
    fn normal_map_leans_follow_the_uv_basis_on_every_face() {
        let lean = 30.0_f32.to_radians();
        let (sine, cosine) = (lean.sin(), lean.cos());
        let base = 0.5_f32 * 2.0_f32.sqrt();
        let normal_map = |tangent_normal: Vec3| Some(uniform_normal_map(tangent_normal));
        let flat = mapped_cube_scene(Color::WHITE, 0.0, 0.0, 0.0, None);

        for face in ALL_FACES {
            let normal = face.normal();
            let center = Vec3::new(0.5, 0.5, 0.5);
            let ray = Ray::try_new(center + normal * 2.0, -normal).unwrap();
            // Tangent-space axis, tilted-normal, and the world axis it must point along.
            for (tilt, axis) in [
                (Vec3::new(sine, 0.0, cosine), face.tangent()),
                (Vec3::new(0.0, sine, cosine), face.bitangent()),
            ] {
                let mapped = mapped_cube_scene(Color::WHITE, 0.0, 0.0, 0.0, normal_map(tilt));
                for (sign, expected) in [
                    (1.0, (sine + cosine) * base),
                    (-1.0, (cosine - sine) * base),
                ] {
                    let lighting = sun_only(axis * sign + normal);
                    let hit = mapped.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
                    assert_eq!(hit.geometry.face, face);

                    let shaded = shade_hit(&mapped, hit, ray.origin(), lighting).unwrap();
                    let unmapped = local_shading(&flat, ray, lighting);
                    assert_f32_approx_eq(unmapped.r, base);
                    assert_f32_approx_eq(shaded.r, expected);
                    // Toward the tilt the face brightens; away it darkens.
                    assert_eq!(shaded.r > unmapped.r, sign > 0.0, "{face:?} {axis:?}");
                }
            }
        }
    }

    #[test]
    fn reflection_ignores_the_shading_normal() {
        // Fully reflective: the result is the reflected radiance alone, so a normal map that
        // visibly changes local shading must leave the pixel unchanged.
        let lean = uniform_normal_map(lean_u(40.0_f32.to_radians()));
        let (min, max) = REFLECTION_WITNESS;
        let build = |normal_map: Option<Texture>| {
            let mut scene = mapped_cube_scene(Color::new(0.2, 0.4, 0.6), 0.5, 0.0, 1.0, normal_map);
            let witness = add_reflective_material(&mut scene, Color::new(0.9, 0.3, 0.1), 0.0);
            scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), witness));
            scene
        };
        let (mapped, flat) = (build(Some(lean)), build(None));
        let lighting = upward_light_with_ambient();
        let environment = environment();

        assert_ne!(
            local_shading(&mapped, angled_top_ray(), lighting),
            local_shading(&flat, angled_top_ray(), lighting)
        );
        assert_color_approx_eq(
            Tracer::new(&mapped, lighting, &environment)
                .trace_ray(angled_top_ray(), PRIMARY_RAY_DEPTH)
                .unwrap(),
            Tracer::new(&flat, lighting, &environment)
                .trace_ray(angled_top_ray(), PRIMARY_RAY_DEPTH)
                .unwrap(),
        );
    }

    #[test]
    fn refraction_and_fresnel_ignore_the_shading_normal() {
        // Fully transparent: only the Fresnel composition of reflection and refraction remains.
        // Secondary hits on the same mapped material are locally lit with their own shading
        // normals, so the traced comparison uses ambient-only light, which is independent of the
        // normal: any difference would have to come from bent or reflected ray geometry.
        let lean = uniform_normal_map(lean_u(40.0_f32.to_radians()));
        let build = |normal_map: Option<Texture>| {
            let mut scene = mapped_cube_scene(Color::new(0.2, 0.4, 0.6), 0.5, 1.0, 0.0, normal_map);
            let (min, max) = TRANSMISSION_WITNESS;
            let witness = add_reflective_material(&mut scene, Color::new(0.9, 0.3, 0.1), 0.0);
            scene.add(SceneObject::new(Aabb::try_new(min, max).unwrap(), witness));
            scene
        };
        let (mapped, flat) = (build(Some(lean)), build(None));
        let lit = upward_light_with_ambient();
        let lighting = ambient_only();
        let environment = environment();

        assert_ne!(
            local_shading(&mapped, angled_top_ray(), lit),
            local_shading(&flat, angled_top_ray(), lit)
        );
        assert_color_approx_eq(
            Tracer::new(&mapped, lighting, &environment)
                .trace_ray(angled_top_ray(), PRIMARY_RAY_DEPTH)
                .unwrap(),
            Tracer::new(&flat, lighting, &environment)
                .trace_ray(angled_top_ray(), PRIMARY_RAY_DEPTH)
                .unwrap(),
        );
    }

    #[test]
    fn normal_mapping_does_not_move_geometry_or_identity() {
        let lean = uniform_normal_map(lean_u(40.0_f32.to_radians()));
        let mapped = mapped_cube_scene(Color::WHITE, 0.0, 0.0, 0.0, Some(lean));
        let flat = mapped_cube_scene(Color::WHITE, 0.0, 0.0, 0.0, None);

        for ray in [downward_ray(), angled_top_ray()] {
            let mapped_hit = mapped.closest_hit(ray, 0.0, f32::INFINITY).unwrap();
            let flat_hit = flat.closest_hit(ray, 0.0, f32::INFINITY).unwrap();

            assert_eq!(mapped_hit.geometry, flat_hit.geometry);
        }
    }
}
