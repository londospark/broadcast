use anyhow::Result;
use clap::{Parser, Subcommand};

use broadcast_core::backend::{PipeWireBackend, RealBackend};
use broadcast_core::state::Backend;
use broadcast_core::{filter, routing, state::BroadcastState};

#[derive(Parser)]
#[command(
    name = "broadcast-ctl",
    about = "AI noise suppression control for PipeWire",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Toggle noise suppression on/off
    Toggle,
    /// Enable noise suppression
    On,
    /// Disable noise suppression (passthrough)
    Off,
    /// Show current status
    Status {
        /// Output format for Ironbar widget
        #[arg(long)]
        ironbar: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Route an app through filtered or direct output
    Route {
        /// App name (matched against binary name)
        app: String,
        /// Route mode: filtered or direct
        mode: String,
    },
    /// List running audio apps and their routing
    Apps,
    /// Apply saved routing preferences to all running streams,
    /// and ensure the default source is the clean mic
    Apply,
    /// List available audio devices
    Devices,
    /// Set preferred audio device
    SetDevice {
        /// Device type: "output" (sink/speakers) or "input" (source/mic)
        device_type: String,
        /// Device node name (use 'devices' command to see available names), or "auto" to clear
        device_name: String,
    },
    /// Check filter health and repair routing issues
    FixRouting,
    /// Install a systemd user service to auto-apply routes at login
    InstallService,
    /// Set the noise suppression backend (deepfilter or maxine)
    SetBackend {
        /// Backend name: "deepfilter" (CPU, any GPU) or "maxine" (NVIDIA RTX, requires SDK)
        backend: String,
    },
    /// Generate PipeWire filter chain config files for the active backend
    InstallConfig {
        /// Actually restart PipeWire after writing config (default: just write files)
        #[arg(long)]
        apply: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let backend = RealBackend;

    match cli.command {
        Commands::Toggle => cmd_toggle(&backend)?,
        Commands::On => cmd_set(&backend, true)?,
        Commands::Off => cmd_set(&backend, false)?,
        Commands::Status { ironbar, json } => cmd_status(&backend, ironbar, json)?,
        Commands::Route { app, mode } => cmd_route(&backend, &app, &mode)?,
        Commands::Apps => cmd_apps(&backend)?,
        Commands::Apply => cmd_apply(&backend)?,
        Commands::Devices => cmd_devices(&backend)?,
        Commands::SetDevice {
            device_type,
            device_name,
        } => cmd_set_device(&backend, &device_type, &device_name)?,
        Commands::FixRouting => cmd_fix_routing(&backend)?,
        Commands::InstallService => cmd_install_service()?,
        Commands::SetBackend { backend: name } => cmd_set_backend(&backend, &name)?,
        Commands::InstallConfig { apply } => cmd_install_config(apply)?,
    }
    Ok(())
}

fn cmd_toggle(backend: &dyn PipeWireBackend) -> Result<()> {
    let mut state = BroadcastState::load()?;
    if !filter::filters_loaded(backend, &state).unwrap_or(false) {
        eprintln!("⚠  Filter chains not loaded — run 'broadcast-ctl install-config --apply' first");
    }
    let active = !state.active;
    state.active = active;

    filter::set_filter_active(backend, &state, active)?;

    if active {
        routing::apply_routes(backend, &state)?;
        ensure_default_source(backend, &state);
    } else {
        routing::bypass_all(backend, &state)?;
    }

    state.save()?;

    let icon = if active { "󰍬" } else { "󰍭" };
    let label = if active { "ON" } else { "OFF" };
    eprintln!("{icon} Broadcast {label}");
    Ok(())
}

fn cmd_set(backend: &dyn PipeWireBackend, active: bool) -> Result<()> {
    let mut state = BroadcastState::load()?;
    if active && !filter::filters_loaded(backend, &state).unwrap_or(false) {
        eprintln!("⚠  Filter chains not loaded — run 'broadcast-ctl install-config --apply' first");
    }
    state.active = active;

    filter::set_filter_active(backend, &state, active)?;

    if active {
        routing::apply_routes(backend, &state)?;
        ensure_default_source(backend, &state);
    } else {
        routing::bypass_all(backend, &state)?;
    }

    state.save()?;

    let icon = if active { "󰍬" } else { "󰍭" };
    let label = if active { "ON" } else { "OFF" };
    eprintln!("{icon} Broadcast {label}");
    Ok(())
}

fn cmd_status(backend: &dyn PipeWireBackend, ironbar: bool, json: bool) -> Result<()> {
    let state = BroadcastState::load()?;
    let health = filter::filter_health(backend, &state);

    if json {
        let status = serde_json::json!({
            "active": state.active,
            "backend": state.backend.to_string(),
            "filters_loaded": health.filters_loaded,
            "input_running": health.input_running,
            "output_running": health.output_running,
            "default_source_correct": health.default_source_correct,
            "health": if health.is_ok() { "ok" } else { "degraded" },
            "issues": health.issues,
            "default_route": state.default_route,
            "app_routes": state.app_routes,
            "preferred_output_sink": state.preferred_output_sink,
            "preferred_input_source": state.preferred_input_source,
        });
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else if ironbar {
        // Ironbar label: icon + text. Color classes not supported inline so we use icon state.
        if state.active && health.filters_loaded && health.is_ok() {
            println!("󰍬 Broadcast");
        } else if state.active && health.filters_loaded {
            // Running but degraded (wrong default source, etc.)
            println!("󰍬 !");
        } else if health.filters_loaded {
            println!("󰍭");
        } else {
            // Filters not loaded at all
            println!("󰍮");
        }
    } else {
        let status = if state.active { "ON" } else { "OFF" };
        let filter_status = if health.filters_loaded {
            "loaded"
        } else {
            "not loaded"
        };
        let health_str = if health.is_ok() { "OK" } else { "degraded" };
        println!("Broadcast: {status}");
        println!("Backend:   {}", state.backend);
        println!("Filters:   {filter_status}");
        println!("Health:    {health_str}");
        println!("Default route: {}", state.default_route);
        println!(
            "Output device: {}",
            state.preferred_output_sink.as_deref().unwrap_or("(auto)")
        );
        println!(
            "Input device:  {}",
            state.preferred_input_source.as_deref().unwrap_or("(auto)")
        );
        if !health.issues.is_empty() {
            println!();
            println!("Issues:");
            for issue in &health.issues {
                println!("  ⚠  {issue}");
            }
        }
        if !state.app_routes.is_empty() {
            println!();
            println!("App routes:");
            for (app, route) in &state.app_routes {
                println!("  {app}: {route}");
            }
        }
    }
    Ok(())
}

fn cmd_route(backend: &dyn PipeWireBackend, app: &str, mode: &str) -> Result<()> {
    let route: broadcast_core::state::AppRoute = mode.parse()?;
    let mut state = BroadcastState::load()?;

    if state.active {
        let moved = routing::route_app(backend, &state, app, route)?;
        eprintln!("Routed {moved} stream(s) for '{app}' → {route}");
    }

    // Save preference
    state.set_app_route(app, route);
    state.save()?;
    Ok(())
}

fn cmd_apps(backend: &dyn PipeWireBackend) -> Result<()> {
    let state = BroadcastState::load()?;
    let apps = routing::list_apps(backend, &state)?;
    if apps.is_empty() {
        println!("No audio streams playing.");
        return Ok(());
    }
    for app in &apps {
        let icon = match app.route {
            broadcast_core::state::AppRoute::Filtered => "■",
            broadcast_core::state::AppRoute::Direct => "○",
        };
        let name = if !app.name.is_empty() {
            &app.name
        } else {
            &app.binary
        };
        println!("  {icon} {name} ({}) → {}", app.binary, app.route);
    }
    Ok(())
}

fn cmd_apply(backend: &dyn PipeWireBackend) -> Result<()> {
    let state = BroadcastState::load()?;
    if !state.active {
        eprintln!("Broadcast is OFF — not applying routes");
        return Ok(());
    }
    routing::apply_routes(backend, &state)?;
    ensure_default_source(backend, &state);
    eprintln!("Applied saved routing preferences");
    Ok(())
}

fn cmd_devices(backend: &dyn PipeWireBackend) -> Result<()> {
    let state = BroadcastState::load()?;

    let output_devices = broadcast_core::list_output_devices(backend, &state.nodes.output_sink)?;
    let input_devices = broadcast_core::list_input_devices(backend)?;

    println!("Output devices (sinks):");
    if output_devices.is_empty() {
        println!("  (none found)");
    } else {
        for dev in &output_devices {
            let marker = if state.preferred_output_sink.as_deref() == Some(&dev.name) {
                " ◉"
            } else {
                "  "
            };
            println!("{marker} {}", dev.description);
            println!("    name: {}", dev.name);
        }
    }

    println!();
    println!("Input devices (sources):");
    if input_devices.is_empty() {
        println!("  (none found)");
    } else {
        for dev in &input_devices {
            let marker = if state.preferred_input_source.as_deref() == Some(&dev.name) {
                " ◉"
            } else {
                "  "
            };
            println!("{marker} {}", dev.description);
            println!("    name: {}", dev.name);
        }
    }

    if state.preferred_output_sink.is_none() && state.preferred_input_source.is_none() {
        println!();
        println!("No preferred devices set (using auto-detect).");
        println!("Use: broadcast-ctl set-device output <name>");
        println!("     broadcast-ctl set-device input <name>");
    }

    Ok(())
}

fn cmd_set_device(
    backend: &dyn PipeWireBackend,
    device_type: &str,
    device_name: &str,
) -> Result<()> {
    let mut state = BroadcastState::load()?;

    match device_type.to_lowercase().as_str() {
        "output" | "sink" | "out" => {
            if device_name == "auto" || device_name == "none" {
                state.set_preferred_output_sink(None);
                eprintln!("Output device set to auto-detect");
            } else {
                // Validate the device exists
                let devices =
                    broadcast_core::list_output_devices(backend, &state.nodes.output_sink)?;
                let found = devices.iter().any(|d| d.name == device_name);
                if !found {
                    anyhow::bail!(
                        "Output device '{}' not found. Run 'broadcast-ctl devices' to see available devices.",
                        device_name
                    );
                }
                state.set_preferred_output_sink(Some(device_name.to_string()));
                let desc = devices
                    .iter()
                    .find(|d| d.name == device_name)
                    .map(|d| d.description.as_str())
                    .unwrap_or(device_name);
                eprintln!("Output device set to: {desc}");
            }
        }
        "input" | "source" | "in" => {
            if device_name == "auto" || device_name == "none" {
                state.set_preferred_input_source(None);
                eprintln!("Input device set to auto-detect");
            } else {
                let devices = broadcast_core::list_input_devices(backend)?;
                let found = devices.iter().any(|d| d.name == device_name);
                if !found {
                    anyhow::bail!(
                        "Input device '{}' not found. Run 'broadcast-ctl devices' to see available devices.",
                        device_name
                    );
                }
                state.set_preferred_input_source(Some(device_name.to_string()));
                let desc = devices
                    .iter()
                    .find(|d| d.name == device_name)
                    .map(|d| d.description.as_str())
                    .unwrap_or(device_name);
                eprintln!("Input device set to: {desc}");
            }
        }
        _ => {
            anyhow::bail!(
                "Invalid device type '{}'. Use 'output' or 'input'.",
                device_type
            );
        }
    }

    state.save()?;

    if state.active {
        routing::apply_routes(backend, &state)?;
        eprintln!("Routes re-applied with new device");
    }

    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Set the default PulseAudio/PipeWire source to the clean mic node.
/// Derives the playback node name from the capture node name (strips "capture." prefix).
fn ensure_default_source(backend: &dyn PipeWireBackend, state: &BroadcastState) {
    let source_name = state.nodes.input_capture.replace("capture.", "");
    if let Err(e) = backend.set_default_source(&source_name) {
        eprintln!("⚠  Could not set default source to '{source_name}': {e}");
    }
}

// ── New commands ───────────────────────────────────────────────────────────

/// Diagnose and repair the audio routing.
fn cmd_fix_routing(backend: &dyn PipeWireBackend) -> Result<()> {
    let state = BroadcastState::load()?;
    let health = filter::filter_health(backend, &state);

    println!("=== Broadcast Routing Diagnostics ===");
    println!(
        "Filters loaded:        {}",
        if health.filters_loaded { "✓" } else { "✗" }
    );
    println!(
        "Input filter running:  {}",
        if health.input_running { "✓" } else { "✗" }
    );
    println!(
        "Output filter running: {}",
        if health.output_running { "✓" } else { "✗" }
    );
    println!(
        "Default source correct:{}",
        if health.default_source_correct {
            " ✓"
        } else {
            " ✗"
        }
    );

    if health.is_ok() {
        println!("\n✓ Everything looks healthy.");
        return Ok(());
    }

    println!("\nIssues found:");
    for issue in &health.issues {
        println!("  ⚠  {issue}");
    }
    println!("\nAttempting repairs...");

    if !health.default_source_correct {
        let source_name = state.nodes.input_capture.replace("capture.", "");
        match backend.set_default_source(&source_name) {
            Ok(()) => println!("  ✓ Default source set to '{source_name}'"),
            Err(e) => println!("  ✗ Failed to set default source: {e}"),
        }
    }

    if state.active && health.filters_loaded {
        match routing::apply_routes(backend, &state) {
            Ok(()) => println!("  ✓ Routes re-applied"),
            Err(e) => println!("  ✗ Failed to apply routes: {e}"),
        }
    }

    if !health.filters_loaded {
        println!(
            "  ! Filter chains missing — run: broadcast-ctl install-config --apply"
        );
    }

    Ok(())
}

/// Generate and enable a systemd user service for broadcast.
fn cmd_install_service() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let service_dir = std::path::PathBuf::from(&home)
        .join(".config/systemd/user");
    std::fs::create_dir_all(&service_dir)?;

    let binary = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| format!("{home}/.local/bin/broadcast-ctl"));

    let service = format!(
        r#"[Unit]
Description=Broadcast Noise Suppression Router
Documentation=https://github.com/yourusername/broadcast
After=pipewire.service wireplumber.service
Wants=pipewire.service wireplumber.service

[Service]
Type=oneshot
RemainAfterExit=yes
# Wait for WirePlumber to finish setting up audio devices
ExecStartPre=/bin/sleep 3
ExecStart={binary} apply
ExecStop={binary} off

[Install]
WantedBy=default.target
"#,
        binary = binary
    );

    let service_path = service_dir.join("broadcast.service");
    std::fs::write(&service_path, &service)?;
    println!("Wrote {}", service_path.display());

    // Enable it
    let status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "broadcast.service"])
        .status()?;
    if status.success() {
        println!("✓ broadcast.service enabled for autostart at login");
        println!("  Start it now with: systemctl --user start broadcast.service");
    } else {
        eprintln!("⚠  systemctl enable failed — you may need to enable it manually");
    }

    Ok(())
}

