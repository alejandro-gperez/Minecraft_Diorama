use crate::{color::Color, geometry::CubeFace};

use super::TextureId;

/// Stable index into the scene's material storage.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialId(u32);

impl MaterialId {
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index of refraction of the air surrounding every scene object.
///
/// Also the neutral index for materials that do not transmit light: with `transparency == 0` no
/// refracted ray is ever traced, so their index has no optical effect.
pub const AIR_IOR: f32 = 1.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    textures: TextureSelection,
    albedo: Color,
    specular: f32,
    transparency: f32,
    reflectivity: f32,
    ior: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureSelection {
    Uniform(TextureId),
    TopSideBottom {
        top: TextureId,
        side: TextureId,
        bottom: TextureId,
    },
}

impl TextureSelection {
    pub const fn for_face(self, face: CubeFace) -> TextureId {
        match self {
            Self::Uniform(texture) => texture,
            Self::TopSideBottom { top, side, bottom } => match face {
                CubeFace::PositiveY => top,
                CubeFace::NegativeY => bottom,
                CubeFace::NegativeX
                | CubeFace::PositiveX
                | CubeFace::NegativeZ
                | CubeFace::PositiveZ => side,
            },
        }
    }
}

impl Material {
    pub fn try_new(
        textures: TextureSelection,
        albedo: Color,
        specular: f32,
        transparency: f32,
        reflectivity: f32,
    ) -> Option<Self> {
        if !color_is_normalized(albedo)
            || !scalar_is_normalized(specular)
            || !scalar_is_normalized(transparency)
            || !scalar_is_normalized(reflectivity)
        {
            return None;
        }

        Some(Self {
            textures,
            albedo,
            specular,
            transparency,
            reflectivity,
            ior: AIR_IOR,
        })
    }

    /// Returns this material with the given index of refraction, or `None` unless it is finite
    /// and strictly positive. Invalid indices are rejected rather than clamped.
    pub fn with_ior(self, ior: f32) -> Option<Self> {
        (ior.is_finite() && ior > 0.0).then_some(Self { ior, ..self })
    }

    pub const fn textures(&self) -> TextureSelection {
        self.textures
    }

    pub const fn albedo(&self) -> Color {
        self.albedo
    }

    pub const fn specular(&self) -> f32 {
        self.specular
    }

    pub const fn transparency(&self) -> f32 {
        self.transparency
    }

    pub const fn reflectivity(&self) -> f32 {
        self.reflectivity
    }

    pub const fn ior(&self) -> f32 {
        self.ior
    }

