/// Fraction of radiance reflected at a dielectric interface, by Schlick's approximation.
///
/// Light travels from a medium of index `eta_incident` into one of index `eta_transmitted`:
///
/// ```text
/// R0 = ((eta_i - eta_t) / (eta_i + eta_t))²
/// F  = R0 + (1 - R0)(1 - cos_theta)⁵
/// ```
///
/// `cos_theta` is the incidence cosine, measured against the interface normal oriented toward
/// the incident side; it is clamped to `[0, 1]`, so `1` is normal incidence (`F = R0`) and `0`
/// is grazing incidence (`F = 1`). The result always lies in `[R0, 1]`, a subset of `[0, 1]`.
/// `R0` is symmetric in the two indices, so air-to-glass and glass-to-air share it.
///
/// Returns `None` for a non-finite `cos_theta`, or for an index that is not finite and strictly
/// positive, the same rule `Material::with_ior` enforces. Total internal reflection is not
/// detected here; callers route it to full reflection before consulting Fresnel.
pub fn schlick_reflectance(cos_theta: f32, eta_incident: f32, eta_transmitted: f32) -> Option<f32> {
    if !cos_theta.is_finite() || !is_valid_ior(eta_incident) || !is_valid_ior(eta_transmitted) {
        return None;
    }

    let r0 = normal_incidence_reflectance(eta_incident, eta_transmitted);
    let m = 1.0 - cos_theta.clamp(0.0, 1.0);
    let m2 = m * m;

    Some(r0 + (1.0 - r0) * m2 * m2 * m)
}

/// Schlick's `R0`: reflectance at normal incidence between two valid indices.
fn normal_incidence_reflectance(eta_incident: f32, eta_transmitted: f32) -> f32 {
    let ratio = (eta_incident - eta_transmitted) / (eta_incident + eta_transmitted);
    ratio * ratio
}

fn is_valid_ior(ior: f32) -> bool {
    ior.is_finite() && ior > 0.0
}

#[cfg(test)]
mod tests {
    use super::{normal_incidence_reflectance, schlick_reflectance};
    use crate::material::{AIR_IOR, GLASS_IOR};

    // Exact R0 for 1.0/1.5 is 0.04; f32 rounding stays far inside this tolerance.
    const R0_TOLERANCE: f32 = 1.0e-6;

    fn reflectance(cos_theta: f32, eta_incident: f32, eta_transmitted: f32) -> f32 {
        schlick_reflectance(cos_theta, eta_incident, eta_transmitted).unwrap()
    }

    #[test]
    fn air_to_glass_normal_incidence_reflects_about_four_percent() {
        assert!((normal_incidence_reflectance(AIR_IOR, GLASS_IOR) - 0.04).abs() < R0_TOLERANCE);
        assert!((reflectance(1.0, AIR_IOR, GLASS_IOR) - 0.04).abs() < R0_TOLERANCE);
    }

    #[test]
    fn glass_to_air_normal_incidence_shares_the_same_reflectance() {
        assert!((reflectance(1.0, GLASS_IOR, AIR_IOR) - 0.04).abs() < R0_TOLERANCE);
        assert_eq!(
            reflectance(1.0, GLASS_IOR, AIR_IOR),
            reflectance(1.0, AIR_IOR, GLASS_IOR)
        );
    }

    #[test]
    fn reflectance_increases_as_incidence_becomes_more_grazing() {
        for (eta_i, eta_t) in [(AIR_IOR, GLASS_IOR), (GLASS_IOR, AIR_IOR), (1.0, 2.4)] {
            let mut previous = reflectance(1.0, eta_i, eta_t);
            for step in 1..=20 {
                let cos_theta = 1.0 - step as f32 / 20.0;
                let current = reflectance(cos_theta, eta_i, eta_t);
                assert!(current > previous, "{cos_theta}: {current} <= {previous}");
                previous = current;
            }
        }
    }

    #[test]
    fn reflectance_approaches_one_at_grazing_incidence() {
        assert_eq!(reflectance(0.0, AIR_IOR, GLASS_IOR), 1.0);
        assert!(reflectance(1.0e-3, AIR_IOR, GLASS_IOR) > 0.99);
        assert!(reflectance(0.05, AIR_IOR, GLASS_IOR) > 0.75);
        // Sixty degrees off the normal is still mostly transmissive.
        assert!(reflectance(0.5, AIR_IOR, GLASS_IOR) < 0.1);
    }

    #[test]
    fn reflectance_stays_finite_and_inside_the_unit_interval() {
        for cos_theta in [
            -1.0e9, -1.0, -1.0e-7, 0.0, 0.3, 0.999_999, 1.0, 1.000_001, 7.0,
        ] {
            for (eta_i, eta_t) in [
                (AIR_IOR, GLASS_IOR),
                (GLASS_IOR, AIR_IOR),
                (1.0e-6, 1.0e6),
                (f32::MAX, f32::MIN_POSITIVE),
                (1.33, 1.33),
            ] {
                let f = reflectance(cos_theta, eta_i, eta_t);
                assert!(f.is_finite() && (0.0..=1.0).contains(&f), "{f}");
            }
        }
        // Out-of-range cosines clamp to the nearest valid incidence.
        assert_eq!(reflectance(-0.5, AIR_IOR, GLASS_IOR), 1.0);
        assert_eq!(
            reflectance(1.5, AIR_IOR, GLASS_IOR),
            reflectance(1.0, AIR_IOR, GLASS_IOR)
        );
    }

    #[test]
    fn matched_indices_do_not_reflect_at_normal_incidence() {
        for ior in [AIR_IOR, 1.33, GLASS_IOR] {
            assert_eq!(reflectance(1.0, ior, ior), 0.0);
        }
    }

    #[test]
    fn rejects_non_finite_cosine_and_invalid_indices() {
        for cos_theta in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(schlick_reflectance(cos_theta, AIR_IOR, GLASS_IOR), None);
        }
        for invalid in [0.0, -0.0, -1.5, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(schlick_reflectance(1.0, invalid, GLASS_IOR), None);
            assert_eq!(schlick_reflectance(1.0, AIR_IOR, invalid), None);
        }
    }
}
