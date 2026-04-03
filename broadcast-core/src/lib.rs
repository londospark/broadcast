pub mod backend;
pub mod filter;
pub mod pipewire;
pub mod routing;
pub mod state;

pub use filter::FilterHealth;
pub use pipewire::AudioDevice;
pub use state::Backend;

use anyhow::Result;
use backend::PipeWireBackend;
use pipewire::{parse_sinks_as_devices, parse_sources_as_devices};

/// List available hardware output devices (sinks), excluding filter/virtual sinks.
pub fn list_output_devices(
    backend: &dyn PipeWireBackend,
    filter_sink_name: &str,
) -> Result<Vec<AudioDevice>> {
    let sinks = backend.list_sinks()?;
    Ok(parse_sinks_as_devices(&sinks, filter_sink_name))
}

/// List available hardware input devices (sources), excluding monitors/virtual sources.
pub fn list_input_devices(backend: &dyn PipeWireBackend) -> Result<Vec<AudioDevice>> {
    let sources = backend.list_sources()?;
    Ok(parse_sources_as_devices(&sources))
}

/// Returns the path to the installed Maxine LADSPA plugin, if available.
/// Checks `~/.local/lib/ladspa/` and `/usr/lib/ladspa/` in that order.
pub fn maxine_plugin_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from(&home).join(".local/lib/ladspa/libmaxine_ladspa.so"),
        std::path::PathBuf::from("/usr/lib/ladspa/libmaxine_ladspa.so"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// Returns true if the Maxine LADSPA plugin is installed and ready to use.
pub fn is_maxine_available() -> bool {
    maxine_plugin_path().is_some()
}

#[cfg(test)]
pub mod test_helpers;