/// Switch the noise suppression backend and regenerate PipeWire configs.
fn cmd_set_backend(backend: &dyn PipeWireBackend, name: &str) -> Result<()> {
    let new_backend: Backend = name.parse()?;
    let mut state = BroadcastState::load()?;
    state.backend = new_backend;
    state.save()?;
    eprintln!("Backend set to: {new_backend}");
    eprintln!("Run 'broadcast-ctl install-config --apply' to reload PipeWire with the new backend.");
    if state.active {
        let _ = filter::set_filter_active(backend, &state, true);
    }
    Ok(())
}

/// Write PipeWire filter chain config files for the active backend.
/// With --apply also restarts pipewire to reload the filter chain.
fn cmd_install_config(apply: bool) -> Result<()> {
    let state = BroadcastState::load()?;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let conf_dir = std::path::PathBuf::from(&home)
        .join(".config/pipewire/pipewire.conf.d");
    std::fs::create_dir_all(&conf_dir)?;

    match state.backend {
        Backend::DeepFilter => write_deepfilter_configs(&conf_dir)?,
        Backend::Maxine => write_maxine_configs(&conf_dir, state.maxine_intensity)?,
    }

    println!("✓ Filter chain configs written to {}", conf_dir.display());

    if apply {
        eprintln!("Restarting PipeWire...");
        let status = std::process::Command::new("systemctl")
            .args(["--user", "restart", "pipewire.service"])
            .status()?;
        if status.success() {
            println!("✓ PipeWire restarted");
            // Give it a moment to load
            std::thread::sleep(std::time::Duration::from_secs(2));
            println!("  Filters should now be available. Run: broadcast-ctl status");
        } else {
            eprintln!("⚠  PipeWire restart failed — try: systemctl --user restart pipewire");
        }
    } else {
        println!("  Restart PipeWire to apply: broadcast-ctl install-config --apply");
    }

    Ok(())
}

