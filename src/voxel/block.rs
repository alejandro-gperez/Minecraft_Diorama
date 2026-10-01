use crate::material::{CanonicalMaterials, MaterialId};

/// Minecraft-style block identity occupying a voxel.
///
/// This says what a block is, not how its surface behaves optically; `BlockMaterials` resolves
/// that separately. Several block types may share one material.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockType {
    Grass,
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
    pub const ALL: [Self; 9] = [
        Self::Grass,
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

/// Material identities for the ore block types, which are not canonical rubric materials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OreMaterials {
    pub coal: MaterialId,
    pub iron: MaterialId,
    pub gold: MaterialId,
    pub diamond: MaterialId,
}

/// Resolves every block type to a scene material without storing a material per voxel.
///
/// Each field holds an ID into the scene's central material storage, so lookup is a constant-time
/// exhaustive match: no strings, hashing, or allocation, and no block type can be left unmapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockMaterials {
    pub grass: MaterialId,
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
    /// Maps the canonical block types to their registered canonical materials.
    pub const fn new(canonical: CanonicalMaterials, ores: OreMaterials) -> Self {
        Self {
            grass: canonical.grass,
            cobblestone: canonical.cobblestone,
            obsidian: canonical.obsidian,
            glass: canonical.glass,
            lava: canonical.lava,
            coal_ore: ores.coal,
            iron_ore: ores.iron,
            gold_ore: ores.gold,
            diamond_ore: ores.diamond,
        }
    }

    pub const fn material(&self, block: BlockType) -> MaterialId {
        match block {
            BlockType::Grass => self.grass,
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
    use super::{BlockMaterials, BlockType, OreMaterials};
    use crate::{
        color::Color,
        material::{CanonicalMaterials, CanonicalTextureIds, MaterialId, Texture},
        scene::Scene,
    };

    fn distinct_materials() -> BlockMaterials {
        BlockMaterials {
            grass: MaterialId::new(10),
            cobblestone: MaterialId::new(11),
            obsidian: MaterialId::new(12),
            glass: MaterialId::new(13),
            lava: MaterialId::new(14),
            coal_ore: MaterialId::new(15),
            iron_ore: MaterialId::new(16),
            gold_ore: MaterialId::new(17),
            diamond_ore: MaterialId::new(18),
        }
    }

    fn registered_canonical_materials() -> CanonicalMaterials {
        let mut scene = Scene::new();
        let mut next_texture = || scene.add_texture(Texture::solid(Color::WHITE)).unwrap();
        let textures = CanonicalTextureIds {
            grass_top: next_texture(),
            grass_side: next_texture(),
            dirt: next_texture(),
            cobblestone: next_texture(),
            cobblestone_normal: next_texture(),
            obsidian: next_texture(),
            glass: next_texture(),
            lava: next_texture(),
        };
        scene.add_canonical_materials(textures).unwrap()
    }

    #[test]
    fn block_type_is_one_byte() {
        assert_eq!(size_of::<BlockType>(), 1);
    }

    #[test]
    fn all_lists_every_block_type_once() {
        for (index, block) in BlockType::ALL.iter().enumerate() {
            assert!(!BlockType::ALL[..index].contains(block));
        }
        assert_eq!(BlockType::ALL.len(), 9);
    }

    #[test]
    fn every_block_type_resolves_to_its_own_field() {
        let materials = distinct_materials();
        let expected = [10, 11, 12, 13, 14, 15, 16, 17, 18].map(MaterialId::new);

        for (block, id) in BlockType::ALL.into_iter().zip(expected) {
            assert_eq!(materials.material(block), id, "{block:?}");
        }
    }

    #[test]
    fn canonical_block_types_use_registered_canonical_material_ids() {
        let canonical = registered_canonical_materials();
        let ores = OreMaterials {
            coal: MaterialId::new(5),
            iron: MaterialId::new(6),
            gold: MaterialId::new(7),
            diamond: MaterialId::new(8),
        };
        let materials = BlockMaterials::new(canonical, ores);

        assert_eq!(materials.material(BlockType::Grass), canonical.grass);
        assert_eq!(
            materials.material(BlockType::Cobblestone),
            canonical.cobblestone
        );
        assert_eq!(materials.material(BlockType::Obsidian), canonical.obsidian);
        assert_eq!(materials.material(BlockType::Glass), canonical.glass);
        assert_eq!(materials.material(BlockType::Lava), canonical.lava);
        assert_eq!(materials.material(BlockType::CoalOre), ores.coal);
        assert_eq!(materials.material(BlockType::IronOre), ores.iron);
        assert_eq!(materials.material(BlockType::GoldOre), ores.gold);
        assert_eq!(materials.material(BlockType::DiamondOre), ores.diamond);
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
        const LAVA: MaterialId = BlockMaterials {
            grass: MaterialId::new(0),
            cobblestone: MaterialId::new(1),
            obsidian: MaterialId::new(2),
            glass: MaterialId::new(3),
            lava: MaterialId::new(4),
            coal_ore: MaterialId::new(5),
            iron_ore: MaterialId::new(6),
            gold_ore: MaterialId::new(7),
            diamond_ore: MaterialId::new(8),
        }
        .material(BlockType::Lava);

        assert_eq!(LAVA, MaterialId::new(4));
    }
}
