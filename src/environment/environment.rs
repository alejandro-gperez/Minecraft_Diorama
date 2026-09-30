use crate::{color::Color, math::Vec3};

const DEFAULT_STAR_SEED: u32 = 0xE667_7A2D;
const DEFAULT_SUN_DIRECTION: Vec3 = Vec3::new(0.6, 0.2, 0.8);

const LOWER_VOID: Color = Color::new(0.012, 0.018, 0.055);
const LOWER_HAZE: Color = Color::new(0.090, 0.045, 0.160);
const SUNSET_HORIZON: Color = Color::new(0.760, 0.220, 0.240);
const WARM_MAGENTA: Color = Color::new(0.430, 0.100, 0.300);
const MID_VIOLET: Color = Color::new(0.100, 0.085, 0.270);
const UPPER_NIGHT: Color = Color::new(0.012, 0.022, 0.090);

const SUN_COLOR: Color = Color::new(1.0, 0.62, 0.22);
const SUN_GLOW_COLOR: Color = Color::new(0.96, 0.34, 0.24);
const STAR_COLOR: Color = Color::new(0.78, 0.84, 1.0);

const VERTICAL_BANDS: f32 = 96.0;
const SUN_DISC_MIN_ALIGNMENT: f32 = 0.998_63;
const SUN_GLOW_MIN_ALIGNMENT: f32 = 0.985;
const STAR_MIN_HEIGHT: f32 = 0.22;
const STAR_GRID_SCALE: f32 = 512.0;
const STAR_HASH_MASK: u32 = 0x0fff;
const UNIT_LENGTH_TOLERANCE: f32 = 1.0e-4;

/// Fixed, CPU-sampled sunset environment in the project's world coordinate system.
///
/// `+Y` is world up, so the horizon is derived exclusively from the sampled direction's Y
/// component. The sun and stars therefore remain fixed while the camera orbits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Environment {
    sun_direction: Vec3,
    star_seed: u32,
}

impl Environment {
    pub fn sunset() -> Self {
        Self::try_new(DEFAULT_SUN_DIRECTION, DEFAULT_STAR_SEED)
            .expect("the built-in sunset environment must be valid")
    }

    pub fn try_new(sun_direction: Vec3, star_seed: u32) -> Option<Self> {
        Some(Self {
            sun_direction: sun_direction.try_normalized()?,
            star_seed,
        })
    }

    pub const fn sun_direction(self) -> Vec3 {
        self.sun_direction
    }

    pub const fn star_seed(self) -> u32 {
        self.star_seed
    }

    /// Samples the environment from a world-space direction without allocation or mutable state.
    pub fn sample(&self, direction: Vec3) -> Color {
        let Some(direction) = normalized_direction(direction) else {
            return Color::BLACK;
        };

        let quantized_height = quantize_height(direction.y);
        let mut color = vertical_gradient(quantized_height);

        let sun_alignment = direction.dot(self.sun_direction).clamp(-1.0, 1.0);
        if sun_alignment >= SUN_DISC_MIN_ALIGNMENT {
            return SUN_COLOR;
        }
        if sun_alignment > SUN_GLOW_MIN_ALIGNMENT {
            let glow = smoothstep(
                SUN_GLOW_MIN_ALIGNMENT,
                SUN_DISC_MIN_ALIGNMENT,
                sun_alignment,
            );
            color = color.lerp(SUN_GLOW_COLOR, glow * 0.42);
        }

        let star = star_intensity(direction, self.star_seed);
        color.lerp(STAR_COLOR, star)
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::sunset()
    }
}

fn normalized_direction(direction: Vec3) -> Option<Vec3> {
    if !direction.is_finite() {
        return None;
    }

    let length_squared = direction.length_squared();
    if !length_squared.is_finite() || length_squared <= 0.0 {
        return None;
    }

    if (length_squared - 1.0).abs() <= UNIT_LENGTH_TOLERANCE {
        Some(direction)
    } else {
        direction.try_normalized()
    }
}

fn quantize_height(height: f32) -> f32 {
    let normalized = (height.clamp(-1.0, 1.0) + 1.0) * 0.5;
    let band = (normalized * VERTICAL_BANDS).floor() / VERTICAL_BANDS;
    band * 2.0 - 1.0
}

fn vertical_gradient(height: f32) -> Color {
    if height < -0.15 {
        remapped_lerp(LOWER_VOID, LOWER_HAZE, height, -1.0, -0.15)
    } else if height < 0.02 {
        remapped_lerp(LOWER_HAZE, SUNSET_HORIZON, height, -0.15, 0.02)
    } else if height < 0.18 {
        remapped_lerp(SUNSET_HORIZON, WARM_MAGENTA, height, 0.02, 0.18)
    } else if height < 0.55 {
        remapped_lerp(WARM_MAGENTA, MID_VIOLET, height, 0.18, 0.55)
    } else {
        remapped_lerp(MID_VIOLET, UPPER_NIGHT, height, 0.55, 1.0)
    }
}

