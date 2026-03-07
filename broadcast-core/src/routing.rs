use anyhow::{Context, Result};
use std::process::Command;

use crate::pipewire;
use crate::state::{AppRoute, BroadcastState};

/// Move a specific sink-input (by pactl index) to a given sink.
fn move_sink_input(input_id: u32, sink_id: u32) -> Result<()> {
    let status = Command::new("pactl")
        .args([
            "move-sink-input",
            &input_id.to_string(),
            &sink_id.to_string(),
        ])
        .status()
        .context("Failed to run pactl move-sink-input")?;

    if !status.success() {
        anyhow::bail!("pactl move-sink-input failed for input {input_id} → sink {sink_id}");
    }
    Ok(())
}

/// Get the pactl sink index for the Broadcast filter sink.
fn broadcast_sink_index(sink_name: &str) -> Result<Option<u32>> {
    pipewire::get_sink_index(sink_name)
}

/// Get the pactl sink index for the default (real) sink.
fn default_sink_index(filter_sink_name: &str) -> Result<u32> {
    let output = Command::new("pactl")
        .args(["--format=json", "list", "sinks"])
        .output()
        .context("Failed to run pactl list sinks")?;

    let sinks: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).unwrap_or_default();

    for sink in &sinks {
        let props = sink.get("properties").and_then(|v| v.as_object());
        let name = props
            .and_then(|p| p.get("node.name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // Skip any sink that is part of our filter chain
        if name == filter_sink_name || name.contains("broadcast_filter") {
            continue;
        }
        let media_class = props
            .and_then(|p| p.get("media.class"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if media_class.is_empty() || !media_class.contains("Virtual") {
            if let Some(idx) = sink.get("index").and_then(|v| v.as_u64()) {
                return Ok(idx as u32);
            }
        }
    }
    anyhow::bail!("Could not find default hardware sink")
}

/// Route a specific app's streams to either filtered or direct output.
pub fn route_app(app_name: &str, route: AppRoute) -> Result<u32> {
    let state = BroadcastState::load().unwrap_or_default();
    let inputs = pipewire::list_sink_inputs()?;
    let broadcast_idx = broadcast_sink_index(&state.nodes.output_sink)?;
    let default_idx = default_sink_index(&state.nodes.output_sink)?;
    let mut routed = 0u32;

    let target = match route {
        AppRoute::Filtered => match broadcast_idx {
            Some(idx) => idx,
            None => anyhow::bail!("Broadcast filter sink not found"),
        },
        AppRoute::Direct => default_idx,
    };

    let app_lower = app_name.to_lowercase();
    for input in &inputs {
        let matches = input.app_binary.to_lowercase().contains(&app_lower)
            || input.client_name.to_lowercase().contains(&app_lower);
        if matches {
            move_sink_input(input.id, target)?;
            routed += 1;
        }
    }
    Ok(routed)
}

/// Route all apps according to saved preferences.
pub fn apply_routes(routes: &std::collections::HashMap<String, AppRoute>, default_route: AppRoute) -> Result<()> {
    let state = BroadcastState::load().unwrap_or_default();
    let inputs = pipewire::list_sink_inputs()?;
    let broadcast_idx = broadcast_sink_index(&state.nodes.output_sink)?;
    let default_idx = default_sink_index(&state.nodes.output_sink)?;

    for input in &inputs {
        let app_key = if !input.app_binary.is_empty() {
            input.app_binary.to_lowercase()
        } else {
            input.client_name.to_lowercase()
        };

        let route = routes.get(&app_key).copied().unwrap_or(default_route);

        let target = match route {
            AppRoute::Filtered => match broadcast_idx {
                Some(idx) => idx,
                None => continue,
            },
            AppRoute::Direct => default_idx,
        };

        let _ = move_sink_input(input.id, target);
    }
    Ok(())
}

/// Move all audio streams to the default (real) speaker sink, bypassing filtering.
pub fn bypass_all() -> Result<()> {
    let state = BroadcastState::load().unwrap_or_default();
    let inputs = pipewire::list_sink_inputs()?;
    let default_idx = default_sink_index(&state.nodes.output_sink)?;

    for input in &inputs {
        let _ = move_sink_input(input.id, default_idx);
    }
    Ok(())
}

/// List currently running audio apps with their current routing.
pub fn list_apps() -> Result<Vec<AppInfo>> {
    let state = BroadcastState::load().unwrap_or_default();
    let inputs = pipewire::list_sink_inputs()?;
    let broadcast_idx = broadcast_sink_index(&state.nodes.output_sink)?;

    let mut apps = Vec::new();
    for input in &inputs {
        let is_filtered = broadcast_idx
            .map(|idx| input.sink_name == idx.to_string())
            .unwrap_or(false);

        apps.push(AppInfo {
            id: input.id,
            name: if !input.client_name.is_empty() {
                input.client_name.clone()
            } else {
                input.app_binary.clone()
            },
            binary: input.app_binary.clone(),
            media: input.media_name.clone(),
            route: if is_filtered {
                AppRoute::Filtered
            } else {
                AppRoute::Direct
            },
        });
    }
    Ok(apps)
}

#[derive(Debug, Clone)]
pub struct AppInfo {
    pub id: u32,
    pub name: String,
    pub binary: String,
    pub media: String,
    pub route: AppRoute,
}
