//! Playing through the machine's default output device (feature `device`).
//!
//! The device callback owns the [`Mixer`] and drains a channel of requests, so
//! the game thread never waits on the audio thread nor the reverse.

use std::sync::mpsc::{self, Receiver, SyncSender};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::mixer::{Mixer, Play};

/// Requests that may queue before the device catches up. A stalled device must
/// not let the queue grow without bound.
const QUEUE: usize = 256;

/// An open output. The stream lives on a thread of its own, because a cpal
/// stream cannot be moved between threads and the game's session can (the
/// editor plays it from a worker). Dropping this ends that thread and the sound.
pub struct Output {
    tx: SyncSender<Play>,
    /// Dropping this wakes the thread, which then drops the stream.
    _stop: SyncSender<()>,
}

impl Output {
    /// Opens the default device, or says why it cannot.
    pub fn open() -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let (stop, stopped) = mpsc::sync_channel::<()>(0);
        let (ready_tx, ready) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || match open_stream(rx) {
                Ok(stream) => {
                    let _ = ready_tx.send(Ok(()));
                    // Blocks until `Output` is dropped, which disconnects.
                    let _ = stopped.recv();
                    drop(stream);
                }
                Err(why) => {
                    let _ = ready_tx.send(Err(why));
                }
            })
            .map_err(|e| e.to_string())?;
        ready.recv().map_err(|e| e.to_string())??;
        Ok(Self { tx, _stop: stop })
    }

    /// Starts a sound. A full queue drops the request: late sound is worse than
    /// none.
    pub fn play(&self, req: Play) {
        let _ = self.tx.try_send(req);
    }
}

fn open_stream(rx: Receiver<Play>) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no audio output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    if config.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!(
            "unsupported sample format {:?}",
            config.sample_format()
        ));
    }
    let channels = usize::from(config.channels());
    let stream = device
        .build_output_stream(
            config.config(),
            callback(config.sample_rate(), channels, rx),
            |e| eprintln!("audio: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// The closure the device calls: mix stereo, then spread it over however many
/// channels the device has.
fn callback(
    rate: u32,
    channels: usize,
    rx: Receiver<Play>,
) -> impl FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static {
    let channels = channels.max(1);
    let mut mixer = Mixer::new(rate);
    let mut stereo = Vec::new();
    move |out, _| {
        while let Ok(req) = rx.try_recv() {
            mixer.play(req);
        }
        stereo.resize(out.len() / channels * 2, 0.0);
        mixer.mix(&mut stereo);
        for (dst, src) in out
            .chunks_exact_mut(channels)
            .zip(stereo.as_chunks::<2>().0)
        {
            dst.fill(0.0);
            dst[0] = src[0];
            if let Some(right) = dst.get_mut(1) {
                *right = src[1];
            }
        }
    }
}
