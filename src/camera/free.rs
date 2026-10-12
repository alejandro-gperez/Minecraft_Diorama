use std::f32::consts::{PI, TAU};

use super::{MAX_ABS_PITCH, OrbitalCamera, ViewCamera};
use crate::{math::Vec3, ray::Ray};

const WORLD_UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);

/// Spectator camera: a position and a yaw/pitch view direction, with no collision or physics.
///
/// Same conventions as `OrbitalCamera` (+X right, +Y up, `right = forward × world_up`, `u` runs
/// left-to-right and `v` top-to-bottom). The view direction is
/// `(cos(pitch) sin(yaw), sin(pitch), cos(pitch) cos(yaw))`, so yaw `0` looks along +Z and
/// positive pitch looks up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreeCamera {
    position: Vec3,
    yaw: f32,
    pitch: f32,
    vertical_fov: f32,
    aspect_ratio: f32,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    viewport_half_width: f32,
    viewport_half_height: f32,
}

impl FreeCamera {
    pub fn try_new(
        position: Vec3,
        yaw: f32,
        pitch: f32,
        vertical_fov: f32,
        aspect_ratio: f32,
    ) -> Option<Self> {
        if !position.is_finite()
            || !yaw.is_finite()
            || !pitch.is_finite()
            || !vertical_fov.is_finite()
            || !aspect_ratio.is_finite()
            || pitch.abs() > MAX_ABS_PITCH
            || vertical_fov <= 0.0
            || vertical_fov >= PI
            || aspect_ratio <= 0.0
        {
            return None;
        }

        let yaw = yaw.rem_euclid(TAU);
        let (yaw_sin, yaw_cos) = yaw.sin_cos();
        let (pitch_sin, pitch_cos) = pitch.sin_cos();
        let forward =
            Vec3::new(pitch_cos * yaw_sin, pitch_sin, pitch_cos * yaw_cos).try_normalized()?;
        let right = forward.cross(WORLD_UP).try_normalized()?;
        let up = right.cross(forward).try_normalized()?;
        let viewport_half_height = (vertical_fov * 0.5).tan();
        let viewport_half_width = viewport_half_height * aspect_ratio;
        if !viewport_half_height.is_finite() || !viewport_half_width.is_finite() {
            return None;
        }

        Some(Self {
            position,
            yaw,
            pitch,
            vertical_fov,
            aspect_ratio,
            forward,
            right,
            up,
            viewport_half_width,
            viewport_half_height,
        })
    }

    /// A free camera with exactly `orbital`'s position, view direction, FOV, and aspect ratio.
    pub fn from_orbital(orbital: &OrbitalCamera) -> Option<Self> {
        let forward = orbital.forward();
        let pitch = forward
            .y
            .clamp(-1.0, 1.0)
            .asin()
            .clamp(-MAX_ABS_PITCH, MAX_ABS_PITCH);
        Self::try_new(
            orbital.position(),
            forward.x.atan2(forward.z),
            pitch,
            orbital.vertical_fov(),
            orbital.aspect_ratio(),
        )
    }

    pub const fn position(&self) -> Vec3 {
        self.position
    }

    pub const fn yaw(&self) -> f32 {
        self.yaw
    }

    pub const fn pitch(&self) -> f32 {
        self.pitch
    }

    pub const fn forward(&self) -> Vec3 {
        self.forward
    }

    pub const fn right(&self) -> Vec3 {
        self.right
    }

    pub const fn up(&self) -> Vec3 {
        self.up
    }

    /// Turns the view by `yaw_delta` (positive turns toward -X from +Z, that is to the left when
    /// looking along +Z) and `pitch_delta` (positive looks up) radians. Pitch stays bounded;
    /// non-finite deltas change nothing.
    pub fn look(&mut self, yaw_delta: f32, pitch_delta: f32) {
        if !yaw_delta.is_finite() || !pitch_delta.is_finite() {
            return;
        }
        let pitch = (self.pitch as f64 + pitch_delta as f64)
            .clamp(-(MAX_ABS_PITCH as f64), MAX_ABS_PITCH as f64) as f32;
        let yaw = (self.yaw as f64 + yaw_delta as f64).rem_euclid(TAU as f64) as f32;
        if let Some(turned) = Self::try_new(
            self.position,
            yaw,
            pitch,
            self.vertical_fov,
            self.aspect_ratio,
        ) {
            *self = turned;
        }
    }