/// PipeWire output loopback config: creates `broadcast_filter_sink` as a
/// virtual sink that passes audio through to real hardware without any
/// processing.  We deliberately do NOT apply a denoiser here — voice
/// denoisers treat music/game audio as noise and effectively mute it.
/// Denoising is done only on the microphone (input) side.
const OUTPUT_LOOPBACK_CONF: &str = r#"context.modules = [
  {
    name = libpipewire-module-loopback
    args = {
      node.description = "Broadcast Filter"
      capture.props = {
        node.name    = "broadcast_filter_sink"
        media.class  = Audio/Sink
        audio.rate   = 48000
      }
      playback.props = {
        node.name    = "broadcast_filter_output"
        node.passive = true
        audio.rate   = 48000
      }
    }
  }
]
"#;

fn write_deepfilter_configs(conf_dir: &std::path::Path) -> Result<()> {
    let input_conf = r#"context.modules = [
  {
    name = libpipewire-module-filter-chain
    args = {
      node.description = "Clean Mic (DeepFilter)"
      media.name        = "Clean Mic"
      filter.graph = {
        nodes = [
          {
            type   = ladspa
            name   = deepfilter
            plugin = /usr/lib/ladspa/libdeep_filter_ladspa.so
            label  = deep_filter_mono
            control = {
              "Attenuation Limit (dB)" = 100
            }
          }
        ]
      }
      capture.props = {
        node.name    = "capture.deepfilter_mic"
        node.passive = true
        audio.rate   = 48000
      }
      playback.props = {
        node.name    = "deepfilter_mic"
        media.class  = Audio/Source
        audio.rate   = 48000
      }
    }
  }
]
"#;

    let output_conf = OUTPUT_LOOPBACK_CONF;

    std::fs::write(conf_dir.join("50-deepfilter-input.conf"), input_conf)?;
    std::fs::write(conf_dir.join("50-deepfilter-output.conf"), output_conf)?;
    // Remove any Maxine configs to avoid conflicts
    let _ = std::fs::remove_file(conf_dir.join("50-maxine-input.conf"));
    let _ = std::fs::remove_file(conf_dir.join("50-maxine-output.conf"));
    println!("  Wrote DeepFilterNet config (CPU-based noise suppression)");
    Ok(())
}

