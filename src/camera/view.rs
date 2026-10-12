use crate::ray::Ray;

/// What the renderer needs from any camera: its aspect ratio and a primary ray per viewport
/// point (`u` left-to-right, `v` top-to-bottom, both in `0..=1`). `Sync` because tiles are traced
/// on scoped worker threads that share the camera by reference.
pub trait ViewCamera: Sync {
    fn aspect_ratio(&self) -> f32;

    fn ray_for_viewport(&self, u: f32, v: f32) -> Option<Ray>;
}
