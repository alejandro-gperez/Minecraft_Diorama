use crate::color::Color;

use super::{Material, MaterialId, TextureId, TextureSelection};

/// Index of refraction of canonical glass, approximately that of ordinary soda-lime glass.
///
/// The other canonical materials do not transmit light and keep the neutral default `AIR_IOR`.
pub const GLASS_IOR: f32 = 1.5;

/// Emission color of canonical lava.
///
/// The lava texture already carries the orange-to-yellow color; emission modulates the texture
/// (see `Material::emitted_radiance`), so this tint only nudges it slightly warmer and leaves most
/// of the painted detail to the texture.
pub const LAVA_EMISSION_COLOR: Color = Color::new(1.00, 0.88, 0.72);
/// Emission strength of canonical lava.
///
/// Above `1` on purpose: the average lava texel (about `0.85, 0.41, 0.10`) then emits roughly
/// `1.3, 0.5, 0.1`, so the bright texels saturate while the darker crust stays readable.
pub const LAVA_EMISSION_STRENGTH: f32 = 1.5;

/// Texture identities required by the five Phase 2 rubric materials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalTextureIds {
    pub grass_top: TextureId,
    pub grass_side: TextureId,
    pub dirt: TextureId,
    pub cobblestone: TextureId,
    /// Tangent-space normal map derived from the cobblestone texture.
    pub cobblestone_normal: TextureId,
    pub obsidian: TextureId,
    pub glass: TextureId,
    pub lava: TextureId,
}

/// Material definitions for the five canonical Phase 2 materials.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalMaterialDefinitions {
    pub grass: Material,
    pub cobblestone: Material,
    pub obsidian: Material,
    pub glass: Material,
    pub lava: Material,
}

/// Stable runtime identities for the five canonical materials after scene registration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalMaterials {
    pub grass: MaterialId,
    pub cobblestone: MaterialId,
    pub obsidian: MaterialId,
    pub glass: MaterialId,
    pub lava: MaterialId,
}

