use crate::{color::Color, math::Vec3};

/// Shared Blinn-Phong exponent for the initial Phase 2 lighting model.
const DEFAULT_SHININESS: f32 = 32.0;
/// Shortest surface-to-light distance with a defined light direction.
///
/// Matches the vector-normalization epsilon; a surface this close to a point light receives no
/// contribution instead of a NaN.
const POINT_LIGHT_MIN_DISTANCE: f32 = 1.0e-6;

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

/// Finite-radius local light for a small, hand-placed set of emissive regions such as lava.
///
/// A point light is not geometry: it has no surface and never blocks rays. Its influence ends at
/// `radius`, so evaluation needs no inverse-square tail and far surfaces early-out.
///
/// Intended use is one or a few representative lights per connected emissive region (for example
/// a lava pool), never one light per emissive voxel or texel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    position: Vec3,
    color: Color,
    intensity: f32,
    radius: f32,
}

impl PointLight {
    /// Accepts a finite position, a finite non-negative color, a finite non-negative intensity,
    /// and a finite strictly positive radius. A zero intensity is a valid, inert light.
    pub fn try_new(position: Vec3, color: Color, intensity: f32, radius: f32) -> Option<Self> {
        if !position.is_finite()
            || !valid_light_color(color)
            || !intensity.is_finite()
            || intensity < 0.0
            || !radius.is_finite()
            || radius <= 0.0
        {
            return None;
        }

        Some(Self {
            position,
            color,
            intensity,
            radius,
        })
    }

    pub const fn position(self) -> Vec3 {
        self.position
    }

    pub const fn color(self) -> Color {
        self.color
    }

    pub const fn intensity(self) -> f32 {
        self.intensity
    }

    pub const fn radius(self) -> f32 {
        self.radius
    }

    /// Finite-radius squared falloff: `clamp(1 - distance / radius, 0, 1)²`.
    ///
    /// This is deliberately not inverse-square. It is `1` at the light, decreases smoothly, and is
    /// exactly `0` at and beyond `radius`, so the artistic radius is also the cost boundary.
    /// An invalid (non-finite or negative) distance yields `0` rather than a NaN or a boost.
    pub fn attenuation(self, distance: f32) -> f32 {
        if !distance.is_finite() || distance < 0.0 {
            return 0.0;
        }

        let remaining = (1.0 - distance / self.radius).clamp(0.0, 1.0);
        remaining * remaining
    }

    /// Light arriving at `position` on a surface with outward unit `normal`, before visibility.
    ///
    /// Returns `None` when this light cannot contribute: the surface lies at or beyond the radius,
    /// the light is inert, the light coincides with the surface (no defined direction), or the
    /// surface faces away from it. Callers may therefore skip the shadow ray and specular work.
    ///
    /// Pass the *geometric* normal: it decides whether the light is on the visible side of the
    /// surface at all. A normal-mapped surface then shades with its own normal in
    /// [`shade_point_light`].
    pub fn incidence_at(self, position: Vec3, normal: Vec3) -> Option<PointLightIncidence> {
        let to_light = self.position - position;
        let distance = to_light.length();
        if !distance.is_finite() || distance < POINT_LIGHT_MIN_DISTANCE || distance >= self.radius {
            return None;
        }

        let effective_intensity = self.intensity * self.attenuation(distance);
        if effective_intensity <= 0.0 {
            return None;
        }

        let direction_to_light = to_light * distance.recip();
        if normal.dot(direction_to_light) <= 0.0 {
            return None;
        }

        Some(PointLightIncidence {
            direction_to_light,
            distance,
            radiance: self.color.scale(effective_intensity),
        })
    }
}

/// Unshadowed point-light contribution at one surface point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLightIncidence {
    /// Unit vector from the surface point toward the light.
    pub direction_to_light: Vec3,
    /// Distance from the surface point to the light, strictly inside the light's radius.
    pub distance: f32,
    /// Light color scaled by intensity and attenuation.
    pub radiance: Color,
}

