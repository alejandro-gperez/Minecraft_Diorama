use crate::material::{AuxiliaryMaterials, CanonicalMaterials, MaterialId};

/// Minecraft-style block identity occupying a voxel.
///
/// This says what a block is, not how its surface behaves optically; `BlockMaterials` resolves
/// that separately. Several block types may share one material.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockType {
    Grass,
    Dirt,
    Cobblestone,
    Obsidian,
    Glass,
    Lava,
    CoalOre,
    IronOre,
    GoldOre,
    DiamondOre,
}

impl BlockType {
    /// Every block type, in declaration order.
    pub const ALL: [Self; 10] = [
        Self::Grass,
        Self::Dirt,
        Self::Cobblestone,
        Self::Obsidian,
        Self::Glass,
        Self::Lava,
        Self::CoalOre,
        Self::IronOre,
        Self::GoldOre,
        Self::DiamondOre,
    ];
}

/// Resolves every block type to a scene material without storing a material per voxel.
///
/// Each field holds an ID into the scene's central material storage, so lookup is a constant-time
/// exhaustive match: no strings, hashing, or allocation, and no block type can be left unmapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockMaterials {
    pub grass: MaterialId,
    pub dirt: MaterialId,
    pub cobblestone: MaterialId,
    pub obsidian: MaterialId,
    pub glass: MaterialId,
    pub lava: MaterialId,
    pub coal_ore: MaterialId,
    pub iron_ore: MaterialId,
    pub gold_ore: MaterialId,
    pub diamond_ore: MaterialId,
}

impl BlockMaterials {
    /// Maps the five canonical block types to the canonical rubric materials and dirt and the
    /// ores to the auxiliary terrain materials.
    pub const fn new(canonical: CanonicalMaterials, auxiliary: AuxiliaryMaterials) -> Self {
        Self {
            grass: canonical.grass,
            dirt: auxiliary.dirt,
            cobblestone: canonical.cobblestone,
            obsidian: canonical.obsidian,
            glass: canonical.glass,
            lava: canonical.lava,
            coal_ore: auxiliary.coal_ore,
            iron_ore: auxiliary.iron_ore,
            gold_ore: auxiliary.gold_ore,
            diamond_ore: auxiliary.diamond_ore,
        }
    }

