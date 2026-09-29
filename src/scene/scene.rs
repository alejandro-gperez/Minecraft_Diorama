use crate::{
    geometry::{Aabb, AabbHit},
    material::{Material, MaterialId},
    ray::Ray,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneObject {
    pub bounds: Aabb,
    pub material_id: MaterialId,
}

impl SceneObject {
    pub const fn new(bounds: Aabb, material_id: MaterialId) -> Self {
        Self {
            bounds,
            material_id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneHit {
    pub geometry: AabbHit,
    pub material_id: MaterialId,
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    objects: Vec<SceneObject>,
    materials: Vec<Material>,
}

impl Scene {
    pub const fn new() -> Self {
        Self {
            objects: Vec::new(),
            materials: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            objects: Vec::with_capacity(capacity),
            materials: Vec::new(),
        }
    }

    pub fn add_material(&mut self, material: Material) -> Option<MaterialId> {
        let index = u32::try_from(self.materials.len()).ok()?;
        let id = MaterialId::new(index);
        self.materials.push(material);
        Some(id)
    }

    pub fn material(&self, id: MaterialId) -> Option<&Material> {
        self.materials.get(id.index())
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
                    material_id: object.material_id,
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

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{Scene, SceneObject};
    use crate::{
        color::Color,
        geometry::Aabb,
        material::{Material, MaterialId, Texture},
        math::Vec3,
        ray::Ray,
    };

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray::try_new(origin, direction).unwrap()
    }

    fn material(color: Color) -> Material {
        Material::try_new(Texture::solid(color), Color::WHITE, 0.0, 0.0, 0.0).unwrap()
    }

    fn object(min: Vec3, max: Vec3, material_id: MaterialId) -> SceneObject {
        SceneObject::new(Aabb::try_new(min, max).unwrap(), material_id)
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
        let red_id = scene.add_material(material(red)).unwrap();
        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), red_id));

        let hit = scene
            .closest_hit(
                ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0)),
                0.0,
                f32::INFINITY,
            )
            .unwrap();
        assert_eq!(hit.material_id, red_id);
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
        let ray = ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));

        let mut near_first = Scene::new();
        let near_id = near_first.add_material(material(near_color)).unwrap();
        let far_id = near_first.add_material(material(far_color)).unwrap();
        let near = object(Vec3::new(0.0, 0.0, 2.0), Vec3::new(1.0, 1.0, 3.0), near_id);
        let far = object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), far_id);
        near_first.add(near);
        near_first.add(far);

        let mut far_first = Scene::new();
        let near_id = far_first.add_material(material(near_color)).unwrap();
        let far_id = far_first.add_material(material(far_color)).unwrap();
        let near = object(Vec3::new(0.0, 0.0, 2.0), Vec3::new(1.0, 1.0, 3.0), near_id);
        let far = object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), far_id);
        far_first.add(far);
        far_first.add(near);

        assert_eq!(
            near_first
                .closest_hit(ray, 0.0, f32::INFINITY)
                .unwrap()
                .material_id,
            near_id
        );
        assert_eq!(
            far_first
                .closest_hit(ray, 0.0, f32::INFINITY)
                .unwrap()
                .material_id,
            near_id
        );
    }

    #[test]
    fn closest_hit_respects_ray_interval() {
        let near_color = Color::new(1.0, 0.0, 0.0);
        let far_color = Color::new(0.0, 0.0, 1.0);
        let mut scene = Scene::new();
        let near_id = scene.add_material(material(near_color)).unwrap();
        let far_id = scene.add_material(material(far_color)).unwrap();
        scene.add(object(
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 1.0, 3.0),
            near_id,
        ));
        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), far_id));
        let ray = ray(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));

        assert_eq!(
            scene.closest_hit(ray, 3.5, 5.0).unwrap().material_id,
            far_id
        );
        assert_eq!(scene.closest_hit(ray, 0.0, 1.5), None);
    }

    #[test]
    fn tracks_scene_length_and_empty_state() {
        let mut scene = Scene::new();
        assert!(scene.is_empty());

        let material_id = scene.add_material(material(Color::WHITE)).unwrap();
        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), material_id));

        assert_eq!(scene.len(), 1);
        assert!(!scene.is_empty());
    }

    #[test]
    fn material_insertion_returns_stable_ids_and_safe_lookup() {
        let red = material(Color::new(1.0, 0.0, 0.0));
        let blue = material(Color::new(0.0, 0.0, 1.0));
        let mut scene = Scene::new();

        let red_id = scene.add_material(red.clone()).unwrap();
        let blue_id = scene.add_material(blue.clone()).unwrap();

        assert_eq!(red_id, MaterialId::new(0));
        assert_eq!(blue_id, MaterialId::new(1));
        assert_eq!(scene.material(red_id), Some(&red));
        assert_eq!(scene.material(blue_id), Some(&blue));
        assert_eq!(scene.material(MaterialId::new(u32::MAX)), None);
    }

    #[test]
    fn multiple_objects_share_one_centrally_owned_material() {
        let mut scene = Scene::new();
        let material_id = scene.add_material(material(Color::WHITE)).unwrap();

        scene.add(object(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), material_id));
        scene.add(object(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 1.0, 1.0),
            material_id,
        ));

        assert_eq!(scene.len(), 2);
        assert_eq!(scene.material_count(), 1);
        assert_eq!(scene.material(material_id).unwrap().texture().width(), 1);
    }
}
