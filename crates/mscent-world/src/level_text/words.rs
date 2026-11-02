//! The words of one line of level text, and the numbers read from them.

use mscent_math::{Vec3, Vec3d};

use crate::vista_scale::{MAX_VISTA_SCALE, MIN_SCALE, parallax_of_scale};

/// The words of one line, read in order.
pub(super) struct Words<'a> {
    pub(super) rest: std::str::SplitWhitespace<'a>,
}

pub(super) type Res<T> = Result<T, String>;

impl<'a> Words<'a> {
    pub(super) fn word(&mut self, what: &str) -> Res<&'a str> {
        self.rest
            .next()
            .ok_or_else(|| format!("expected {what} at the end of the line"))
    }

    pub(super) fn parse<T: std::str::FromStr>(&mut self, what: &str) -> Res<T> {
        let w = self.word(what)?;
        w.parse()
            .map_err(|_| format!("expected {what}, not '{}'", clip(w)))
    }

    pub(super) fn f32(&mut self, what: &str) -> Res<f32> {
        let v: f32 = self.parse(what)?;
        if v.is_finite() {
            Ok(v)
        } else {
            Err(format!("{what} must be finite"))
        }
    }

    pub(super) fn f64(&mut self, what: &str) -> Res<f64> {
        let v: f64 = self.parse(what)?;
        if v.is_finite() {
            Ok(v)
        } else {
            Err(format!("{what} must be finite"))
        }
    }

    /// A `u32` in decimal or, after `0x`, hex.
    pub(super) fn colour(&mut self, what: &str) -> Res<u32> {
        let w = self.word(what)?;
        let parsed = match w.strip_prefix("0x") {
            Some(hex) => u32::from_str_radix(hex, 16),
            None => w.parse(),
        };
        parsed.map_err(|_| format!("expected {what}, not '{}'", clip(w)))
    }

    /// A vista's parallax: a number, or `x` and a scale of at least 1 for its
    /// inverse (`x16` is 1/16), which is how people think of how small a room is.
    pub(super) fn parallax(&mut self, what: &str) -> Res<f32> {
        let w = self.word(what)?;
        let Some(scale) = w.strip_prefix('x') else {
            return w
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("expected {what}, not '{}'", clip(w)));
        };
        match scale.parse::<f32>() {
            Ok(s) if (MIN_SCALE..=MAX_VISTA_SCALE).contains(&s) => Ok(parallax_of_scale(s)),
            _ => Err(format!(
                "a scale is x then a number from {MIN_SCALE} to {MAX_VISTA_SCALE}, not '{}'",
                clip(w)
            )),
        }
    }

    pub(super) fn v3(&mut self, what: &str) -> Res<Vec3> {
        Ok(Vec3::new(self.f32(what)?, self.f32(what)?, self.f32(what)?))
    }

    pub(super) fn v3d(&mut self, what: &str) -> Res<Vec3d> {
        Ok(Vec3d::new(
            self.f64(what)?,
            self.f64(what)?,
            self.f64(what)?,
        ))
    }
}

/// Hostile lines can be megabytes long; messages quote only the start.
pub(super) fn clip(w: &str) -> String {
    w.chars().take(32).collect()
}
