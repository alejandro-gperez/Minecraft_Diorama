//! Deterministic procedural world generation.
//!
//! Generators run once during scene construction and write block identities into the scene's one
//! global `VoxelGrid`; the renderer then traverses that grid like any other. Nothing here is
//! sampled per ray, per pixel, or per frame, and nothing here depends on rendering.

pub mod island;
pub mod noise;

pub use island::{
    DEFAULT_FOOTPRINT, IslandBounds, IslandConfig, IslandError, IslandStats, generate_island,
};
