//! The placement rules every prop set must satisfy before `place_decorations` returns it.

use super::{DecorationError, DecorationMaterial, Decorations, Mount};
use crate::{
    geometry::Aabb,
    math::Vec3,
    voxel::{VoxelGrid, VoxelPosition},
};

/// Cells whose interior a box overlaps (touching a cell's face does not count).
pub(super) fn overlapped_cells(bounds: Aabb) -> impl Iterator<Item = VoxelPosition> {
    let (min, max) = (bounds.min(), bounds.max());
    let range = |low: f32, high: f32| low.floor() as i32..high.ceil() as i32;
    let (xs, ys, zs) = (
        range(min.x, max.x),
        range(min.y, max.y),
        range(min.z, max.z),
    );
    ys.flat_map(move |y| {
        let xs = xs.clone();
        zs.clone()
            .flat_map(move |z| xs.clone().map(move |x| VoxelPosition::new(x, y, z)))
    })
}

/// Whether two boxes share interior volume (touching faces do not count).
pub(super) fn overlaps(a: Aabb, b: Aabb) -> bool {
    let (a0, a1, b0, b1) = (a.min(), a.max(), b.min(), b.max());
    a0.x < b1.x && b0.x < a1.x && a0.y < b1.y && b0.y < a1.y && a0.z < b1.z && b0.z < a1.z
}

pub(super) fn cell_box(cell: VoxelPosition) -> Aabb {
    let min = cell.min_corner();
    Aabb::try_new(min, min + Vec3::new(1.0, 1.0, 1.0)).expect("a voxel cell is a valid box")
}

/// Every prop must:
///
/// - lie inside the grid;
/// - overlap no block, except that an arrowhead overlaps exactly its target and also reaches out
///   of it, so the arrow is visibly stuck rather than floating or buried;
/// - if resting, have every bottom face (its lowest parts) on top of its support block;
/// - overlap no part of another prop.
///
/// Parts of one prop may touch or overlap each other (shaft and head, chest walls and lid); these
/// intentional contacts are the only intersections allowed.
pub(super) fn validate(grid: &VoxelGrid, decorations: &Decorations) -> Result<(), DecorationError> {
    for (index, decoration) in decorations.decorations().iter().enumerate() {
        let parts = decorations.parts_of(decoration);
        for part in parts {
            let embedded = match decoration.mount {
                Mount::Embedded { target, .. }
                    if part.material == DecorationMaterial::ArrowHead =>
                {
                    Some(target)
                }
                _ => None,
            };
            let mut hit_target = false;
            for cell in overlapped_cells(part.bounds) {
                if !grid.contains_world(cell) {
                    return Err(DecorationError::OutsideGrid { decoration: index });
                }
                if !grid.is_occupied_world(cell) {
                    continue;
                }
                match embedded {
                    Some(target) if cell == target => hit_target = true,
                    Some(_) => return Err(DecorationError::NotEmbedded { decoration: index }),
                    None => {
                        return Err(DecorationError::IntersectsBlock {
                            decoration: index,
                            block: cell,
                        });
                    }
                }
            }
            if let Some(target) = embedded {
                let target = cell_box(target);
                let (min, max) = (part.bounds.min(), part.bounds.max());
                let (t0, t1) = (target.min(), target.max());
                let buried = min.x >= t0.x
                    && min.y >= t0.y
                    && min.z >= t0.z
                    && max.x <= t1.x
                    && max.y <= t1.y
                    && max.z <= t1.z;
                if !hit_target || buried {
                    return Err(DecorationError::NotEmbedded { decoration: index });
                }
            }
        }

        if let Mount::Resting { support } = decoration.mount {
            let top = support.y as f32 + 1.0;
            let bottom = parts
                .iter()
                .map(|part| part.bounds.min().y)
                .fold(f32::INFINITY, f32::min);
            let column = cell_box(support);
            let on_support = parts
                .iter()
                .filter(|part| part.bounds.min().y == bottom)
                .all(|part| {
                    let (min, max) = (part.bounds.min(), part.bounds.max());
                    min.x >= column.min().x
                        && max.x <= column.max().x
                        && min.z >= column.min().z
                        && max.z <= column.max().z
                });
            if !grid.is_occupied_world(support) || bottom != top || !on_support {
                return Err(DecorationError::Unsupported { decoration: index });
            }
        }
    }

    let all = decorations.decorations();
    for (first, a) in all.iter().enumerate() {
        for (second, b) in all.iter().enumerate().skip(first + 1) {
            let clash = decorations.parts_of(a).iter().any(|pa| {
                decorations
                    .parts_of(b)
                    .iter()
                    .any(|pb| overlaps(pa.bounds, pb.bounds))
            });
            if clash {
                return Err(DecorationError::Overlap { first, second });
            }
        }
    }
    Ok(())
}