    /// Applies the material tint to a sampled texture color component by component.
    pub fn surface_color(&self, texture_sample: Color) -> Color {
        texture_sample * self.albedo
    }
}

fn scalar_is_normalized(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn color_is_normalized(color: Color) -> bool {
    scalar_is_normalized(color.r) && scalar_is_normalized(color.g) && scalar_is_normalized(color.b)
}

#[cfg(test)]
mod tests {
    use super::{AIR_IOR, Material, MaterialId, TextureSelection};
    use crate::{color::Color, geometry::CubeFace, material::TextureId};

    const TEXTURE_ID: TextureId = TextureId::new(7);

    fn material_with(
        albedo: Color,
        specular: f32,
        transparency: f32,
        reflectivity: f32,
    ) -> Option<Material> {
        Material::try_new(
            TextureSelection::Uniform(TEXTURE_ID),
            albedo,
            specular,
            transparency,
            reflectivity,
        )
    }

    #[test]
    fn constructs_valid_material_and_retains_properties() {
        let textures = TextureSelection::Uniform(TEXTURE_ID);
        let material =
            Material::try_new(textures, Color::new(0.5, 1.0, 0.25), 0.3, 0.1, 0.2).unwrap();

        assert_eq!(material.textures(), textures);
        assert_eq!(material.albedo(), Color::new(0.5, 1.0, 0.25));
        assert_eq!(material.specular(), 0.3);
        assert_eq!(material.transparency(), 0.1);
        assert_eq!(material.reflectivity(), 0.2);
        assert_eq!(
            material.surface_color(Color::new(0.8, 0.6, 0.4)),
            Color::new(0.4, 0.6, 0.1)
        );
    }

    #[test]
    fn uniform_selection_uses_one_texture_for_every_face() {
        let selection = TextureSelection::Uniform(TEXTURE_ID);

        for face in [
            CubeFace::NegativeX,
            CubeFace::PositiveX,
            CubeFace::NegativeY,
            CubeFace::PositiveY,
            CubeFace::NegativeZ,
            CubeFace::PositiveZ,
        ] {
            assert_eq!(selection.for_face(face), TEXTURE_ID);
        }
    }

    #[test]
    fn top_side_bottom_selection_respects_y_axis_orientation() {
        let top = TextureId::new(1);
        let side = TextureId::new(2);
        let bottom = TextureId::new(3);
        let selection = TextureSelection::TopSideBottom { top, side, bottom };

        assert_eq!(selection.for_face(CubeFace::PositiveY), top);
        assert_eq!(selection.for_face(CubeFace::NegativeY), bottom);
        for face in [
            CubeFace::NegativeX,
            CubeFace::PositiveX,
            CubeFace::NegativeZ,
            CubeFace::PositiveZ,
        ] {
            assert_eq!(selection.for_face(face), side);
        }
    }

    #[test]
    fn accepts_zero_and_one_parameter_boundaries() {
        assert!(material_with(Color::BLACK, 0.0, 0.0, 0.0).is_some());
        assert!(material_with(Color::WHITE, 1.0, 1.0, 1.0).is_some());
    }

    #[test]
    fn rejects_negative_parameters() {
        assert!(material_with(Color::WHITE, -0.1, 0.0, 0.0).is_none());
        assert!(material_with(Color::WHITE, 0.0, -0.1, 0.0).is_none());
        assert!(material_with(Color::WHITE, 0.0, 0.0, -0.1).is_none());
    }

    #[test]
    fn rejects_parameters_above_one() {
        assert!(material_with(Color::WHITE, 1.1, 0.0, 0.0).is_none());
        assert!(material_with(Color::WHITE, 0.0, 1.1, 0.0).is_none());
        assert!(material_with(Color::WHITE, 0.0, 0.0, 1.1).is_none());
    }

    #[test]
    fn rejects_nan_and_infinite_parameters() {
        assert!(material_with(Color::WHITE, f32::NAN, 0.0, 0.0).is_none());
        assert!(material_with(Color::WHITE, 0.0, f32::INFINITY, 0.0).is_none());
        assert!(material_with(Color::WHITE, 0.0, 0.0, f32::NEG_INFINITY).is_none());
    }

    #[test]
    fn rejects_invalid_albedo_channels() {
        assert!(material_with(Color::new(-0.1, 0.0, 0.0), 0.0, 0.0, 0.0).is_none());
        assert!(material_with(Color::new(0.0, 1.1, 0.0), 0.0, 0.0, 0.0).is_none());
        assert!(material_with(Color::new(0.0, 0.0, f32::NAN), 0.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn new_materials_default_to_the_air_index_of_refraction() {
        assert_eq!(AIR_IOR, 1.0);
        assert_eq!(
            material_with(Color::WHITE, 0.0, 0.0, 0.0).unwrap().ior(),
            AIR_IOR
        );
    }

    #[test]
    fn accepts_valid_index_of_refraction_and_retains_other_properties() {
        let material = material_with(Color::new(0.5, 1.0, 0.25), 0.3, 0.1, 0.2)
            .unwrap()
            .with_ior(1.33)
            .unwrap();

        assert_eq!(material.ior(), 1.33);
        assert_eq!(material.textures(), TextureSelection::Uniform(TEXTURE_ID));
        assert_eq!(material.albedo(), Color::new(0.5, 1.0, 0.25));
        assert_eq!(material.specular(), 0.3);
        assert_eq!(material.transparency(), 0.1);
        assert_eq!(material.reflectivity(), 0.2);
    }

    #[test]
    fn accepts_unit_and_glass_indices_of_refraction() {
        let base = material_with(Color::WHITE, 0.0, 0.0, 0.0).unwrap();

        assert_eq!(base.clone().with_ior(1.0).unwrap().ior(), 1.0);
        assert_eq!(base.with_ior(1.5).unwrap().ior(), 1.5);
    }

    #[test]
    fn rejects_zero_and_negative_indices_of_refraction() {
        let base = material_with(Color::WHITE, 0.0, 0.0, 0.0).unwrap();

        assert!(base.clone().with_ior(0.0).is_none());
        assert!(base.clone().with_ior(-0.0).is_none());
        assert!(base.with_ior(-1.5).is_none());
    }

    #[test]
    fn rejects_nan_and_infinite_indices_of_refraction() {
        let base = material_with(Color::WHITE, 0.0, 0.0, 0.0).unwrap();

        assert!(base.clone().with_ior(f32::NAN).is_none());
        assert!(base.clone().with_ior(f32::INFINITY).is_none());
        assert!(base.with_ior(f32::NEG_INFINITY).is_none());
    }

    #[test]
    fn material_id_is_compact_and_preserves_index() {
        let id = MaterialId::new(42);

        assert_eq!(id.index(), 42);
        assert_eq!(
            std::mem::size_of::<MaterialId>(),
            std::mem::size_of::<u32>()
        );
    }
}
