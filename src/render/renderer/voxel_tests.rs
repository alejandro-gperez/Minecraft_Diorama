//! Renderer integration of hybrid voxel + AABB traversal.
//!
//! Voxel hits reach the renderer as ordinary `SceneHit`s, so these tests drive the real `Tracer`
//! and `Renderer` over voxel scenes and check that every material effect applies unchanged, and
//! that a voxel renders like the unit `Aabb` occupying the same cell.

use super::{
    MAX_RAY_DEPTH, PRIMARY_RAY_DEPTH, Renderer, Tracer, blend_reflection, blend_transmissive,
    compose_fresnel, optical_interface, reflection_ray, refraction_ray, shade_hit,
};
use crate::{
    camera::OrbitalCamera,
    environment::Environment,
    geometry::{Aabb, CubeFace},
    lighting::{AmbientLight, DirectionalLight, Lighting, PointLight},
    material::{
        AuxiliaryTextureIds, CanonicalTextureIds, GLASS_IOR, Material, MaterialId, Texture,
        TextureSelection,
    },
    math::Vec3,
    ray::Ray,
    render::{Color, Framebuffer},
    scene::{HitSource, Scene, SceneHit, SceneObject},
    voxel::{BlockMaterials, BlockType, Voxel, VoxelGrid, VoxelPosition},
};

const INF: f32 = f32::INFINITY;

// One distinct solid color per texture slot, so the sampled texture identifies itself.
const GRASS_TOP: Color = Color::new(0.20, 0.70, 0.15);
const GRASS_SIDE: Color = Color::new(0.45, 0.55, 0.20);
const DIRT: Color = Color::new(0.50, 0.35, 0.20);
const COBBLESTONE: Color = Color::new(0.55, 0.55, 0.55);
const OBSIDIAN: Color = Color::new(0.15, 0.10, 0.25);
const GLASS: Color = Color::new(0.70, 0.85, 0.90);
const LAVA: Color = Color::new(0.95, 0.45, 0.10);
const COAL_ORE: Color = Color::new(0.25, 0.25, 0.25);
const IRON_ORE: Color = Color::new(0.75, 0.60, 0.50);
const GOLD_ORE: Color = Color::new(0.90, 0.80, 0.30);
const DIAMOND_ORE: Color = Color::new(0.40, 0.85, 0.85);

/// Tangent-space normal leaning 0.5 rad toward `+u`, encoded as a texel.
fn leaning_normal_map() -> Texture {
    let angle: f32 = 0.5;
    Texture::solid(Color::new(
        angle.sin() * 0.5 + 0.5,
        0.5,
        angle.cos() * 0.5 + 0.5,
    ))
}

fn flat_normal_map() -> Texture {
    Texture::solid(Color::new(0.5, 0.5, 1.0))
}

/// The real canonical and auxiliary material definitions over distinct solid textures, with
/// every block type mapped. No geometry.
fn block_scene_with_normal_map(cobblestone_normal: Texture) -> (Scene, BlockMaterials) {
    let mut scene = Scene::new();
    let mut texture = |color: Color| scene.add_texture(Texture::solid(color)).unwrap();
    let (grass_top, grass_side, dirt) = (texture(GRASS_TOP), texture(GRASS_SIDE), texture(DIRT));
    let (cobblestone, obsidian) = (texture(COBBLESTONE), texture(OBSIDIAN));
    let (glass, lava) = (texture(GLASS), texture(LAVA));
    let (coal_ore, iron_ore) = (texture(COAL_ORE), texture(IRON_ORE));
    let (gold_ore, diamond_ore) = (texture(GOLD_ORE), texture(DIAMOND_ORE));
    let cobblestone_normal = scene.add_texture(cobblestone_normal).unwrap();

    let canonical = scene
        .add_canonical_materials(CanonicalTextureIds {
            grass_top,
            grass_side,
            dirt,
            cobblestone,
            cobblestone_normal,
            obsidian,
            glass,
            lava,
        })
        .unwrap();
    let auxiliary = scene
        .add_auxiliary_materials(AuxiliaryTextureIds {
            dirt,
            coal_ore,
            iron_ore,
            gold_ore,
            diamond_ore,
        })
        .unwrap();
    let block_materials = BlockMaterials::new(canonical, auxiliary);
    scene.set_block_materials(block_materials).unwrap();
    (scene, block_materials)
}

