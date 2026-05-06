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

/// Return common paths used by the Maxine enable/disable helpers:
/// (config dir, pipewire.conf.d, saved dir)
fn maxine_paths() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let home = std::env::var("HOME").unwrap_or_default();
    let conf_dir = std::path::PathBuf::from(&home).join(".config/pipewire");
    let conf_d = conf_dir.join("pipewire.conf.d");
    let saved = conf_dir.join("maxine.saved");
    (conf_dir, conf_d, saved)
}

/// Returns true if the Maxine filter config is present (enabled).
pub fn is_maxine_enabled() -> bool {
    let (_conf_dir, conf_d, _saved) = maxine_paths();
    if let Ok(mut entries) = std::fs::read_dir(&conf_d) {
        while let Some(Ok(entry)) = entries.next() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("50-maxine-") && name.ends_with(".conf") {
                    return true;
                }
            }
        }
    }
    false
}

/// Enable or disable Maxine filter configs by moving 50-maxine-*.conf files
/// between pipewire.conf.d and maxine.saved, then restart PipeWire user services.
pub fn set_maxine_enabled(enabled: bool) -> Result<()> {
    if enabled && !is_maxine_available() {
        anyhow::bail!("Maxine plugin not found; install broadcast-maxine-ladspa and models");
    }

    let (_conf_dir, conf_d, saved) = maxine_paths();

    if enabled {
        std::fs::create_dir_all(&conf_d).map_err(|e| anyhow::anyhow!("failed to create {}: {}", conf_d.display(), e))?;
    } else {
        std::fs::create_dir_all(&saved).map_err(|e| anyhow::anyhow!("failed to create {}: {}", saved.display(), e))?;
    }

    if enabled {
        if let Ok(entries) = std::fs::read_dir(&saved) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                if name.starts_with("50-maxine-") && name.ends_with(".conf") {
                    let from = entry.path();
                    let to = conf_d.join(name.as_ref());
                    let _ = std::fs::rename(&from, &to).or_else(|_| {
                        std::fs::copy(&from, &to).and_then(|_| std::fs::remove_file(&from))
                    });
                }
            }
        }
    } else {
        if let Ok(entries) = std::fs::read_dir(&conf_d) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                if name.starts_with("50-maxine-") && name.ends_with(".conf") {
                    let from = entry.path();
                    let to = saved.join(name.as_ref());
                    let _ = std::fs::rename(&from, &to).or_else(|_| {
                        std::fs::copy(&from, &to).and_then(|_| std::fs::remove_file(&from))
                    });
                }
            }
        }
    }

    // Best-effort restart
    let _ = std::process::Command::new("systemctl").args(["--user", "reset-failed", "pipewire"]).status();
    let _ = std::process::Command::new("systemctl").args(["--user", "restart", "pipewire"]).status();
    let _ = std::process::Command::new("systemctl").args(["--user", "restart", "pipewire-pulse"]).status();
    let _ = std::process::Command::new("systemctl").args(["--user", "restart", "wireplumber"]).status();

    Ok(())
}

#[cfg(test)]
pub mod test_helpers;
