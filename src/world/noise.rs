//! Stateless deterministic hashing and smooth value noise for procedural generation.
//!
//! Every random decision is a pure function of `(seed, domain, integer coordinates)`. There is no
//! RNG stream, so no feature depends on how many values another feature consumed or in which
//! order cells are visited. Each feature owns a `Domain` salt: adding a feature, or sampling an
//! existing one more or less often, never changes the values any other domain produces.
//!
//! Noise uses only `+`, `-`, `*`, and `floor` on `f32` over hashed lattice values, all of which
//! IEEE 754 rounds exactly, so a given seed yields bit-identical noise on every platform. No
//! transcendental `libm` function (whose last bit may differ between platforms) is involved.

/// Feature-specific salt that keeps one procedural feature's randomness independent of another's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Domain(u64);

impl Domain {
    /// A domain named by an eight-byte ASCII tag, so salts are distinct and readable.
    pub const fn from_tag(tag: [u8; 8]) -> Self {
        Self(u64::from_be_bytes(tag))
    }
}

/// SplitMix64's increment; added before every mix so zero inputs do not stay at zero.
const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// SplitMix64's finalizer: a bijective avalanche mix of 64 bits.
const fn mix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

/// Hash of a seed, a domain, and an integer lattice coordinate.
///
/// Each input is folded in with a full avalanche mix, so neighboring coordinates, seeds, and
/// domains produce unrelated values.
pub const fn hash3(seed: u64, domain: Domain, x: i32, y: i32, z: i32) -> u64 {
    let mut hash = mix64(seed.wrapping_add(GOLDEN_GAMMA) ^ domain.0);
    hash = mix64(hash.wrapping_add(GOLDEN_GAMMA) ^ x as u32 as u64);
    hash = mix64(hash.wrapping_add(GOLDEN_GAMMA) ^ y as u32 as u64);
    mix64(hash.wrapping_add(GOLDEN_GAMMA) ^ z as u32 as u64)
}

/// Maps a hash to `[0, 1)` using its top 24 bits, which `f32` represents exactly.
pub fn unit_interval(hash: u64) -> f32 {
    (hash >> 40) as f32 * (1.0 / (1u64 << 24) as f32)
}

/// Remaps `[0, 1)` to `[-1, 1)`.
pub fn signed(value: f32) -> f32 {
    value * 2.0 - 1.0
}

