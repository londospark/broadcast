// When the NVAFX SDK is present, build.rs compiles maxine_ladspa.c and sets has_nvafx.
// We re-export the C entry point as the standard LADSPA ladspa_descriptor() symbol.
//
// When the SDK is absent, this crate compiles to an empty cdylib with a cargo warning.

#[cfg(has_nvafx)]
extern "C" {
    /// Internal entry point compiled from maxine_ladspa.c
    fn _maxine_ladspa_descriptor_impl(index: core::ffi::c_ulong) -> *const core::ffi::c_void;
}

/// LADSPA host entry point.
/// index 0 → maxine_denoiser_mono
/// index 1 → maxine_denoiser_stereo
/// Returns NULL for any other index.
#[cfg(has_nvafx)]
#[no_mangle]
pub unsafe extern "C" fn ladspa_descriptor(
    index: core::ffi::c_ulong,
) -> *const core::ffi::c_void {
    _maxine_ladspa_descriptor_impl(index)
}
