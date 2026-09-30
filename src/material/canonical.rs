use crate::{color::Color, scene::Scene};

use super::{Material, MaterialId, TextureId, TextureSelection};

/// Texture identities required by the five Phase 2 rubric materials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalTextureIds {
    pub grass_top: TextureId,
    pub grass_side: TextureId,
    pub dirt: TextureId,
    pub cobblestone: TextureId,
    pub obsidian: TextureId,
    pub glass: TextureId,
    pub lava: TextureId,
}

/// Stable runtime identities for the five canonical materials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalMaterials {
    pub grass: MaterialId,
    pub cobblestone: MaterialId,
    pub obsidian: MaterialId,
    pub glass: MaterialId,
    pub lava: MaterialId,
}

/// Registers the canonical Phase 2 material set without cloning or duplicating textures.
pub fn register_canonical_materials(
    scene: &mut Scene,
    textures: CanonicalTextureIds,
) -> Option<CanonicalMaterials> {
    let grass = scene.add_material(Material::try_new(
        TextureSelection::TopSideBottom {
            top: textures.grass_top,
            side: textures.grass_side,
            bottom: textures.dirt,
        },
        Color::WHITE,
        0.05,
        0.0,
        0.02,
    )?)?;
    let cobblestone = scene.add_material(Material::try_new(
        TextureSelection::Uniform(textures.cobblestone),
        Color::WHITE,
        0.08,
        0.0,
        0.03,
    )?)?;
    let obsidian = scene.add_material(Material::try_new(
        TextureSelection::Uniform(textures.obsidian),
        Color::new(0.90, 0.90, 1.00),
        0.55,
        0.0,
        0.35,
    )?)?;
    let glass = scene.add_material(Material::try_new(
        TextureSelection::Uniform(textures.glass),
        Color::new(0.90, 0.97, 1.00),
        0.80,
        0.85,
        0.15,
    )?)?;
    let lava = scene.add_material(Material::try_new(
        TextureSelection::Uniform(textures.lava),
        Color::new(1.00, 0.95, 0.90),
        0.10,
        0.0,
        0.05,
    )?)?;

    Some(CanonicalMaterials {
        grass,
        cobblestone,
        obsidian,
        glass,
        lava,
    })
}

#[cfg(test)]
mod tests {
    use super::{CanonicalMaterials, CanonicalTextureIds, register_canonical_materials};
    use crate::{
        color::Color,
        geometry::CubeFace,
        material::{Material, Texture, TextureId, TextureSelection},
        scene::Scene,
    };

    fn register_test_textures(scene: &mut Scene) -> CanonicalTextureIds {
        let mut next_texture = || scene.add_texture(Texture::solid(Color::WHITE)).unwrap();

        CanonicalTextureIds {
            grass_top: next_texture(),
            grass_side: next_texture(),
            dirt: next_texture(),
            cobblestone: next_texture(),
            obsidian: next_texture(),
            glass: next_texture(),
            lava: next_texture(),
        }
    }

    fn registered_scene() -> (Scene, CanonicalTextureIds, CanonicalMaterials) {
        let mut scene = Scene::new();
        let textures = register_test_textures(&mut scene);
        let materials = register_canonical_materials(&mut scene, textures).unwrap();
        (scene, textures, materials)
    }

    fn assert_properties(
        material: &Material,
        albedo: Color,
        specular: f32,
        transparency: f32,
        reflectivity: f32,
    ) {
        assert_eq!(material.albedo(), albedo);
        assert_eq!(material.specular(), specular);
        assert_eq!(material.transparency(), transparency);
        assert_eq!(material.reflectivity(), reflectivity);
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
        assert_eq!(scene.texture_count(), 7);
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
        );
        assert_properties(
            scene.material(materials.cobblestone).unwrap(),
            Color::WHITE,
            0.08,
            0.0,
            0.03,
        );
        assert_properties(
            scene.material(materials.obsidian).unwrap(),
            Color::new(0.90, 0.90, 1.00),
            0.55,
            0.0,
            0.35,
        );
        assert_properties(
            scene.material(materials.glass).unwrap(),
            Color::new(0.90, 0.97, 1.00),
            0.80,
            0.85,
            0.15,
        );
        assert_properties(
            scene.material(materials.lava).unwrap(),
            Color::new(1.00, 0.95, 0.90),
            0.10,
            0.0,
            0.05,
        );
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