    /// Moves along the camera's right and forward vectors and along world up. Non-finite input
    /// or a non-finite result changes nothing.
    pub fn translate(&mut self, right: f32, forward: f32, up: f32) {
        if !right.is_finite() || !forward.is_finite() || !up.is_finite() {
            return;
        }
        let position = self.position + self.right * right + self.forward * forward + WORLD_UP * up;
        if position.is_finite() {
            self.position = position;
        }
    }

    pub fn ray_for_viewport(&self, u: f32, v: f32) -> Option<Ray> {
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return None;
        }
        let horizontal_offset = (2.0 * u - 1.0) * self.viewport_half_width;
        let vertical_offset = (1.0 - 2.0 * v) * self.viewport_half_height;
        let direction = self.forward + self.right * horizontal_offset + self.up * vertical_offset;
        Ray::try_new(self.position, direction)
    }
}

impl ViewCamera for FreeCamera {
    fn aspect_ratio(&self) -> f32 {
        self.aspect_ratio
    }

    fn ray_for_viewport(&self, u: f32, v: f32) -> Option<Ray> {
        FreeCamera::ray_for_viewport(self, u, v)
    }
}

impl ViewCamera for OrbitalCamera {
    fn aspect_ratio(&self) -> f32 {
        OrbitalCamera::aspect_ratio(*self)
    }

    fn ray_for_viewport(&self, u: f32, v: f32) -> Option<Ray> {
        OrbitalCamera::ray_for_viewport(self, u, v)
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

    use super::*;

    const EPSILON: f32 = 1.0e-4;

    fn camera() -> FreeCamera {
        FreeCamera::try_new(Vec3::new(1.0, 2.0, 3.0), 0.0, 0.0, FRAC_PI_2, 16.0 / 9.0).unwrap()
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() <= EPSILON
    }

    #[test]
    fn basis_is_orthonormal_with_world_up_sideways_free() {
        for (yaw, pitch) in [(0.0, 0.0), (1.2, 0.7), (4.0, -1.3), (6.0, 1.5)] {
            let c = FreeCamera::try_new(Vec3::ZERO, yaw, pitch, 1.0, 1.5).unwrap();
            for v in [c.forward(), c.right(), c.up()] {
                assert!((v.length() - 1.0).abs() <= EPSILON);
            }
            assert!(c.forward().dot(c.right()).abs() <= EPSILON);
            assert!(c.forward().dot(c.up()).abs() <= EPSILON);
            assert!(c.right().dot(c.up()).abs() <= EPSILON);
            assert!(c.right().y.abs() <= EPSILON, "right stays horizontal");
        }
    }

    #[test]
    fn center_ray_follows_forward_and_corner_rays_are_finite_unit() {
        let c = FreeCamera::try_new(Vec3::ZERO, 0.9, 0.4, 1.0, 16.0 / 9.0).unwrap();
        let center = c.ray_for_viewport(0.5, 0.5).unwrap();
        assert!(near(center.direction(), c.forward()));
        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let ray = c.ray_for_viewport(u, v).unwrap();
            assert!(ray.direction().is_finite());
            assert!((ray.direction().length() - 1.0).abs() <= EPSILON);
        }
        assert!(c.ray_for_viewport(-0.1, 0.5).is_none());
        assert!(c.ray_for_viewport(f32::NAN, 0.5).is_none());
    }