fn remapped_lerp(start: Color, end: Color, value: f32, minimum: f32, maximum: f32) -> Color {
    let amount = ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0);
    start.lerp(end, amount)
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let amount = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    amount * amount * (3.0 - 2.0 * amount)
}

fn star_intensity(direction: Vec3, seed: u32) -> f32 {
    if direction.y < STAR_MIN_HEIGHT {
        return 0.0;
    }

    let cell_x = ((direction.x + 1.0) * STAR_GRID_SCALE).floor() as i32;
    let cell_y = ((direction.y + 1.0) * STAR_GRID_SCALE).floor() as i32;
    let cell_z = ((direction.z + 1.0) * STAR_GRID_SCALE).floor() as i32;
    let hash = hash_cell(cell_x, cell_y, cell_z, seed);

    if hash & STAR_HASH_MASK != 0 {
        return 0.0;
    }

    0.62 + ((hash >> 16) & 0xff) as f32 / 255.0 * 0.30
}

fn hash_cell(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut hash = seed;
    hash ^= (x as u32).wrapping_mul(0x8DA6_B343);
    hash ^= (y as u32).wrapping_mul(0xD816_3841);
    hash ^= (z as u32).wrapping_mul(0xCB1A_B31F);
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x7FEB_352D);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x846C_A68B);
    hash ^ (hash >> 16)
}

#[cfg(test)]
mod tests {
    use super::{Environment, STAR_MIN_HEIGHT, star_intensity};
    use crate::{color::Color, math::Vec3};

    fn direction(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z).try_normalized().unwrap()
    }

    fn assert_finite(color: Color) {
        assert!(color.is_finite());
    }

    #[test]
    fn vertical_regions_have_distinct_colors() {
        let environment = Environment::sunset();
        let upper = environment.sample(direction(-0.4, 0.9, -0.3));
        let horizon = environment.sample(direction(-0.4, 0.0, -0.3));
        let lower = environment.sample(direction(-0.4, -0.9, -0.3));

        assert_ne!(upper, horizon);
        assert_ne!(horizon, lower);
        assert_ne!(upper, lower);
    }

    #[test]
    fn repeated_and_equivalent_direction_samples_are_deterministic() {
        let environment = Environment::sunset();
        let sample_direction = Vec3::new(-0.3, 0.7, -0.6);

        let first = environment.sample(sample_direction);
        assert_eq!(environment.sample(sample_direction), first);
        assert_eq!(environment.sample(sample_direction * 4.0), first);
    }

    #[test]
    fn sun_disc_glow_and_opposite_direction_are_distinct() {
        let environment = Environment::sunset();
        let sun_direction = environment.sun_direction();
        let offset = sun_direction
            .cross(Vec3::new(0.0, 1.0, 0.0))
            .try_normalized()
            .unwrap();
        let outside_disc = (sun_direction + offset * 0.08).try_normalized().unwrap();

        let disc = environment.sample(sun_direction);
        let nearby = environment.sample(outside_disc);
        let opposite = environment.sample(-sun_direction);

        assert_eq!(disc, Color::new(1.0, 0.62, 0.22));
        assert_ne!(nearby, disc);
        assert_ne!(opposite, disc);
    }

    #[test]
    fn procedural_stars_are_deterministic_when_a_star_cell_is_found() {
        let environment = Environment::sunset();
        let mut found = None;

        'search: for x in -96..=96 {
            for z in -96..=96 {
                let candidate = direction(x as f32 / 96.0, 0.75, z as f32 / 96.0);
                if star_intensity(candidate, environment.star_seed()) > 0.0 {
                    found = Some(candidate);
                    break 'search;
                }
            }
        }

        let star_direction = found.expect("fixed seed must produce an upper-sky star");
        let first = environment.sample(star_direction);
        assert_eq!(environment.sample(star_direction), first);
    }

    #[test]
    fn stars_are_excluded_from_horizon_and_lower_hemisphere() {
        let environment = Environment::sunset();
        for y in [-1.0, -0.3, 0.0, STAR_MIN_HEIGHT - 0.01] {
            for x in -16..=16 {
                let candidate = direction(x as f32 / 16.0, y, -0.5);
                assert_eq!(star_intensity(candidate, environment.star_seed()), 0.0);
            }
        }
    }

    #[test]
    fn invalid_directions_return_finite_black() {
        let environment = Environment::sunset();
        for invalid in [
            Vec3::ZERO,
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(f32::INFINITY, 0.0, 0.0),
        ] {
            let color = environment.sample(invalid);
            assert_eq!(color, Color::BLACK);
            assert_finite(color);
        }
    }

    #[test]
    fn valid_environment_samples_remain_finite() {
        let environment = Environment::sunset();
        for sample_direction in [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            environment.sun_direction(),
        ] {
            assert_finite(environment.sample(sample_direction));
        }
    }
}
