//! The overrides of `common.wgsl`, which every pipeline that includes it gives
//! alongside its own.

use mscent_render::texel_light::{
    DITHER_BITS, DITHER_DROP, DITHER_FOLD, DITHER_MIX, DITHER_X, DITHER_Y,
};

/// `own`, and what `common.wgsl` takes from the software renderer.
pub(crate) fn with_common<'a>(own: &[(&'a str, f64)]) -> Vec<(&'a str, f64)> {
    let common = [
        ("DITHER_X", f64::from(DITHER_X)),
        ("DITHER_Y", f64::from(DITHER_Y)),
        ("DITHER_MIX", f64::from(DITHER_MIX)),
        ("DITHER_FOLD", f64::from(DITHER_FOLD)),
        ("DITHER_DROP", f64::from(DITHER_DROP)),
        ("DITHER_BITS", f64::from(DITHER_BITS)),
    ];
    own.iter().copied().chain(common).collect()
}
