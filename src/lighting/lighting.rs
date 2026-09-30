use crate::{color::Color, math::Vec3};

/// Shared Blinn-Phong exponent for the initial Phase 2 lighting model.
const DEFAULT_SHININESS: f32 = 32.0;

/// Infinitely distant light whose direction points from a surface toward the light source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    direction_to_light: Vec3,
    color: Color,
    intensity: f32,
}

impl DirectionalLight {
    pub fn try_new(direction_to_light: Vec3, color: Color, intensity: f32) -> Option<Self> {
        let direction_to_light = direction_to_light.try_normalized()?;
        if !valid_light_color(color) || !intensity.is_finite() || intensity < 0.0 {
            return None;
        }

        Some(Self {
            direction_to_light,
            color,
            intensity,
        })
    }

    pub const fn direction_to_light(self) -> Vec3 {
        self.direction_to_light
    }

    pub const fn color(self) -> Color {
        self.color
    }

    pub const fn intensity(self) -> f32 {
        self.intensity
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmbientLight {
    color: Color,
    intensity: f32,
}

impl AmbientLight {
    pub fn try_new(color: Color, intensity: f32) -> Option<Self> {
        if !valid_light_color(color) || !intensity.is_finite() || intensity < 0.0 {
            return None;
        }

        Some(Self { color, intensity })
    }

    pub const fn color(self) -> Color {
        self.color
    }

    pub const fn intensity(self) -> f32 {
        self.intensity
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    ambient: AmbientLight,
    directional: DirectionalLight,
}

impl Lighting {
    pub const fn new(ambient: AmbientLight, directional: DirectionalLight) -> Self {
        Self {
            ambient,
            directional,
        }
    }

    pub const fn ambient(self) -> AmbientLight {
        self.ambient
    }

    pub const fn directional(self) -> DirectionalLight {
        self.directional
    }
}

/// Applies ambient, Lambert diffuse, and Blinn-Phong specular illumination.
///
/// `normal`, `view_direction`, and the stored light direction are expected to be normalized.
/// A non-positive Lambert term gates both direct components so back-facing surfaces cannot
/// receive a specular highlight.
pub fn shade_surface(
    base_color: Color,
    normal: Vec3,
    view_direction: Vec3,
    material_specular: f32,
    lighting: Lighting,
) -> Color {
    let ambient = base_color * lighting.ambient.color.scale(lighting.ambient.intensity);
    let light = lighting.directional;
    let diffuse_factor = normal.dot(light.direction_to_light).max(0.0);
    if diffuse_factor <= 0.0 {
        return ambient;
    }

    let diffuse = base_color * light.color.scale(light.intensity * diffuse_factor);
    let specular_factor = (light.direction_to_light + view_direction)
        .try_normalized()
        .map(|half_vector| normal.dot(half_vector).max(0.0).powf(DEFAULT_SHININESS))
        .unwrap_or(0.0);
    let specular = light
        .color
        .scale(light.intensity * material_specular * specular_factor);

    ambient + diffuse + specular
}

fn valid_light_color(color: Color) -> bool {
    color.is_finite() && color.r >= 0.0 && color.g >= 0.0 && color.b >= 0.0
}

#[cfg(test)]
mod tests {
    use super::{AmbientLight, DirectionalLight, Lighting, shade_surface};
    use crate::{color::Color, math::Vec3};

    const EPSILON: f32 = 1.0e-5;
    const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
    const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    const NEG_Y: Vec3 = Vec3::new(0.0, -1.0, 0.0);

    fn assert_approx_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= EPSILON,
            "{actual} != {expected}"
        );
    }

    fn assert_color_approx_eq(actual: Color, expected: Color) {
        assert_approx_eq(actual.r, expected.r);
        assert_approx_eq(actual.g, expected.g);
        assert_approx_eq(actual.b, expected.b);
    }

    fn lighting(
        direction_to_light: Vec3,
        light_color: Color,
        light_intensity: f32,
        ambient_color: Color,
        ambient_intensity: f32,
    ) -> Lighting {
        Lighting::new(
            AmbientLight::try_new(ambient_color, ambient_intensity).unwrap(),
            DirectionalLight::try_new(direction_to_light, light_color, light_intensity).unwrap(),
        )
    }

    fn direct_only(direction_to_light: Vec3, color: Color, intensity: f32) -> Lighting {
        lighting(direction_to_light, color, intensity, Color::BLACK, 0.0)
    }

    #[test]
    fn constructs_valid_directional_light_with_normalized_direction() {
        let light = DirectionalLight::try_new(Vec3::new(0.0, 3.0, 4.0), Color::WHITE, 2.0).unwrap();

        assert_color_approx_eq(light.color(), Color::WHITE);
        assert_approx_eq(light.intensity(), 2.0);
        assert_approx_eq(light.direction_to_light().length(), 1.0);
        assert_approx_eq(light.direction_to_light().y, 0.6);
        assert_approx_eq(light.direction_to_light().z, 0.8);
    }

    #[test]
    fn rejects_zero_near_zero_and_non_finite_directions() {
        for direction in [
            Vec3::ZERO,
            Vec3::new(1.0e-7, 0.0, 0.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(f32::INFINITY, 0.0, 0.0),
        ] {
            assert_eq!(
                DirectionalLight::try_new(direction, Color::WHITE, 1.0),
                None
            );
        }
    }

    #[test]
    fn rejects_invalid_directional_light_color_and_intensity() {
        assert_eq!(
            DirectionalLight::try_new(Y, Color::new(f32::NAN, 1.0, 1.0), 1.0),
            None
        );
        assert_eq!(
            DirectionalLight::try_new(Y, Color::new(-0.1, 1.0, 1.0), 1.0),
            None
        );
        for intensity in [-0.1, f32::NAN, f32::INFINITY] {
            assert_eq!(DirectionalLight::try_new(Y, Color::WHITE, intensity), None);
        }
    }

    #[test]
    fn ambient_light_rejects_invalid_configuration() {
        assert_eq!(
            AmbientLight::try_new(Color::new(1.0, f32::INFINITY, 1.0), 0.2),
            None
        );
        assert_eq!(AmbientLight::try_new(Color::WHITE, -0.1), None);
        assert_eq!(AmbientLight::try_new(Color::WHITE, f32::NAN), None);
    }

    #[test]
    fn facing_surface_receives_lambert_diffuse() {
        let result = shade_surface(
            Color::new(0.5, 0.4, 0.3),
            Y,
            Y,
            0.0,
            direct_only(Y, Color::WHITE, 1.0),
        );

        assert_color_approx_eq(result, Color::new(0.5, 0.4, 0.3));
    }

    #[test]
    fn perpendicular_and_back_facing_surfaces_receive_no_direct_light() {
        let lighting = direct_only(Y, Color::WHITE, 1.0);

        assert_eq!(
            shade_surface(Color::WHITE, X, Y, 1.0, lighting),
            Color::BLACK
        );
        assert_eq!(
            shade_surface(Color::WHITE, NEG_Y, NEG_Y, 1.0, lighting),
            Color::BLACK
        );
    }

    #[test]
    fn light_intensity_scales_diffuse_contribution() {
        let dim = shade_surface(
            Color::new(0.4, 0.2, 0.1),
            Y,
            X,
            0.0,
            direct_only(Y, Color::WHITE, 0.5),
        );
        let bright = shade_surface(
            Color::new(0.4, 0.2, 0.1),
            Y,
            X,
            0.0,
            direct_only(Y, Color::WHITE, 1.0),
        );

        assert_color_approx_eq(bright, dim.scale(2.0));
    }

    #[test]
    fn light_color_tints_diffuse_contribution() {
        let result = shade_surface(
            Color::WHITE,
            Y,
            X,
            0.0,
            direct_only(Y, Color::new(1.0, 0.5, 0.25), 1.0),
        );

        assert_color_approx_eq(result, Color::new(1.0, 0.5, 0.25));
    }

    #[test]
    fn ambient_remains_without_direct_light_and_scales_with_intensity() {
        let dim = shade_surface(
            Color::new(0.8, 0.6, 0.4),
            NEG_Y,
            NEG_Y,
            1.0,
            lighting(Y, Color::WHITE, 1.0, Color::new(0.5, 0.4, 0.3), 0.25),
        );
        let bright = shade_surface(
            Color::new(0.8, 0.6, 0.4),
            NEG_Y,
            NEG_Y,
            1.0,
            lighting(Y, Color::WHITE, 1.0, Color::new(0.5, 0.4, 0.3), 0.5),
        );

        assert!(dim.r > 0.0 && dim.g > 0.0 && dim.b > 0.0);
        assert_color_approx_eq(bright, dim.scale(2.0));
    }

    #[test]
    fn zero_material_specular_produces_no_highlight() {
        let result = shade_surface(Color::BLACK, Y, Y, 0.0, direct_only(Y, Color::WHITE, 1.0));

        assert_eq!(result, Color::BLACK);
    }

    #[test]
    fn aligned_view_and_light_produce_specular_highlight() {
        let result = shade_surface(Color::BLACK, Y, Y, 0.5, direct_only(Y, Color::WHITE, 1.0));

        assert_color_approx_eq(result, Color::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn back_facing_surface_receives_no_specular_highlight() {
        let result = shade_surface(
            Color::BLACK,
            NEG_Y,
            NEG_Y,
            1.0,
            direct_only(Y, Color::WHITE, 1.0),
        );

        assert_eq!(result, Color::BLACK);
    }

    #[test]
    fn higher_material_specular_increases_highlight() {
        let lighting = direct_only(Y, Color::WHITE, 1.0);
        let low = shade_surface(Color::BLACK, Y, Y, 0.2, lighting);
        let high = shade_surface(Color::BLACK, Y, Y, 0.8, lighting);

        assert!(high.r > low.r);
        assert!(high.g > low.g);
        assert!(high.b > low.b);
    }
}