fn block_scene() -> (Scene, BlockMaterials) {
    block_scene_with_normal_map(leaning_normal_map())
}

fn at(x: i32, y: i32, z: i32) -> VoxelPosition {
    VoxelPosition::new(x, y, z)
}

/// Installs a 16³ grid centered on the world origin holding `blocks`.
fn set_voxels(scene: &mut Scene, blocks: &[(VoxelPosition, BlockType)]) {
    let mut grid = VoxelGrid::try_new(at(-8, -8, -8), 16, 16, 16).unwrap();
    for &(position, block) in blocks {
        grid.set_world(position, Voxel::Block(block)).unwrap();
    }
    scene.set_voxel_grid(grid).unwrap();
}

/// Adds the unit `Aabb` occupying `position`, as a naive per-block scene would.
fn add_unit_object(scene: &mut Scene, position: VoxelPosition, material_id: MaterialId) {
    let min = position.min_corner();
    scene.add(SceneObject::new(
        Aabb::try_new(min, min + Vec3::new(1.0, 1.0, 1.0)).unwrap(),
        material_id,
    ));
}

fn voxel_scene(blocks: &[(VoxelPosition, BlockType)]) -> Scene {
    let (mut scene, _) = block_scene();
    set_voxels(&mut scene, blocks);
    scene
}

/// The same blocks as unit `Aabb` objects, with no grid.
fn object_scene(blocks: &[(VoxelPosition, BlockType)]) -> Scene {
    let (mut scene, block_materials) = block_scene();
    for &(position, block) in blocks {
        add_unit_object(&mut scene, position, block_materials.material(block));
    }
    scene
}

fn ray(origin: Vec3, direction: Vec3) -> Ray {
    Ray::try_new(origin, direction).unwrap()
}

fn down_onto(x: f32, z: f32) -> Ray {
    ray(Vec3::new(x, 5.0, z), Vec3::new(0.0, -1.0, 0.0))
}

fn ambient_only() -> Lighting {
    Lighting::new(
        AmbientLight::try_new(Color::WHITE, 1.0).unwrap(),
        DirectionalLight::try_new(Vec3::new(0.0, 1.0, 0.0), Color::WHITE, 0.0).unwrap(),
    )
}

fn darkness() -> Lighting {
    Lighting::new(
        AmbientLight::try_new(Color::BLACK, 0.0).unwrap(),
        DirectionalLight::try_new(Vec3::new(0.0, 1.0, 0.0), Color::WHITE, 0.0).unwrap(),
    )
}

fn sun(direction_to_light: Vec3) -> Lighting {
    Lighting::new(
        AmbientLight::try_new(Color::BLACK, 0.0).unwrap(),
        DirectionalLight::try_new(direction_to_light, Color::WHITE, 1.0).unwrap(),
    )
}

/// The showcase's ambient and sun, so every lighting term is active.
fn full_lighting() -> Lighting {
    Lighting::new(
        AmbientLight::try_new(Color::new(0.30, 0.34, 0.46), 0.50).unwrap(),
        DirectionalLight::try_new(Vec3::new(0.6, 1.0, 0.8), Color::new(1.0, 0.84, 0.70), 1.0)
            .unwrap(),
    )
}

fn hit(scene: &Scene, ray: Ray) -> SceneHit {
    scene.closest_hit(ray, 0.0, INF).unwrap()
}

/// Local shading of the ray's closest hit, viewed back along the ray as `trace_ray` does.
fn local(scene: &Scene, ray: Ray, lighting: Lighting) -> Color {
    let hit = hit(scene, ray);
    shade_hit(
        scene,
        hit,
        hit.geometry.position - ray.direction(),
        lighting,
    )
    .unwrap()
}

