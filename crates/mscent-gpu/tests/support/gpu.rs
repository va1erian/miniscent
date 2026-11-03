//! The GPU a test draws on. A machine with no GPU skips, unless
//! `MSCENT_REQUIRE_GPU` is set, as CI sets it.

use mscent_gpu::{Gpu, GpuError};

/// The GPU, or `None` where a skip is allowed.
pub fn gpu() -> Option<Gpu> {
    match Gpu::new() {
        Ok(gpu) => Some(gpu),
        Err(why @ (GpuError::NoAdapter(_) | GpuError::NoDevice(_))) => {
            assert!(
                std::env::var_os("MSCENT_REQUIRE_GPU").is_none(),
                "MSCENT_REQUIRE_GPU is set: {why}"
            );
            eprintln!("skipped: {why}");
            None
        }
        Err(why) => panic!("{why}"),
    }
}