/// Cubic smoothstep weight on `[0, 1]`, so noise has no visible creases at lattice lines.
fn fade(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(start: f32, end: f32, amount: f32) -> f32 {
    start + (end - start) * amount
}

/// Smooth 2D value noise in `[0, 1)`: hashed values on the integer lattice, blended bilinearly
/// with smoothstep weights. Adjacent samples closer than one lattice unit are correlated, which is
/// what makes terrain variation coherent instead of independent per cell.
pub fn value_noise_2d(seed: u64, domain: Domain, x: f32, z: f32) -> f32 {
    let x0 = x.floor();
    let z0 = z.floor();
    let (tx, tz) = (fade(x - x0), fade(z - z0));
    let (ix, iz) = (x0 as i32, z0 as i32);
    let corner = |dx: i32, dz: i32| {
        unit_interval(hash3(
            seed,
            domain,
            ix.wrapping_add(dx),
            0,
            iz.wrapping_add(dz),
        ))
    };

    let near = lerp(corner(0, 0), corner(1, 0), tx);
    let far = lerp(corner(0, 1), corner(1, 1), tx);
    lerp(near, far, tz)
}

/// Smooth periodic 1D value noise in `[0, 1)` with `period` lattice points: position `t` and
/// `t + period` agree, so it can vary a closed outline without a seam.
///
/// `t` is expected in `[0, period)`; `period` must be positive.
pub fn periodic_noise_1d(seed: u64, domain: Domain, t: f32, period: u32) -> f32 {
    debug_assert!(
        period > 0,
        "periodic noise needs at least one lattice point"
    );
    let t0 = t.floor();
    let index = (t0 as i64).rem_euclid(i64::from(period)) as u32;
    let next = (index + 1) % period;
    let value = |i: u32| unit_interval(hash3(seed, domain, i as i32, 0, 0));
    lerp(value(index), value(next), fade(t - t0))
}

/// Monotonic stand-in for the polar angle of `(x, z)`, as a fraction of a full turn in `[0, 1)`.
///
/// This is the "diamond angle": it uses one division instead of `atan2`, keeping noise sampled
/// along it exactly reproducible across platforms. It is not uniform in true angle, which does not
/// matter for an organic outline. The origin maps to `0`.
pub fn turn_fraction(x: f32, z: f32) -> f32 {
    let sum = x.abs() + z.abs();
    if sum == 0.0 {
        return 0.0;
    }
    let quadrant = if z >= 0.0 {
        if x >= 0.0 { z / sum } else { 1.0 - x / sum }
    } else if x < 0.0 {
        2.0 - z / sum
    } else {
        3.0 + x / sum
    };
    (quadrant * 0.25).clamp(0.0, 1.0 - f32::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::{
        Domain, hash3, periodic_noise_1d, signed, turn_fraction, unit_interval, value_noise_2d,
    };

    const DOMAIN: Domain = Domain::from_tag(*b"testdom1");
    const OTHER: Domain = Domain::from_tag(*b"testdom2");

    #[test]
    fn hash_is_a_pure_function_of_its_inputs() {
        assert_eq!(hash3(7, DOMAIN, 1, -2, 3), hash3(7, DOMAIN, 1, -2, 3));
        // Changing any single input changes the hash.
        let base = hash3(7, DOMAIN, 1, -2, 3);
        assert_ne!(base, hash3(8, DOMAIN, 1, -2, 3));
        assert_ne!(base, hash3(7, OTHER, 1, -2, 3));
        assert_ne!(base, hash3(7, DOMAIN, 2, -2, 3));
        assert_ne!(base, hash3(7, DOMAIN, 1, -1, 3));
        assert_ne!(base, hash3(7, DOMAIN, 1, -2, 4));
    }

    #[test]
    fn zero_and_maximum_seeds_hash_normally() {
        let zero = hash3(0, DOMAIN, 0, 0, 0);
        let max = hash3(u64::MAX, DOMAIN, 0, 0, 0);
        assert_ne!(zero, 0);
        assert_ne!(max, 0);
        assert_ne!(zero, max);
        assert_ne!(zero, hash3(0, DOMAIN, 1, 0, 0));
    }

    #[test]
    fn domains_do_not_share_values() {
        let matches = (0..256)
            .filter(|&x| hash3(42, DOMAIN, x, 0, 0) == hash3(42, OTHER, x, 0, 0))
            .count();
        assert_eq!(matches, 0);
    }

    #[test]
    fn unit_interval_spans_zero_to_below_one() {
        assert_eq!(unit_interval(0), 0.0);
        assert!(unit_interval(u64::MAX) < 1.0);
        assert!(unit_interval(u64::MAX) > 0.999);
        assert_eq!(signed(0.0), -1.0);
        assert_eq!(signed(0.5), 0.0);
    }

    #[test]
    fn value_noise_matches_lattice_values_and_stays_in_range() {
        for (x, z) in [(0, 0), (3, -5), (-7, 2)] {
            let lattice = unit_interval(hash3(9, DOMAIN, x, 0, z));
            assert_eq!(value_noise_2d(9, DOMAIN, x as f32, z as f32), lattice);
        }
        for step in 0..400 {
            let x = step as f32 * 0.137 - 20.0;
            let z = step as f32 * -0.071 + 4.0;
            let value = value_noise_2d(9, DOMAIN, x, z);
            assert!((0.0..1.0).contains(&value), "{value}");
        }
    }

    #[test]
    fn value_noise_is_continuous() {
        // A small step moves the value only slightly: the noise has no per-cell jumps.
        for step in 0..200 {
            let x = step as f32 * 0.05 - 5.0;
            let a = value_noise_2d(3, DOMAIN, x, 1.3);
            let b = value_noise_2d(3, DOMAIN, x + 0.01, 1.3);
            assert!((a - b).abs() < 0.05, "{x}: {a} vs {b}");
        }
    }

    #[test]
    fn periodic_noise_wraps_without_a_seam() {
        for period in [1, 5, 9] {
            let start = periodic_noise_1d(11, DOMAIN, 0.0, period);
            let almost_end = periodic_noise_1d(11, DOMAIN, period as f32 - 1.0e-4, period);
            assert!((start - almost_end).abs() < 1.0e-3);
        }
    }

    #[test]
    fn turn_fraction_increases_counterclockwise_around_the_origin() {
        let points = [
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (-1.0, 1.0),
            (-1.0, 0.0),
            (-1.0, -1.0),
            (0.0, -1.0),
            (1.0, -1.0),
        ];
        let fractions: Vec<f32> = points.iter().map(|&(x, z)| turn_fraction(x, z)).collect();
        assert_eq!(fractions[0], 0.0);
        assert!(fractions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(fractions.iter().all(|f| (0.0..1.0).contains(f)));
        assert_eq!(turn_fraction(0.0, 0.0), 0.0);
        // Scale does not change the direction.
        assert_eq!(turn_fraction(0.3, -0.2), turn_fraction(3.0, -2.0));
    }
}
