//! The one pixel format: RGBA at 16 bits per channel, so any operator's output
//! feeds any other's input. Grey data has R = G = B. Sampling always wraps.

use crate::error::TexError;

/// Largest side of an image. Recipes are untrusted, and at this size one image
/// is already 8 MB.
pub const MAX_SIDE: u32 = 1024;

/// One texel, RGBA, `0..=65535`.
pub type Px = [u16; 4];

/// Checked image dimensions: powers of two up to [`MAX_SIDE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Size {
    w: u32,
    h: u32,
}

impl Size {
    pub fn new(w: u32, h: u32) -> Result<Self, TexError> {
        let ok = |n: u32| n.is_power_of_two() && n <= MAX_SIDE;
        if ok(w) && ok(h) {
            Ok(Self { w, h })
        } else {
            Err(TexError::BadSize)
        }
    }

    pub fn width(self) -> u32 {
        self.w
    }

    pub fn height(self) -> u32 {
        self.h
    }

    /// Texels of an image of this size; cannot overflow, the sides are capped.
    pub fn texels(self) -> usize {
        self.w as usize * self.h as usize
    }

    /// Bytes of an image of this size.
    pub fn bytes(self) -> usize {
        self.texels() * size_of::<Px>()
    }
}

/// Float channel to stored channel, clamped. NaN stores as 0.
#[inline]
pub fn to_u16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
}

/// Stored channel to float, `0.0..=1.0`.
#[inline]
pub fn unit(v: u16) -> f32 {
    f32::from(v) / 65535.0
}

/// Stored texel to floats.
#[inline]
pub fn unit4(p: Px) -> [f32; 4] {
    [unit(p[0]), unit(p[1]), unit(p[2]), unit(p[3])]
}

/// Float texel to stored.
#[inline]
pub fn to_px(c: [f32; 4]) -> Px {
    [to_u16(c[0]), to_u16(c[1]), to_u16(c[2]), to_u16(c[3])]
}

/// A tile of pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    size: Size,
    px: Vec<Px>,
}

impl Image {
    /// An image of one colour.
    pub fn flat(size: Size, px: Px) -> Self {
        Self {
            size,
            px: vec![px; size.texels()],
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn pixels(&self) -> &[Px] {
        &self.px
    }

    /// Texel with wrap-around addressing.
    #[inline]
    pub fn at(&self, x: i32, y: i32) -> Px {
        let xm = (x as u32 & (self.size.w - 1)) as usize;
        let ym = (y as u32 & (self.size.h - 1)) as usize;
        self.px[ym * self.size.w as usize + xm]
    }

    /// A new image from a function of each texel, by coordinates.
    pub fn from_xy(size: Size, mut f: impl FnMut(i32, i32) -> Px) -> Self {
        let mut px = Vec::with_capacity(size.texels());
        for y in 0..size.h as i32 {
            for x in 0..size.w as i32 {
                px.push(f(x, y));
            }
        }
        Self { size, px }
    }

    /// A new image from a function of each texel of this one.
    pub fn map(&self, mut f: impl FnMut(Px) -> Px) -> Self {
        Self {
            size: self.size,
            px: self.px.iter().map(|&p| f(p)).collect(),
        }
    }

    /// Bilinear fetch at texel coordinates, where `(0.5, 0.5)` is the centre of
    /// the first texel. Wraps.
    pub fn bilinear(&self, x: f32, y: f32) -> [f32; 4] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (ix, iy) = (x0 as i32, y0 as i32);
        let row = |y: i32| {
            let (a, b) = (unit4(self.at(ix, y)), unit4(self.at(ix.wrapping_add(1), y)));
            core::array::from_fn::<f32, 4, _>(|i| a[i] + (b[i] - a[i]) * tx)
        };
        let (a, b) = (row(iy), row(iy.wrapping_add(1)));
        core::array::from_fn(|i| a[i] + (b[i] - a[i]) * ty)
    }
}

/// Where an operator is drawing. Patterns are functions of normalized tile
/// coordinates, and a tile is one unit wide. `origin` is `(0, 0)` except when a
/// test moves the window to prove that the pattern repeats (see [`crate::ops`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub size: Size,
    pub origin: [f32; 2],
}

impl Frame {
    pub fn new(size: Size) -> Self {
        Self {
            size,
            origin: [0.0, 0.0],
        }
    }

    /// Normalized coordinates of a texel's centre.
    #[inline]
    pub fn uv(&self, x: i32, y: i32) -> (f32, f32) {
        (
            self.origin[0] + (x as f32 + 0.5) / self.size.w as f32,
            self.origin[1] + (y as f32 + 0.5) / self.size.h as f32,
        )
    }

    /// An image from a function of normalized coordinates, RGBA in `0.0..=1.0`.
    pub fn fill(&self, mut f: impl FnMut(f32, f32) -> [f32; 4]) -> Image {
        Image::from_xy(self.size, |x, y| {
            let (u, v) = self.uv(x, y);
            to_px(f(u, v))
        })
    }

    /// Bilinear fetch from `image`, which covers this frame, at normalized
    /// coordinates. This is how warps read, so they stay right when the window
    /// moves.
    pub fn sample(&self, image: &Image, u: f32, v: f32) -> [f32; 4] {
        image.bilinear(
            (u - self.origin[0]) * self.size.w as f32,
            (v - self.origin[1]) * self.size.h as f32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s8() -> Size {
        Size::new(8, 8).expect("size")
    }

    #[test]
    fn sizes_are_checked() {
        for (w, h) in [(0, 8), (8, 0), (3, 8), (8, 2048), (u32::MAX, 1)] {
            assert_eq!(Size::new(w, h), Err(TexError::BadSize));
        }
        assert!(Size::new(1, 1024).is_ok());
    }

    #[test]
    fn channels_round_trip_and_clamp() {
        for v in [0u16, 1, 255, 32768, 65535] {
            assert_eq!(to_u16(unit(v)), v);
        }
        assert_eq!(to_u16(-1.0), 0);
        assert_eq!(to_u16(2.0), 65535);
        assert_eq!(to_u16(f32::NAN), 0);
    }

    #[test]
    fn addressing_wraps_at_every_extreme() {
        let img = Image::from_xy(s8(), |x, y| [x as u16, y as u16, 0, 0]);
        assert_eq!(img.at(-1, 8), [7, 0, 0, 0]);
        assert_eq!(img.at(i32::MIN, i32::MAX), [0, 7, 0, 0]);
    }

    #[test]
    fn a_texel_centre_reads_that_texel_and_midpoints_blend_across_the_edge() {
        let img = Image::from_xy(s8(), |x, _| [if x == 0 { 65535 } else { 0 }; 4]);
        assert_eq!(img.bilinear(0.5, 0.5)[0], 1.0);
        assert_eq!(img.bilinear(1.5, 0.5)[0], 0.0);
        assert_eq!(img.bilinear(0.0, 0.5)[0], 0.5);
        assert_eq!(img.bilinear(8.0, 0.5)[0], 0.5);
    }

    #[test]
    fn fetching_garbage_coordinates_does_not_panic() {
        let img = Image::flat(s8(), [1, 2, 3, 4]);
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30, -1e30] {
            let _ = img.bilinear(v, 0.5);
            let _ = img.bilinear(0.5, v);
        }
    }
}
