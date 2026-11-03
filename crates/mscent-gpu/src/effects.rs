//! The looks only the GPU draws, each with a switch: projected shadows and
//! parallax relief (`docs/hardware-effects.md`). They follow the frame's quality,
//! on at High, unless a caller sets them. A frame with an effect switched off is
//! the frame as it was drawn before the effect existed.

use mscent_render::Quality;

/// Which of the GPU-only looks a frame draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuEffects {
    /// Projected shadows from the frame's dynamic lights.
    pub shadows: bool,
    /// Parallax relief on faces whose material has a height map.
    pub parallax: bool,
}

impl GpuEffects {
    /// Neither.
    pub const OFF: Self = Self {
        shadows: false,
        parallax: false,
    };

    /// What a frame of `quality` draws unless told otherwise: both at High.
    pub fn at(quality: Quality) -> Self {
        let high = quality == Quality::High;
        Self {
            shadows: high,
            parallax: high,
        }
    }

    /// The bytes of `parallax_on` and `shadows_on` in `Frame`.
    pub(crate) fn push(self, bytes: &mut Vec<u8>) {
        for on in [self.parallax, self.shadows] {
            bytes.extend_from_slice(&u32::from(on).to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_draws_both_and_the_rest_neither() {
        assert_eq!(
            GpuEffects::at(Quality::High),
            GpuEffects {
                shadows: true,
                parallax: true
            }
        );
        assert_eq!(GpuEffects::at(Quality::Medium), GpuEffects::OFF);
        assert_eq!(GpuEffects::at(Quality::Low), GpuEffects::OFF);
    }
}
