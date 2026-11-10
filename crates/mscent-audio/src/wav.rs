//! Reading a `.wav` file into a [`Sound`].
//!
//! Only uncompressed PCM, 8 or 16 bit, one or two channels: what Freedoom's
//! sound effects are. The file is untrusted, so every size is checked against
//! the bytes that are there and against a cap before anything is allocated.

use std::sync::Arc;

/// Longest sound accepted, in frames. Ten minutes at 48 kHz would be 28.8 M;
/// sound effects are seconds, so this stops a hostile header, not a long clip.
pub const MAX_FRAMES: usize = 1 << 21;
/// Slowest and fastest sample rate accepted.
pub const RATE_RANGE: core::ops::RangeInclusive<u32> = 4_000..=96_000;

/// A mono sound: signed 16-bit samples at their own rate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sound {
    pub rate: u32,
    pub samples: Arc<[i16]>,
}

/// Why a file is not a sound this crate can play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavError {
    /// Not a RIFF/WAVE file, or a chunk runs past the end.
    Malformed,
    /// Compressed, float, or a bit depth other than 8 or 16.
    Unsupported,
    /// No `fmt ` or no `data` chunk.
    Missing,
    /// A rate or length outside what is accepted.
    OutOfRange,
}

impl core::fmt::Display for WavError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "not a well-formed WAVE file",
            Self::Unsupported => "only 8 and 16 bit PCM, mono or stereo, is supported",
            Self::Missing => "no fmt or data chunk",
            Self::OutOfRange => "sample rate or length out of range",
        })
    }
}

