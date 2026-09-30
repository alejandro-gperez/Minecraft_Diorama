pub mod material;
pub mod ppm;
pub mod texture;
pub mod texture_registry;

pub use material::{Material, MaterialId, TextureSelection};
pub use ppm::{PpmLoadError, load_ppm, parse_ppm};
pub use texture::Texture;
pub use texture_registry::{TextureId, TextureRegistry};
