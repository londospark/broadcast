pub mod backend;
pub mod filter;
pub mod pipewire;
pub mod routing;
pub mod state;

pub use pipewire::AudioDevice;

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

#[cfg(test)]
pub mod test_helpers;
