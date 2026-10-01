use crate::color::Color;

use super::{Material, MaterialId, TextureId, TextureSelection};

/// Texture identities required by the auxiliary terrain materials.
///
/// `dirt` is the same texture canonical grass uses for its bottom face; callers pass that
/// registered ID rather than loading the image twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxiliaryTextureIds {
    pub dirt: TextureId,
    pub coal_ore: TextureId,
    pub iron_ore: TextureId,
    pub gold_ore: TextureId,
    pub diamond_ore: TextureId,
}

/// Material definitions for the auxiliary terrain blocks.
#[derive(Clone, Debug, PartialEq)]
pub struct AuxiliaryMaterialDefinitions {
    pub dirt: Material,
    pub coal_ore: Material,
    pub iron_ore: Material,
    pub gold_ore: Material,
    pub diamond_ore: Material,
}

/// Stable runtime identities of the auxiliary materials after scene registration.
///
/// These are world materials for terrain blocks, not rubric materials: they stay separate from
/// `CanonicalMaterials`, whose five entries are the deliberately differentiated rubric set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxiliaryMaterials {
    pub dirt: MaterialId,
    pub coal_ore: MaterialId,
    pub iron_ore: MaterialId,
    pub gold_ore: MaterialId,
    pub diamond_ore: MaterialId,
}

