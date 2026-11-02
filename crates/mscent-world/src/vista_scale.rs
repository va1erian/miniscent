//! Scale, the way people set how small a vista's room is. The stored field is
//! `parallax`, the share of the player's movement the layer's eye follows, and
//! scale is its inverse: a room at scale 16 is authored 16 times smaller than it
//! looks. Parallax 0 is infinitely far, which has no finite scale.

/// Smallest scale: the room shown at its own size.
pub const MIN_SCALE: f32 = 1.0;
/// Largest scale a panel or a `vista` line sets.
pub const MAX_VISTA_SCALE: f32 = 4096.0;

/// How much coarser than the play space a face in a layer's room is baked at scale
/// 1, and in an infinitely far room: one-unit luxels. At 32 (four units, the first
/// choice) a two-unit pole's shadow washed out to 5% of its depth; at 8 it keeps
/// 80%. A face's luxels per side are capped (`MAX_FACE_LUXELS`), so a big room
/// cannot take more atlas than that however fine this is.
pub const VIEW_LUXEL_SCALE: f32 = 8.0;

/// The parallax for `scale`: 0 (and anything not a number) is infinite, the rest
/// is held to `1..=MAX_VISTA_SCALE` first.
pub fn parallax_of_scale(scale: f32) -> f32 {
    if scale == 0.0 || scale.is_nan() {
        return 0.0;
    }
    1.0 / scale.clamp(MIN_SCALE, MAX_VISTA_SCALE)
}

/// The scale of `parallax`, or 0 for an infinitely far layer. A parallax below
/// the smallest the scale range holds reads as that range's end.
pub fn scale_of_parallax(parallax: f32) -> f32 {
    if parallax > 0.0 {
        (1.0 / parallax).clamp(MIN_SCALE, MAX_VISTA_SCALE)
    } else {
        0.0
    }
}

/// How many times coarser than the play space a face is baked in a room whose
/// layer has this `parallax`: a luxel there is as big on screen as one of
/// [`VIEW_LUXEL_SCALE`] at scale 1, so `VIEW_LUXEL_SCALE / scale`, and never finer
/// than the play space's own. An infinitely far room keeps [`VIEW_LUXEL_SCALE`].
pub fn luxel_factor(parallax: f32) -> f32 {
    if parallax > 0.0 {
        (VIEW_LUXEL_SCALE * parallax).clamp(1.0, VIEW_LUXEL_SCALE)
    } else {
        VIEW_LUXEL_SCALE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_luxel_factor_follows_scale() {
        let factor = |scale: f32| luxel_factor(parallax_of_scale(scale));
        assert_eq!(factor(1.0), 8.0);
        assert_eq!(factor(2.0), 4.0);
        assert_eq!(factor(4.0), 2.0);
        assert_eq!(factor(16.0), 1.0, "held to the play space's own density");
        assert_eq!(factor(32.0), 1.0);
        assert_eq!(factor(64.0), 1.0);
        assert_eq!(factor(4096.0), 1.0);
        assert_eq!(
            factor(0.0),
            VIEW_LUXEL_SCALE,
            "infinite keeps the base factor"
        );
        assert_eq!(luxel_factor(f32::NAN), VIEW_LUXEL_SCALE);
    }

    #[test]
    fn scale_and_parallax_convert_and_clamp() {
        assert_eq!(parallax_of_scale(16.0), 0.0625);
        assert_eq!(scale_of_parallax(0.0625), 16.0);
        assert_eq!(scale_of_parallax(0.0), 0.0);
        assert_eq!(parallax_of_scale(0.0), 0.0);
        assert_eq!(parallax_of_scale(0.5), 1.0, "below 1 is held to 1");
        assert_eq!(parallax_of_scale(1.0e9), 1.0 / MAX_VISTA_SCALE);
        assert_eq!(parallax_of_scale(f32::NAN), 0.0);
        assert_eq!(scale_of_parallax(1.0e-9), MAX_VISTA_SCALE);
    }
}
