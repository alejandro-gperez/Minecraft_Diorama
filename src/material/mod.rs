pub mod auxiliary;
pub mod canonical;
pub mod material;
pub mod normal_map;
pub mod ppm;
pub mod texture;
pub mod texture_registry;

pub use auxiliary::{
    AuxiliaryMaterialDefinitions, AuxiliaryMaterials, AuxiliaryTextureIds,
    auxiliary_material_definitions,
};
pub use canonical::{
    CanonicalMaterialDefinitions, CanonicalMaterials, CanonicalTextureIds, GLASS_IOR,
    LAVA_EMISSION_COLOR, LAVA_EMISSION_STRENGTH, canonical_material_definitions,
};
pub use material::{AIR_IOR, Material, MaterialId, TextureSelection};
pub use normal_map::{decode_tangent_normal, shading_normal, tangent_to_world};
pub use ppm::{PpmLoadError, load_ppm, parse_ppm};
pub use texture::Texture;
pub use texture_registry::{TextureId, TextureRegistry};
