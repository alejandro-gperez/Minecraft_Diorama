use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::{math::Vec3, ray::Ray};

/// Keeps the camera away from the world-up poles where its right vector is undefined.
pub const MAX_ABS_PITCH: f32 = FRAC_PI_2 - 1.0e-3;

/// Keeps camera position distinct from its target and above Vec3's normalization threshold.
pub const MIN_ORBIT_RADIUS: f32 = 1.0e-3;

const WORLD_UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);

/// Orbital camera using +X as right/east, +Y as up, and +Z as forward/south.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitalCamera {
    target: Vec3,
    yaw: f32,
    pitch: f32,
    radius: f32,
    vertical_fov: f32,
    aspect_ratio: f32,
    position: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    viewport_half_width: f32,
    viewport_half_height: f32,
}

impl OrbitalCamera {
    pub fn try_new(
        target: Vec3,
        yaw: f32,
        pitch: f32,
        radius: f32,
        vertical_fov: f32,
        aspect_ratio: f32,
    ) -> Option<Self> {
        if !target.is_finite()
            || !yaw.is_finite()
            || !pitch.is_finite()
            || !radius.is_finite()
            || !vertical_fov.is_finite()
            || !aspect_ratio.is_finite()
            || pitch.abs() > MAX_ABS_PITCH
            || radius < MIN_ORBIT_RADIUS
            || vertical_fov <= 0.0
            || vertical_fov >= PI
            || aspect_ratio <= 0.0
        {
            return None;
        }

        let yaw = yaw.rem_euclid(TAU);
        let derived =
            DerivedCamera::try_new(target, yaw, pitch, radius, vertical_fov, aspect_ratio)?;

        Some(Self {
            target,
            yaw,
            pitch,
            radius,
            vertical_fov,
            aspect_ratio,
            position: derived.position,
            forward: derived.forward,
            right: derived.right,
            up: derived.up,
            viewport_half_width: derived.viewport_half_width,
            viewport_half_height: derived.viewport_half_height,
        })
    }

    pub const fn target(self) -> Vec3 {
        self.target
    }

    pub const fn yaw(self) -> f32 {
        self.yaw
    }

    pub const fn pitch(self) -> f32 {
        self.pitch
    }

    pub const fn radius(self) -> f32 {
        self.radius
    }

    pub const fn vertical_fov(self) -> f32 {
        self.vertical_fov
    }

    pub const fn aspect_ratio(self) -> f32 {
        self.aspect_ratio
    }

    pub const fn position(self) -> Vec3 {
        self.position
    }

    pub const fn forward(self) -> Vec3 {
        self.forward
    }

    pub const fn right(self) -> Vec3 {
        self.right
    }

    pub const fn up(self) -> Vec3 {
        self.up
    }

    /// Generates a perspective ray where u runs left-to-right and v runs top-to-bottom.
    pub fn ray_for_viewport(&self, u: f32, v: f32) -> Option<Ray> {
        if !u.is_finite()
            || !v.is_finite()
            || !(0.0..=1.0).contains(&u)
            || !(0.0..=1.0).contains(&v)
        {
            return None;
        }

        let horizontal_offset = (2.0 * u - 1.0) * self.viewport_half_width;
        let vertical_offset = (1.0 - 2.0 * v) * self.viewport_half_height;
        let direction = self.forward + self.right * horizontal_offset + self.up * vertical_offset;

        Ray::try_new(self.position, direction)
    }

    pub fn orbit_yaw(&mut self, delta_radians: f32) {
        if !delta_radians.is_finite() {
            return;
        }

        let yaw = (self.yaw + delta_radians.rem_euclid(TAU)).rem_euclid(TAU);
        self.try_apply_orbit(yaw, self.pitch, self.radius);
    }

    pub fn orbit_pitch(&mut self, delta_radians: f32) {
        if !delta_radians.is_finite() {
            return;
        }

        let pitch = (self.pitch as f64 + delta_radians as f64)
            .clamp(-(MAX_ABS_PITCH as f64), MAX_ABS_PITCH as f64) as f32;
        self.try_apply_orbit(self.yaw, pitch, self.radius);
    }

