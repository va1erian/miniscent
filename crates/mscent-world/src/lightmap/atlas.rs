//! Shelf packing of per-face luxel rectangles into one atlas.

/// Gutter, in luxels, around every rectangle. Filtering that reads one luxel
/// past a face edge sees a copy of the edge, never a neighbour's light.
pub const PADDING: u32 = 1;

/// Position of a face's luxels inside the atlas (without the gutter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Why packing failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasError {
    /// A rectangle (with its gutter) is wider than the atlas.
    TooWide(usize),
    /// Everything together exceeds the height limit.
    TooTall,
    /// A zero-sized rectangle or a sum that overflows.
    BadSize(usize),
}

/// A finished layout. `rects()[i]` belongs to input `i`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atlas {
    width: u32,
    height: u32,
    rects: Vec<Rect>,
}

impl Atlas {
    /// Packs `sizes` (width, height in luxels) into an atlas `width` wide that
    /// grows up to `max_height`. Tallest first keeps shelves tight; ties break
    /// by input order so the layout is deterministic.
    pub fn pack(sizes: &[(u32, u32)], width: u32, max_height: u32) -> Result<Self, AtlasError> {
        let mut order: Vec<usize> = (0..sizes.len()).collect();
        order.sort_by_key(|&i| {
            (
                std::cmp::Reverse(sizes[i].1),
                std::cmp::Reverse(sizes[i].0),
                i,
            )
        });
        let empty = Rect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        };
        let mut rects = vec![empty; sizes.len()];
        let (mut x, mut y, mut shelf_h) = (0u32, 0u32, 0u32);
        for i in order {
            let (w, h) = sizes[i];
            let (pw, ph) = padded(w, h).ok_or(AtlasError::BadSize(i))?;
            if pw > width {
                return Err(AtlasError::TooWide(i));
            }
            if x.checked_add(pw).ok_or(AtlasError::BadSize(i))? > width {
                y = y.checked_add(shelf_h).ok_or(AtlasError::TooTall)?;
                (x, shelf_h) = (0, 0);
            }
            rects[i] = Rect {
                x: x + PADDING,
                y: y + PADDING,
                w,
                h,
            };
            x += pw;
            shelf_h = shelf_h.max(ph);
        }
        let height = y.checked_add(shelf_h).ok_or(AtlasError::TooTall)?;
        if height > max_height {
            return Err(AtlasError::TooTall);
        }
        Ok(Self {
            width,
            height,
            rects,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// Debug dump as a binary PPM (P6): black free space, grey gutter, a
    /// distinct flat colour per rectangle.
    pub fn debug_ppm(&self) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut px = vec![[0u8; 3]; w * h];
        for (i, r) in self.rects.iter().enumerate() {
            let colour = rect_colour(i);
            for y in r.y - PADDING..r.y + r.h + PADDING {
                for x in r.x - PADDING..r.x + r.w + PADDING {
                    let inside = x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;
                    px[y as usize * w + x as usize] = if inside { colour } else { [64, 64, 64] };
                }
            }
        }
        ppm(w, h, px.iter().copied())
    }
}

/// Binary PPM from row-major RGB pixels.
pub fn ppm(w: usize, h: usize, pixels: impl Iterator<Item = [u8; 3]>) -> Vec<u8> {
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in pixels {
        out.extend_from_slice(&p);
    }
    out
}

fn padded(w: u32, h: u32) -> Option<(u32, u32)> {
    if w == 0 || h == 0 {
        return None;
    }
    Some((w.checked_add(2 * PADDING)?, h.checked_add(2 * PADDING)?))
}

fn rect_colour(i: usize) -> [u8; 3] {
    let k = i as u32;
    // Different multipliers per channel so neighbours differ visibly.
    [
        (k.wrapping_mul(97) % 200 + 55) as u8,
        (k.wrapping_mul(61) % 200 + 55) as u8,
        (k.wrapping_mul(139) % 200 + 55) as u8,
    ]
}