    #[test]
    fn viewport_orientation_matches_the_orbital_camera() {
        let orbital = OrbitalCamera::try_new(Vec3::ZERO, 0.7, 0.35, 10.0, 0.9, 16.0 / 9.0).unwrap();
        let free = FreeCamera::from_orbital(&orbital).unwrap();
        for (u, v) in [(0.5, 0.5), (0.0, 0.0), (1.0, 0.2), (0.3, 1.0)] {
            let a = orbital.ray_for_viewport(u, v).unwrap();
            let b = free.ray_for_viewport(u, v).unwrap();
            assert!(near(a.origin(), b.origin()));
            assert!(near(a.direction(), b.direction()), "({u}, {v})");
        }
    }

    #[test]
    fn movement_follows_orientation_and_vertical_uses_world_up() {
        let mut c = FreeCamera::try_new(Vec3::ZERO, FRAC_PI_2, 0.0, 1.0, 1.0).unwrap();
        // Yaw 90 degrees looks along +X.
        c.translate(0.0, 2.0, 0.0);
        assert!(near(c.position(), Vec3::new(2.0, 0.0, 0.0)));
        let mut c = FreeCamera::try_new(Vec3::ZERO, 0.0, FRAC_PI_4, 1.0, 1.0).unwrap();
        c.translate(0.0, 0.0, 3.0);
        assert!(
            near(c.position(), Vec3::new(0.0, 3.0, 0.0)),
            "up is world up, not camera up"
        );
        let mut c = camera();
        let start = c.position();
        c.translate(1.0, 0.0, 0.0);
        assert!(near(c.position() - start, c.right()));
    }

    #[test]
    fn look_updates_yaw_and_pitch_and_bounds_pitch() {
        let mut c = camera();
        c.look(0.25, 0.1);
        assert!((c.yaw() - 0.25).abs() <= EPSILON && (c.pitch() - 0.1).abs() <= EPSILON);
        c.look(0.0, 100.0);
        assert_eq!(c.pitch(), MAX_ABS_PITCH);
        c.look(0.0, -1000.0);
        assert_eq!(c.pitch(), -MAX_ABS_PITCH);
        c.look(-100.0, 0.0);
        assert!((0.0..TAU).contains(&c.yaw()));
        assert!(c.forward().is_finite() && c.right().is_finite());
    }

    #[test]
    fn invalid_input_does_not_corrupt_the_camera() {
        let mut c = camera();
        let original = c;
        c.look(f32::NAN, 0.0);
        c.look(0.0, f32::INFINITY);
        c.translate(f32::NAN, 0.0, 0.0);
        c.translate(0.0, f32::INFINITY, 0.0);
        assert_eq!(c, original);
        // A move that overflows to infinity is refused.
        let mut edge =
            FreeCamera::try_new(Vec3::new(f32::MAX, 0.0, 0.0), FRAC_PI_2, 0.0, 1.0, 1.0).unwrap();
        let before = edge;
        edge.translate(0.0, f32::MAX, 0.0);
        assert_eq!(edge, before);
        assert!(FreeCamera::try_new(Vec3::new(f32::NAN, 0.0, 0.0), 0.0, 0.0, 1.0, 1.0).is_none());
        assert!(FreeCamera::try_new(Vec3::ZERO, 0.0, 2.0, 1.0, 1.0).is_none());
        assert!(FreeCamera::try_new(Vec3::ZERO, 0.0, 0.0, 0.0, 1.0).is_none());
        assert!(FreeCamera::try_new(Vec3::ZERO, 0.0, 0.0, 1.0, -1.0).is_none());
    }

    #[test]
    fn from_orbital_preserves_position_direction_and_projection() {
        for (yaw, pitch) in [(0.0, 0.0), (0.7, 0.35), (3.5, -0.9), (5.9, 1.2)] {
            let o = OrbitalCamera::try_new(Vec3::new(1.0, 2.0, 3.0), yaw, pitch, 7.0, 0.8, 1.7)
                .unwrap();
            let f = FreeCamera::from_orbital(&o).unwrap();
            assert!(near(f.position(), o.position()));
            assert!(near(f.forward(), o.forward()), "yaw {yaw} pitch {pitch}");
            assert!(near(f.right(), o.right()) && near(f.up(), o.up()));
            assert_eq!(ViewCamera::aspect_ratio(&f), o.aspect_ratio());
        }
    }
}
