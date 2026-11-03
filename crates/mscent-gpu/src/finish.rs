//! What a GPU adds to a frame that the software renderer cannot afford.

use mscent_render::Quality;

use crate::device::Gpu;

/// Samples per pixel in a fine frame.
const FINE_SAMPLES: u32 = 4;
/// The most texels a fine frame's textures are filtered over along a slant.
const FINE_ANISOTROPY: u16 = 16;

/// How finely a GPU draws a frame, chosen when a level is put on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    /// The software renderer's picture, which `tests/parity.rs` holds the GPU to.
    Reference,
    /// The same shading, with edges anti-aliased by multisampling and textures
    /// filtered trilinearly and anisotropically, so slanted floors stay sharp and
    /// edges do not crawl.
    Fine,
}

impl Finish {
    /// How many finishes there are, for what is kept for each.
    pub(crate) const COUNT: usize = 2;

    /// This finish's place among [`Finish::COUNT`].
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Reference => 0,
            Self::Fine => 1,
        }
    }

    /// What a frame of `quality` gets on `gpu`: [`Finish::Fine`] for the highest,
    /// on a GPU. A CPU driver (Mesa's llvmpipe) gets the reference: multisampling
    /// costs it what it costs software, and its anisotropic filtering does not
    /// sharpen, so a fine frame there is further from the truth than the
    /// reference (5.70 levels against 5.49 down the showcase's hall, in CI).
    pub fn of(quality: Quality, gpu: &Gpu) -> Self {
        if quality == Quality::High && !gpu.is_cpu() {
            Self::Fine
        } else {
            Self::Reference
        }
    }

    pub(crate) fn samples(self) -> u32 {
        match self {
            Self::Reference => 1,
            Self::Fine => FINE_SAMPLES,
        }
    }

    pub(crate) fn multisample(self) -> wgpu::MultisampleState {
        wgpu::MultisampleState {
            count: self.samples(),
            ..Default::default()
        }
    }

    /// The anisotropy of a texture sampler, 1 for none.
    pub(crate) fn anisotropy(self) -> u16 {
        match self {
            Self::Reference => 1,
            Self::Fine => FINE_ANISOTROPY,
        }
    }
}
