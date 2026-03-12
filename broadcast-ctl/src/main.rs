use anyhow::Result;
use clap::{Parser, Subcommand};

use broadcast_core::backend::{PipeWireBackend, RealBackend};
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
    /// Apply saved routing preferences to all running streams
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
    }
    Ok(())
}

fn cmd_toggle(backend: &dyn PipeWireBackend) -> Result<()> {
    let mut state = BroadcastState::load()?;
    let active = !state.active;
    state.active = active;

    filter::set_filter_active(backend, &state, active)?;

    if active {
        routing::apply_routes(backend, &state)?;
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
    state.active = active;

    filter::set_filter_active(backend, &state, active)?;

    if active {
        routing::apply_routes(backend, &state)?;
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
    let loaded = filter::filters_loaded(backend, &state).unwrap_or(false);

    if json {
        let status = serde_json::json!({
            "active": state.active,
            "filters_loaded": loaded,
            "default_route": state.default_route,
            "app_routes": state.app_routes,
            "preferred_output_sink": state.preferred_output_sink,
            "preferred_input_source": state.preferred_input_source,
        });
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else if ironbar {
        if state.active && loaded {
            println!("󰍬 Broadcast");
        } else if loaded {
            println!("󰍭");
        } else {
            println!("󰍮");
        }
    } else {
        let status = if state.active { "ON" } else { "OFF" };
        let filter_status = if loaded { "loaded" } else { "not loaded" };
        println!("Broadcast: {status}");
        println!("Filters: {filter_status}");
        println!("Default route: {}", state.default_route);
        println!(
            "Output device: {}",
            state.preferred_output_sink.as_deref().unwrap_or("(auto)")
        );
        println!(
            "Input device: {}",
            state.preferred_input_source.as_deref().unwrap_or("(auto)")
        );
        if !state.app_routes.is_empty() {
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
