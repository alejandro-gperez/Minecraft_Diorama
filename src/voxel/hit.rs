use crate::{
    geometry::{CubeFace, SurfaceHit},
    material::MaterialId,
    math::Vec3,
};

use super::{BlockMaterials, BlockType, VoxelPosition};

/// How far, in world units, a hit position may stray from the voxel face it claims to lie on.
///
/// A traversal computes positions as `ray.at(t)`, which misses the exact boundary plane by
/// rounding error; this tolerance only rejects a position that belongs to another face or voxel.
/// It assumes unit-scale voxels, like the renderer's ray-origin biases.
const VOXEL_SURFACE_TOLERANCE: f32 = 1.0e-3;

/// A ray's intersection with one face of an occupied voxel: a traversal result, not a traversal.
///
/// `surface` is exactly what an AABB spanning the voxel's cell would report for the same position
/// and face, so shading cannot tell the two apart. The voxel coordinate, block identity, and face
/// stay available for consumers that need to know which cell was crossed, such as future glass
/// handling. It holds IDs only, never textures or materials.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoxelHit {
    pub surface: SurfaceHit,
    pub voxel: VoxelPosition,
    pub block: BlockType,
    pub material_id: MaterialId,
}

impl VoxelHit {
    /// Builds the hit on `face` of the voxel `voxel` at `position`, distance `t` along the ray.
    ///
    /// The voxel at `(x, y, z)` occupies `[x, x + 1) × [y, y + 1) × [z, z + 1)`. The normal comes
    /// from `face` and the UV from the shared `CubeFace::uv` table, with local coordinates clamped
    /// to `[0, 1]` like an AABB's. The material is resolved from `block` through `materials`, so
    /// the two cannot disagree. Returns `None` for a non-finite `t` or `position`, or a position
    /// farther than `VOXEL_SURFACE_TOLERANCE` from that face of that voxel. Finding the face is
    /// the caller's job; nothing here intersects or traverses.
    pub fn try_new(
        voxel: VoxelPosition,
        face: CubeFace,
        position: Vec3,
        t: f32,
        block: BlockType,
        materials: &BlockMaterials,
    ) -> Option<Self> {
        if !t.is_finite() || !position.is_finite() {
            return None;
        }

        let normal = face.normal();
        let offset = position - voxel.min_corner();
        let local = Vec3::new(
            face_local_coordinate(offset.x, normal.x)?,
            face_local_coordinate(offset.y, normal.y)?,
            face_local_coordinate(offset.z, normal.z)?,
        );

        Some(Self {
            surface: SurfaceHit {
                t,
                position,
                normal,
                face,
                uv: Some(face.uv(local)),
            },
            voxel,
            block,
            material_id: materials.material(block),
        })
    }
}

