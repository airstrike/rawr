//! Return freed allocator memory to the OS once the API phase has ended.
//!
//! The in-process importer leaves a few hundred MB that the allocator has freed
//! but still holds; the live heap after the API phase is far smaller. This module
//! hands those free pages back before the runner build starts.
//!
//! Revisitable (user-approved 2026-10-20): this is one isolated `unsafe`
//! allocator call. A better design would not hold the memory in the first place.
#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe extern "C" {
    fn malloc_trim(pad: usize) -> std::ffi::c_int;
}

/// Ask the platform allocator to return free pages to the OS.
///
/// Best effort: the result is ignored and this never fails. A no-op on
/// platforms without a release call.
pub(crate) fn freed_memory() {
    #[cfg(target_os = "macos")]
    // SAFETY: a null zone means every malloc zone and a zero goal means release as
    // much as possible; the call touches only allocator-owned free pages and
    // invalidates no live allocation.
    unsafe {
        let _ = malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: glibc's `malloc_trim(0)` only returns free heap memory to the OS;
    // live allocations are untouched.
    unsafe {
        let _ = malloc_trim(0);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn freed_memory_links_and_returns() {
        let held = vec![0_u8; 1 << 20];
        drop(held);
        super::freed_memory();
        super::freed_memory();
    }
}
