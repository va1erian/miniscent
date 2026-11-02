//! What the operators' behaviour tests share: run one by name, read texels as
//! floats, build small inputs.

use super::{Frame, Image, Params, Value, eval, find};
use crate::error::TexError;
use crate::image::{Size, to_px, unit4};

pub(super) fn square(n: u32) -> Size {
    Size::new(n, n).expect("size")
}

/// Runs the named operator with some parameters changed from their defaults.
pub(super) fn run(
    name: &str,
    set: &[(&str, Value)],
    inputs: &[&Image],
    size: Size,
) -> Result<Image, TexError> {
    let spec = find(name).expect(name);
    let mut params = Params::defaults(spec);
    for (key, value) in set {
        params.set(spec, key, value.clone())?;
    }
    eval(spec, &params, inputs, Frame::new(size))
}

/// [`run`] for a case that must succeed.
pub(super) fn make(name: &str, set: &[(&str, Value)], inputs: &[&Image], size: Size) -> Image {
    run(name, set, inputs, size).expect(name)
}

pub(super) fn rgba(image: &Image, x: i32, y: i32) -> [f32; 4] {
    unit4(image.at(x, y))
}

/// The red channel of a texel, `0.0..=1.0`.
pub(super) fn red(image: &Image, x: i32, y: i32) -> f32 {
    rgba(image, x, y)[0]
}

/// An image from a function of texel coordinates giving floats.
pub(super) fn image_of(size: Size, f: impl Fn(i32, i32) -> [f32; 4]) -> Image {
    Image::from_xy(size, |x, y| to_px(f(x, y)))
}

pub(super) fn grey_image(size: Size, f: impl Fn(i32, i32) -> f32) -> Image {
    image_of(size, |x, y| {
        let v = f(x, y);
        [v, v, v, 1.0]
    })
}

pub(super) fn flat(size: Size, c: [f32; 4]) -> Image {
    image_of(size, |_, _| c)
}

/// The sum of the red channel over the tile, in texels' worth of white.
pub(super) fn white_area(image: &Image) -> f32 {
    image.pixels().iter().map(|p| unit4(*p)[0]).sum()
}

pub(super) fn close(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}
