use super::Texture;

/// Stable index into CPU-owned texture storage.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(u32);

impl TextureId {
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Default)]
pub struct TextureRegistry {
    textures: Vec<Texture>,
}

impl TextureRegistry {
    pub const fn new() -> Self {
        Self {
            textures: Vec::new(),
        }
    }

    pub fn insert(&mut self, texture: Texture) -> Option<TextureId> {
        let index = u32::try_from(self.textures.len()).ok()?;
        let id = TextureId::new(index);
        self.textures.push(texture);
        Some(id)
    }

    pub fn get(&self, id: TextureId) -> Option<&Texture> {
        self.textures.get(id.index())
    }

    pub fn len(&self) -> usize {
        self.textures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{TextureId, TextureRegistry};
    use crate::{color::Color, material::Texture};

    #[test]
    fn insertion_ids_are_stable_and_lookups_are_bounds_checked() {
        let red = Texture::solid(Color::new(1.0, 0.0, 0.0));
        let blue = Texture::solid(Color::new(0.0, 0.0, 1.0));
        let mut registry = TextureRegistry::new();

        let red_id = registry.insert(red.clone()).unwrap();
        let blue_id = registry.insert(blue.clone()).unwrap();

        assert_eq!(red_id, TextureId::new(0));
        assert_eq!(blue_id, TextureId::new(1));
        assert_eq!(registry.get(red_id), Some(&red));
        assert_eq!(registry.get(blue_id), Some(&blue));
        assert_eq!(registry.get(TextureId::new(u32::MAX)), None);
        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());
    }
}
