//! Software rasterizer: textures, camera, clipping, span rasterization and
//! 2D drawing into an offscreen framebuffer.
//!
//! Lighting plugs in through [`shade::SpanShader`]; BSP traversal and the
//! worker threads are layered on top of [`raster::draw_polygon`].

pub mod bump;
pub mod camera;
pub mod clip;
pub mod draw2d;
pub mod dynlight;
mod face_points;
pub mod font;
pub mod framebuffer;
pub mod inflate;
pub mod jobs;
pub mod level;
pub mod light_sampler;
pub mod lighting_shader;
pub mod lightmap_mips;
pub mod manifest;
pub mod material;
pub mod mesh;
pub mod mips;
pub mod particles;
pub mod png;
pub mod png_encode;
pub mod raster;
pub mod resolution;
pub mod scene;
pub mod shade;
pub mod simd;
pub mod specular;
pub mod texture;
pub mod tiles;
pub mod uv_luxel;
pub mod voxel;

pub use camera::{Camera, Frustum, ScreenPoint};
pub use draw2d::{Sprite, blit, draw_text, fill_rect, stroke_rect, text_size};
pub use dynlight::{DynLight, MAX_DYN_LIGHTS};
pub use framebuffer::Framebuffer;
pub use jobs::JobPool;
pub use level::{
    Actor, ActorKind, BakeCache, BakeReport, BakedLight, BuildConfig, BuildError, Compiled,
    Effects, LevelRenderer, MoverPose, Overlay, Quality, Source, View, VoxelChange,
};
pub use lighting_shader::{FaceLight, LightingShader, UvToLuxel};
pub use lightmap_mips::LightMips;
pub use material::{Material, MaterialId, MaterialTable};
pub use raster::{DrawStats, Vertex, draw_polygon, draw_polygon_flat};
pub use shade::{FlatShader, Fog, SpanEnd, SpanShader};
pub use specular::SpecTable;
pub use texture::{Texture, TextureError, load_texture};
pub use tiles::{MAX_WORKERS, SizeMismatch, TileRenderer};
