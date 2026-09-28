use crate::{
    color::Color,
    geometry::{Aabb, AabbHit},
    ray::Ray,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneObject {
    pub bounds: Aabb,
    pub color: Color,
}

impl SceneObject {
    pub const fn new(bounds: Aabb, color: Color) -> Self {
        Self { bounds, color }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneHit {
    pub geometry: AabbHit,
    pub color: Color,
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    objects: Vec<SceneObject>,
}

impl Scene {
    pub const fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            objects: Vec::with_capacity(capacity),
        }
    }

    pub fn add(&mut self, object: SceneObject) {
        self.objects.push(object);
    }

    pub fn closest_hit(&self, ray: Ray, t_min: f32, t_max: f32) -> Option<SceneHit> {
        let mut closest_t = t_max;
        let mut closest_hit: Option<SceneHit> = None;

        for object in &self.objects {
            if let Some(geometry) = object.bounds.intersect(ray, t_min, closest_t)
                && closest_hit
                    .as_ref()
                    .is_none_or(|current| geometry.t < current.geometry.t)
            {
                closest_t = geometry.t;
                closest_hit = Some(SceneHit {
                    geometry,
                    color: object.color,
                });
            }
        }

        closest_hit
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Scene, SceneObject};
    use crate::{color::Color, geometry::Aabb, math::Vec3, ray::Ray};

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray::try_new(origin, direction).unwrap()
    }

    fn object(min: Vec3, max: Vec3, color: Color) -> SceneObject {
        SceneObject::new(Aabb::try_new(min, max).unwrap(), color)
    }

    #[test]
    fn empty_scene_misses() {
        assert_eq!(
            Scene::new().closest_hit(
                ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            ),
            None
        );
    }

    #[test]
    fn one_object_can_be_hit_or_missed() {
        let red = Color::new(1.0, 0.0, 0.0);
        let mut scene = Scene::new();
        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), red));

        let hit = scene
            .closest_hit(
                ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();
        assert_eq!(hit.color, red);
        assert_eq!(
            scene.closest_hit(
                ray(Vec3::new(2.0, 2.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            ),
            None
        );
    }

    #[test]
    fn nearest_object_wins_regardless_of_insertion_order() {
        let near_color = Color::new(1.0, 0.0, 0.0);
        let far_color = Color::new(0.0, 0.0, 1.0);
        let near = object(
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 1.0, 3.0),
            near_color,
        );
        let far = object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), far_color);
        let ray = ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));

        let mut near_first = Scene::new();
        near_first.add(near);
        near_first.add(far);
        let mut far_first = Scene::new();
        far_first.add(far);
        far_first.add(near);

        assert_eq!(
            near_first
                .closest_hit(ray, 0.0, f32::INFINITY)
                .unwrap()
                .color,
            near_color
        );
        assert_eq!(
            far_first
                .closest_hit(ray, 0.0, f32::INFINITY)
                .unwrap()
                .color,
            near_color
        );
    }

    #[test]
    fn closest_hit_respects_ray_interval() {
        let near_color = Color::new(1.0, 0.0, 0.0);
        let far_color = Color::new(0.0, 0.0, 1.0);
        let mut scene = Scene::new();
        scene.add(object(
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 1.0, 3.0),
            near_color,
        ));
        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), far_color));
        let ray = ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));

        assert_eq!(scene.closest_hit(ray, 3.5, 5.0).unwrap().color, far_color);
        assert_eq!(scene.closest_hit(ray, 0.0, 1.5), None);
    }

    #[test]
    fn tracks_scene_length_and_empty_state() {
        let mut scene = Scene::new();
        assert!(scene.is_empty());

        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), Color::WHITE));

        assert_eq!(scene.len(), 1);
        assert!(!scene.is_empty());
    }
}