#[derive(Clone, Copy)]
struct Format {
    channels: usize,
    rate: u32,
    bits: u16,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn parse_format(chunk: &[u8]) -> Result<Format, WavError> {
    let tag = u16_at(chunk, 0).ok_or(WavError::Malformed)?;
    let channels = u16_at(chunk, 2).ok_or(WavError::Malformed)?;
    let rate = u32_at(chunk, 4).ok_or(WavError::Malformed)?;
    let bits = u16_at(chunk, 14).ok_or(WavError::Malformed)?;
    if tag != 1 || !matches!(bits, 8 | 16) || !matches!(channels, 1 | 2) {
        return Err(WavError::Unsupported);
    }
    if !RATE_RANGE.contains(&rate) {
        return Err(WavError::OutOfRange);
    }
    Ok(Format {
        channels: usize::from(channels),
        rate,
        bits,
    })
}

/// One frame to a mono sample: channels averaged, 8-bit widened to 16.
fn frame_sample(fmt: Format, frame: &[u8]) -> i16 {
    let width = usize::from(fmt.bits / 8);
    let sum: i32 = frame
        .chunks_exact(width)
        .map(|s| {
            if width == 1 {
                // 8-bit WAVE is unsigned with 128 as silence.
                (i32::from(s[0]) - 128) << 8
            } else {
                i32::from(i16::from_le_bytes([s[0], s[1]]))
            }
        })
        .sum();
    // The mean of values that fit in i16 fits in i16.
    (sum / fmt.channels as i32) as i16
}

fn decode(fmt: Format, data: &[u8]) -> Result<Sound, WavError> {
    let frame_bytes = usize::from(fmt.bits / 8) * fmt.channels;
    let frames = data.len() / frame_bytes;
    if frames == 0 || frames > MAX_FRAMES {
        return Err(WavError::OutOfRange);
    }
    let samples = data
        .chunks_exact(frame_bytes)
        .map(|frame| frame_sample(fmt, frame))
        .collect();
    Ok(Sound {
        rate: fmt.rate,
        samples,
    })
}

/// Parses a `.wav` file. Chunks other than `fmt ` and `data` are skipped, and a
/// `data` chunk whose declared size overruns the file is cut to what is there,
/// as players do for a file that was cut short.
pub fn parse(bytes: &[u8]) -> Result<Sound, WavError> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::Malformed);
    }
    let mut format = None;
    let mut at = 12usize;
    while let (Some(id), Some(size)) = (bytes.get(at..at + 4), u32_at(bytes, at + 4)) {
        let body = at.checked_add(8).ok_or(WavError::Malformed)?;
        let end = body
            .checked_add(size as usize)
            .ok_or(WavError::Malformed)?
            .min(bytes.len());
        let chunk = &bytes[body.min(end)..end];
        match id {
            b"fmt " => format = Some(parse_format(chunk)?),
            b"data" => return decode(format.ok_or(WavError::Missing)?, chunk),
            _ => {}
        }
        // Chunks are padded to an even length.
        at = end
            .checked_add(size as usize & 1)
            .ok_or(WavError::Malformed)?;
    }
    Err(WavError::Missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAVE file with the given format fields and data.
    fn file(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut f = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        f.extend(16u32.to_le_bytes());
        f.extend(tag.to_le_bytes());
        f.extend(channels.to_le_bytes());
        f.extend(rate.to_le_bytes());
        f.extend((rate * u32::from(channels) * u32::from(bits / 8)).to_le_bytes());
        f.extend((channels * bits / 8).to_le_bytes());
        f.extend(bits.to_le_bytes());
        f.extend(b"data");
        f.extend((data.len() as u32).to_le_bytes());
        f.extend(data);
        f
    }

    #[test]
    fn eight_bit_is_centred_and_widened() {
        let s = parse(&file(1, 1, 11025, 8, &[128, 255, 0])).unwrap();
        assert_eq!(s.rate, 11025);
        assert_eq!(&*s.samples, &[0, 127 << 8, -128 << 8]);
    }

    #[test]
    fn stereo_sixteen_bit_is_averaged_to_mono() {
        let data = [100i16, 300, -50, -150]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let s = parse(&file(1, 2, 22050, 16, &data)).unwrap();
        assert_eq!(&*s.samples, &[200, -100]);
    }

    #[test]
    fn an_unknown_chunk_before_the_data_is_skipped() {
        let mut f = file(1, 1, 8000, 8, &[]);
        f.truncate(36);
        f.extend(b"LIST\x03\0\0\0abc\0"); // odd size, padded
        f.extend(b"data\x02\0\0\0\x80\x90");
        assert_eq!(parse(&f).unwrap().samples.len(), 2);
    }

    #[test]
    fn a_data_size_past_the_end_is_cut_not_trusted() {
        let mut f = file(1, 1, 8000, 8, &[128; 10]);
        f[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse(&f).unwrap().samples.len(), 10);
    }

    #[test]
    fn hostile_files_are_refused_not_panicked_on() {
        assert_eq!(parse(b""), Err(WavError::Malformed));
        assert_eq!(parse(b"RIFF\0\0\0\0WAVE"), Err(WavError::Missing));
        assert_eq!(
            parse(&file(3, 1, 8000, 16, &[0; 4])),
            Err(WavError::Unsupported)
        );
        assert_eq!(
            parse(&file(1, 6, 8000, 16, &[0; 12])),
            Err(WavError::Unsupported)
        );
        assert_eq!(
            parse(&file(1, 1, 8000, 24, &[0; 6])),
            Err(WavError::Unsupported)
        );
        assert_eq!(parse(&file(1, 1, 1, 8, &[0; 6])), Err(WavError::OutOfRange));
        assert_eq!(
            parse(&file(1, 1, u32::MAX, 8, &[0; 6])),
            Err(WavError::OutOfRange)
        );
        assert_eq!(parse(&file(1, 1, 8000, 8, &[])), Err(WavError::OutOfRange));
        // Data before any fmt chunk.
        assert_eq!(
            parse(b"RIFF\0\0\0\0WAVEdata\x01\0\0\0\x80"),
            Err(WavError::Missing)
        );
    }

    #[test]
    fn every_truncation_of_a_good_file_is_handled() {
        let f = file(1, 2, 22050, 16, &[1, 2, 3, 4, 5, 6, 7, 8]);
        for n in 0..f.len() {
            let _ = parse(&f[..n]);
        }
    }

    #[test]
    fn a_sound_over_the_cap_is_refused() {
        let big = vec![128u8; MAX_FRAMES + 1];
        assert_eq!(parse(&file(1, 1, 8000, 8, &big)), Err(WavError::OutOfRange));
    }
}
