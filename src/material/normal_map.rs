//! Tangent-space normal-map decoding and the per-face tangent-to-world transform.
//!
//! A normal map perturbs the *shading* normal only. The geometric normal of the AABB face stays
//! authoritative for intersection, UVs, ray origins, and medium classification.
//!
//! # Convention
//!
//! Texels store `n * 0.5 + 0.5` per channel, so `(0.5, 0.5, 1.0)` is the flat normal. Tangent
//! space is `+X = increasing u` (image right), `+Y = increasing v` (image down), `+Z` = outward
//! geometric normal. A surface `P + h * N` has perturbed normal `(-dh/du, -dh/dv, 1)`, which is
//! what `scripts/prepare_assets.py` encodes from cobblestone luminance. The world-space result is
//! `T * n.x + B * n.y + N * n.z` with `T`, `B` from [`CubeFace::tangent`] and
//! [`CubeFace::bitangent`], which follow the project's UV mapping.

use crate::{color::Color, geometry::CubeFace, math::Vec3};

/// Smallest accepted tangent-space `z` after decoding and normalizing.
///
/// A height-derived map always has a clearly positive `z`. Anything at or below this is within
/// about 3 degrees of the surface plane (or pointing into it), which would shade as a grazing
/// sliver or turn the surface inside out, so it is treated as corrupt. Because the result is
/// `N * n.z` plus tangential terms, any accepted normal stays in the geometric hemisphere.
pub const MIN_TANGENT_NORMAL_Z: f32 = 0.05;

/// Decodes a sampled normal-map texel into a unit tangent-space normal.
///
/// Returns `None` for non-finite or degenerate texels and for those that do not face out of the
/// surface (`z <= MIN_TANGENT_NORMAL_Z`). Nothing is clamped: a bad texel is rejected whole.
pub fn decode_tangent_normal(sample: Color) -> Option<Vec3> {
    let decoded = Vec3::new(
        sample.r * 2.0 - 1.0,
        sample.g * 2.0 - 1.0,
        sample.b * 2.0 - 1.0,
    );
    let normal = decoded.try_normalized()?;

    (normal.z > MIN_TANGENT_NORMAL_Z).then_some(normal)
}

/// Transforms a unit tangent-space normal into a unit world-space normal on `face`.
pub fn tangent_to_world(face: CubeFace, tangent_normal: Vec3) -> Option<Vec3> {
    (face.tangent() * tangent_normal.x
        + face.bitangent() * tangent_normal.y
        + face.normal() * tangent_normal.z)
        .try_normalized()
}

/// World-space shading normal for a normal-map texel sampled on `face`.
///
/// Falls back to the face's geometric normal when the texel is invalid, so a corrupt texel can
/// never produce NaN or an inward-facing normal.
pub fn shading_normal(face: CubeFace, sample: Color) -> Vec3 {
    decode_tangent_normal(sample)
        .and_then(|normal| tangent_to_world(face, normal))
        .unwrap_or_else(|| face.normal())
}

#[cfg(test)]
mod tests {
    use super::{MIN_TANGENT_NORMAL_Z, decode_tangent_normal, shading_normal, tangent_to_world};
    use crate::{
        color::Color,
        geometry::{Aabb, CubeFace},
        material::load_ppm,
        math::Vec3,
        ray::Ray,
    };
    use std::path::PathBuf;

    const EPSILON: f32 = 1.0e-5;
    const ALL_FACES: [CubeFace; 6] = [
        CubeFace::NegativeX,
        CubeFace::PositiveX,
        CubeFace::NegativeY,
        CubeFace::PositiveY,
        CubeFace::NegativeZ,
        CubeFace::PositiveZ,
    ];
    const FLAT: Color = Color::new(0.5, 0.5, 1.0);

    fn assert_vec_approx_eq(actual: Vec3, expected: Vec3) {
        assert!(
            (actual - expected).length() <= EPSILON,
            "{actual:?} != {expected:?}"
        );
    }

    fn encode(normal: Vec3) -> Color {
        let unit = normal.try_normalized().unwrap();
        Color::new(unit.x * 0.5 + 0.5, unit.y * 0.5 + 0.5, unit.z * 0.5 + 0.5)
    }