fn trace(scene: &Scene, ray: Ray, lighting: Lighting, depth: u32) -> Color {
    let environment = Environment::sunset();
    Tracer::new(scene, lighting, &environment)
        .trace_ray(ray, depth)
        .unwrap()
}

fn assert_color_near(actual: Color, expected: Color, tolerance: f32) {
    for (a, e) in [
        (actual.r, expected.r),
        (actual.g, expected.g),
        (actual.b, expected.b),
    ] {
        assert!(
            (a - e).abs() <= tolerance,
            "{actual:?} != {expected:?} (±{tolerance})"
        );
    }
}

fn difference(a: Color, b: Color) -> f32 {
    (a.r - b.r)
        .abs()
        .max((a.g - b.g).abs())
        .max((a.b - b.b).abs())
}

fn surface_color(
    scene: &Scene,
    block_materials: BlockMaterials,
    block: BlockType,
    texel: Color,
) -> Color {
    scene
        .material(block_materials.material(block))
        .unwrap()
        .surface_color(texel)
}

fn voxel_source(position: VoxelPosition, block: BlockType) -> HitSource {
    HitSource::Voxel { position, block }
}

// ------------------------------------------------------------------ equivalence

#[test]
fn every_block_type_traces_like_the_unit_aabb_in_its_cell() {
    let position = at(0, 0, 0);
    let center = Vec3::new(0.5, 0.5, 0.5);
    let mut probes: Vec<Ray> = CubeFace::ALL
        .into_iter()
        .map(|face| {
            ray(
                center + face.normal() * 3.0 + Vec3::new(0.13, -0.07, 0.11),
                -face.normal(),
            )
        })
        .collect();
    probes.extend([
        ray(Vec3::new(-1.7, 2.6, -2.2), Vec3::new(0.8, -0.85, 1.0)),
        ray(Vec3::new(2.9, 1.9, -1.4), Vec3::new(-1.0, -0.5, 0.7)),
        ray(Vec3::new(0.3, -2.5, 2.8), Vec3::new(0.05, 1.0, -1.0)),
    ]);
    let point_light = PointLight::try_new(
        Vec3::new(1.8, 1.6, -0.9),
        Color::new(1.0, 0.45, 0.12),
        3.0,
        5.0,
    )
    .unwrap();

    for block in BlockType::ALL {
        let mut voxels = voxel_scene(&[(position, block)]);
        let mut objects = object_scene(&[(position, block)]);
        voxels.add_point_light(point_light);
        objects.add_point_light(point_light);

        for probe in &probes {
            assert_eq!(hit(&voxels, *probe).source, voxel_source(position, block));
            assert_eq!(hit(&objects, *probe).source, HitSource::Object);
            for depth in [PRIMARY_RAY_DEPTH, MAX_RAY_DEPTH] {
                let from_voxel = trace(&voxels, *probe, full_lighting(), depth);
                let from_object = trace(&objects, *probe, full_lighting(), depth);
                // Voxel hit positions are snapped onto the face plane, so secondary origins may
                // differ from the slab test's by rounding; colors agree to well below one 8-bit step.
                assert_color_near(from_voxel, from_object, 1.0e-5);
                assert!(from_voxel.is_finite());
            }
        }
    }
}

