use anyhow::Result;
use clap::{Parser, Subcommand};

use broadcast_core::{filter, routing, state::BroadcastState};

#[derive(Parser)]
#[command(name = "broadcast-ctl", about = "AI noise suppression control for PipeWire")]
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Toggle => cmd_toggle()?,
        Commands::On => cmd_set(true)?,
        Commands::Off => cmd_set(false)?,
        Commands::Status { ironbar, json } => cmd_status(ironbar, json)?,
        Commands::Route { app, mode } => cmd_route(&app, &mode)?,
        Commands::Apps => cmd_apps()?,
        Commands::Apply => cmd_apply()?,
    }
    Ok(())
}

fn cmd_toggle() -> Result<()> {
    let mut state = BroadcastState::load()?;
    state.output_filter = !state.output_filter;

    if state.output_filter {
        routing::apply_routes(&state.app_routes, state.default_route)?;
    } else {
        routing::bypass_all()?;
    }

    state.save()?;

    let icon = if state.output_filter { "󰍬" } else { "󰍭" };
    let label = if state.output_filter { "ON" } else { "OFF" };
    eprintln!("{icon} Broadcast {label}");
    Ok(())
}

fn cmd_set(active: bool) -> Result<()> {
    let mut state = BroadcastState::load()?;
    state.master = active;

    filter::set_filter_active(active)?;

    if active {
        routing::apply_routes(&state.app_routes, state.default_route)?;
    }

    state.save()?;

    let icon = if active { "󰍬" } else { "󰍭" };
    let label = if active { "ON" } else { "OFF" };
    eprintln!("{icon} Broadcast {label}");
    Ok(())
}

fn cmd_status(ironbar: bool, json: bool) -> Result<()> {
    let state = BroadcastState::load()?;
    let loaded = filter::filters_loaded().unwrap_or(false);

    if json {
        let status = serde_json::json!({
            "master": state.master,
            "input_filter": state.input_filter,
            "output_filter": state.output_filter,
            "filters_loaded": loaded,
            "default_route": state.default_route,
            "app_routes": state.app_routes,
        });
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else if ironbar {
        if state.output_filter && loaded {
            println!("󰍬 Broadcast");
        } else if loaded {
            println!("󰍭");
        } else {
            println!("󰍮");
        }
    } else {
        let status = if state.master { "ON" } else { "OFF" };
        let filter_status = if loaded { "loaded" } else { "not loaded" };
        println!("Broadcast: {status}");
        println!("Filters: {filter_status}");
        println!("Input filter: {}", if state.input_filter { "on" } else { "off" });
        println!("Output filter: {}", if state.output_filter { "on" } else { "off" });
        println!("Default route: {}", state.default_route);
        if !state.app_routes.is_empty() {
            println!("App routes:");
            for (app, route) in &state.app_routes {
                println!("  {app}: {route}");
            }
        }
    }
    Ok(())
}

fn cmd_route(app: &str, mode: &str) -> Result<()> {
    let route: broadcast_core::state::AppRoute = mode.parse()?;
    let mut state = BroadcastState::load()?;

    // Apply immediately if master is on
    if state.master {
        let moved = routing::route_app(app, route)?;
        eprintln!("Routed {moved} stream(s) for '{app}' → {route}");
    }

    // Save preference
    state.set_app_route(app, route);
    state.save()?;
    Ok(())
}

fn cmd_apps() -> Result<()> {
    let apps = routing::list_apps()?;
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

fn cmd_apply() -> Result<()> {
    let state = BroadcastState::load()?;
    if !state.master {
        eprintln!("Broadcast is OFF — not applying routes");
        return Ok(());
    }
    routing::apply_routes(&state.app_routes, state.default_route)?;
    eprintln!("Applied saved routing preferences");
    Ok(())
}
