use std::{error::Error, fmt};

use crate::{
    geometry::{Aabb, SurfaceHit},
    lighting::PointLight,
    material::{
        AuxiliaryMaterials, AuxiliaryTextureIds, CanonicalMaterials, CanonicalTextureIds, Material,
        MaterialId, Texture, TextureId, TextureRegistry, auxiliary_material_definitions,
        canonical_material_definitions,
    },
    ray::Ray,
    voxel::{BlockMaterials, BlockType, VoxelGrid, VoxelHit, VoxelPosition},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneObject {
    pub bounds: Aabb,
    pub material_id: MaterialId,
}

impl SceneObject {
    pub const fn new(bounds: Aabb, material_id: MaterialId) -> Self {
        Self {
            bounds,
            material_id,
        }
    }
}

/// Which kind of scene geometry produced a hit.
///
/// Shading never needs this: everything it reads is in `SceneHit::geometry` and
/// `SceneHit::material_id`. It exists for traversal, debugging, and logic that must know the
/// exact voxel crossed, such as future connected-glass handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitSource {
    /// An arbitrary AABB `SceneObject`.
    Object,
    /// A face of an occupied voxel in the scene's grid.
    Voxel {
        position: VoxelPosition,
        block: BlockType,
    },
}

/// The renderer-facing result of a scene query, identical in shape for every geometry source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneHit {
    pub geometry: SurfaceHit,
    pub material_id: MaterialId,
    pub source: HitSource,
}

impl SceneHit {
    pub const fn object(geometry: SurfaceHit, material_id: MaterialId) -> Self {
        Self {
            geometry,
            material_id,
            source: HitSource::Object,
        }
    }
}

impl From<VoxelHit> for SceneHit {
    fn from(hit: VoxelHit) -> Self {
        Self {
            geometry: hit.surface,
            material_id: hit.material_id,
            source: HitSource::Voxel {
                position: hit.voxel,
                block: hit.block,
            },
        }
    }
}

/// Why the scene rejected voxel world data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoxelSceneError {
    /// The block type maps to a `MaterialId` that is not registered in this scene.
    UnregisteredBlockMaterial(BlockType),
    /// A voxel grid was supplied before the block-to-material mapping that resolves it.
    MissingBlockMaterials,
}

impl fmt::Display for VoxelSceneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnregisteredBlockMaterial(block) => {
                write!(formatter, "{block:?} maps to an unregistered material")
            }
            Self::MissingBlockMaterials => {
                formatter.write_str("a voxel grid requires block materials to be set first")
            }
        }
    }
}

impl Error for VoxelSceneError {}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    objects: Vec<SceneObject>,
    materials: Vec<Material>,
    textures: TextureRegistry,
    point_lights: Vec<PointLight>,
    /// Resolves every `BlockType` to a registered material; validated on insertion.
    block_materials: Option<BlockMaterials>,
    /// The single primary voxel world, if any. Present only together with `block_materials`.
    voxel_grid: Option<VoxelGrid>,
}

impl Scene {
    pub const fn new() -> Self {
        Self {
            objects: Vec::new(),
            materials: Vec::new(),
            textures: TextureRegistry::new(),
            point_lights: Vec::new(),
            block_materials: None,
            voxel_grid: None,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            objects: Vec::with_capacity(capacity),
            materials: Vec::new(),
            textures: TextureRegistry::new(),
            point_lights: Vec::new(),
            block_materials: None,
            voxel_grid: None,
        }
    }

    pub fn add_texture(&mut self, texture: Texture) -> Option<TextureId> {
        self.textures.insert(texture)
    }

    pub fn texture(&self, id: TextureId) -> Option<&Texture> {
        self.textures.get(id)
    }

    pub fn add_material(&mut self, material: Material) -> Option<MaterialId> {
        let index = u32::try_from(self.materials.len()).ok()?;
        let id = MaterialId::new(index);
        self.materials.push(material);
        Some(id)
    }

    /// Registers the canonical Phase 2 definitions in the scene's central material storage.
    pub fn add_canonical_materials(
        &mut self,
        textures: CanonicalTextureIds,
    ) -> Option<CanonicalMaterials> {
        let definitions = canonical_material_definitions(textures)?;
        let grass = self.add_material(definitions.grass)?;
        let cobblestone = self.add_material(definitions.cobblestone)?;
        let obsidian = self.add_material(definitions.obsidian)?;
        let glass = self.add_material(definitions.glass)?;
        let lava = self.add_material(definitions.lava)?;

        Some(CanonicalMaterials {
            grass,
            cobblestone,
            obsidian,
            glass,
            lava,
        })
    }