    /// Expected world directions per face, written out from the UV convention of
    /// `Aabb::intersect` rather than from `CubeFace`, so accidental mirroring is caught.
    /// `(face, outward normal, increasing u, increasing v)`.
    const EXPECTED_BASES: [(CubeFace, Vec3, Vec3, Vec3); 6] = [
        // u = 1 - z, v = 1 - y
        (
            CubeFace::PositiveX,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, -1.0, 0.0),
        ),
        // u = z, v = 1 - y
        (
            CubeFace::NegativeX,
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, -1.0, 0.0),
        ),
        // u = x, v = z
        (
            CubeFace::PositiveY,
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ),
        // u = x, v = 1 - z
        (
            CubeFace::NegativeY,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
        ),
        // u = x, v = 1 - y
        (
            CubeFace::PositiveZ,
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        ),
        // u = 1 - x, v = 1 - y
        (
            CubeFace::NegativeZ,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        ),
    ];

    #[test]
    fn flat_texel_decodes_to_the_tangent_space_up_axis() {
        // 8-bit flat is (128, 128, 255): x and y are only a quantization step off zero.
        let flat = decode_tangent_normal(Color::new(128.0 / 255.0, 128.0 / 255.0, 1.0)).unwrap();
        assert!(flat.x.abs() < 0.01 && flat.y.abs() < 0.01 && (flat.z - 1.0).abs() < 1.0e-3);

        assert_vec_approx_eq(
            decode_tangent_normal(FLAT).unwrap(),
            Vec3::new(0.0, 0.0, 1.0),
        );
    }

    #[test]
    fn decoded_normals_lean_toward_their_encoded_axes() {
        let right = decode_tangent_normal(encode(Vec3::new(1.0, 0.0, 1.0))).unwrap();
        let left = decode_tangent_normal(encode(Vec3::new(-1.0, 0.0, 1.0))).unwrap();
        let down = decode_tangent_normal(encode(Vec3::new(0.0, 1.0, 1.0))).unwrap();
        let up = decode_tangent_normal(encode(Vec3::new(0.0, -1.0, 1.0))).unwrap();

        assert!(right.x > 0.5 && right.y.abs() < EPSILON);
        assert!(left.x < -0.5 && left.y.abs() < EPSILON);
        assert!(down.y > 0.5 && down.x.abs() < EPSILON);
        assert!(up.y < -0.5 && up.x.abs() < EPSILON);
    }

    #[test]
    fn decoded_normals_are_finite_and_unit_length() {
        for sample in [
            FLAT,
            Color::new(0.9, 0.2, 0.8),
            Color::new(0.1, 0.7, 0.6),
            Color::new(0.5, 0.5, 0.75),
        ] {
            let normal = decode_tangent_normal(sample).unwrap();
            assert!(normal.is_finite());
            assert!((normal.length() - 1.0).abs() <= EPSILON);
        }
    }

    #[test]
    fn invalid_texels_are_rejected_rather_than_clamped() {
        for sample in [
            Color::new(0.5, 0.5, 0.5),  // zero vector
            Color::new(0.5, 0.5, 0.0),  // points straight into the surface
            Color::new(1.0, 0.5, 0.5),  // in the tangent plane
            Color::new(1.0, 0.5, 0.4),  // slightly into the surface
            Color::new(0.9, 0.5, 0.51), // below the minimum z
            Color::new(f32::NAN, 0.5, 1.0),
            Color::new(0.5, f32::INFINITY, 1.0),
            Color::new(0.5, 0.5, f32::NEG_INFINITY),
        ] {
            assert_eq!(decode_tangent_normal(sample), None, "{sample:?}");
        }
        assert!(MIN_TANGENT_NORMAL_Z > 0.0);
    }

    #[test]
    fn basis_matches_the_table_for_every_face() {
        for (face, normal, tangent, bitangent) in EXPECTED_BASES {
            assert_eq!(face.normal(), normal, "{face:?} normal");
            assert_eq!(face.tangent(), tangent, "{face:?} tangent");
            assert_eq!(face.bitangent(), bitangent, "{face:?} bitangent");
        }
    }

    #[test]
    fn basis_is_orthonormal_for_every_face() {
        for face in ALL_FACES {
            let (n, t, b) = (face.normal(), face.tangent(), face.bitangent());
            assert!((t.length() - 1.0).abs() <= EPSILON);
            assert!((b.length() - 1.0).abs() <= EPSILON);
            assert!(n.dot(t).abs() <= EPSILON, "{face:?}");
            assert!(n.dot(b).abs() <= EPSILON, "{face:?}");
            assert!(t.dot(b).abs() <= EPSILON, "{face:?}");
        }
    }

    /// Hits `face` of a translated, non-unit box at two nearby points and checks that the world
    /// displacement follows the UV displacement along the declared tangent and bitangent.
    #[test]
    fn basis_follows_the_real_uv_mapping_on_every_face() {
        let bounds = Aabb::try_new(Vec3::new(1.0, 2.0, 3.0), Vec3::new(3.0, 3.5, 4.0)).unwrap();
        let center = Vec3::new(2.0, 2.75, 3.5);
        let step = 0.1;

        for face in ALL_FACES {
            let surface_hit = |offset: Vec3| {
                let normal = face.normal();
                let origin = center + normal * 3.0 + offset;
                bounds
                    .intersect(Ray::try_new(origin, -normal).unwrap(), 0.0, f32::INFINITY)
                    .unwrap()
            };
            let base = surface_hit(Vec3::ZERO);
            assert_eq!(base.face, face);

            for (direction, increasing_u) in [(face.tangent(), true), (face.bitangent(), false)] {
                let moved = surface_hit(direction * step);
                let (du, dv) = (
                    moved.uv.unwrap().u - base.uv.unwrap().u,
                    moved.uv.unwrap().v - base.uv.unwrap().v,
                );
                let (along, across) = if increasing_u { (du, dv) } else { (dv, du) };
                assert!(along > 0.0, "{face:?}: texture coordinate must increase");
                assert!(
                    across.abs() <= EPSILON,
                    "{face:?}: other coordinate must not move"
                );
            }
        }
    }

    #[test]
    fn flat_normal_transforms_to_the_geometric_normal() {
        for face in ALL_FACES {
            let world = tangent_to_world(face, Vec3::new(0.0, 0.0, 1.0)).unwrap();
            assert_vec_approx_eq(world, face.normal());
            assert_vec_approx_eq(shading_normal(face, FLAT), face.normal());
        }
    }

    #[test]
    fn tangent_leans_transform_toward_the_expected_world_directions() {
        let lean = 0.5_f32;
        let up = (1.0 - lean * lean).sqrt();

        for (face, normal, tangent, bitangent) in EXPECTED_BASES {
            for (tangent_space, expected_axis) in [
                (Vec3::new(lean, 0.0, up), tangent),
                (Vec3::new(-lean, 0.0, up), -tangent),
                (Vec3::new(0.0, lean, up), bitangent),
                (Vec3::new(0.0, -lean, up), -bitangent),
            ] {
                let world = tangent_to_world(face, tangent_space).unwrap();
                assert_vec_approx_eq(world, expected_axis * lean + normal * up);
                assert!((world.length() - 1.0).abs() <= EPSILON);
                assert!(world.dot(expected_axis) > 0.0, "{face:?}");
                assert!(
                    world.dot(normal) > 0.0,
                    "{face:?}: left the geometric hemisphere"
                );

                // The same result arrives through the encoded-texel path.
                assert_vec_approx_eq(shading_normal(face, encode(tangent_space)), world);
            }
        }
    }

    #[test]
    fn shading_normal_falls_back_to_the_geometric_normal_for_bad_texels() {
        for face in ALL_FACES {
            for sample in [
                Color::new(0.5, 0.5, 0.5),
                Color::new(0.5, 0.5, 0.0),
                Color::new(f32::NAN, f32::NAN, f32::NAN),
            ] {
                assert_eq!(shading_normal(face, sample), face.normal(), "{face:?}");
            }
        }
    }

    #[test]
    fn every_accepted_texel_stays_in_the_geometric_hemisphere() {
        for face in ALL_FACES {
            for r in 0..=16 {
                for g in 0..=16 {
                    for b in 0..=16 {
                        let sample = Color::new(r as f32 / 16.0, g as f32 / 16.0, b as f32 / 16.0);
                        let world = shading_normal(face, sample);
                        assert!(world.is_finite());
                        assert!((world.length() - 1.0).abs() <= 1.0e-4);
                        assert!(world.dot(face.normal()) > 0.0, "{face:?} {sample:?}");
                    }
                }
            }
        }
    }

    // --- Generated cobblestone asset -----------------------------------------------------------

    fn texture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets/textures")
            .join(name)
    }

    fn bytes(color: Color) -> [u8; 3] {
        color.to_rgb8()
    }

    #[test]
    fn generated_cobblestone_normal_map_is_16x16() {
        let map = load_ppm(&texture_path("cobblestone_normal.ppm")).unwrap();
        let source = load_ppm(&texture_path("cobblestone.ppm")).unwrap();

        assert_eq!((map.width(), map.height()), (16, 16));
        assert_eq!((source.width(), source.height()), (16, 16));
    }

    #[test]
    fn generated_normals_are_finite_unit_and_face_outward() {
        let map = load_ppm(&texture_path("cobblestone_normal.ppm")).unwrap();
        let mut tilted = 0;

        for y in 0..16 {
            for x in 0..16 {
                let texel = map.texel(x, y).unwrap();
                let normal = decode_tangent_normal(texel)
                    .unwrap_or_else(|| panic!("texel ({x}, {y}) must decode"));

                assert!(normal.is_finite());
                assert!((normal.length() - 1.0).abs() <= EPSILON);
                assert!(normal.z > 0.2, "({x}, {y}) leans too far: {normal:?}");
                tilted += usize::from(normal.x.abs() > 0.1 || normal.y.abs() > 0.1);
            }
        }
        // Not a flat map: cobblestone has a lot of relief.
        assert!(tilted > 128, "only {tilted} of 256 texels are tilted");
    }

    /// Re-derives the map in test code from the committed cobblestone texture, mirroring the
    /// documented convention of `prepare_assets.py`: Rec. 709 luminance height, wrapped central
    /// differences, strength 2, `-strength * d` tilt, `n * 0.5 + 0.5` encoding. It checks that the
    /// committed asset follows that derivation (within one encoding step) and that the derivation
    /// wraps at the borders instead of clamping.
    #[test]
    fn committed_normal_map_follows_the_documented_derivation() {
        const STRENGTH: f64 = 2.0;
        let source = load_ppm(&texture_path("cobblestone.ppm")).unwrap();
        let map = load_ppm(&texture_path("cobblestone_normal.ppm")).unwrap();
        let height = |x: i32, y: i32| {
            let texel = bytes(
                source
                    .texel(x.rem_euclid(16) as usize, y.rem_euclid(16) as usize)
                    .unwrap(),
            );
            (0.2126 * f64::from(texel[0])
                + 0.7152 * f64::from(texel[1])
                + 0.0722 * f64::from(texel[2]))
                / 255.0
        };

        for y in 0..16 {
            for x in 0..16 {
                let nx = -STRENGTH * (height(x + 1, y) - height(x - 1, y));
                let ny = -STRENGTH * (height(x, y + 1) - height(x, y - 1));
                let inverse_length = 1.0 / (nx * nx + ny * ny + 1.0).sqrt();
                let expected = [nx, ny, 1.0].map(|component| {
                    ((component * inverse_length * 0.5 + 0.5) * 255.0 + 0.5).floor()
                });
                let actual = bytes(map.texel(x as usize, y as usize).unwrap());

                for channel in 0..3 {
                    assert!(
                        (f64::from(actual[channel]) - expected[channel]).abs() <= 1.0,
                        "texel ({x}, {y}) channel {channel}: {} vs {}",
                        actual[channel],
                        expected[channel]
                    );
                }
            }
        }
    }
}
