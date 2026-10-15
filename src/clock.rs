//! The pipeline's stage timer.
//!
//! `std::time::Instant::now()` panics on wasm32-unknown-unknown (no
//! clock without a host binding), which is the target Cloudflare
//! Workers runs. The timings only feed `OXIMG_TIMING` logs and metrics,
//! so there they read as zero rather than pulling a JS dependency into
//! the library.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use std::time::Instant;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Instant;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl Instant {
    pub(crate) fn now() -> Self {
        Instant
    }

    pub(crate) fn elapsed(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}
