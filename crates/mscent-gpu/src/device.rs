//! The GPU itself. A device outlives the levels drawn on it, and its errors
//! come back as values: left alone, wgpu panics on one nobody asked about.

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::task::{Context, Poll, Waker};

use crate::face_pipeline::Compiled;
use crate::finish::Finish;

/// Why a GPU cannot draw, in words for a log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GpuError {
    /// No Vulkan driver, or none that wgpu can use.
    NoAdapter(String),
    /// The adapter refused to make a device.
    NoDevice(String),
    /// A level is larger than this adapter can hold.
    TooLarge(&'static str),
    /// The device has gone, as a driver reset or a removed GPU takes it.
    Lost(String),
    /// wgpu refused what a level or a frame asked of it, which the checks before
    /// it missed.
    Rejected(String),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter(why) => write!(f, "no Vulkan adapter: {why}"),
            Self::NoDevice(why) => write!(f, "no Vulkan device: {why}"),
            Self::TooLarge(what) => write!(f, "too large for this GPU: {what}"),
            Self::Lost(why) => write!(f, "the GPU device was lost: {why}"),
            Self::Rejected(why) => write!(f, "the GPU refused what it was asked: {why}"),
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

/// Keeps the first of what goes into `slot`.
fn record(slot: &Mutex<Option<String>>, what: String) {
    let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
    slot.get_or_insert(what);
}

/// An open GPU. A clone is the same device, for the next level drawn on it.
#[derive(Clone)]
pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    adapter: String,
    /// The adapter is a driver that draws on the CPU.
    cpu: bool,
    first_error: Arc<Mutex<Option<String>>>,
    /// Why the device went, once it has: it does not come back.
    lost: Arc<Mutex<Option<String>>>,
    /// Compiled once for the device, one for each finish: the editor makes a
    /// scene on every edit, and this pipeline is a fifth of a voxel scene's first
    /// frame.
    pub(crate) voxel_pipelines: Arc<[OnceLock<wgpu::RenderPipeline>; Finish::COUNT]>,
    /// The same for the faces' pipelines and the viewport faces'.
    pub(crate) face_pipelines: Arc<[OnceLock<Compiled>; Finish::COUNT]>,
    pub(crate) hole_pipelines: Arc<[OnceLock<wgpu::RenderPipeline>; Finish::COUNT]>,
}

impl Gpu {
    /// Opens the best Vulkan adapter on this machine, asking for all it offers:
    /// a level's lightmap atlas can be far past wgpu's default limits.
    pub fn new() -> Result<Self, GpuError> {
        Self::with_limits(|offered| offered)
    }

    /// Opens the GPU as [`Self::new`] does, with the limits `cap` makes of all
    /// the adapter offers: a test stands in a smaller GPU with this.
    pub fn with_limits(cap: impl FnOnce(wgpu::Limits) -> wgpu::Limits) -> Result<Self, GpuError> {
        let instance = wgpu::Instance::default();
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        };
        let adapter = block_on(instance.request_adapter(&options))
            .map_err(|e| GpuError::NoAdapter(e.to_string()))?;
        let wanted = wgpu::DeviceDescriptor {
            label: Some("miniscent"),
            required_limits: cap(adapter.limits()),
            ..Default::default()
        };
        let (device, queue) = block_on(adapter.request_device(&wanted))
            .map_err(|e| GpuError::NoDevice(e.to_string()))?;
        let first_error = Arc::new(Mutex::new(None));
        let slot = first_error.clone();
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            record(&slot, error.to_string());
        }));
        let lost = Arc::new(Mutex::new(None));
        let (lost_slot, error_slot) = (lost.clone(), first_error.clone());
        device.set_device_lost_callback(move |reason, message| {
            let why = format!("{reason:?} {message}").trim_end().to_string();
            record(&lost_slot, why.clone());
            record(&error_slot, format!("the GPU device was lost: {why}"));
        });
        Ok(Self {
            device,
            queue,
            adapter: adapter.get_info().name,
            cpu: adapter.get_info().device_type == wgpu::DeviceType::Cpu,
            first_error,
            lost,
            voxel_pipelines: Arc::default(),
            face_pipelines: Arc::default(),
            hole_pipelines: Arc::default(),
        })
    }

    /// Why the device has gone, once it has. Every renderer on it fails from
    /// then on; only a new [`Gpu`] draws again.
    pub fn lost(&self) -> Option<String> {
        let slot = self.lost.lock().unwrap_or_else(PoisonError::into_inner);
        slot.clone()
    }

    /// Ends the device now, as a driver reset would, for tests of what follows.
    pub fn destroy(&self) {
        record(&self.lost, "destroyed".to_string());
        self.device.destroy();
    }

    /// What `make` builds, or the first error wgpu raised while it did: those
    /// go to the caller, not to [`Self::take_error`]. A lost device builds
    /// nothing.
    pub(crate) fn checked<T>(
        &self,
        make: impl FnOnce() -> Result<T, GpuError>,
    ) -> Result<T, GpuError> {
        if let Some(why) = self.lost() {
            return Err(GpuError::Lost(why));
        }
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let made = make();
        let invalid = block_on(validation.pop());
        let refused = invalid.or(block_on(memory.pop()));
        if let Some(why) = self.lost() {
            return Err(GpuError::Lost(why));
        }
        match refused {
            Some(error) => Err(GpuError::Rejected(error.to_string())),
            None => made,
        }
    }

    /// Keeps `why` for [`Self::take_error`], unless an earlier error is there.
    pub(crate) fn report(&self, why: String) {
        record(&self.first_error, why);
    }

    /// Whether the adapter draws on the CPU, as Mesa's llvmpipe does.
    pub fn is_cpu(&self) -> bool {
        self.cpu
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
