use crate::color::Color;

use super::{Material, MaterialId, TextureId, TextureSelection};

/// Material definitions for the small decorative props of the EggWars aftermath: eggs and their
/// shell fragments, chests, and arrows.
#[derive(Clone, Debug, PartialEq)]
pub struct DecorativeMaterialDefinitions {
    pub egg_shell: Material,
    pub chest_wood: Material,
    pub chest_interior: Material,
    pub chest_latch: Material,
    pub arrow_shaft: Material,
    pub arrow_head: Material,
    pub arrow_fletching: Material,
}

/// Stable runtime identities of the decorative materials after scene registration.
///
/// Like `AuxiliaryMaterials`, these are not rubric materials and stay separate from
/// `CanonicalMaterials`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecorativeMaterials {
    pub egg_shell: MaterialId,
    pub chest_wood: MaterialId,
    pub chest_interior: MaterialId,
    pub chest_latch: MaterialId,
    pub arrow_shaft: MaterialId,
    pub arrow_head: MaterialId,
    pub arrow_fletching: MaterialId,
}

impl DecorativeMaterials {
    /// Every decorative material, in registration order.
    pub const fn all(self) -> [MaterialId; 7] {
        [
            self.egg_shell,
            self.chest_wood,
            self.chest_interior,
            self.chest_latch,
            self.arrow_shaft,
            self.arrow_head,
            self.arrow_fletching,
        ]
    }
}

pub const EGG_SHELL_COLOR: Color = Color::new(0.93, 0.88, 0.76);
pub const CHEST_WOOD_COLOR: Color = Color::new(0.62, 0.42, 0.20);
pub const CHEST_INTERIOR_COLOR: Color = Color::new(0.20, 0.12, 0.06);
pub const CHEST_LATCH_COLOR: Color = Color::new(0.78, 0.78, 0.74);
pub const ARROW_SHAFT_COLOR: Color = Color::new(0.55, 0.40, 0.24);
pub const ARROW_HEAD_COLOR: Color = Color::new(0.42, 0.42, 0.44);
pub const ARROW_FLETCHING_COLOR: Color = Color::new(0.92, 0.92, 0.90);

/// Builds the decorative definitions over one shared solid white texture, so each material's
/// albedo is its color and no image asset is involved.
///
/// All are opaque and non-emissive. Only the iron latch reflects, faintly: the latch is a few
/// pixels in a close-up, so its recursive ray is rare, while the large egg and wood surfaces keep
/// zero reflectivity and never spawn one. The egg shell has a soft highlight; wood, the dark chest
/// interior, and the feathers are matte.
pub fn decorative_material_definitions(white: TextureId) -> Option<DecorativeMaterialDefinitions> {
    let solid = |albedo, specular, reflectivity| {
        Material::try_new(
            TextureSelection::Uniform(white),
            albedo,
            specular,
            0.0,
            reflectivity,
        )
    };

    Some(DecorativeMaterialDefinitions {
        egg_shell: solid(EGG_SHELL_COLOR, 0.25, 0.0)?,
        chest_wood: solid(CHEST_WOOD_COLOR, 0.05, 0.0)?,
        chest_interior: solid(CHEST_INTERIOR_COLOR, 0.0, 0.0)?,
        chest_latch: solid(CHEST_LATCH_COLOR, 0.6, 0.2)?,
        arrow_shaft: solid(ARROW_SHAFT_COLOR, 0.05, 0.0)?,
        arrow_head: solid(ARROW_HEAD_COLOR, 0.3, 0.0)?,
        arrow_fletching: solid(ARROW_FLETCHING_COLOR, 0.0, 0.0)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{DecorativeMaterials, EGG_SHELL_COLOR};
    use crate::{
        color::Color,
        geometry::CubeFace,
        material::{AIR_IOR, Texture},
        scene::Scene,
    };

    fn registered() -> (Scene, DecorativeMaterials) {
        let mut scene = Scene::new();
        let white = scene.add_texture(Texture::solid(Color::WHITE)).unwrap();
        let materials = scene.add_decorative_materials(white).unwrap();
        (scene, materials)
    }

    #[test]
    fn registers_seven_distinct_materials_over_one_shared_texture() {
        let (scene, materials) = registered();
        let ids = materials.all();

        assert_eq!(scene.material_count(), 7);
        assert_eq!(scene.texture_count(), 1);
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id));
            let material = scene.material(*id).unwrap();
            for face in CubeFace::ALL {
                assert_eq!(material.textures().for_face(face).index(), 0);
            }
        }
    }

    #[test]
    fn decorations_are_opaque_non_emissive_and_mostly_non_reflective() {
        let (scene, materials) = registered();

        for id in materials.all() {
            let material = scene.material(id).unwrap();
            assert_eq!(material.transparency(), 0.0);
            assert_eq!(material.ior(), AIR_IOR);
            assert!(!material.is_emissive());
            assert_eq!(material.normal_map(), None);
            let expected = if id == materials.chest_latch {
                0.2
            } else {
                0.0
            };
            assert_eq!(material.reflectivity(), expected);
        }
    }

    #[test]
    fn egg_shell_is_pale_and_warm_and_the_chest_interior_is_darker_than_its_wood() {
        let (scene, materials) = registered();
        let shell = scene.material(materials.egg_shell).unwrap().albedo();
        let wood = scene.material(materials.chest_wood).unwrap().albedo();
        let interior = scene.material(materials.chest_interior).unwrap().albedo();

        assert_eq!(shell, EGG_SHELL_COLOR);
        assert!(shell.r > 0.85 && shell.r >= shell.g && shell.g >= shell.b);
        assert!(interior.r < wood.r && interior.g < wood.g && interior.b < wood.b);
    }
}