/// Find or install the Maxine LADSPA plugin, returning its absolute path.
///
/// Search order:
///   1. `~/.local/lib/ladspa/libmaxine_ladspa.so` (already installed)
///   2. `/usr/lib/ladspa/libmaxine_ladspa.so` (system-wide install)
///   3. Same directory as the current broadcast-ctl binary (freshly built from cargo)
///      → named `libbroadcast_maxine_ladspa.so` (Rust cdylib naming convention)
///      → if found here it is automatically copied to `~/.local/lib/ladspa/`
fn resolve_maxine_plugin() -> Result<String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let user_ladspa = std::path::PathBuf::from(&home).join(".local/lib/ladspa");
    let user_plugin = user_ladspa.join("libmaxine_ladspa.so");
    let system_plugin = std::path::PathBuf::from("/usr/lib/ladspa/libmaxine_ladspa.so");

    if user_plugin.exists() {
        return Ok(user_plugin.to_string_lossy().into_owned());
    }
    if system_plugin.exists() {
        return Ok(system_plugin.to_string_lossy().into_owned());
    }

    // Check next to the current binary (cargo target dir after `cargo build --release`)
    let cargo_built = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("libbroadcast_maxine_ladspa.so")));

    if let Some(ref built_so) = cargo_built {
        if built_so.exists() {
            // Auto-install to user LADSPA dir
            std::fs::create_dir_all(&user_ladspa)?;
            std::fs::copy(built_so, &user_plugin).map_err(|e| {
                anyhow::anyhow!(
                    "Built Maxine plugin found at {} but could not install to {}: {e}",
                    built_so.display(),
                    user_plugin.display()
                )
            })?;
            println!(
                "  ✓ Installed Maxine plugin: {} → {}",
                built_so.display(),
                user_plugin.display()
            );
            return Ok(user_plugin.to_string_lossy().into_owned());
        }
    }

    anyhow::bail!(
        "NVIDIA Maxine LADSPA plugin not found.\n\
         Build it first:\n\
         \n\
         \x20  cargo build --release -p broadcast-maxine-ladspa\n\
         \n\
         The build requires the NVIDIA Audio Effects SDK:\n\
         \x20  https://developer.nvidia.com/nvidia-audio-effects-sdk\n\
         \x20  export NVAFX_SDK=/path/to/sdk && cargo build --release -p broadcast-maxine-ladspa\n\
         \n\
         Then re-run: broadcast-ctl install-config --apply"
    )
}

