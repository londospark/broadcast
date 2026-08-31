use anyhow::Result;

use crate::backend::PipeWireBackend;
use crate::pipewire;
use crate::state::{AppRoute, BroadcastState};

/// Pure function: find the default hardware sink's node.name from a list of
/// sinks, skipping broadcast filter sinks and virtual sinks.
/// If `preferred` is set and found, use it; otherwise fall back to first hardware sink.
///
/// Returns a name rather than a numeric index — see `move_sink_input` for why.
pub fn find_default_sink_name(
    sinks: &[serde_json::Value],
    filter_sink_name: &str,
    preferred: Option<&str>,
) -> Result<String> {
    // If a preferred sink is specified, try to find it first
    if let Some(preferred_name) = preferred {
        for sink in sinks {
            let props = sink.get("properties").and_then(|v| v.as_object());
            let name = props
                .and_then(|p| p.get("node.name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if name == preferred_name {
                return Ok(name.to_string());
            }
        }
        // Preferred not found — fall through to auto-detect
    }

    for sink in sinks {
        let props = sink.get("properties").and_then(|v| v.as_object());
        let name = props
            .and_then(|p| p.get("node.name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if name.is_empty() {
            continue;
        }
        // Skip any sink that is part of our filter chain
        if name == filter_sink_name || pipewire::is_broadcast_virtual_sink(name) {
            continue;
        }
        let media_class = props
            .and_then(|p| p.get("media.class"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if media_class.is_empty() || !media_class.contains("Virtual") {
            return Ok(name.to_string());
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
    let sinks = backend.list_sinks()?;
    let default_name = find_default_sink_name(
        &sinks,
        &state.nodes.output_sink,
        state.preferred_output_sink.as_deref(),
    )?;
    let mut routed = 0u32;

    // The filter sink's own name is always the routing target for
    // "Filtered" — no need to resolve it through `list_sinks` first (it may
    // not be enumerated there yet; see `move_sink_input`).
    let target: &str = match route {
        AppRoute::Filtered => &state.nodes.output_sink,
        AppRoute::Direct => &default_name,
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
/// The filter chain's own output is always routed to the preferred hardware sink.
pub fn apply_routes(backend: &dyn PipeWireBackend, state: &BroadcastState) -> Result<()> {
    let inputs = backend.list_sink_inputs()?;
    let sinks = backend.list_sinks()?;
    let default_name = find_default_sink_name(
        &sinks,
        &state.nodes.output_sink,
        state.preferred_output_sink.as_deref(),
    )?;

    for input in &inputs {
        // The filter chain's playback node must always target the preferred
        // hardware sink — never route it back into the filter (loop).
        // Also ensure it's unmuted (WirePlumber stream-restore may mute it).
        if input.node_name == state.nodes.output_playback {
            let _ = backend.move_sink_input(input.id, &default_name);
            let _ = backend.ensure_sink_input_unmuted(input.id);
            continue;
        }

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

        let target: &str = match route {
            AppRoute::Filtered => &state.nodes.output_sink,
            AppRoute::Direct => &default_name,
        };

        let _ = backend.move_sink_input(input.id, target);
    }
    Ok(())
}

/// Route a single audio stream, identified by its PipeWire sink-input id,
/// independently of any other stream from the same app.
///
/// Some apps — most notably Chromium-based browsers — run every window/tab's
/// audio through one shared process and never expose which stream belongs to
/// which tab or window, so PipeWire (and `route_app`, which matches on
/// app/client name) can only ever see them as indistinguishable copies of the
/// same app. Each one is still a distinct sink-input id, though, so routing
/// by id lets a specific stream be moved without touching its siblings.
///
/// Unlike `route_app`, this is not persisted to `app_routes` — there's no
/// stable name to persist against, since the id is only valid for the
/// lifetime of that particular stream (it changes on tab reload, replay,
/// etc). `apply_routes` will fall back to the app-level default the next
/// time it runs.
pub fn route_stream_id(
    backend: &dyn PipeWireBackend,
    state: &BroadcastState,
    stream_id: u32,
    route: AppRoute,
) -> Result<()> {
    let sinks = backend.list_sinks()?;
    let default_name = find_default_sink_name(
        &sinks,
        &state.nodes.output_sink,
        state.preferred_output_sink.as_deref(),
    )?;

    let target: &str = match route {
        AppRoute::Filtered => &state.nodes.output_sink,
        AppRoute::Direct => &default_name,
    };

    backend.move_sink_input(stream_id, target)
}

/// Move all audio streams to the default (real) speaker sink, bypassing filtering.
pub fn bypass_all(backend: &dyn PipeWireBackend, state: &BroadcastState) -> Result<()> {
    let inputs = backend.list_sink_inputs()?;
    let sinks = backend.list_sinks()?;
    let default_name = find_default_sink_name(
        &sinks,
        &state.nodes.output_sink,
        state.preferred_output_sink.as_deref(),
    )?;

    for input in &inputs {
        let _ = backend.move_sink_input(input.id, &default_name);
    }
    Ok(())
}

/// List currently running audio apps with their current routing.
pub fn list_apps(backend: &dyn PipeWireBackend, state: &BroadcastState) -> Result<Vec<AppInfo>> {
    let inputs = backend.list_sink_inputs()?;
    let broadcast_idx = backend.get_sink_index(&state.nodes.output_sink)?;

    let mut apps = Vec::new();
    for input in &inputs {
        // Skip the filter chain's own playback node — it's not a user app
        if input.node_name == state.nodes.output_playback {
            continue;
        }

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
            node_name: String::new(),
        }
    }

    fn make_filter_output(id: u32, sink: u32) -> SinkInput {
        SinkInput {
            id,
            sink_name: sink.to_string(),
            client_name: String::new(),
            app_binary: String::new(),
            media_name: "Broadcast Filter".to_string(),
            node_name: "broadcast_filter_output".to_string(),
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

    const HW: &str = "alsa_output.pci-0000_00_1f.3.analog-stereo";
    const FILTER: &str = "broadcast_filter_sink";
    const HW2: &str = "alsa_output.pci-0000_0c_00.4.analog-stereo";

    // ── find_default_sink_name ────────────────────────────────────────

    #[test]
    fn test_find_default_sink_name_basic() {
        let sinks = vec![hw_sink(), filter_sink()];
        let name = find_default_sink_name(&sinks, FILTER, None).unwrap();
        assert_eq!(name, HW);
    }

    #[test]
    fn test_find_default_sink_name_skips_filter() {
        // Filter sink listed first; should be skipped
        let sinks = vec![filter_sink(), hw_sink()];
        let name = find_default_sink_name(&sinks, FILTER, None).unwrap();
        assert_eq!(name, HW);
    }

    #[test]
    fn test_find_default_sink_name_skips_virtual() {
        let sinks = vec![virtual_sink(), filter_sink(), hw_sink()];
        let name = find_default_sink_name(&sinks, FILTER, None).unwrap();
        assert_eq!(name, HW);
    }

    #[test]
    fn test_find_default_sink_name_no_sinks() {
        let sinks: Vec<serde_json::Value> = vec![];
        assert!(find_default_sink_name(&sinks, FILTER, None).is_err());
    }

    #[test]
    fn test_find_default_sink_name_skips_maxine_filter_sink_too() {
        let maxine_sink = json!({
            "index": 14,
            "properties": {
                "node.name": "broadcast_maxine_sink",
                "media.class": "Audio/Sink"
            }
        });
        let sinks = vec![maxine_sink, hw_sink()];
        let name = find_default_sink_name(&sinks, FILTER, None).unwrap();
        assert_eq!(name, HW);
    }

    #[test]
    fn test_find_default_sink_name_preferred() {
        let second_hw_sink = json!({
            "index": 10,
            "properties": {
                "node.name": HW2,
                "media.class": "Audio/Sink"
            }
        });
        let sinks = vec![hw_sink(), filter_sink(), second_hw_sink];
        // With preferred set, should pick the preferred sink even though it's not first
        let name = find_default_sink_name(&sinks, FILTER, Some(HW2)).unwrap();
        assert_eq!(name, HW2);
    }

    #[test]
    fn test_find_default_sink_name_preferred_not_found_falls_back() {
        let sinks = vec![hw_sink(), filter_sink()];
        // Preferred sink doesn't exist — should fall back to first hardware sink
        let name = find_default_sink_name(&sinks, FILTER, Some("nonexistent_sink")).unwrap();
        assert_eq!(name, HW);
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
        assert_eq!(moved[0], (100, FILTER.to_string())); // moved to the filter sink, by name
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
        assert_eq!(moved[0], (100, HW.to_string())); // moved to the hw sink, by name
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

    // ── route_stream_id ───────────────────────────────────────────────

    #[test]
    fn test_route_stream_id_filtered() {
        // Two indistinguishable streams from the same app (e.g. two browser
        // windows) — only the targeted id should move.
        let inputs = vec![
            make_input(100, 5, "brave", "Brave", "Playback"),
            make_input(101, 5, "brave", "Brave", "Playback"),
        ];
        let backend = backend_with_sinks(inputs);
        let state = default_state();

        route_stream_id(&backend, &state, 101, AppRoute::Filtered).unwrap();

        let moved = backend.moved_inputs.borrow();
        assert_eq!(*moved, vec![(101, FILTER.to_string())]); // only 101 moved
    }

    #[test]
    fn test_route_stream_id_direct() {
        let inputs = vec![make_input(100, 8, "brave", "Brave", "Playback")];
        let backend = backend_with_sinks(inputs);
        let state = default_state();

        route_stream_id(&backend, &state, 100, AppRoute::Direct).unwrap();

        let moved = backend.moved_inputs.borrow();
        assert_eq!(*moved, vec![(100, HW.to_string())]); // moved to the hw sink
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
        // brave → filter sink, spotify → hw sink
        assert_eq!(moved[0], (100, FILTER.to_string()));
        assert_eq!(moved[1], (101, HW.to_string()));
    }

    // ── filter chain output routing ────────────────────────────────────

    #[test]
    fn test_apply_routes_filter_output_goes_to_hw_sink() {
        let inputs = vec![
            make_filter_output(43, 12), // filter output currently on wrong sink
            make_input(100, 8, "brave", "Brave Browser", "Playback"),
        ];
        let backend = backend_with_sinks(inputs);
        let mut state = default_state();
        state.set_app_route("brave", AppRoute::Filtered);

        apply_routes(&backend, &state).unwrap();

        let moved = backend.moved_inputs.borrow();
        assert_eq!(moved.len(), 2);
        // filter output → hw sink, brave → filter sink
        assert_eq!(moved[0], (43, HW.to_string()));
        assert_eq!(moved[1], (100, FILTER.to_string()));
        // filter output should be unmuted
        let unmuted = backend.unmuted_inputs.borrow();
        assert_eq!(unmuted.len(), 1);
        assert_eq!(unmuted[0], 43);
    }

    #[test]
    fn test_apply_routes_filter_output_never_loops_to_filter_sink() {
        // Even when default_route is Filtered, the filter chain output
        // must go to the hardware sink, not back into the filter.
        let inputs = vec![make_filter_output(43, 8)];
        let backend = backend_with_sinks(inputs);
        let mut state = default_state();
        state.default_route = AppRoute::Filtered;

        apply_routes(&backend, &state).unwrap();

        let moved = backend.moved_inputs.borrow();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0], (43, HW.to_string())); // hw sink, NOT filter sink
    }

    #[test]
    fn test_apply_routes_filter_output_uses_preferred_sink() {
        let second_hw = json!({
            "index": 10,
            "properties": {
                "node.name": HW2,
                "media.class": "Audio/Sink"
            }
        });
        let inputs = vec![make_filter_output(43, 5)];
        let b = MockBackend::new();
        *b.sink_inputs.borrow_mut() = inputs;
        b.sink_indices
            .borrow_mut()
            .insert("broadcast_filter_sink".into(), 8);
        *b.sinks.borrow_mut() = vec![hw_sink(), filter_sink(), second_hw];

        let mut state = default_state();
        state.set_preferred_output_sink(Some(HW2.into()));

        apply_routes(&b, &state).unwrap();

        let moved = b.moved_inputs.borrow();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0], (43, HW2.to_string())); // preferred sink
    }

    #[test]
    fn test_list_apps_hides_filter_output() {
        let inputs = vec![
            make_filter_output(43, 5),
            make_input(100, 8, "brave", "Brave Browser", "Playback"),
        ];
        let backend = backend_with_sinks(inputs);
        let state = default_state();

        let apps = list_apps(&backend, &state).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].binary, "brave");
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
        // Both moved to the hw sink
        assert_eq!(moved[0], (100, HW.to_string()));
        assert_eq!(moved[1], (101, HW.to_string()));
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