/// Lambert diffuse and Blinn-Phong specular from one point light that is already known to be
/// visible. Ambient is not included; the directional model in [`shade_surface`] owns it.
///
/// Shares the directional light's shininess, so both lights use one specular model.
///
/// `normal` is the shading normal, which may differ from the geometric normal that admitted the
/// light in [`PointLight::incidence_at`]. A non-positive Lambert term gates the specular highlight
/// as well, mirroring [`shade_surface`]; with a geometric normal it is always positive here.
pub fn shade_point_light(
    incidence: PointLightIncidence,
    base_color: Color,
    normal: Vec3,
    view_direction: Vec3,
    material_specular: f32,
) -> Color {
    let diffuse_factor = normal.dot(incidence.direction_to_light);
    if diffuse_factor <= 0.0 {
        return Color::BLACK;
    }

    let diffuse = base_color * incidence.radiance.scale(diffuse_factor);
    if material_specular <= 0.0 {
        return diffuse;
    }

    let specular_factor = blinn_phong_factor(normal, incidence.direction_to_light, view_direction);
    diffuse
        + incidence
            .radiance
            .scale(material_specular * specular_factor)
}

/// Blinn-Phong highlight term `max(N·H, 0)^shininess`, or `0` when `H` is degenerate.
fn blinn_phong_factor(normal: Vec3, direction_to_light: Vec3, view_direction: Vec3) -> f32 {
    (direction_to_light + view_direction)
        .try_normalized()
        .map(|half_vector| normal.dot(half_vector).max(0.0).powf(DEFAULT_SHININESS))
        .unwrap_or(0.0)
}