#[test]
fn voxel_terrain_renders_like_naive_per_block_aabbs() {
    let mut blocks = Vec::new();
    for z in -3..3 {
        for x in -3..3 {
            blocks.push((at(x, -2, z), BlockType::Dirt));
            let top = match (x + 3 * z).rem_euclid(7) {
                0 => BlockType::Lava,
                1 => BlockType::Cobblestone,
                2 => BlockType::GoldOre,
                _ => BlockType::Grass,
            };
            blocks.push((at(x, -1, z), top));
        }
    }
    // The glass floats clear of other blocks. Per-block AABBs give a ray leaving glass into a
    // touching block a tie between the glass exit face and the neighbor's entry face, settled by
    // insertion order; the DDA always reports the glass exit (see the test below).
    blocks.extend([
        (at(-2, 0, 0), BlockType::Obsidian),
        (at(-2, 1, 0), BlockType::Obsidian),
        (at(1, 1, -1), BlockType::Glass),
        (at(0, 0, 1), BlockType::Cobblestone),
    ]);
    let point_light = PointLight::try_new(
        Vec3::new(0.5, 0.6, -1.5),
        Color::new(1.0, 0.45, 0.12),
        3.0,
        5.0,
    )
    .unwrap();
    let mut voxels = voxel_scene(&blocks);
    let mut objects = object_scene(&blocks);
    voxels.add_point_light(point_light);
    objects.add_point_light(point_light);
    let environment = Environment::sunset();
    let (width, height) = (48, 27);
    let mut voxel_frame = Framebuffer::try_new(width, height).unwrap();
    let mut object_frame = Framebuffer::try_new(width, height).unwrap();

    let mut compared = 0;
    let mut mismatched = 0;
    for (yaw, pitch, radius) in [
        (3.7, 0.15, 8.0),
        (0.9, 0.7, 6.0),
        (2.2, -0.2, 4.0),
        (5.1, 1.2, 9.0),
    ] {
        let camera = OrbitalCamera::try_new(
            Vec3::new(0.0, -0.5, 0.0),
            yaw,
            pitch,
            radius,
            50.0_f32.to_radians(),
            width as f32 / height as f32,
        )
        .unwrap();
        Renderer::render(
            &camera,
            &voxels,
            &full_lighting(),
            &environment,
            &mut voxel_frame,
        )
        .unwrap();
        Renderer::render(
            &camera,
            &objects,
            &full_lighting(),
            &environment,
            &mut object_frame,
        )
        .unwrap();

        for (a, b) in voxel_frame.pixels().iter().zip(object_frame.pixels()) {
            assert!(a.is_finite());
            compared += 1;
            mismatched += usize::from(difference(*a, *b) > 1.0e-4);
        }
    }

    // Only rays meeting an exact block edge, where half-open voxel ownership and the closed slab
    // test legitimately differ, may disagree; none are expected for these generic poses.
    assert_eq!(compared, 4 * width * height);
    assert_eq!(mismatched, 0);
}

#[test]
fn ray_leaving_voxel_glass_into_a_touching_block_starts_inside_that_block() {
    // Known limitation, like touching glass voxels: every interface is glass -> air, so the
    // biased exit origin lands inside the block under the glass, which reports its far face.
    let glass = (at(0, 0, 0), BlockType::Glass);
    let ground = (at(0, -1, 0), BlockType::Grass);
    let scene = voxel_scene(&[glass, ground]);
    let incoming = ray(Vec3::new(0.2, 2.0, 0.5), Vec3::new(0.3, -1.0, 0.0));

    let inside = refraction_ray(incoming, hit(&scene, incoming), GLASS_IOR)
        .unwrap()
        .unwrap();
    let exit = hit(&scene, inside);
    assert_eq!(exit.source, voxel_source(glass.0, glass.1));
    assert_eq!(exit.geometry.face, CubeFace::NegativeY);

    let leaving = refraction_ray(inside, exit, GLASS_IOR).unwrap().unwrap();
    let beyond = hit(&scene, leaving);
    assert_eq!(beyond.source, voxel_source(ground.0, ground.1));
    assert!(beyond.geometry.normal.dot(leaving.direction()) > 0.0);
    assert!(trace(&scene, incoming, full_lighting(), 0).is_finite());
}

// ------------------------------------------------------------------ primary rays and textures