    /// Registers the auxiliary terrain materials (dirt and the ores) after the canonical set.
    pub fn add_auxiliary_materials(
        &mut self,
        textures: AuxiliaryTextureIds,
    ) -> Option<AuxiliaryMaterials> {
        let definitions = auxiliary_material_definitions(textures)?;
        let dirt = self.add_material(definitions.dirt)?;
        let coal_ore = self.add_material(definitions.coal_ore)?;
        let iron_ore = self.add_material(definitions.iron_ore)?;
        let gold_ore = self.add_material(definitions.gold_ore)?;
        let diamond_ore = self.add_material(definitions.diamond_ore)?;

        Some(AuxiliaryMaterials {
            dirt,
            coal_ore,
            iron_ore,
            gold_ore,
            diamond_ore,
        })
    }

    pub fn material(&self, id: MaterialId) -> Option<&Material> {
        self.materials.get(id.index())
    }

    /// Registers a local point light.
    ///
    /// Every shaded hit iterates every point light, so the set is meant to stay small: one or a
    /// few representative lights per emissive region, not one per emissive block.
    pub fn add_point_light(&mut self, light: PointLight) {
        self.point_lights.push(light);
    }

    pub fn point_lights(&self) -> &[PointLight] {
        &self.point_lights
    }

    /// Sets the mapping that resolves voxel block types to materials.
    ///
    /// Every block type must map to a material already registered here, so a voxel hit can never
    /// carry an unknown `MaterialId`. On error the scene is unchanged.
    pub fn set_block_materials(
        &mut self,
        block_materials: BlockMaterials,
    ) -> Result<(), VoxelSceneError> {
        if let Some(block) = BlockType::ALL
            .into_iter()
            .find(|block| self.material(block_materials.material(*block)).is_none())
        {
            return Err(VoxelSceneError::UnregisteredBlockMaterial(block));
        }

        self.block_materials = Some(block_materials);
        Ok(())
    }

    pub const fn block_materials(&self) -> Option<BlockMaterials> {
        self.block_materials
    }

    /// Installs the scene's single primary voxel grid, replacing any previous one.
    ///
    /// Requires `set_block_materials` first, so every stored block resolves to a material. The
    /// grid is owned logical occupancy only: `closest_hit` and `is_occluded` do not traverse it
    /// until 3D DDA exists, and no voxel is ever turned into a `SceneObject`.
    pub fn set_voxel_grid(&mut self, grid: VoxelGrid) -> Result<(), VoxelSceneError> {
        if self.block_materials.is_none() {
            return Err(VoxelSceneError::MissingBlockMaterials);
        }

        self.voxel_grid = Some(grid);
        Ok(())
    }

    pub const fn voxel_grid(&self) -> Option<&VoxelGrid> {
        self.voxel_grid.as_ref()
    }

    pub fn add(&mut self, object: SceneObject) {
        self.objects.push(object);
    }

    /// Nearest AABB object hit in the interval. The voxel grid is not traversed yet.
    pub fn closest_hit(&self, ray: Ray, t_min: f32, t_max: f32) -> Option<SceneHit> {
        let mut closest_t = t_max;
        let mut closest_hit: Option<SceneHit> = None;

        for object in &self.objects {
            if let Some(geometry) = object.bounds.intersect(ray, t_min, closest_t)
                && closest_hit
                    .as_ref()
                    .is_none_or(|current| geometry.t < current.geometry.t)
            {
                closest_t = geometry.t;
                closest_hit = Some(SceneHit::object(geometry, object.material_id));
            }
        }

        closest_hit
    }

    /// Returns as soon as any opaque scene AABB intersects the requested ray interval.
    ///
    /// Every AABB is an opaque blocker, regardless of material transparency: glass blocks both
    /// directional and point-light shadow rays.
    /// The voxel grid is not tested yet.
    pub fn is_occluded(&self, ray: Ray, t_min: f32, t_max: f32) -> bool {
        for object in &self.objects {
            if object.bounds.intersects(ray, t_min, t_max) {
                return true;
            }
        }

        false
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{HitSource, Scene, SceneHit, SceneObject, VoxelSceneError};
    use crate::{
        color::Color,
        geometry::{Aabb, CubeFace, Uv},
        lighting::PointLight,
        material::{Material, MaterialId, Texture, TextureId, TextureSelection},
        math::Vec3,
        ray::Ray,
        voxel::{BlockMaterials, BlockType, Voxel, VoxelGrid, VoxelHit, VoxelPosition},
    };

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray::try_new(origin, direction).unwrap()
    }

