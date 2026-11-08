//! A texture's height map, and the look settings of the relief the hardware renderer
//! draws from it (parallax occlusion mapping). The software renderer does not read
//! any of this; the numbers live here, beside `BUMP_FULL`, so that the GPU gets them
//! from Rust and never spells them again.

use crate::alpha::build;

/// The deepest relief a material may ask for, in texels. A deeper one would march
/// further than a pixel's neighbours can be trusted to agree with.
pub const MAX_DEPTH: u8 = 32;
/// Relief a material gets from a manifest line that gives none: none. A height map
/// is often only the source of a normal map, and parallax is a choice per material.
pub const DEFAULT_DEPTH: u8 = 0;
/// Texels per pixel up to which relief keeps its full depth, and the rate at which
/// it has faded out (the height map aliases beyond that).
pub const PARALLAX_FULL: f32 = 0.75;
pub const PARALLAX_GONE: f32 = 3.0;
/// The steps a march takes looking straight on, and edge on.
pub const MIN_STEPS: f32 = 8.0;
pub const MAX_STEPS: f32 = 32.0;

/// A height map and its halvings, with the depth of the relief it makes.
#[derive(Clone, Debug)]
pub struct HeightPlane {
    base: Vec<u8>,
    /// Same levels as the colour chain: level `l` is `1 << (l + 1)` times smaller.
    mips: Vec<Vec<u8>>,
    top: u8,
    depth: u8,
}

impl HeightPlane {
    pub(crate) fn new(base: Vec<u8>) -> Self {
        let top = base.iter().copied().max().unwrap_or(0);
        Self {
            base,
            mips: Vec::new(),
            top,
            depth: DEFAULT_DEPTH,
        }
    }

    /// Builds the chain for a `w` x `h` plane; a chain that exists is kept.
    pub(crate) fn with_mips(mut self, w: u32, h: u32) -> Self {
        if self.mips.is_empty() {
            self.mips = build(w, h, &self.base);
        }
        self
    }

    pub(crate) fn with_depth(mut self, depth: u8) -> Self {
        self.depth = depth.min(MAX_DEPTH);
        self
    }

    /// Texels of level `level`, 0 the base, clamped to the chain.
    pub fn level(&self, level: u32) -> &[u8] {
        match (level as usize).min(self.mips.len()).checked_sub(1) {
            None => &self.base,
            Some(l) => &self.mips[l],
        }
    }

    /// The highest texel. A march starts from the plane of the face and counts
    /// this as its surface, so a map drawn lower than 255 does not sink the wall
    /// and a flat one draws as no relief at all.
    pub fn top(&self) -> u8 {
        self.top
    }

    /// How deep the relief goes, in texels; 0 draws none.
    pub fn depth(&self) -> u8 {
        self.depth
    }
}