/// Builds the canonical Phase 2 definitions without owning or duplicating textures.
pub fn canonical_material_definitions(
    textures: CanonicalTextureIds,
) -> Option<CanonicalMaterialDefinitions> {
    Some(CanonicalMaterialDefinitions {
        grass: Material::try_new(
            TextureSelection::TopSideBottom {
                top: textures.grass_top,
                side: textures.grass_side,
                bottom: textures.dirt,
            },
            Color::WHITE,
            0.05,
            0.0,
            0.02,
        )?,
        cobblestone: Material::try_new(
            TextureSelection::Uniform(textures.cobblestone),
            Color::WHITE,
            0.08,
            0.0,
            0.03,
        )?
        .with_normal_map(textures.cobblestone_normal),
        obsidian: Material::try_new(
            TextureSelection::Uniform(textures.obsidian),
            Color::new(0.90, 0.90, 1.00),
            0.55,
            0.0,
            0.35,
        )?,
        glass: Material::try_new(
            TextureSelection::Uniform(textures.glass),
            Color::new(0.90, 0.97, 1.00),
            0.80,
            0.85,
            0.15,
        )?
        .with_ior(GLASS_IOR)?,
        lava: Material::try_new(
            TextureSelection::Uniform(textures.lava),
            Color::new(1.00, 0.95, 0.90),
            0.10,
            0.0,
            0.05,
        )?
        .with_emission(LAVA_EMISSION_COLOR, LAVA_EMISSION_STRENGTH)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CanonicalMaterials, CanonicalTextureIds, GLASS_IOR, LAVA_EMISSION_COLOR,
        LAVA_EMISSION_STRENGTH,
    };
    use crate::{
        color::Color,
        geometry::CubeFace,
        material::{AIR_IOR, Material, Texture, TextureId, TextureSelection},
        scene::Scene,
    };

    fn register_test_textures(scene: &mut Scene) -> CanonicalTextureIds {
        let mut next_texture = || scene.add_texture(Texture::solid(Color::WHITE)).unwrap();

        CanonicalTextureIds {
            grass_top: next_texture(),
            grass_side: next_texture(),
            dirt: next_texture(),
            cobblestone: next_texture(),
            cobblestone_normal: next_texture(),
            obsidian: next_texture(),
            glass: next_texture(),
            lava: next_texture(),
        }
    }

    fn registered_scene() -> (Scene, CanonicalTextureIds, CanonicalMaterials) {
        let mut scene = Scene::new();
        let textures = register_test_textures(&mut scene);
        let materials = scene.add_canonical_materials(textures).unwrap();
        (scene, textures, materials)
    }

    fn assert_properties(
        material: &Material,
        albedo: Color,
        specular: f32,
        transparency: f32,
        reflectivity: f32,
        ior: f32,
    ) {
        assert_eq!(material.albedo(), albedo);
        assert_eq!(material.specular(), specular);
        assert_eq!(material.transparency(), transparency);
        assert_eq!(material.reflectivity(), reflectivity);
        assert_eq!(material.ior(), ior);
    }

    fn assert_uniform(selection: TextureSelection, expected: TextureId) {
        for face in [
            CubeFace::NegativeX,
            CubeFace::PositiveX,
            CubeFace::NegativeY,
            CubeFace::PositiveY,
            CubeFace::NegativeZ,
            CubeFace::PositiveZ,
        ] {
            assert_eq!(selection.for_face(face), expected);
        }
    }

    #[test]
    fn registers_five_distinct_materials_without_duplicating_textures() {
        let (scene, _, materials) = registered_scene();
        let ids = [
            materials.grass,
            materials.cobblestone,
            materials.obsidian,
            materials.glass,
            materials.lava,
        ];

        assert_eq!(scene.material_count(), 5);
        assert_eq!(scene.texture_count(), 8);
        for (index, id) in ids.iter().enumerate() {
            assert!(scene.material(*id).is_some());
            assert!(!ids[..index].contains(id));
        }
    }

    #[test]
    fn canonical_materials_have_expected_optical_parameters() {
        let (scene, _, materials) = registered_scene();

        assert_properties(
            scene.material(materials.grass).unwrap(),
            Color::WHITE,
            0.05,
            0.0,
            0.02,
            AIR_IOR,
        );
        assert_properties(
            scene.material(materials.cobblestone).unwrap(),
            Color::WHITE,
            0.08,
            0.0,
            0.03,
            AIR_IOR,
        );
        assert_properties(
            scene.material(materials.obsidian).unwrap(),
            Color::new(0.90, 0.90, 1.00),
            0.55,
            0.0,
            0.35,
            AIR_IOR,
        );
        assert_properties(
            scene.material(materials.glass).unwrap(),
            Color::new(0.90, 0.97, 1.00),
            0.80,
            0.85,
            0.15,
            GLASS_IOR,
        );
        assert_properties(
            scene.material(materials.lava).unwrap(),
            Color::new(1.00, 0.95, 0.90),
            0.10,
            0.0,
            0.05,
            AIR_IOR,
        );
    }

    #[test]
    fn canonical_lava_is_the_only_emissive_material() {
        let (scene, _, materials) = registered_scene();

        let lava = scene.material(materials.lava).unwrap();
        assert!(lava.is_emissive());
        assert_eq!(lava.emission_color(), LAVA_EMISSION_COLOR);
        assert_eq!(lava.emission_strength(), LAVA_EMISSION_STRENGTH);
        assert!(LAVA_EMISSION_STRENGTH > 1.0);

        for id in [
            materials.grass,
            materials.cobblestone,
            materials.obsidian,
            materials.glass,
        ] {
            let material = scene.material(id).unwrap();
            assert!(!material.is_emissive());
            assert_eq!(material.emission_strength(), 0.0);
            assert_eq!(material.emission_color(), Color::BLACK);
        }
    }

    #[test]
    fn only_cobblestone_carries_a_shared_normal_map() {
        let (scene, textures, materials) = registered_scene();

        let cobblestone = scene.material(materials.cobblestone).unwrap();
        assert_eq!(cobblestone.normal_map(), Some(textures.cobblestone_normal));
        assert_ne!(textures.cobblestone_normal, textures.cobblestone);
        for id in [
            materials.grass,
            materials.obsidian,
            materials.glass,
            materials.lava,
        ] {
            assert_eq!(scene.material(id).unwrap().normal_map(), None);
        }
    }

    #[test]
    fn canonical_indices_of_refraction_are_air_and_glass() {
        assert_eq!(AIR_IOR, 1.0);
        assert_eq!(GLASS_IOR, 1.5);
    }

    #[test]
    fn canonical_materials_select_their_expected_textures() {
        let (scene, textures, materials) = registered_scene();
        let grass = scene.material(materials.grass).unwrap().textures();

        assert_eq!(grass.for_face(CubeFace::PositiveY), textures.grass_top);
        assert_eq!(grass.for_face(CubeFace::NegativeY), textures.dirt);
        for face in [
            CubeFace::NegativeX,
            CubeFace::PositiveX,
            CubeFace::NegativeZ,
            CubeFace::PositiveZ,
        ] {
            assert_eq!(grass.for_face(face), textures.grass_side);
        }

        assert_uniform(
            scene.material(materials.cobblestone).unwrap().textures(),
            textures.cobblestone,
        );
        assert_uniform(
            scene.material(materials.obsidian).unwrap().textures(),
            textures.obsidian,
        );
        assert_uniform(
            scene.material(materials.glass).unwrap().textures(),
            textures.glass,
        );
        assert_uniform(
            scene.material(materials.lava).unwrap().textures(),
            textures.lava,
        );
    }
}