/// Applies ambient, Lambert diffuse, and Blinn-Phong specular illumination.
///
/// `normal`, `view_direction`, and the stored light direction are expected to be normalized.
/// A non-positive Lambert term gates both direct components so back-facing surfaces cannot
/// receive a specular highlight.
// Called once per shaded hit; measured renders slow down when LLVM leaves it out of line.
#[inline]
pub fn shade_surface(
    base_color: Color,
    normal: Vec3,
    view_direction: Vec3,
    material_specular: f32,
    direct_light_visible: bool,
    lighting: Lighting,
) -> Color {
    let ambient = base_color * lighting.ambient.color.scale(lighting.ambient.intensity);
    if !direct_light_visible {
        return ambient;
    }

    let light = lighting.directional;
    let diffuse_factor = normal.dot(light.direction_to_light).max(0.0);
    if diffuse_factor <= 0.0 {
        return ambient;
    }

    let diffuse = base_color * light.color.scale(light.intensity * diffuse_factor);
    let specular_factor = blinn_phong_factor(normal, light.direction_to_light, view_direction);
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
    use super::{
        AmbientLight, DirectionalLight, Lighting, PointLight, shade_point_light, shade_surface,
    };
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
            true,
            direct_only(Y, Color::WHITE, 1.0),
        );

        assert_color_approx_eq(result, Color::new(0.5, 0.4, 0.3));
    }

    #[test]
    fn perpendicular_and_back_facing_surfaces_receive_no_direct_light() {
        let lighting = direct_only(Y, Color::WHITE, 1.0);

        assert_eq!(
            shade_surface(Color::WHITE, X, Y, 1.0, true, lighting),
            Color::BLACK
        );
        assert_eq!(
            shade_surface(Color::WHITE, NEG_Y, NEG_Y, 1.0, true, lighting),
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
            true,
            direct_only(Y, Color::WHITE, 0.5),
        );
        let bright = shade_surface(
            Color::new(0.4, 0.2, 0.1),
            Y,
            X,
            0.0,
            true,
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
            true,
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
            true,
            lighting(Y, Color::WHITE, 1.0, Color::new(0.5, 0.4, 0.3), 0.25),
        );
        let bright = shade_surface(
            Color::new(0.8, 0.6, 0.4),
            NEG_Y,
            NEG_Y,
            1.0,
            true,
            lighting(Y, Color::WHITE, 1.0, Color::new(0.5, 0.4, 0.3), 0.5),
        );

        assert!(dim.r > 0.0 && dim.g > 0.0 && dim.b > 0.0);
        assert_color_approx_eq(bright, dim.scale(2.0));
    }

    #[test]
    fn zero_material_specular_produces_no_highlight() {
        let result = shade_surface(
            Color::BLACK,
            Y,
            Y,
            0.0,
            true,
            direct_only(Y, Color::WHITE, 1.0),
        );

        assert_eq!(result, Color::BLACK);
    }

    #[test]
    fn aligned_view_and_light_produce_specular_highlight() {
        let result = shade_surface(
            Color::BLACK,
            Y,
            Y,
            0.5,
            true,
            direct_only(Y, Color::WHITE, 1.0),
        );

        assert_color_approx_eq(result, Color::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn back_facing_surface_receives_no_specular_highlight() {
        let result = shade_surface(
            Color::BLACK,
            NEG_Y,
            NEG_Y,
            1.0,
            true,
            direct_only(Y, Color::WHITE, 1.0),
        );

        assert_eq!(result, Color::BLACK);
    }

    #[test]
    fn higher_material_specular_increases_highlight() {
        let lighting = direct_only(Y, Color::WHITE, 1.0);
        let low = shade_surface(Color::BLACK, Y, Y, 0.2, true, lighting);
        let high = shade_surface(Color::BLACK, Y, Y, 0.8, true, lighting);

        assert!(high.r > low.r);
        assert!(high.g > low.g);
        assert!(high.b > low.b);
    }

    #[test]
    fn occlusion_removes_direct_terms_but_preserves_ambient() {
        let lighting = lighting(Y, Color::WHITE, 1.0, Color::WHITE, 0.2);
        let visible = shade_surface(Color::WHITE, Y, Y, 1.0, true, lighting);
        let blocked = shade_surface(Color::WHITE, Y, Y, 1.0, false, lighting);

        assert!(visible.r > blocked.r);
        assert_color_approx_eq(blocked, Color::new(0.2, 0.2, 0.2));
    }

    fn point_light(position: Vec3, color: Color, intensity: f32, radius: f32) -> PointLight {
        PointLight::try_new(position, color, intensity, radius).unwrap()
    }

    /// White unit-intensity light `distance` above the origin with radius 4.
    fn light_above(distance: f32) -> PointLight {
        point_light(Vec3::new(0.0, distance, 0.0), Color::WHITE, 1.0, 4.0)
    }

    fn incidence_above(light: PointLight, normal: Vec3) -> super::PointLightIncidence {
        light.incidence_at(Vec3::ZERO, normal).unwrap()
    }

    #[test]
    fn constructs_valid_point_light_and_retains_properties() {
        let light = PointLight::try_new(
            Vec3::new(1.0, -2.0, 3.0),
            Color::new(1.0, 0.5, 0.25),
            2.5,
            4.0,
        )
        .unwrap();

        assert_eq!(light.position(), Vec3::new(1.0, -2.0, 3.0));
        assert_eq!(light.color(), Color::new(1.0, 0.5, 0.25));
        assert_eq!(light.intensity(), 2.5);
        assert_eq!(light.radius(), 4.0);
    }

    #[test]
    fn point_light_rejects_non_finite_position() {
        for position in [
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
            Vec3::new(0.0, 0.0, f32::NEG_INFINITY),
        ] {
            assert_eq!(PointLight::try_new(position, Color::WHITE, 1.0, 1.0), None);
        }
    }

    #[test]
    fn point_light_rejects_invalid_color() {
        for color in [
            Color::new(-0.1, 1.0, 1.0),
            Color::new(1.0, -0.1, 1.0),
            Color::new(1.0, 1.0, -0.1),
            Color::new(f32::NAN, 1.0, 1.0),
            Color::new(1.0, f32::INFINITY, 1.0),
        ] {
            assert_eq!(PointLight::try_new(Vec3::ZERO, color, 1.0, 1.0), None);
        }
    }

    #[test]
    fn point_light_rejects_negative_and_non_finite_intensity() {
        for intensity in [-0.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                PointLight::try_new(Vec3::ZERO, Color::WHITE, intensity, 1.0),
                None
            );
        }
    }

    #[test]
    fn point_light_rejects_zero_negative_and_non_finite_radius() {
        for radius in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                PointLight::try_new(Vec3::ZERO, Color::WHITE, 1.0, radius),
                None
            );
        }
    }

    #[test]
    fn point_light_accepts_zero_intensity_but_it_never_contributes() {
        let light = PointLight::try_new(Vec3::new(0.0, 1.0, 0.0), Color::WHITE, 0.0, 4.0).unwrap();

        assert_eq!(light.intensity(), 0.0);
        assert_eq!(light.incidence_at(Vec3::ZERO, Y), None);
    }

    #[test]
    fn attenuation_is_maximal_and_finite_at_the_light() {
        let light = light_above(1.0);

        assert_approx_eq(light.attenuation(0.0), 1.0);
        assert_approx_eq(light.attenuation(1.0e-6), 1.0);
        assert!(light.attenuation(0.0).is_finite());
    }

    #[test]
    fn attenuation_is_squared_linear_falloff() {
        let light = point_light(Vec3::ZERO, Color::WHITE, 1.0, 4.0);

        assert_approx_eq(light.attenuation(2.0), 0.25);
        assert_approx_eq(light.attenuation(1.0), 0.5625);
        assert_approx_eq(light.attenuation(3.0), 0.0625);
    }

    #[test]
    fn attenuation_is_zero_at_and_beyond_the_radius() {
        let light = point_light(Vec3::ZERO, Color::WHITE, 1.0, 4.0);

        assert_eq!(light.attenuation(4.0), 0.0);
        assert_eq!(light.attenuation(4.0001), 0.0);
        assert_eq!(light.attenuation(1.0e9), 0.0);
    }

    #[test]
    fn attenuation_never_becomes_negative_and_decreases_monotonically() {
        let light = point_light(Vec3::ZERO, Color::WHITE, 1.0, 4.0);
        let mut previous = light.attenuation(0.0);

        for step in 1..=100 {
            let current = light.attenuation(step as f32 * 0.05);
            assert!(current >= 0.0);
            assert!(current <= previous);
            previous = current;
        }
    }

    #[test]
    fn attenuation_handles_invalid_distances_deliberately() {
        let light = point_light(Vec3::ZERO, Color::WHITE, 1.0, 4.0);

        for distance in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            assert_eq!(light.attenuation(distance), 0.0);
        }
    }

    #[test]
    fn incidence_reports_direction_distance_and_scaled_radiance() {
        let light = point_light(
            Vec3::new(0.0, 2.0, 0.0),
            Color::new(1.0, 0.5, 0.25),
            2.0,
            4.0,
        );

        let incidence = incidence_above(light, Y);

        assert_approx_eq(incidence.distance, 2.0);
        assert_approx_eq(incidence.direction_to_light.length(), 1.0);
        assert_approx_eq(incidence.direction_to_light.y, 1.0);
        // intensity 2 * attenuation 0.25 = 0.5
        assert_color_approx_eq(incidence.radiance, Color::new(0.5, 0.25, 0.125));
    }

    #[test]
    fn incidence_is_none_at_or_beyond_the_radius() {
        assert_eq!(light_above(4.0).incidence_at(Vec3::ZERO, Y), None);
        assert_eq!(light_above(4.5).incidence_at(Vec3::ZERO, Y), None);
        assert!(light_above(3.9).incidence_at(Vec3::ZERO, Y).is_some());
    }

    #[test]
    fn incidence_is_none_when_the_light_coincides_with_the_surface() {
        let light = point_light(Vec3::ZERO, Color::WHITE, 1.0, 4.0);

        assert_eq!(light.incidence_at(Vec3::ZERO, Y), None);
        assert_eq!(light.incidence_at(Vec3::new(1.0e-7, 0.0, 0.0), Y), None);
    }

    #[test]
    fn incidence_is_none_for_non_finite_surface_positions() {
        let light = light_above(1.0);

        assert_eq!(light.incidence_at(Vec3::new(f32::NAN, 0.0, 0.0), Y), None);
        assert_eq!(
            light.incidence_at(Vec3::new(0.0, f32::INFINITY, 0.0), Y),
            None
        );
    }

    #[test]
    fn incidence_is_none_for_perpendicular_and_back_facing_surfaces() {
        let light = light_above(1.0);

        assert_eq!(light.incidence_at(Vec3::ZERO, X), None);
        assert_eq!(light.incidence_at(Vec3::ZERO, NEG_Y), None);
    }

    #[test]
    fn point_light_facing_surface_receives_positive_lambert_diffuse() {
        let incidence = incidence_above(light_above(1.0), Y);

        // attenuation (1 - 1/4)^2 = 0.5625, N·L = 1
        assert_color_approx_eq(
            shade_point_light(incidence, Color::new(0.5, 0.4, 0.3), Y, Y, 0.0),
            Color::new(0.28125, 0.225, 0.16875),
        );
    }

    #[test]
    fn shading_normal_facing_away_from_an_admitted_light_receives_nothing() {
        // The geometric normal Y admits the light; a bumped shading normal tilted below the light's
        // horizon must get neither diffuse nor a leaked specular highlight.
        let light = point_light(Vec3::new(1.0, 0.2, 0.0), Color::WHITE, 1.0, 10.0);
        let incidence = light.incidence_at(Vec3::ZERO, Y).unwrap();
        let view = incidence.direction_to_light;
        let shading_normal = Vec3::new(-0.8, 0.6, 0.0);

        assert!(shading_normal.dot(incidence.direction_to_light) < 0.0);
        assert_eq!(
            shade_point_light(incidence, Color::WHITE, shading_normal, view, 1.0),
            Color::BLACK
        );
    }

    #[test]
    fn point_light_diffuse_follows_the_cosine_of_the_incidence_angle() {
        let light = point_light(Vec3::new(0.0, 1.0, 1.0), Color::WHITE, 1.0, 10.0);
        let incidence = light.incidence_at(Vec3::ZERO, Y).unwrap();
        let cosine = std::f32::consts::FRAC_1_SQRT_2;
        let attenuation = light.attenuation(2.0_f32.sqrt());

        assert_color_approx_eq(
            shade_point_light(incidence, Color::WHITE, Y, Y, 0.0),
            Color::new(1.0, 1.0, 1.0).scale(attenuation * cosine),
        );
    }

    #[test]
    fn point_light_color_tints_diffuse_contribution() {
        let light = point_light(
            Vec3::new(0.0, 0.0, 0.0) + Y * 1.0,
            Color::new(1.0, 0.5, 0.25),
            1.0,
            2.0,
        );
        let incidence = incidence_above(light, Y);

        // attenuation (1 - 1/2)^2 = 0.25
        assert_color_approx_eq(
            shade_point_light(incidence, Color::WHITE, Y, X, 0.0),
            Color::new(0.25, 0.125, 0.0625),
        );
    }

    #[test]
    fn material_specular_scales_the_point_light_highlight() {
        let incidence = incidence_above(light_above(1.0), Y);

        let none = shade_point_light(incidence, Color::BLACK, Y, Y, 0.0);
        let low = shade_point_light(incidence, Color::BLACK, Y, Y, 0.2);
        let high = shade_point_light(incidence, Color::BLACK, Y, Y, 0.8);

        assert_eq!(none, Color::BLACK);
        assert!(low.r > 0.0);
        assert!(high.r > low.r);
        // Aligned view and light: factor 1, radiance 0.5625.
        assert_color_approx_eq(high, Color::new(0.45, 0.45, 0.45));
    }

    #[test]
    fn point_light_specular_falls_off_away_from_the_mirror_direction() {
        let incidence = incidence_above(light_above(1.0), Y);
        let aligned = shade_point_light(incidence, Color::BLACK, Y, Y, 1.0);
        let off_axis = shade_point_light(
            incidence,
            Color::BLACK,
            Y,
            Vec3::new(1.0, 1.0, 0.0).try_normalized().unwrap(),
            1.0,
        );

        assert!(aligned.r > off_axis.r);
        assert!(off_axis.r >= 0.0);
    }

    #[test]
    fn point_light_specular_is_tinted_by_the_light_color() {
        let light = point_light(
            Vec3::new(0.0, 1.0, 0.0),
            Color::new(1.0, 0.5, 0.0),
            1.0,
            4.0,
        );
        let specular = shade_point_light(incidence_above(light, Y), Color::BLACK, Y, Y, 1.0);

        assert!(specular.r > specular.g);
        assert!(specular.g > 0.0);
        assert_eq!(specular.b, 0.0);
    }

    #[test]
    fn increasing_distance_reduces_the_point_light_contribution() {
        let near = shade_point_light(
            incidence_above(light_above(0.5), Y),
            Color::WHITE,
            Y,
            Y,
            0.5,
        );
        let far = shade_point_light(
            incidence_above(light_above(2.0), Y),
            Color::WHITE,
            Y,
            Y,
            0.5,
        );

        assert!(near.r > far.r);
        assert!(far.r > 0.0);
    }

    #[test]
    fn point_light_outside_its_radius_contributes_nothing() {
        assert_eq!(light_above(5.0).incidence_at(Vec3::ZERO, Y), None);
    }
}