/// Builds the auxiliary definitions without owning or duplicating textures.
///
/// Dirt and the ores are plain opaque terrain: white albedo so the original textures read
/// unchanged, a faint specular highlight, and zero transparency, reflectivity, and emission. Zero
/// reflectivity is deliberate: these blocks will make up most of the terrain, and a non-zero
/// value would spawn a recursive reflection ray from every hit for an invisible contribution.
/// Dirt is matter than the canonical grass; the four ores share one rock-like set of scalars
/// matching cobblestone's specular and differ only in texture. None carries a normal map.
pub fn auxiliary_material_definitions(
    textures: AuxiliaryTextureIds,
) -> Option<AuxiliaryMaterialDefinitions> {
    let ore = |texture| {
        Material::try_new(
            TextureSelection::Uniform(texture),
            Color::WHITE,
            0.08,
            0.0,
            0.0,
        )
    };

    Some(AuxiliaryMaterialDefinitions {
        dirt: Material::try_new(
            TextureSelection::Uniform(textures.dirt),
            Color::WHITE,
            0.03,
            0.0,
            0.0,
        )?,
        coal_ore: ore(textures.coal_ore)?,
        iron_ore: ore(textures.iron_ore)?,
        gold_ore: ore(textures.gold_ore)?,
        diamond_ore: ore(textures.diamond_ore)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{AuxiliaryMaterials, AuxiliaryTextureIds};
    use crate::{
        color::Color,
        geometry::CubeFace,
        material::{
            AIR_IOR, CanonicalMaterials, CanonicalTextureIds, Material, MaterialId, Texture,
        },
        scene::Scene,
    };

    struct Registered {
        scene: Scene,
        canonical_textures: CanonicalTextureIds,
        auxiliary_textures: AuxiliaryTextureIds,
        canonical: CanonicalMaterials,
        auxiliary: AuxiliaryMaterials,
    }

    /// Registers canonical then auxiliary materials the way the application does, reusing the
    /// canonical dirt texture and loading only the four ore textures.
    fn registered() -> Registered {
        let mut scene = Scene::new();
        let mut next_texture = || scene.add_texture(Texture::solid(Color::WHITE)).unwrap();
        let canonical_textures = CanonicalTextureIds {
            grass_top: next_texture(),
            grass_side: next_texture(),
            dirt: next_texture(),
            cobblestone: next_texture(),
            cobblestone_normal: next_texture(),
            obsidian: next_texture(),
            glass: next_texture(),
            lava: next_texture(),
        };
        let auxiliary_textures = AuxiliaryTextureIds {
            dirt: canonical_textures.dirt,
            coal_ore: next_texture(),
            iron_ore: next_texture(),
            gold_ore: next_texture(),
            diamond_ore: next_texture(),
        };
        let canonical = scene.add_canonical_materials(canonical_textures).unwrap();
        let auxiliary = scene.add_auxiliary_materials(auxiliary_textures).unwrap();

        Registered {
            scene,
            canonical_textures,
            auxiliary_textures,
            canonical,
            auxiliary,
        }
    }

    fn auxiliary_ids(materials: AuxiliaryMaterials) -> [MaterialId; 5] {
        [
            materials.dirt,
            materials.coal_ore,
            materials.iron_ore,
            materials.gold_ore,
            materials.diamond_ore,
        ]
    }

    fn canonical_ids(materials: CanonicalMaterials) -> [MaterialId; 5] {
        [
            materials.grass,
            materials.cobblestone,
            materials.obsidian,
            materials.glass,
            materials.lava,
        ]
    }

    fn assert_plain_opaque_terrain(material: &Material, specular: f32) {
        assert_eq!(material.albedo(), Color::WHITE);
        assert_eq!(material.specular(), specular);
        assert_eq!(material.transparency(), 0.0);
        assert_eq!(material.reflectivity(), 0.0);
        assert_eq!(material.ior(), AIR_IOR);
        assert!(!material.is_emissive());
        assert_eq!(material.emission_strength(), 0.0);
        assert_eq!(material.emission_color(), Color::BLACK);
        assert_eq!(material.normal_map(), None);
    }

    #[test]
    fn registers_five_distinct_materials_once_after_the_canonical_set() {
        let registered = registered();
        let auxiliary = auxiliary_ids(registered.auxiliary);
        let canonical = canonical_ids(registered.canonical);

        assert_eq!(registered.scene.material_count(), 10);
        for (index, id) in auxiliary.iter().enumerate() {
            assert!(registered.scene.material(*id).is_some());
            assert!(!auxiliary[..index].contains(id));
            assert!(!canonical.contains(id));
        }
    }

    #[test]
    fn dirt_reuses_the_canonical_dirt_texture_and_ores_add_four() {
        let registered = registered();
        let scene = &registered.scene;
        let grass = scene.material(registered.canonical.grass).unwrap();
        let dirt = scene.material(registered.auxiliary.dirt).unwrap();

        // Eight canonical textures plus the four ores; dirt adds no texture of its own.
        assert_eq!(scene.texture_count(), 12);
        assert_eq!(
            registered.auxiliary_textures.dirt,
            registered.canonical_textures.dirt
        );
        for face in CubeFace::ALL {
            assert_eq!(
                dirt.textures().for_face(face),
                registered.canonical_textures.dirt
            );
        }
        assert_eq!(
            grass.textures().for_face(CubeFace::NegativeY),
            dirt.textures().for_face(CubeFace::NegativeY)
        );
    }

    #[test]
    fn ores_use_their_own_distinct_textures_on_every_face() {
        let registered = registered();
        let textures = registered.auxiliary_textures;
        let expected = [
            (registered.auxiliary.coal_ore, textures.coal_ore),
            (registered.auxiliary.iron_ore, textures.iron_ore),
            (registered.auxiliary.gold_ore, textures.gold_ore),
            (registered.auxiliary.diamond_ore, textures.diamond_ore),
        ];

        for (index, (material_id, texture_id)) in expected.iter().enumerate() {
            assert!(
                !expected[..index]
                    .iter()
                    .any(|(_, other)| other == texture_id)
            );
            assert_ne!(*texture_id, textures.dirt);
            let selection = registered.scene.material(*material_id).unwrap().textures();
            for face in CubeFace::ALL {
                assert_eq!(selection.for_face(face), *texture_id);
            }
        }
    }

    #[test]
    fn dirt_is_matte_opaque_terrain() {
        let registered = registered();

        assert_plain_opaque_terrain(
            registered
                .scene
                .material(registered.auxiliary.dirt)
                .unwrap(),
            0.03,
        );
    }

    #[test]
    fn ores_share_rock_like_opaque_parameters() {
        let registered = registered();
        let auxiliary = registered.auxiliary;

        for id in [
            auxiliary.coal_ore,
            auxiliary.iron_ore,
            auxiliary.gold_ore,
            auxiliary.diamond_ore,
        ] {
            assert_plain_opaque_terrain(registered.scene.material(id).unwrap(), 0.08);
        }
    }

    #[test]
    fn auxiliary_registration_leaves_canonical_materials_unchanged() {
        let registered = registered();
        let mut canonical_only = Scene::new();
        for _ in 0..8 {
            canonical_only
                .add_texture(Texture::solid(Color::WHITE))
                .unwrap();
        }
        let expected = canonical_only
            .add_canonical_materials(registered.canonical_textures)
            .unwrap();

        assert_eq!(registered.canonical, expected);
        for id in canonical_ids(expected) {
            assert_eq!(
                registered.scene.material(id),
                canonical_only.material(id),
                "{id:?}"
            );
        }
    }
}
