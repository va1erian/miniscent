//! Software rasterizer: textures, camera, clipping, span rasterization and
//! 2D drawing into an offscreen framebuffer.
//!
//! Lighting plugs in through [`shade::SpanShader`]; BSP traversal and the
//! worker threads are layered on top of [`raster::draw_polygon`].

pub mod camera;
pub mod clip;
pub mod draw2d;
pub mod font;
pub mod framebuffer;
pub mod inflate;
pub mod png;
pub mod raster;
pub mod shade;
pub mod texture;

pub use camera::{Camera, Frustum, ScreenPoint};
pub use draw2d::{Sprite, blit, draw_text, fill_rect, stroke_rect, text_size};
pub use framebuffer::Framebuffer;
pub use raster::{DrawStats, Vertex, draw_polygon, draw_polygon_flat};
pub use shade::{FlatShader, Fog, SpanEnd, SpanShader};
pub use texture::{Material, MaterialId, MaterialTable, Texture, load_texture};