    /// Adjusts orbit radius; positive values zoom out and negative values zoom in.
    pub fn adjust_radius(&mut self, delta_radius: f32) {
        if !delta_radius.is_finite() {
            return;
        }

        let radius = (self.radius as f64 + delta_radius as f64)
            .clamp(MIN_ORBIT_RADIUS as f64, f32::MAX as f64) as f32;
        self.try_apply_orbit(self.yaw, self.pitch, radius);
    }

    fn try_apply_orbit(&mut self, yaw: f32, pitch: f32, radius: f32) {
        let Some(derived) = DerivedCamera::try_new(
            self.target,
            yaw,
            pitch,
            radius,
            self.vertical_fov,
            self.aspect_ratio,
        ) else {
            return;
        };

        self.yaw = yaw;
        self.pitch = pitch;
        self.radius = radius;
        self.position = derived.position;
        self.forward = derived.forward;
        self.right = derived.right;
        self.up = derived.up;
        self.viewport_half_width = derived.viewport_half_width;
        self.viewport_half_height = derived.viewport_half_height;
    }
}

struct DerivedCamera {
    position: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    viewport_half_width: f32,
    viewport_half_height: f32,
}

impl DerivedCamera {
    fn try_new(
        target: Vec3,
        yaw: f32,
        pitch: f32,
        radius: f32,
        vertical_fov: f32,
        aspect_ratio: f32,
    ) -> Option<Self> {
        let (yaw_sin, yaw_cos) = yaw.sin_cos();
        let (pitch_sin, pitch_cos) = pitch.sin_cos();

        // position = target + radius * (cos(pitch)sin(yaw), sin(pitch), cos(pitch)cos(yaw)).
        // Therefore yaw = 0 places the camera on +Z and positive yaw moves it toward +X.
        let orbit_offset = Vec3::new(
            radius * pitch_cos * yaw_sin,
            radius * pitch_sin,
            radius * pitch_cos * yaw_cos,
        );
        let position = target + orbit_offset;
        let forward = (target - position).try_normalized()?;

        // For a camera looking along -Z, forward × world_up produces the expected +X right.
        let right = forward.cross(WORLD_UP).try_normalized()?;
        let up = right.cross(forward).try_normalized()?;

        let viewport_half_height = (vertical_fov * 0.5).tan();
        let viewport_half_width = viewport_half_height * aspect_ratio;

        if !position.is_finite()
            || !viewport_half_height.is_finite()
            || !viewport_half_width.is_finite()
        {
            return None;
        }

        Some(Self {
            position,
            forward,
            right,
            up,
            viewport_half_width,
            viewport_half_height,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, PI, TAU};

    use super::{MAX_ABS_PITCH, MIN_ORBIT_RADIUS, OrbitalCamera};
    use crate::math::Vec3;

    const EPSILON: f32 = 1.0e-5;

    fn camera() -> OrbitalCamera {
        OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0).unwrap()
    }

    fn assert_approx_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= EPSILON,
            "{actual} != {expected}"
        );
    }

    fn assert_vec_approx_eq(actual: Vec3, expected: Vec3) {
        assert_approx_eq(actual.x, expected.x);
        assert_approx_eq(actual.y, expected.y);
        assert_approx_eq(actual.z, expected.z);
    }

    #[test]
    fn constructs_valid_camera_with_expected_orbit_convention() {
        let target = Vec3::new(1.0, 2.0, 3.0);
        let camera = OrbitalCamera::try_new(target, 0.0, 0.0, 5.0, FRAC_PI_3, 16.0 / 9.0).unwrap();

        assert_eq!(camera.target(), target);
        assert_approx_eq(camera.yaw(), 0.0);
        assert_approx_eq(camera.pitch(), 0.0);
        assert_approx_eq(camera.radius(), 5.0);
        assert_approx_eq(camera.vertical_fov(), FRAC_PI_3);
        assert_approx_eq(camera.aspect_ratio(), 16.0 / 9.0);
        assert_vec_approx_eq(camera.position(), Vec3::new(1.0, 2.0, 8.0));
    }

    #[test]
    fn rejects_non_finite_camera_configuration() {
        let valid = (Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 1.0);

        assert_eq!(
            OrbitalCamera::try_new(
                Vec3::new(f32::NAN, 0.0, 0.0),
                valid.1,
                valid.2,
                valid.3,
                valid.4,
                valid.5,
            ),
            None
        );
        assert_eq!(
            OrbitalCamera::try_new(valid.0, f32::INFINITY, valid.2, valid.3, valid.4, valid.5),
            None
        );
        assert_eq!(
            OrbitalCamera::try_new(valid.0, valid.1, f32::NAN, valid.3, valid.4, valid.5),
            None
        );
        assert_eq!(
            OrbitalCamera::try_new(valid.0, valid.1, valid.2, f32::INFINITY, valid.4, valid.5),
            None
        );
        assert_eq!(
            OrbitalCamera::try_new(valid.0, valid.1, valid.2, valid.3, f32::NAN, valid.5),
            None
        );
        assert_eq!(
            OrbitalCamera::try_new(valid.0, valid.1, valid.2, valid.3, valid.4, f32::INFINITY),
            None
        );
    }

    #[test]
    fn rejects_invalid_radius() {
        assert_eq!(
            OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 0.0, FRAC_PI_2, 1.0),
            None
        );
        assert_eq!(
            OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, -1.0, FRAC_PI_2, 1.0),
            None
        );
        assert_eq!(
            OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, MIN_ORBIT_RADIUS * 0.5, FRAC_PI_2, 1.0),
            None
        );
    }

    #[test]
    fn rejects_invalid_vertical_fov() {
        for fov in [-1.0, 0.0, PI, PI + 0.1] {
            assert_eq!(
                OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, fov, 1.0),
                None
            );
        }
    }

    #[test]
    fn rejects_invalid_aspect_ratio() {
        for aspect_ratio in [-1.0, 0.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, aspect_ratio),
                None
            );
        }
    }

    #[test]
    fn rejects_pitch_at_or_beyond_poles() {
        assert_eq!(
            OrbitalCamera::try_new(Vec3::ZERO, 0.0, FRAC_PI_2, 5.0, FRAC_PI_2, 1.0),
            None
        );
        assert_eq!(
            OrbitalCamera::try_new(Vec3::ZERO, 0.0, -FRAC_PI_2, 5.0, FRAC_PI_2, 1.0),
            None
        );
    }

    #[test]
    fn position_remains_at_orbit_radius_from_target() {
        let target = Vec3::new(2.0, -3.0, 4.0);
        let camera = OrbitalCamera::try_new(target, 1.2, -0.6, 7.5, FRAC_PI_3, 1.5).unwrap();

        assert_approx_eq((camera.position() - target).length(), 7.5);
    }

    #[test]
    fn forward_points_from_position_to_target() {
        let target = Vec3::new(2.0, 1.0, -3.0);
        let camera = OrbitalCamera::try_new(target, 0.8, 0.4, 6.0, FRAC_PI_3, 1.0).unwrap();
        let expected = (target - camera.position()).try_normalized().unwrap();

        assert_vec_approx_eq(camera.forward(), expected);
    }

    #[test]
    fn basis_is_orthonormal() {
        let camera = OrbitalCamera::try_new(Vec3::ZERO, 0.7, 0.5, 5.0, FRAC_PI_3, 1.5).unwrap();

        assert_approx_eq(camera.forward().length(), 1.0);
        assert_approx_eq(camera.right().length(), 1.0);
        assert_approx_eq(camera.up().length(), 1.0);
        assert_approx_eq(camera.forward().dot(camera.right()), 0.0);
        assert_approx_eq(camera.forward().dot(camera.up()), 0.0);
        assert_approx_eq(camera.right().dot(camera.up()), 0.0);
    }

    #[test]
    fn right_vector_matches_coordinate_convention() {
        assert_vec_approx_eq(camera().right(), Vec3::new(1.0, 0.0, 0.0));
        assert_vec_approx_eq(camera().up(), Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn center_viewport_ray_points_at_target() {
        let camera = camera();
        let ray = camera.ray_for_viewport(0.5, 0.5).unwrap();

        assert_eq!(ray.origin(), camera.position());
        assert_vec_approx_eq(ray.direction(), camera.forward());
    }

    #[test]
    fn left_and_right_rays_diverge_horizontally() {
        let camera = camera();
        let left = camera.ray_for_viewport(0.0, 0.5).unwrap();
        let right = camera.ray_for_viewport(1.0, 0.5).unwrap();

        assert!(left.direction().dot(camera.right()) < 0.0);
        assert!(right.direction().dot(camera.right()) > 0.0);
        assert_approx_eq(
            left.direction().dot(camera.right()).abs(),
            right.direction().dot(camera.right()).abs(),
        );
    }

    #[test]
    fn top_and_bottom_rays_diverge_vertically() {
        let camera = camera();
        let top = camera.ray_for_viewport(0.5, 0.0).unwrap();
        let bottom = camera.ray_for_viewport(0.5, 1.0).unwrap();

        assert!(top.direction().dot(camera.up()) > 0.0);
        assert!(bottom.direction().dot(camera.up()) < 0.0);
    }

    #[test]
    fn wider_aspect_ratio_increases_horizontal_spread() {
        let square = camera();
        let wide = OrbitalCamera::try_new(Vec3::ZERO, 0.0, 0.0, 5.0, FRAC_PI_2, 2.0).unwrap();
        let square_ray = square.ray_for_viewport(1.0, 0.5).unwrap();
        let wide_ray = wide.ray_for_viewport(1.0, 0.5).unwrap();

        assert!(
            wide_ray.direction().dot(wide.right()).abs()
                > square_ray.direction().dot(square.right()).abs()
        );
    }

    #[test]
    fn yaw_orbit_changes_position_and_preserves_radius() {
        let mut camera = camera();
        let original_position = camera.position();

        camera.orbit_yaw(FRAC_PI_2);

        assert_ne!(camera.position(), original_position);
        assert_vec_approx_eq(camera.position(), Vec3::new(5.0, 0.0, 0.0));
        assert_approx_eq(camera.position().length(), 5.0);
    }

    #[test]
    fn yaw_wraps_periodically() {
        let mut camera = camera();

        camera.orbit_yaw(TAU + FRAC_PI_4);

        assert_approx_eq(camera.yaw(), FRAC_PI_4);
    }

    #[test]
    fn pitch_orbit_changes_elevation_and_preserves_radius() {
        let mut camera = camera();

        camera.orbit_pitch(FRAC_PI_4);

        assert!(camera.position().y > 0.0);
        assert_approx_eq(camera.position().length(), 5.0);
    }

    #[test]
    fn pitch_is_clamped_away_from_poles() {
        let mut camera = camera();

        camera.orbit_pitch(PI);
        assert_approx_eq(camera.pitch(), MAX_ABS_PITCH);

        camera.orbit_pitch(-2.0 * PI);
        assert_approx_eq(camera.pitch(), -MAX_ABS_PITCH);
    }

    #[test]
    fn radius_adjustment_zooms_and_respects_minimum() {
        let mut camera = camera();

        camera.adjust_radius(3.0);
        assert_approx_eq(camera.radius(), 8.0);
        assert_approx_eq(camera.position().length(), 8.0);

        camera.adjust_radius(-100.0);
        assert_approx_eq(camera.radius(), MIN_ORBIT_RADIUS);
        assert_approx_eq(camera.position().length(), MIN_ORBIT_RADIUS);
    }

    #[test]
    fn non_finite_updates_do_not_corrupt_camera() {
        let mut camera = camera();
        let original = camera;

        camera.orbit_yaw(f32::NAN);
        camera.orbit_pitch(f32::INFINITY);
        camera.adjust_radius(f32::NEG_INFINITY);

        assert_eq!(camera, original);
    }

    #[test]
    fn generated_rays_are_finite_and_normalized() {
        let camera = OrbitalCamera::try_new(
            Vec3::new(1.0, 2.0, 3.0),
            0.7,
            0.4,
            10.0,
            FRAC_PI_3,
            16.0 / 9.0,
        )
        .unwrap();

        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let ray = camera.ray_for_viewport(u, v).unwrap();

            assert!(ray.origin().is_finite());
            assert!(ray.direction().is_finite());
            assert_approx_eq(ray.direction().length(), 1.0);
        }
    }

    #[test]
    fn rejects_invalid_viewport_coordinates() {
        let camera = camera();

        assert_eq!(camera.ray_for_viewport(-0.1, 0.5), None);
        assert_eq!(camera.ray_for_viewport(1.1, 0.5), None);
        assert_eq!(camera.ray_for_viewport(0.5, f32::NAN), None);
    }
}
