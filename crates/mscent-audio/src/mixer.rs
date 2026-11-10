//! Mixing sounds into stereo at the output's rate.
//!
//! The mixer owns no device and does no locking: the device callback owns one
//! and feeds it [`Play`] requests it received over a channel. That keeps the
//! audio thread free of blocking calls, and lets tests read the mixed samples.

use crate::wav::Sound;

/// Sounds playing at once. A new one past this replaces the oldest, which in a
/// firefight is the one nobody is listening to any more.
pub const MAX_VOICES: usize = 24;
/// Headroom so several loud sounds sum without clipping the output.
const MASTER: f32 = 0.5;

/// A request to start a sound.
#[derive(Clone, Debug)]
pub struct Play {
    pub sound: Sound,
    /// Linear gain, 0 silent to 1 full.
    pub gain: f32,
    /// -1 left, 0 centre, 1 right.
    pub pan: f32,
}

struct Voice {
    sound: Sound,
    /// Position in the sound's own samples.
    at: f64,
    left: f32,
    right: f32,
}

pub struct Mixer {
    rate: u32,
    voices: Vec<Voice>,
}

/// Left and right gains for `gain` and `pan`, constant power so a sound does
/// not get quieter as it crosses the middle. NaN and out-of-range inputs are
/// brought into range: a level or script can feed these numbers.
fn pan_gains(gain: f32, pan: f32) -> (f32, f32) {
    let gain = if gain.is_finite() {
        gain.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let pan = if pan.is_finite() {
        pan.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let angle = (pan + 1.0) * core::f32::consts::FRAC_PI_4;
    (gain * angle.cos(), gain * angle.sin())
}

impl Mixer {
    pub fn new(rate: u32) -> Self {
        Self {
            rate: rate.max(1),
            voices: Vec::with_capacity(MAX_VOICES),
        }
    }

    pub fn voices(&self) -> usize {
        self.voices.len()
    }

    pub fn play(&mut self, req: Play) {
        if self.voices.len() == MAX_VOICES {
            self.voices.remove(0);
        }
        let (left, right) = pan_gains(req.gain, req.pan);
        self.voices.push(Voice {
            sound: req.sound,
            at: 0.0,
            left,
            right,
        });
    }

    /// Mixes the next `out.len() / 2` frames of every voice into `out`, which is
    /// interleaved left, right and is overwritten. Finished voices are dropped.
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let rate = self.rate;
        self.voices.retain_mut(|v| v.mix_into(out, rate));
        for s in out.iter_mut() {
            *s = (*s * MASTER).clamp(-1.0, 1.0);
        }
    }
}

impl Voice {
    /// Adds this voice to `out`; false once the sound has played out.
    fn mix_into(&mut self, out: &mut [f32], out_rate: u32) -> bool {
        let step = f64::from(self.sound.rate) / f64::from(out_rate);
        let samples = &self.sound.samples;
        let (frames, _) = out.as_chunks_mut::<2>();
        for frame in frames {
            let i = self.at as usize;
            let Some(&a) = samples.get(i) else {
                return false;
            };
            // Linear interpolation; the last sample fades towards silence.
            let b = samples.get(i + 1).copied().unwrap_or(0);
            let t = (self.at - i as f64) as f32;
            let s = (f32::from(a) + (f32::from(b) - f32::from(a)) * t) / 32768.0;
            frame[0] += s * self.left;
            frame[1] += s * self.right;
            self.at += step;
        }
        (self.at as usize) < samples.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, len: usize, value: i16) -> Sound {
        Sound {
            rate,
            samples: vec![value; len].into(),
        }
    }

    fn play(sound: Sound, gain: f32, pan: f32) -> Play {
        Play { sound, gain, pan }
    }

    #[test]
    fn centre_is_equal_power_on_both_sides() {
        let mut m = Mixer::new(1000);
        m.play(play(tone(1000, 8, 16384), 1.0, 0.0));
        let mut out = [0.0; 4];
        m.mix(&mut out);
        assert!((out[0] - out[1]).abs() < 1e-6);
        // Half of full scale, times cos(45 degrees), times the master gain.
        assert!(
            (out[0] - 0.5 * core::f32::consts::FRAC_1_SQRT_2 * MASTER).abs() < 1e-4,
            "{out:?}"
        );
    }

    #[test]
    fn hard_pan_silences_the_other_side() {
        let mut m = Mixer::new(1000);
        m.play(play(tone(1000, 8, 16384), 1.0, -1.0));
        let mut out = [0.0; 4];
        m.mix(&mut out);
        assert!(out[0] > 0.2 && out[1].abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn a_sound_ends_and_its_voice_is_freed() {
        let mut m = Mixer::new(1000);
        m.play(play(tone(1000, 3, 1000), 1.0, 0.0));
        let mut out = [0.0; 16];
        m.mix(&mut out);
        assert_eq!(m.voices(), 0);
        assert!(out[0] != 0.0 && out[8..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn a_slow_sound_is_stretched_to_the_output_rate() {
        let mut m = Mixer::new(2000);
        m.play(play(tone(1000, 4, 20000), 1.0, 0.0));
        let mut out = [0.0; 32];
        m.mix(&mut out);
        let live = out.chunks(2).filter(|f| f[0] != 0.0).count();
        // Four samples at half the output rate last eight frames.
        assert_eq!(live, 8);
    }

    #[test]
    fn bad_gain_and_pan_are_harmless() {
        let bad = [
            (f32::NAN, 0.0),
            (1.0, f32::NAN),
            (9.0, 50.0),
            (-3.0, f32::INFINITY),
        ];
        for (g, p) in bad {
            let mut m = Mixer::new(1000);
            m.play(play(tone(1000, 4, i16::MAX), g, p));
            let mut out = [0.0; 8];
            m.mix(&mut out);
            assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        }
    }

    #[test]
    fn the_oldest_voice_is_replaced_at_the_cap() {
        let mut m = Mixer::new(1000);
        for _ in 0..MAX_VOICES + 5 {
            m.play(play(tone(1000, 100, 1), 1.0, 0.0));
        }
        assert_eq!(m.voices(), MAX_VOICES);
    }

    #[test]
    fn loud_voices_sum_without_clipping_the_output() {
        let mut m = Mixer::new(1000);
        for _ in 0..MAX_VOICES {
            m.play(play(tone(1000, 4, i16::MAX), 1.0, 0.0));
        }
        let mut out = [0.0; 8];
        m.mix(&mut out);
        assert!(out.iter().all(|s| s.abs() <= 1.0));
    }
}
