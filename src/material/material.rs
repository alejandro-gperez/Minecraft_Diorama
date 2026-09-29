use crate::color::Color;

use super::Texture;

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

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    texture: Texture,
    albedo: Color,
    specular: f32,
    transparency: f32,
    reflectivity: f32,
}

impl Material {
    pub fn try_new(
        texture: Texture,
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
            texture,
            albedo,
            specular,
            transparency,
            reflectivity,
        })
    }

    pub const fn texture(&self) -> &Texture {
        &self.texture
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
    use super::{Material, MaterialId};
    use crate::{color::Color, material::Texture};

    fn material_with(
        albedo: Color,
        specular: f32,
        transparency: f32,
        reflectivity: f32,
    ) -> Option<Material> {
        Material::try_new(
            Texture::solid(Color::new(0.8, 0.6, 0.4)),
            albedo,
            specular,
            transparency,
            reflectivity,
        )
    }

    #[test]
    fn constructs_valid_material_and_retains_properties() {
        let texture = Texture::solid(Color::new(0.8, 0.6, 0.4));
        let material =
            Material::try_new(texture.clone(), Color::new(0.5, 1.0, 0.25), 0.3, 0.1, 0.2).unwrap();

        assert_eq!(material.texture(), &texture);
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
    fn material_id_is_compact_and_preserves_index() {
        let id = MaterialId::new(42);

        assert_eq!(id.index(), 42);
        assert_eq!(
            std::mem::size_of::<MaterialId>(),
            std::mem::size_of::<u32>()
        );
    }
}