    fn material(color: Color) -> Material {
        Material::try_new(
            TextureSelection::Uniform(TextureId::new(0)),
            color,
            0.0,
            0.0,
            0.0,
        )
        .unwrap()
    }

    fn object(min: Vec3, max: Vec3, material_id: MaterialId) -> SceneObject {
        SceneObject::new(Aabb::try_new(min, max).unwrap(), material_id)
    }

    #[test]
    fn empty_scene_misses() {
        assert_eq!(
            Scene::new().closest_hit(
                ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            ),
            None
        );
    }

    #[test]
    fn one_object_can_be_hit_or_missed() {
        let red = Color::new(1.0, 0.0, 0.0);
        let mut scene = Scene::new();
        let red_id = scene.add_material(material(red)).unwrap();
        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), red_id));

        let hit = scene
            .closest_hit(
                ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();
        assert_eq!(hit.material_id, red_id);
        assert_eq!(hit.geometry.face, CubeFace::PositiveZ);
        assert_eq!(hit.geometry.uv, Some(Uv::new(0.5, 0.5)));
        assert_eq!(
            scene.closest_hit(
                ray(Vec3::new(2.0, 2.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            ),
            None
        );
    }

    #[test]
    fn nearest_object_wins_regardless_of_insertion_order() {
        let near_color = Color::new(1.0, 0.0, 0.0);
        let far_color = Color::new(0.0, 0.0, 1.0);
        let ray = ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));

        let mut near_first = Scene::new();
        let near_id = near_first.add_material(material(near_color)).unwrap();
        let far_id = near_first.add_material(material(far_color)).unwrap();
        let near = object(Vec3::new(0.0, 0.0, 2.0), Vec3::new(1.0, 1.0, 3.0), near_id);
        let far = object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), far_id);
        near_first.add(near);
        near_first.add(far);

        let mut far_first = Scene::new();
        let near_id = far_first.add_material(material(near_color)).unwrap();
        let far_id = far_first.add_material(material(far_color)).unwrap();
        let near = object(Vec3::new(0.0, 0.0, 2.0), Vec3::new(1.0, 1.0, 3.0), near_id);
        let far = object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), far_id);
        far_first.add(far);
        far_first.add(near);

        assert_eq!(
            near_first
                .closest_hit(ray, 0.0, f32::INFINITY)
                .unwrap()
                .material_id,
            near_id
        );
        assert_eq!(
            far_first
                .closest_hit(ray, 0.0, f32::INFINITY)
                .unwrap()
                .material_id,
            near_id
        );
    }

    #[test]
    fn closest_hit_respects_ray_interval() {
        let near_color = Color::new(1.0, 0.0, 0.0);
        let far_color = Color::new(0.0, 0.0, 1.0);
        let mut scene = Scene::new();
        let near_id = scene.add_material(material(near_color)).unwrap();
        let far_id = scene.add_material(material(far_color)).unwrap();
        scene.add(object(
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 1.0, 3.0),
            near_id,
        ));
        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), far_id));
        let ray = ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));

        assert_eq!(
            scene.closest_hit(ray, 3.5, 5.0).unwrap().material_id,
            far_id
        );
        assert_eq!(scene.closest_hit(ray, 0.0, 1.5), None);
    }

    #[test]
    fn tracks_scene_length_and_empty_state() {
        let mut scene = Scene::new();
        assert!(scene.is_empty());

        let material_id = scene.add_material(material(Color::WHITE)).unwrap();
        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), material_id));

        assert_eq!(scene.len(), 1);
        assert!(!scene.is_empty());
    }

    #[test]
    fn material_insertion_returns_stable_ids_and_safe_lookup() {
        let red = material(Color::new(1.0, 0.0, 0.0));
        let blue = material(Color::new(0.0, 0.0, 1.0));
        let mut scene = Scene::new();

        let red_id = scene.add_material(red.clone()).unwrap();
        let blue_id = scene.add_material(blue.clone()).unwrap();

        assert_eq!(red_id, MaterialId::new(0));
        assert_eq!(blue_id, MaterialId::new(1));
        assert_eq!(scene.material(red_id), Some(&red));
        assert_eq!(scene.material(blue_id), Some(&blue));
        assert_eq!(scene.material(MaterialId::new(u32::MAX)), None);
    }

    #[test]
    fn multiple_objects_share_one_centrally_owned_material() {
        let mut scene = Scene::new();
        let texture_id = scene.add_texture(Texture::solid(Color::WHITE)).unwrap();
        let shared_material = Material::try_new(
            TextureSelection::Uniform(texture_id),
            Color::WHITE,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();
        let material_id = scene.add_material(shared_material).unwrap();

        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), material_id));
        scene.add(object(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 1.0, 1.0),
            material_id,
        ));

        assert_eq!(scene.len(), 2);
        assert_eq!(scene.material_count(), 1);
        assert_eq!(scene.texture_count(), 1);
        assert_eq!(scene.texture(texture_id).unwrap().width(), 1);
        assert_eq!(
            scene.material(material_id).unwrap().textures(),
            TextureSelection::Uniform(texture_id)
        );
    }

    #[test]
    fn multiple_materials_can_reference_one_registered_texture() {
        let mut scene = Scene::new();
        let texture_id = scene
            .add_texture(Texture::solid(Color::new(0.4, 0.5, 0.6)))
            .unwrap();
        let first = Material::try_new(
            TextureSelection::Uniform(texture_id),
            Color::WHITE,
            0.0,
            0.0,
            0.0,
        )
        .unwrap();
        let second = Material::try_new(
            TextureSelection::Uniform(texture_id),
            Color::new(0.5, 0.5, 0.5),
            0.3,
            0.0,
            0.1,
        )
        .unwrap();

        let first_id = scene.add_material(first).unwrap();
        let second_id = scene.add_material(second).unwrap();

        assert_eq!(scene.texture_count(), 1);
        assert_eq!(
            scene.material(first_id).unwrap().textures(),
            TextureSelection::Uniform(texture_id)
        );
        assert_eq!(
            scene.material(second_id).unwrap().textures(),
            TextureSelection::Uniform(texture_id)
        );
    }

    #[test]
    fn empty_scene_is_not_occluded() {
        assert!(!Scene::new().is_occluded(
            ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)),
            0.0,
            f32::INFINITY,
        ));
    }

    #[test]
    fn occlusion_detects_blocker_directly_along_ray() {
        let mut scene = Scene::new();
        scene.add(object(
            Vec3::new(2.0, -0.5, -0.5),
            Vec3::new(3.0, 0.5, 0.5),
            MaterialId::new(0),
        ));

        assert!(scene.is_occluded(
            ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)),
            0.0,
            f32::INFINITY,
        ));
    }

    #[test]
    fn objects_behind_origin_or_outside_path_do_not_occlude() {
        let mut scene = Scene::new();
        scene.add(object(
            Vec3::new(-3.0, -0.5, -0.5),
            Vec3::new(-2.0, 0.5, 0.5),
            MaterialId::new(0),
        ));
        scene.add(object(
            Vec3::new(2.0, 2.0, -0.5),
            Vec3::new(3.0, 3.0, 0.5),
            MaterialId::new(0),
        ));

        assert!(!scene.is_occluded(
            ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)),
            0.0,
            f32::INFINITY,
        ));
    }

    #[test]
    fn occlusion_respects_minimum_and_maximum_distance() {
        let mut scene = Scene::new();
        scene.add(object(
            Vec3::new(2.0, -0.5, -0.5),
            Vec3::new(3.0, 0.5, 0.5),
            MaterialId::new(0),
        ));
        let ray = ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));

        assert!(!scene.is_occluded(ray, 0.0, 1.99));
        assert!(scene.is_occluded(ray, 2.0, 2.5));
        assert!(!scene.is_occluded(ray, 3.01, f32::INFINITY));
    }

    #[test]
    fn occlusion_uses_any_hit_instead_of_closest_hit_semantics() {
        let mut scene = Scene::new();
        scene.add(object(
            Vec3::new(5.0, -0.5, -0.5),
            Vec3::new(6.0, 0.5, 0.5),
            MaterialId::new(0),
        ));
        scene.add(object(
            Vec3::new(2.0, -0.5, -0.5),
            Vec3::new(3.0, 0.5, 0.5),
            MaterialId::new(0),
        ));

        assert!(scene.is_occluded(
            ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)),
            0.0,
            f32::INFINITY,
        ));
    }

    #[test]
    fn transparent_materials_still_block_phase_two_shadow_rays() {
        let mut scene = Scene::new();
        let transparent = Material::try_new(
            TextureSelection::Uniform(TextureId::new(0)),
            Color::WHITE,
            0.0,
            1.0,
            0.0,
        )
        .unwrap();
        let transparent_id = scene.add_material(transparent).unwrap();
        scene.add(object(
            Vec3::new(2.0, -0.5, -0.5),
            Vec3::new(3.0, 0.5, 0.5),
            transparent_id,
        ));

        assert!(scene.is_occluded(
            ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)),
            0.0,
            f32::INFINITY,
        ));
    }

    #[test]
    fn scene_starts_without_point_lights_and_retains_added_ones_in_order() {
        let mut scene = Scene::new();
        assert!(scene.point_lights().is_empty());

        let first = PointLight::try_new(Vec3::ZERO, Color::WHITE, 1.0, 2.0).unwrap();
        let second = PointLight::try_new(
            Vec3::new(1.0, 2.0, 3.0),
            Color::new(1.0, 0.5, 0.2),
            0.5,
            3.0,
        )
        .unwrap();
        scene.add_point_light(first);
        scene.add_point_light(second);

        assert_eq!(scene.point_lights(), &[first, second]);
    }

    #[test]
    fn point_lights_are_not_scene_geometry() {
        let mut scene = Scene::new();
        scene.add_point_light(PointLight::try_new(Vec3::ZERO, Color::WHITE, 1.0, 2.0).unwrap());

        assert!(scene.is_empty());
        assert_eq!(
            scene.closest_hit(
                ray(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            ),
            None
        );
        assert!(!scene.is_occluded(
            ray(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
            0.0,
            f32::INFINITY,
        ));
    }

    /// Registers ten materials and maps the block types to them in declaration order.
    fn scene_with_block_materials() -> (Scene, BlockMaterials) {
        let mut scene = Scene::new();
        let mut next = || scene.add_material(material(Color::WHITE)).unwrap();
        let block_materials = BlockMaterials {
            grass: next(),
            dirt: next(),
            cobblestone: next(),
            obsidian: next(),
            glass: next(),
            lava: next(),
            coal_ore: next(),
            iron_ore: next(),
            gold_ore: next(),
            diamond_ore: next(),
        };
        scene.set_block_materials(block_materials).unwrap();
        (scene, block_materials)
    }

    fn grid_with_block_at(position: VoxelPosition, block: BlockType) -> VoxelGrid {
        let mut grid = VoxelGrid::try_new(VoxelPosition::new(-4, -4, -4), 8, 8, 8).unwrap();
        grid.set_world(position, Voxel::Block(block)).unwrap();
        grid
    }

    #[test]
    fn scenes_start_without_voxel_data() {
        let scene = Scene::new();

        assert_eq!(scene.voxel_grid(), None);
        assert_eq!(scene.block_materials(), None);
        assert_eq!(Scene::with_capacity(4).voxel_grid(), None);
    }

    #[test]
    fn aabb_hits_report_an_object_source() {
        let mut scene = Scene::new();
        let id = scene.add_material(material(Color::WHITE)).unwrap();
        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), id));

        let hit = scene
            .closest_hit(
                ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();

        assert_eq!(hit.source, HitSource::Object);
        assert_eq!(hit, SceneHit::object(hit.geometry, id));
    }

    #[test]
    fn voxel_and_aabb_hits_expose_identical_renderer_fields() {
        let (mut scene, block_materials) = scene_with_block_materials();
        let voxel = VoxelPosition::new(-3, 5, 11);
        let min = voxel.min_corner();
        let block = BlockType::IronOre;
        scene.add(SceneObject::new(
            Aabb::try_new(min, min + Vec3::new(1.0, 1.0, 1.0)).unwrap(),
            block_materials.material(block),
        ));
        let probe = ray(Vec3::new(-2.7, 5.4, 15.0), Vec3::new(0.05, 0.05, -1.0));

        let from_object = scene.closest_hit(probe, 0.0, f32::INFINITY).unwrap();
        let geometry = from_object.geometry;
        let from_voxel = SceneHit::from(
            VoxelHit::try_new(
                voxel,
                geometry.face,
                geometry.position,
                geometry.t,
                block,
                &block_materials,
            )
            .unwrap(),
        );

        assert_eq!(from_voxel.geometry, from_object.geometry);
        assert_eq!(from_voxel.material_id, from_object.material_id);
        assert_eq!(from_object.source, HitSource::Object);
        assert_eq!(
            from_voxel.source,
            HitSource::Voxel {
                position: voxel,
                block
            }
        );
        assert_eq!(from_voxel.geometry.face, CubeFace::PositiveZ);
        assert!(from_voxel.geometry.uv.is_some());
    }

    #[test]
    fn block_materials_must_reference_registered_materials() {
        let mut scene = Scene::new();
        for _ in 0..9 {
            scene.add_material(material(Color::WHITE)).unwrap();
        }
        let ids = BlockMaterials {
            grass: MaterialId::new(0),
            dirt: MaterialId::new(1),
            cobblestone: MaterialId::new(2),
            obsidian: MaterialId::new(3),
            glass: MaterialId::new(4),
            lava: MaterialId::new(5),
            coal_ore: MaterialId::new(6),
            iron_ore: MaterialId::new(7),
            gold_ore: MaterialId::new(8),
            diamond_ore: MaterialId::new(9),
        };

        assert_eq!(
            scene.set_block_materials(ids),
            Err(VoxelSceneError::UnregisteredBlockMaterial(
                BlockType::DiamondOre
            ))
        );
        assert_eq!(scene.block_materials(), None);

        scene.add_material(material(Color::WHITE)).unwrap();
        assert_eq!(scene.set_block_materials(ids), Ok(()));
        assert_eq!(scene.block_materials(), Some(ids));
    }

    #[test]
    fn voxel_grid_requires_block_materials() {
        let grid = VoxelGrid::try_new(VoxelPosition::new(0, 0, 0), 2, 2, 2).unwrap();
        let mut scene = Scene::new();

        assert_eq!(
            scene.set_voxel_grid(grid.clone()),
            Err(VoxelSceneError::MissingBlockMaterials)
        );
        assert_eq!(scene.voxel_grid(), None);
    }

    #[test]
    fn scene_stores_an_empty_voxel_grid() {
        let (mut scene, _) = scene_with_block_materials();
        let grid = VoxelGrid::try_new(VoxelPosition::new(-8, -4, -8), 16, 8, 16).unwrap();

        scene.set_voxel_grid(grid.clone()).unwrap();

        assert_eq!(scene.voxel_grid(), Some(&grid));
        assert!(scene.is_empty());
        assert_eq!(
            scene.closest_hit(
                ray(Vec3::new(0.5, 0.5, 20.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            ),
            None
        );
    }

    #[test]
    fn occupied_voxels_do_not_change_aabb_queries_yet() {
        let (mut scene, block_materials) = scene_with_block_materials();
        // An AABB behind an occupied voxel along the same ray.
        scene.add(object(
            Vec3::new(0.0, 0.0, -3.0),
            Vec3::new(1.0, 1.0, -2.0),
            block_materials.lava,
        ));
        let probe = ray(Vec3::new(0.5, 0.5, 3.5), Vec3::new(0.0, 0.0, -1.0));
        let before_hit = scene.closest_hit(probe, 0.0, f32::INFINITY);
        let before_occluded = scene.is_occluded(probe, 0.0, 4.0);

        scene
            .set_voxel_grid(grid_with_block_at(
                VoxelPosition::new(0, 0, 0),
                BlockType::Cobblestone,
            ))
            .unwrap();

        assert_eq!(scene.closest_hit(probe, 0.0, f32::INFINITY), before_hit);
        assert_eq!(
            before_hit.unwrap().geometry.position,
            Vec3::new(0.5, 0.5, -2.0)
        );
        assert_eq!(before_hit.unwrap().source, HitSource::Object);
        assert_eq!(scene.is_occluded(probe, 0.0, 4.0), before_occluded);
        assert!(!scene.is_occluded(probe, 0.0, 4.0));
        assert_eq!(scene.len(), 1);
    }

    #[test]
    fn scene_hit_is_small_plain_copy_data() {
        fn copy_value<T: Copy>() {}
        copy_value::<SceneHit>();
        copy_value::<HitSource>();
        assert!(size_of::<SceneHit>() <= 64, "{}", size_of::<SceneHit>());
    }
}