    pub const fn material(&self, block: BlockType) -> MaterialId {
        match block {
            BlockType::Grass => self.grass,
            BlockType::Dirt => self.dirt,
            BlockType::Cobblestone => self.cobblestone,
            BlockType::Obsidian => self.obsidian,
            BlockType::Glass => self.glass,
            BlockType::Lava => self.lava,
            BlockType::CoalOre => self.coal_ore,
            BlockType::IronOre => self.iron_ore,
            BlockType::GoldOre => self.gold_ore,
            BlockType::DiamondOre => self.diamond_ore,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockMaterials, BlockType};
    use crate::{
        color::Color,
        material::{
            AuxiliaryMaterials, AuxiliaryTextureIds, CanonicalMaterials, CanonicalTextureIds,
            MaterialId, Texture,
        },
        scene::Scene,
        voxel::Voxel,
    };

    fn distinct_materials() -> BlockMaterials {
        BlockMaterials {
            grass: MaterialId::new(10),
            dirt: MaterialId::new(11),
            cobblestone: MaterialId::new(12),
            obsidian: MaterialId::new(13),
            glass: MaterialId::new(14),
            lava: MaterialId::new(15),
            coal_ore: MaterialId::new(16),
            iron_ore: MaterialId::new(17),
            gold_ore: MaterialId::new(18),
            diamond_ore: MaterialId::new(19),
        }
    }

    fn registered_materials() -> (CanonicalMaterials, AuxiliaryMaterials) {
        let mut scene = Scene::new();
        let mut next_texture = || scene.add_texture(Texture::solid(Color::WHITE)).unwrap();
        let canonical_textures = CanonicalTextureIds {
            grass_top: next_texture(),
            grass_side: next_texture(),
            dirt: next_texture(),
            cobblestone: next_texture(),
            cobblestone_normal: next_texture(),
            obsidian: next_texture(),
            glass: next_texture(),
            lava: next_texture(),
        };
        let auxiliary_textures = AuxiliaryTextureIds {
            dirt: canonical_textures.dirt,
            coal_ore: next_texture(),
            iron_ore: next_texture(),
            gold_ore: next_texture(),
            diamond_ore: next_texture(),
        };
        let canonical = scene.add_canonical_materials(canonical_textures).unwrap();
        let auxiliary = scene.add_auxiliary_materials(auxiliary_textures).unwrap();
        (canonical, auxiliary)
    }

    #[test]
    fn block_type_and_voxel_are_one_byte() {
        assert_eq!(size_of::<BlockType>(), 1);
        assert_eq!(size_of::<Voxel>(), 1);
    }

    #[test]
    fn all_lists_every_block_type_once() {
        for (index, block) in BlockType::ALL.iter().enumerate() {
            assert!(!BlockType::ALL[..index].contains(block));
            // The discriminant is the declaration index, so a variant missing from `ALL` would
            // leave a gap that this catches together with the length check.
            assert_eq!(*block as usize, index);
        }
        assert_eq!(BlockType::ALL.len(), 10);
        assert!(BlockType::ALL.contains(&BlockType::Dirt));
    }

    #[test]
    fn every_block_type_resolves_to_its_own_field() {
        let materials = distinct_materials();
        let expected = [10, 11, 12, 13, 14, 15, 16, 17, 18, 19].map(MaterialId::new);

        for (block, id) in BlockType::ALL.into_iter().zip(expected) {
            assert_eq!(materials.material(block), id, "{block:?}");
        }
    }

    #[test]
    fn every_block_type_maps_to_its_registered_canonical_or_auxiliary_material() {
        let (canonical, auxiliary) = registered_materials();
        let materials = BlockMaterials::new(canonical, auxiliary);

        for block in BlockType::ALL {
            let expected = match block {
                BlockType::Grass => canonical.grass,
                BlockType::Dirt => auxiliary.dirt,
                BlockType::Cobblestone => canonical.cobblestone,
                BlockType::Obsidian => canonical.obsidian,
                BlockType::Glass => canonical.glass,
                BlockType::Lava => canonical.lava,
                BlockType::CoalOre => auxiliary.coal_ore,
                BlockType::IronOre => auxiliary.iron_ore,
                BlockType::GoldOre => auxiliary.gold_ore,
                BlockType::DiamondOre => auxiliary.diamond_ore,
            };
            assert_eq!(materials.material(block), expected, "{block:?}");
        }
    }

    #[test]
    fn registered_block_types_resolve_to_distinct_materials() {
        let (canonical, auxiliary) = registered_materials();
        let materials = BlockMaterials::new(canonical, auxiliary);
        let resolved = BlockType::ALL.map(|block| materials.material(block));

        for (index, id) in resolved.iter().enumerate() {
            assert!(
                !resolved[..index].contains(id),
                "{:?}",
                BlockType::ALL[index]
            );
        }
    }

    #[test]
    fn block_types_may_share_one_material() {
        let shared = MaterialId::new(1);
        let materials = BlockMaterials {
            coal_ore: shared,
            iron_ore: shared,
            ..distinct_materials()
        };

        assert_eq!(materials.material(BlockType::CoalOre), shared);
        assert_eq!(materials.material(BlockType::IronOre), shared);
        assert_ne!(materials.material(BlockType::GoldOre), shared);
    }

    #[test]
    fn material_lookup_evaluates_at_compile_time() {
        // Const evaluation cannot allocate, so this proves the lookup is allocation-free.
        const MATERIALS: BlockMaterials = BlockMaterials {
            grass: MaterialId::new(0),
            dirt: MaterialId::new(1),
            cobblestone: MaterialId::new(2),
            obsidian: MaterialId::new(3),
            glass: MaterialId::new(4),
            lava: MaterialId::new(5),
            coal_ore: MaterialId::new(6),
            iron_ore: MaterialId::new(7),
            gold_ore: MaterialId::new(8),
            diamond_ore: MaterialId::new(9),
        };
        const LAVA: MaterialId = MATERIALS.material(BlockType::Lava);
        const DIRT: MaterialId = MATERIALS.material(BlockType::Dirt);

        assert_eq!(LAVA, MaterialId::new(5));
        assert_eq!(DIRT, MaterialId::new(1));
    }
}
