use anyhow::Result;

use crate::backend::PipeWireBackend;
use crate::state::{AppRoute, BroadcastState};

/// Pure function: find the default hardware sink index from a list of sinks,
/// skipping broadcast filter sinks and virtual sinks.
pub fn find_default_sink_index(sinks: &[serde_json::Value], filter_sink_name: &str) -> Result<u32> {
    for sink in sinks {
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
pub fn route_app(
    backend: &dyn PipeWireBackend,
    state: &BroadcastState,
    app_name: &str,
    route: AppRoute,
) -> Result<u32> {
    let inputs = backend.list_sink_inputs()?;
    let broadcast_idx = backend.get_sink_index(&state.nodes.output_sink)?;
    let sinks = backend.list_sinks()?;
    let default_idx = find_default_sink_index(&sinks, &state.nodes.output_sink)?;
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
            backend.move_sink_input(input.id, target)?;
            routed += 1;
        }
    }
    Ok(routed)
}

/// Route all apps according to saved preferences.
pub fn apply_routes(backend: &dyn PipeWireBackend, state: &BroadcastState) -> Result<()> {
    let inputs = backend.list_sink_inputs()?;
    let broadcast_idx = backend.get_sink_index(&state.nodes.output_sink)?;
    let sinks = backend.list_sinks()?;
    let default_idx = find_default_sink_index(&sinks, &state.nodes.output_sink)?;

    for input in &inputs {
        let app_key = if !input.app_binary.is_empty() {
            input.app_binary.to_lowercase()
        } else {
            input.client_name.to_lowercase()
        };

        let route = state
            .app_routes
            .get(&app_key)
            .copied()
            .unwrap_or(state.default_route);

        let target = match route {
            AppRoute::Filtered => match broadcast_idx {
                Some(idx) => idx,
                None => continue,
            },
            AppRoute::Direct => default_idx,
        };

        let _ = backend.move_sink_input(input.id, target);
    }
    Ok(())
}

/// Move all audio streams to the default (real) speaker sink, bypassing filtering.
pub fn bypass_all(backend: &dyn PipeWireBackend, state: &BroadcastState) -> Result<()> {
    let inputs = backend.list_sink_inputs()?;
    let sinks = backend.list_sinks()?;
    let default_idx = find_default_sink_index(&sinks, &state.nodes.output_sink)?;

    for input in &inputs {
        let _ = backend.move_sink_input(input.id, default_idx);
    }
    Ok(())
}