#[test]
fn renderer_primary_rays_see_voxel_grass_with_face_specific_textures() {
    let (mut scene, block_materials) = block_scene();
    let mut blocks = Vec::new();
    for z in -2..=2 {
        for x in -2..=2 {
            blocks.push((at(x, -1, z), BlockType::Grass));
        }
    }
    set_voxels(&mut scene, &blocks);
    let camera = OrbitalCamera::try_new(
        Vec3::new(0.5, 0.0, 0.5),
        0.0,
        1.2,
        4.0,
        30.0_f32.to_radians(),
        1.0,
    )
    .unwrap();
    let mut framebuffer = Framebuffer::try_new(3, 3).unwrap();
    Renderer::render(
        &camera,
        &scene,
        &ambient_only(),
        &Environment::sunset(),
        &mut framebuffer,
    )
    .unwrap();

    let center = camera.ray_for_viewport(0.5, 0.5).unwrap();
    let center_hit = hit(&scene, center);
    assert_eq!(
        center_hit.source,
        voxel_source(at(0, -1, 0), BlockType::Grass)
    );
    assert_eq!(center_hit.geometry.face, CubeFace::PositiveY);
    assert_eq!(center_hit.material_id, block_materials.grass);
    assert_eq!(
        framebuffer.pixel(1, 1),
        Some(trace(&scene, center, ambient_only(), PRIMARY_RAY_DEPTH))
    );
    assert_color_near(
        local(&scene, center, ambient_only()),
        surface_color(&scene, block_materials, BlockType::Grass, GRASS_TOP),
        1.0e-6,
    );

    // The exposed side and bottom of the slab select the side and bottom textures.
    let side = ray(Vec3::new(4.0, -0.5, 0.5), Vec3::new(-1.0, 0.0, 0.0));
    let bottom = ray(Vec3::new(0.5, -4.0, 0.5), Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(hit(&scene, side).geometry.face, CubeFace::PositiveX);
    assert_color_near(
        local(&scene, side, ambient_only()),
        surface_color(&scene, block_materials, BlockType::Grass, GRASS_SIDE),
        1.0e-6,
    );
    assert_color_near(
        local(&scene, bottom, ambient_only()),
        surface_color(&scene, block_materials, BlockType::Grass, DIRT),
        1.0e-6,
    );
}

#[test]
fn voxel_dirt_and_ores_resolve_their_auxiliary_materials_and_textures() {
    let (mut scene, block_materials) = block_scene();
    let row = [
        (BlockType::Dirt, DIRT),
        (BlockType::CoalOre, COAL_ORE),
        (BlockType::IronOre, IRON_ORE),
        (BlockType::GoldOre, GOLD_ORE),
        (BlockType::DiamondOre, DIAMOND_ORE),
    ];
    let blocks: Vec<_> = row
        .iter()
        .enumerate()
        .map(|(i, &(block, _))| (at(i as i32 * 2 - 4, 0, 0), block))
        .collect();
    set_voxels(&mut scene, &blocks);

    for (&(position, block), &(_, texel)) in blocks.iter().zip(&row) {
        let probe = down_onto(position.x as f32 + 0.5, 0.5);
        let found = hit(&scene, probe);
        assert_eq!(found.source, voxel_source(position, block));
        assert_eq!(found.material_id, block_materials.material(block));
        let material = scene.material(found.material_id).unwrap();
        assert_eq!(material.reflectivity(), 0.0);
        assert_color_near(
            local(&scene, probe, ambient_only()),
            surface_color(&scene, block_materials, block, texel),
            1.0e-6,
        );
        // No reflection or refraction: the traced color is the local shading.
        assert_eq!(
            trace(&scene, probe, full_lighting(), PRIMARY_RAY_DEPTH),
            local(&scene, probe, full_lighting())
        );
    }
}

#[test]
fn voxel_cobblestone_is_lit_through_its_normal_map() {
    let blocks = [(at(0, 0, 0), BlockType::Cobblestone)];
    let (mut mapped, _) = block_scene_with_normal_map(leaning_normal_map());
    let (mut flat, _) = block_scene_with_normal_map(flat_normal_map());
    set_voxels(&mut mapped, &blocks);
    set_voxels(&mut flat, &blocks);
    let probe = ray(Vec3::new(0.3, 4.0, 0.4), Vec3::new(0.1, -1.0, 0.05));
    let lighting = sun(Vec3::new(0.6, 1.0, 0.8));

    assert_eq!(
        hit(&mapped, probe).source,
        voxel_source(at(0, 0, 0), BlockType::Cobblestone)
    );
    // A leaning normal map changes the voxel's lighting; a flat one reproduces geometric shading.
    let leaning = local(&mapped, probe, lighting);
    let geometric = local(&flat, probe, lighting);
    assert!(
        difference(leaning, geometric) > 0.02,
        "{leaning:?} {geometric:?}"
    );
    // The geometric normal still owns the hit itself.
    assert_eq!(hit(&mapped, probe).geometry, hit(&flat, probe).geometry);
}

// ------------------------------------------------------------------ recursion

#[test]
fn voxel_obsidian_reflects_a_voxel_witness() {
    // `incoming` strikes the obsidian top at (0.5, 1, 0.5); its mirror ray climbs along
    // y = x + 0.5 and enters the lava witness through its -X face.
    let incoming = ray(Vec3::new(-0.5, 2.0, 0.5), Vec3::new(1.0, -1.0, 0.0));
    let mirror = (at(0, 0, 0), BlockType::Obsidian);
    let witness = (at(1, 1, 0), BlockType::Lava);
    let with_witness = voxel_scene(&[mirror, witness]);
    let without_witness = voxel_scene(&[mirror]);
    let lighting = full_lighting();

    let mirror_hit = hit(&with_witness, incoming);
    assert_eq!(mirror_hit.source, voxel_source(mirror.0, mirror.1));
    let reflected = reflection_ray(incoming, mirror_hit).unwrap();
    let reflected_hit = hit(&with_witness, reflected);
    assert_eq!(reflected_hit.source, voxel_source(witness.0, witness.1));
    assert_eq!(reflected_hit.geometry.face, CubeFace::NegativeX);

    let reflectivity = with_witness
        .material(mirror_hit.material_id)
        .unwrap()
        .reflectivity();
    let expected = blend_reflection(
        local(&with_witness, incoming, lighting),
        trace(&with_witness, reflected, lighting, PRIMARY_RAY_DEPTH + 1),
        reflectivity,
    );
    let traced = trace(&with_witness, incoming, lighting, PRIMARY_RAY_DEPTH);
    assert_eq!(traced, expected);
    assert!(difference(traced, trace(&without_witness, incoming, lighting, 0)) > 0.05);
}

#[test]
fn reflected_ray_from_an_aabb_mirror_hits_a_voxel() {
    let (mut scene, block_materials) = block_scene();
    let texture = scene
        .add_texture(Texture::solid(Color::new(0.2, 0.4, 0.6)))
        .unwrap();
    let mirror = scene
        .add_material(
            Material::try_new(
                TextureSelection::Uniform(texture),
                Color::WHITE,
                0.0,
                0.0,
                1.0,
            )
            .unwrap(),
        )
        .unwrap();
    add_unit_object(&mut scene, at(0, 0, 0), mirror);
    set_voxels(&mut scene, &[(at(1, 1, 0), BlockType::DiamondOre)]);
    let incoming = ray(Vec3::new(-0.5, 2.0, 0.5), Vec3::new(1.0, -1.0, 0.0));

    assert_eq!(hit(&scene, incoming).source, HitSource::Object);
    let reflected = reflection_ray(incoming, hit(&scene, incoming)).unwrap();
    assert_eq!(
        hit(&scene, reflected).source,
        voxel_source(at(1, 1, 0), BlockType::DiamondOre)
    );
    // Full reflectivity: the mirror shows exactly the voxel the reflected ray reaches.
    assert_eq!(
        trace(&scene, incoming, ambient_only(), 0),
        trace(&scene, reflected, ambient_only(), 1)
    );
    assert_color_near(
        trace(&scene, incoming, ambient_only(), 0),
        surface_color(&scene, block_materials, BlockType::DiamondOre, DIAMOND_ORE),
        1.0e-6,
    );
}

#[test]
fn refracted_ray_through_aabb_glass_hits_a_voxel_floor() {
    let (mut scene, block_materials) = block_scene();
    add_unit_object(&mut scene, at(0, 0, 0), block_materials.glass);
    let floor: Vec<_> = (-3..5).map(|x| (at(x, -2, 0), BlockType::Lava)).collect();
    let empty_scene = scene.clone();
    set_voxels(&mut scene, &floor);
    let incoming = ray(Vec3::new(0.2, 2.0, 0.5), Vec3::new(0.3, -1.0, 0.0));

    // Entry through the glass top, exit through its bottom, then down onto the voxel floor.
    let entry = hit(&scene, incoming);
    assert_eq!(entry.source, HitSource::Object);
    let inside = refraction_ray(incoming, entry, GLASS_IOR).unwrap().unwrap();
    let exit = hit(&scene, inside);
    assert_eq!(
        (exit.source, exit.geometry.face),
        (HitSource::Object, CubeFace::NegativeY)
    );
    let outside = refraction_ray(inside, exit, GLASS_IOR).unwrap().unwrap();
    let below = hit(&scene, outside);
    assert!(matches!(
        below.source,
        HitSource::Voxel {
            block: BlockType::Lava,
            ..
        }
    ));
    assert_eq!(below.geometry.face, CubeFace::PositiveY);

    let lighting = full_lighting();
    assert!(
        difference(
            trace(&scene, incoming, lighting, 0),
            trace(&empty_scene, incoming, lighting, 0)
        ) > 0.02
    );
}

#[test]
fn voxel_glass_composes_fresnel_reflection_and_refraction() {
    let glass = (at(0, 0, 0), BlockType::Glass);
    let floor: Vec<_> = (-3..5).map(|x| (at(x, -2, 0), BlockType::Lava)).collect();
    let mut blocks = vec![glass];
    blocks.extend(&floor);
    let scene = voxel_scene(&blocks);
    let lighting = full_lighting();
    let incoming = ray(Vec3::new(0.2, 2.0, 0.5), Vec3::new(0.3, -1.0, 0.0));

    let entry = hit(&scene, incoming);
    assert_eq!(entry.source, voxel_source(glass.0, glass.1));
    let material = scene.material(entry.material_id).unwrap();
    let transparency = material.transparency();
    assert!(transparency > 0.0);
    assert_eq!(material.ior(), GLASS_IOR);

    // The refracted ray starts just inside the glass voxel and reports its exit face.
    let inside = refraction_ray(incoming, entry, GLASS_IOR).unwrap().unwrap();
    let exit = hit(&scene, inside);
    assert_eq!(exit.source, voxel_source(glass.0, glass.1));
    assert_eq!(exit.geometry.face, CubeFace::NegativeY);
    assert!(exit.geometry.normal.dot(inside.direction()) > 0.0);

    // The tracer's composition, written out from its parts.
    let reflectance = optical_interface(incoming.direction(), entry.geometry.normal, GLASS_IOR)
        .fresnel_reflectance(incoming.direction())
        .unwrap();
    assert!(reflectance > 0.0 && reflectance < 1.0);
    let optical = compose_fresnel(
        trace(
            &scene,
            reflection_ray(incoming, entry).unwrap(),
            lighting,
            1,
        ),
        trace(&scene, inside, lighting, 1),
        reflectance,
    );
    let expected = blend_transmissive(local(&scene, incoming, lighting), optical, transparency);
    assert_eq!(trace(&scene, incoming, lighting, 0), expected);

    // What lies beyond the voxel glass shows through it.
    let without_floor = voxel_scene(&[glass]);
    assert!(difference(expected, trace(&without_floor, incoming, lighting, 0)) > 0.02);
}

// ------------------------------------------------------------------ emission

#[test]
fn voxel_lava_emits_with_no_light_and_creates_no_point_light() {
    let (mut scene, block_materials) = block_scene();
    set_voxels(&mut scene, &[(at(0, 0, 0), BlockType::Lava)]);
    let probe = down_onto(0.5, 0.5);
    let lava = scene.material(block_materials.lava).unwrap();

    assert!(lava.is_emissive());
    assert_eq!(hit(&scene, probe).material_id, block_materials.lava);
    assert_color_near(
        local(&scene, probe, darkness()),
        lava.emitted_radiance(LAVA),
        1.0e-6,
    );
    assert!(scene.point_lights().is_empty());
}

// ------------------------------------------------------------------ shadows

#[test]
fn a_voxel_casts_a_directional_shadow_without_touching_ambient_or_emission() {
    let floor = (at(0, 0, 0), BlockType::Lava);
    let blocker = (at(0, 3, 0), BlockType::Glass);
    let lit = voxel_scene(&[floor]);
    let shadowed = voxel_scene(&[floor, blocker]);
    let probe = ray(Vec3::new(0.5, 2.5, 0.5), Vec3::new(0.1, -1.0, 0.1));
    let up = sun(Vec3::new(0.0, 1.0, 0.0));
    let lava = lit.material(lit.block_materials().unwrap().lava).unwrap();
    let emission = lava.emitted_radiance(LAVA);

    assert_eq!(hit(&shadowed, probe).geometry, hit(&lit, probe).geometry);
    // Shadowed: only emission remains (no ambient in this lighting).
    assert_color_near(local(&shadowed, probe, up), emission, 1.0e-6);
    assert!(difference(local(&lit, probe, up), emission) > 0.1);
}

#[test]
fn a_voxel_between_surface_and_point_light_blocks_it_and_one_beyond_does_not() {
    let floor = (at(0, 0, 0), BlockType::Dirt);
    let light = PointLight::try_new(Vec3::new(0.5, 3.5, 0.5), Color::WHITE, 1.0, 6.0).unwrap();
    let probe = down_onto(0.5, 0.5);
    let scene_with = |extra: &[(VoxelPosition, BlockType)]| {
        let mut blocks = vec![floor];
        blocks.extend_from_slice(extra);
        let mut scene = voxel_scene(&blocks);
        scene.add_point_light(light);
        scene
    };
    // The probe starts above y = 5 only in `down_onto`; shade the floor hit found unobstructed.
    let open = scene_with(&[]);
    let floor_hit = hit(&open, probe);
    let shade = |scene: &Scene| {
        shade_hit(
            scene,
            floor_hit,
            floor_hit.geometry.position + Vec3::new(0.0, 1.0, 0.0),
            darkness(),
        )
        .unwrap()
    };

    let unobstructed = shade(&open);
    assert!(unobstructed.r > 0.05, "{unobstructed:?}");
    assert_eq!(
        shade(&scene_with(&[(at(0, 2, 0), BlockType::Cobblestone)])),
        Color::BLACK
    );
    assert_eq!(
        shade(&scene_with(&[(at(0, 2, 0), BlockType::Glass)])),
        Color::BLACK
    );
    assert_eq!(
        shade(&scene_with(&[(at(0, 4, 0), BlockType::Cobblestone)])),
        unobstructed
    );
}

#[test]
fn hybrid_recursion_stays_bounded_and_finite() {
    // Facing voxel mirrors around a glass voxel and an emissive voxel, with an AABB in between.
    let mut scene = voxel_scene(&[
        (at(-2, 0, 0), BlockType::Obsidian),
        (at(2, 0, 0), BlockType::Obsidian),
        (at(0, 0, 0), BlockType::Glass),
        (at(0, -1, 0), BlockType::Lava),
    ]);
    let glass = scene.block_materials().unwrap().glass;
    scene.add(SceneObject::new(
        Aabb::try_new(Vec3::new(-0.8, 1.2, 0.2), Vec3::new(-0.2, 1.6, 0.8)).unwrap(),
        glass,
    ));
    scene.add_point_light(
        PointLight::try_new(
            Vec3::new(0.5, 2.5, -0.5),
            Color::new(1.0, 0.45, 0.12),
            3.0,
            5.0,
        )
        .unwrap(),
    );

    for i in 0..48 {
        let angle = i as f32 * 0.29;
        let origin = Vec3::new(
            0.5 + 3.0 * angle.cos(),
            0.4 + (i % 4) as f32 * 0.4,
            0.5 + 3.0 * angle.sin(),
        );
        let target = Vec3::new(0.5, 0.5 + (i % 3) as f32 * 0.3 - 0.3, 0.5);
        let color = trace(&scene, ray(origin, target - origin), full_lighting(), 0);
        assert!(color.is_finite(), "{color:?}");
    }
}