/// Validates one axis of a cell-relative offset and returns it clamped to the unit cell.
///
/// Along the face normal the offset must sit on the face's plane, `1` for a positive face and `0`
/// for a negative one; across the face it must lie within the cell.
fn face_local_coordinate(offset: f32, normal_component: f32) -> Option<f32> {
    let on_surface = if normal_component > 0.0 {
        (offset - 1.0).abs() <= VOXEL_SURFACE_TOLERANCE
    } else if normal_component < 0.0 {
        offset.abs() <= VOXEL_SURFACE_TOLERANCE
    } else {
        (-VOXEL_SURFACE_TOLERANCE..=1.0 + VOXEL_SURFACE_TOLERANCE).contains(&offset)
    };

    on_surface.then(|| offset.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::{VOXEL_SURFACE_TOLERANCE, VoxelHit};
    use crate::{
        geometry::{Aabb, CubeFace, SurfaceHit, Uv},
        material::MaterialId,
        math::Vec3,
        ray::Ray,
        voxel::{BlockMaterials, BlockType, VoxelPosition},
    };

    /// Asymmetric in-cell offsets, exact in `f32`, so any swapped or flipped axis changes the UV.
    const LOCAL_X: f32 = 0.25;
    const LOCAL_Y: f32 = 0.625;
    const LOCAL_Z: f32 = 0.875;

    fn materials() -> BlockMaterials {
        BlockMaterials {
            grass: MaterialId::new(20),
            dirt: MaterialId::new(21),
            cobblestone: MaterialId::new(22),
            obsidian: MaterialId::new(23),
            glass: MaterialId::new(24),
            lava: MaterialId::new(25),
            coal_ore: MaterialId::new(26),
            iron_ore: MaterialId::new(27),
            gold_ore: MaterialId::new(28),
            diamond_ore: MaterialId::new(29),
        }
    }

    fn unit_aabb(voxel: VoxelPosition) -> Aabb {
        let min = voxel.min_corner();
        Aabb::try_new(min, min + Vec3::new(1.0, 1.0, 1.0)).unwrap()
    }

    /// The point on `face` of `voxel` at the asymmetric in-plane offsets.
    fn face_point(voxel: VoxelPosition, face: CubeFace) -> Vec3 {
        let normal = face.normal();
        let on_plane = |component: f32, local: f32| match component {
            c if c > 0.0 => 1.0,
            c if c < 0.0 => 0.0,
            _ => local,
        };
        voxel.min_corner()
            + Vec3::new(
                on_plane(normal.x, LOCAL_X),
                on_plane(normal.y, LOCAL_Y),
                on_plane(normal.z, LOCAL_Z),
            )
    }

    /// The Mission 9 table written out per face for the asymmetric offsets above.
    fn expected_uv(face: CubeFace) -> Uv {
        match face {
            CubeFace::PositiveX => Uv::new(1.0 - LOCAL_Z, 1.0 - LOCAL_Y),
            CubeFace::NegativeX => Uv::new(LOCAL_Z, 1.0 - LOCAL_Y),
            CubeFace::PositiveY => Uv::new(LOCAL_X, LOCAL_Z),
            CubeFace::NegativeY => Uv::new(LOCAL_X, 1.0 - LOCAL_Z),
            CubeFace::PositiveZ => Uv::new(LOCAL_X, 1.0 - LOCAL_Y),
            CubeFace::NegativeZ => Uv::new(1.0 - LOCAL_X, 1.0 - LOCAL_Y),
        }
    }

    /// Hits the equivalent unit AABB from outside `face`, `skew` sideways from head-on.
    fn aabb_hit(voxel: VoxelPosition, face: CubeFace, skew: Vec3) -> SurfaceHit {
        let target = face_point(voxel, face);
        let origin = target + face.normal() * 2.0 + skew;
        let ray = Ray::try_new(origin, target - origin).unwrap();
        let hit = unit_aabb(voxel).intersect(ray, 0.0, f32::INFINITY).unwrap();
        assert_eq!(
            hit.face, face,
            "skew {skew:?} must still enter through {face:?}"
        );
        hit
    }

    fn voxel_hit_matching(
        voxel: VoxelPosition,
        aabb: SurfaceHit,
        block: BlockType,
    ) -> Option<VoxelHit> {
        VoxelHit::try_new(voxel, aabb.face, aabb.position, aabb.t, block, &materials())
    }

    #[test]
    fn voxel_uvs_match_the_equivalent_unit_aabb_on_every_face() {
        let voxel = VoxelPosition::new(-3, 5, 11);

        for face in CubeFace::ALL {
            let aabb = aabb_hit(voxel, face, Vec3::ZERO);
            let hit = voxel_hit_matching(voxel, aabb, BlockType::Cobblestone).unwrap();

            assert_eq!(aabb.uv, Some(expected_uv(face)), "{face:?}");
            assert_eq!(hit.surface, aabb, "{face:?}");
        }
    }

    #[test]
    fn voxel_uvs_match_the_unit_aabb_for_oblique_rays() {
        let voxel = VoxelPosition::new(-3, 5, 11);
        let skews = [Vec3::new(0.3, -0.2, 0.1), Vec3::new(-0.4, 0.35, -0.25)];

        for face in CubeFace::ALL {
            for skew in skews {
                // Remove the skew's component along the normal so it only tilts the ray.
                let normal = face.normal();
                let skew = skew - normal * skew.dot(normal);
                let aabb = aabb_hit(voxel, face, skew);
                let hit = voxel_hit_matching(voxel, aabb, BlockType::Glass).unwrap();

                assert_eq!(hit.surface, aabb, "{face:?} {skew:?}");
            }
        }
    }

    #[test]
    fn uv_orientations_differ_across_faces_so_flips_are_detectable() {
        let uvs = CubeFace::ALL.map(expected_uv);

        assert_ne!(uvs[0], uvs[1]);
        assert_ne!(uvs[2], uvs[3]);
        assert_ne!(uvs[4], uvs[5]);
        for uv in uvs {
            assert_ne!(uv.u, uv.v);
        }
    }

    #[test]
    fn negative_voxel_coordinates_produce_consistent_hits() {
        for voxel in [
            VoxelPosition::new(-1, -1, -1),
            VoxelPosition::new(-8, -3, -5),
            VoxelPosition::new(-16, 0, -2),
        ] {
            for face in CubeFace::ALL {
                let position = face_point(voxel, face);
                let hit =
                    VoxelHit::try_new(voxel, face, position, 1.5, BlockType::Lava, &materials())
                        .unwrap();

                assert_eq!(hit.voxel, voxel);
                assert_eq!(hit.block, BlockType::Lava);
                assert_eq!(hit.material_id, MaterialId::new(25));
                assert_eq!(hit.surface.t, 1.5);
                assert_eq!(hit.surface.position, position);
                assert_eq!(hit.surface.face, face);
                assert_eq!(hit.surface.normal, face.normal());
                assert_eq!(
                    hit.surface.uv,
                    Some(expected_uv(face)),
                    "{voxel:?} {face:?}"
                );
                let aabb = aabb_hit(voxel, face, Vec3::ZERO);
                assert_eq!(hit.surface.uv, aabb.uv);
            }
        }
    }

    #[test]
    fn negative_cell_spans_toward_positive_infinity() {
        // Voxel -1 spans [-1, 0): its +X face is the plane x = 0, its -X face x = -1.
        let voxel = VoxelPosition::new(-1, -1, -1);
        let materials = materials();
        let at = |x| Vec3::new(x, -0.5, -0.5);

        assert!(
            VoxelHit::try_new(
                voxel,
                CubeFace::PositiveX,
                at(0.0),
                1.0,
                BlockType::Dirt,
                &materials
            )
            .is_some()
        );
        assert!(
            VoxelHit::try_new(
                voxel,
                CubeFace::NegativeX,
                at(-1.0),
                1.0,
                BlockType::Dirt,
                &materials
            )
            .is_some()
        );
        assert!(
            VoxelHit::try_new(
                voxel,
                CubeFace::PositiveX,
                at(-1.0),
                1.0,
                BlockType::Dirt,
                &materials
            )
            .is_none()
        );
        assert!(
            VoxelHit::try_new(
                voxel,
                CubeFace::NegativeX,
                at(0.0),
                1.0,
                BlockType::Dirt,
                &materials
            )
            .is_none()
        );
    }

    #[test]
    fn every_block_type_resolves_its_material_through_block_materials() {
        let materials = materials();
        let voxel = VoxelPosition::new(2, 3, 4);
        let position = face_point(voxel, CubeFace::PositiveY);

        for block in BlockType::ALL {
            let hit =
                VoxelHit::try_new(voxel, CubeFace::PositiveY, position, 1.0, block, &materials)
                    .unwrap();
            assert_eq!(hit.block, block);
            assert_eq!(hit.material_id, materials.material(block), "{block:?}");
        }
    }

    #[test]
    fn rejects_non_finite_distance_or_position() {
        let voxel = VoxelPosition::new(0, 0, 0);
        let position = face_point(voxel, CubeFace::PositiveZ);
        let materials = materials();
        let build = |position, t| {
            VoxelHit::try_new(
                voxel,
                CubeFace::PositiveZ,
                position,
                t,
                BlockType::Grass,
                &materials,
            )
        };

        assert!(build(position, 1.0).is_some());
        assert!(build(position, f32::NAN).is_none());
        assert!(build(position, f32::INFINITY).is_none());
        assert!(build(Vec3::new(f32::NAN, 0.5, 1.0), 1.0).is_none());
        assert!(build(Vec3::new(0.5, f32::INFINITY, 1.0), 1.0).is_none());
    }

    #[test]
    fn rejects_positions_off_the_claimed_face() {
        let voxel = VoxelPosition::new(-3, 5, 11);
        let materials = materials();
        let build = |face, position| {
            VoxelHit::try_new(voxel, face, position, 1.0, BlockType::Obsidian, &materials)
        };

        // Inside the cell, on the opposite face, outside the cell across the face, and a
        // neighbor's face.
        assert!(build(CubeFace::PositiveX, Vec3::new(-2.5, 5.5, 11.5)).is_none());
        assert!(build(CubeFace::PositiveX, Vec3::new(-3.0, 5.5, 11.5)).is_none());
        assert!(build(CubeFace::PositiveY, Vec3::new(-2.5, 6.0, 12.5)).is_none());
        assert!(build(CubeFace::NegativeY, Vec3::new(-1.5, 5.0, 11.5)).is_none());
    }

    #[test]
    fn rounding_error_within_tolerance_is_accepted_and_clamped() {
        let voxel = VoxelPosition::new(-3, 5, 11);
        let error = VOXEL_SURFACE_TOLERANCE * 0.5;
        // Slightly past the +X plane and slightly below the cell's bottom edge.
        let position = Vec3::new(-2.0 + error, 5.0 - error, 11.5);
        let hit = VoxelHit::try_new(
            voxel,
            CubeFace::PositiveX,
            position,
            1.0,
            BlockType::Cobblestone,
            &materials(),
        )
        .unwrap();

        assert_eq!(hit.surface.position, position);
        assert_eq!(hit.surface.uv, Some(Uv::new(0.5, 1.0)));
        assert!(
            VoxelHit::try_new(
                voxel,
                CubeFace::PositiveX,
                Vec3::new(-2.0 + VOXEL_SURFACE_TOLERANCE * 2.0, 5.5, 11.5),
                1.0,
                BlockType::Cobblestone,
                &materials(),
            )
            .is_none()
        );
    }

    #[test]
    fn voxel_hit_is_small_plain_copy_data() {
        fn copy_value<T: Copy>() {}
        copy_value::<VoxelHit>();
        assert!(size_of::<VoxelHit>() <= 64, "{}", size_of::<VoxelHit>());
    }
}