/// List currently running audio apps with their current routing.
pub fn list_apps(backend: &dyn PipeWireBackend, state: &BroadcastState) -> Result<Vec<AppInfo>> {
    let inputs = backend.list_sink_inputs()?;
    let broadcast_idx = backend.get_sink_index(&state.nodes.output_sink)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipewire::SinkInput;
    use crate::test_helpers::MockBackend;
    use serde_json::json;

    fn hw_sink() -> serde_json::Value {
        json!({
            "index": 5,
            "properties": {
                "node.name": "alsa_output.pci-0000_00_1f.3.analog-stereo",
                "media.class": "Audio/Sink"
            }
        })
    }

    fn filter_sink() -> serde_json::Value {
        json!({
            "index": 8,
            "properties": {
                "node.name": "broadcast_filter_sink",
                "media.class": "Audio/Sink"
            }
        })
    }

    fn virtual_sink() -> serde_json::Value {
        json!({
            "index": 12,
            "properties": {
                "node.name": "virtual_mic_sink",
                "media.class": "Audio/Sink/Virtual"
            }
        })
    }

    fn make_input(id: u32, sink: u32, binary: &str, client: &str, media: &str) -> SinkInput {
        SinkInput {
            id,
            sink_name: sink.to_string(),
            client_name: client.to_string(),
            app_binary: binary.to_string(),
            media_name: media.to_string(),
        }
    }

    fn default_state() -> BroadcastState {
        BroadcastState::default()
    }

    fn backend_with_sinks(inputs: Vec<SinkInput>) -> MockBackend {
        let b = MockBackend::new();
        *b.sink_inputs.borrow_mut() = inputs;
        b.sink_indices
            .borrow_mut()
            .insert("broadcast_filter_sink".into(), 8);
        *b.sinks.borrow_mut() = vec![hw_sink(), filter_sink()];
        b
    }

    // ── find_default_sink_index ────────────────────────────────────────

    #[test]
    fn test_find_default_sink_index_basic() {
        let sinks = vec![hw_sink(), filter_sink()];
        let idx = find_default_sink_index(&sinks, "broadcast_filter_sink").unwrap();
        assert_eq!(idx, 5);
    }

    #[test]
    fn test_find_default_sink_index_skips_filter() {
        // Filter sink listed first; should be skipped
        let sinks = vec![filter_sink(), hw_sink()];
        let idx = find_default_sink_index(&sinks, "broadcast_filter_sink").unwrap();
        assert_eq!(idx, 5);
    }

    #[test]
    fn test_find_default_sink_index_skips_virtual() {
        let sinks = vec![virtual_sink(), filter_sink(), hw_sink()];
        let idx = find_default_sink_index(&sinks, "broadcast_filter_sink").unwrap();
        assert_eq!(idx, 5);
    }

    #[test]
    fn test_find_default_sink_index_no_sinks() {
        let sinks: Vec<serde_json::Value> = vec![];
        assert!(find_default_sink_index(&sinks, "broadcast_filter_sink").is_err());
    }

    // ── route_app ──────────────────────────────────────────────────────

    #[test]
    fn test_route_app_filtered() {
        let inputs = vec![
            make_input(100, 5, "brave", "Brave Browser", "Playback"),
            make_input(101, 5, "spotify", "Spotify", "Music"),
        ];
        let backend = backend_with_sinks(inputs);
        let state = default_state();

        let routed = route_app(&backend, &state, "brave", AppRoute::Filtered).unwrap();
        assert_eq!(routed, 1);

        let moved = backend.moved_inputs.borrow();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0], (100, 8)); // moved to filter sink index 8
    }

    #[test]
    fn test_route_app_direct() {
        let inputs = vec![make_input(100, 8, "brave", "Brave Browser", "Playback")];
        let backend = backend_with_sinks(inputs);
        let state = default_state();

        let routed = route_app(&backend, &state, "brave", AppRoute::Direct).unwrap();
        assert_eq!(routed, 1);

        let moved = backend.moved_inputs.borrow();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0], (100, 5)); // moved to hw sink index 5
    }

    #[test]
    fn test_route_app_no_match() {
        let inputs = vec![make_input(100, 5, "brave", "Brave Browser", "Playback")];
        let backend = backend_with_sinks(inputs);
        let state = default_state();

        let routed = route_app(&backend, &state, "firefox", AppRoute::Filtered).unwrap();
        assert_eq!(routed, 0);
        assert!(backend.moved_inputs.borrow().is_empty());
    }

    // ── apply_routes ───────────────────────────────────────────────────

    #[test]
    fn test_apply_routes_mixed() {
        let inputs = vec![
            make_input(100, 5, "brave", "Brave Browser", "Playback"),
            make_input(101, 8, "spotify", "Spotify", "Music"),
        ];
        let backend = backend_with_sinks(inputs);
        let mut state = default_state();
        state.set_app_route("brave", AppRoute::Filtered);
        state.set_app_route("spotify", AppRoute::Direct);

        apply_routes(&backend, &state).unwrap();

        let moved = backend.moved_inputs.borrow();
        assert_eq!(moved.len(), 2);
        // brave → filter sink (8), spotify → hw sink (5)
        assert_eq!(moved[0], (100, 8));
        assert_eq!(moved[1], (101, 5));
    }

    // ── bypass_all ─────────────────────────────────────────────────────

    #[test]
    fn test_bypass_all() {
        let inputs = vec![
            make_input(100, 8, "brave", "Brave Browser", "Playback"),
            make_input(101, 8, "spotify", "Spotify", "Music"),
        ];
        let backend = backend_with_sinks(inputs);
        let state = default_state();

        bypass_all(&backend, &state).unwrap();

        let moved = backend.moved_inputs.borrow();
        assert_eq!(moved.len(), 2);
        // Both moved to hw sink (5)
        assert_eq!(moved[0], (100, 5));
        assert_eq!(moved[1], (101, 5));
    }

    // ── list_apps ──────────────────────────────────────────────────────

    #[test]
    fn test_list_apps_identifies_filtered() {
        let inputs = vec![
            make_input(100, 8, "brave", "Brave Browser", "Playback"),
            make_input(101, 5, "spotify", "Spotify", "Music"),
        ];
        let backend = backend_with_sinks(inputs);
        let state = default_state();

        let apps = list_apps(&backend, &state).unwrap();
        assert_eq!(apps.len(), 2);

        // brave is on sink 8 (broadcast), so Filtered
        assert_eq!(apps[0].name, "Brave Browser");
        assert_eq!(apps[0].route, AppRoute::Filtered);

        // spotify is on sink 5 (hw), so Direct
        assert_eq!(apps[1].name, "Spotify");
        assert_eq!(apps[1].route, AppRoute::Direct);
    }
}
