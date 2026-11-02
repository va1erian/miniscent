//! The GPU itself. A device outlives the levels drawn on it, and its errors
//! come back as values: left alone, wgpu panics on one nobody asked about.

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

/// Why a GPU cannot draw, in words for a log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GpuError {
    /// No Vulkan driver, or none that wgpu can use.
    NoAdapter(String),
    /// The adapter refused to make a device.
    NoDevice(String),
    /// A level is larger than this adapter can hold.
    TooLarge(&'static str),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter(why) => write!(f, "no Vulkan adapter: {why}"),
            Self::NoDevice(why) => write!(f, "no Vulkan device: {why}"),
            Self::TooLarge(what) => write!(f, "too large for this GPU: {what}"),
        }
    }
}

/// wgpu's futures resolve on the spot on a native backend, so this is all the
/// executor they need.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::yield_now();
    }
}

/// An open GPU. A clone is the same device, for the next level drawn on it.
#[derive(Clone)]
pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    adapter: String,
    first_error: Arc<Mutex<Option<String>>>,
}

impl Gpu {
    /// Opens the best Vulkan adapter on this machine, asking for all it offers:
    /// a level's lightmap atlas can be far past wgpu's default limits.
    pub fn new() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::default();
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        };
        let adapter = block_on(instance.request_adapter(&options))
            .map_err(|e| GpuError::NoAdapter(e.to_string()))?;
        let wanted = wgpu::DeviceDescriptor {
            label: Some("miniscent"),
            required_limits: adapter.limits(),
            ..Default::default()
        };
        let (device, queue) = block_on(adapter.request_device(&wanted))
            .map_err(|e| GpuError::NoDevice(e.to_string()))?;
        let first_error = Arc::new(Mutex::new(None));
        let slot = first_error.clone();
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
            slot.get_or_insert_with(|| error.to_string());
        }));
        Ok(Self {
            device,
            queue,
            adapter: adapter.get_info().name,
            first_error,
        })
    }

    /// The adapter's name, for reports.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// The first error since the last call, if any.
    pub fn take_error(&self) -> Option<String> {
        self.first_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Skipped on a machine with no GPU; `tests/parity.rs` is what insists on one.
    #[test]
    fn a_misused_device_reports_an_error_instead_of_panicking() {
        let Ok(gpu) = Gpu::new() else { return };
        assert_eq!(gpu.take_error(), None);
        let _ = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            // wgpu holds to the rule of WebGPU: one or the other.
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        });
        assert!(gpu.take_error().is_some());
        assert_eq!(gpu.take_error(), None);
    }
}