fn write_maxine_configs(conf_dir: &std::path::Path, intensity: f32) -> Result<()> {
    let plugin_path = resolve_maxine_plugin()?;

    let input_conf = format!(
        r#"context.modules = [
  {{
    name = libpipewire-module-filter-chain
    args = {{
      node.description = "Clean Mic (Maxine)"
      media.name        = "Clean Mic"
      filter.graph = {{
        nodes = [
          {{
            type   = ladspa
            name   = maxine_denoiser
            plugin = {plugin_path}
            label  = maxine_denoiser_mono
            control = {{
              "Intensity" = {intensity}
            }}
          }}
        ]
      }}
      capture.props = {{
        node.name    = "capture.deepfilter_mic"
        node.passive = true
        audio.rate   = 48000
      }}
      playback.props = {{
        node.name    = "deepfilter_mic"
        media.class  = Audio/Source
        audio.rate   = 48000
      }}
    }}
  }}
]
"#,
        plugin_path = plugin_path,
        intensity = intensity,
    );

    let output_conf = format!(
        r#"context.modules = [
  {{
    name = libpipewire-module-filter-chain
    args = {{
      node.description = "Broadcast Filter"
      media.name        = "Broadcast Filter"
      filter.graph = {{
        nodes = [
          {{
            type   = ladspa
            name   = maxine_denoiser
            plugin = {plugin_path}
            label  = maxine_denoiser_stereo
            control = {{
              "Intensity" = {intensity}
            }}
          }}
        ]
      }}
      capture.props = {{
        node.name    = "broadcast_filter_sink"
        media.class  = Audio/Sink
        audio.rate   = 48000
      }}
      playback.props = {{
        node.name    = "broadcast_filter_output"
        node.passive = true
        audio.rate   = 48000
      }}
    }}
  }}
]
"#,
        plugin_path = plugin_path,
        intensity = intensity,
    );

    std::fs::write(conf_dir.join("50-maxine-input.conf"), input_conf)?;
    std::fs::write(conf_dir.join("50-maxine-output.conf"), output_conf)?;
    // Remove DeepFilter configs to avoid both loading simultaneously
    let _ = std::fs::remove_file(conf_dir.join("50-deepfilter-input.conf"));
    let _ = std::fs::remove_file(conf_dir.join("50-deepfilter-output.conf"));
    println!("  Wrote NVIDIA Maxine config (GPU-accelerated noise suppression)");
    Ok(())
}
