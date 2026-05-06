use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use crate::backend::PipeWireBackend;
use crate::pipewire;
use crate::state::BroadcastState;

/// Health report for the broadcast filter chains.
#[derive(Debug, Clone, Serialize)]
pub struct FilterHealth {
    /// Both filter chain nodes exist in PipeWire.
    pub filters_loaded: bool,
    /// The input (mic) filter chain is in a running state (not suspended).
    pub input_running: bool,
    /// The output (speaker) filter chain is in a running state (not suspended).
    pub output_running: bool,
    /// The system default audio source is the clean (filtered) mic.
    pub default_source_correct: bool,
    /// Human-readable list of detected issues.
    pub issues: Vec<String>,
}

impl FilterHealth {
    /// Returns true when everything is healthy and no issues are present.
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Check the health of the broadcast filter chains.
/// Verifies that the filter nodes exist, are running, and the default source is correct.
pub fn filter_health(backend: &dyn PipeWireBackend, state: &BroadcastState) -> FilterHealth {
    let mut issues = Vec::new();

    let objects = match backend.pw_dump() {
        Ok(o) => o,
        Err(e) => {
            return FilterHealth {
                filters_loaded: false,
                input_running: false,
                output_running: false,
                default_source_correct: false,
                issues: vec![format!("Failed to query PipeWire: {e}")],
            };
        }
    };

    let input_id = pipewire::find_node_id_in(&objects, &state.nodes.input_capture);
    let output_id = pipewire::find_node_id_in(&objects, &state.nodes.output_sink);

    let filters_loaded = input_id.is_some() && output_id.is_some();
    if !filters_loaded {
        if input_id.is_none() {
            issues.push(format!(
                "Input filter node '{}' not found — check PipeWire filter chain config",
                state.nodes.input_capture
            ));
        }
        if output_id.is_none() {
            issues.push(format!(
                "Output filter node '{}' not found — check PipeWire filter chain config",
                state.nodes.output_sink
            ));
        }
    }

    // Check node states in pw-dump output
    let input_running = check_node_running(&objects, &state.nodes.input_capture);
    let output_running = check_node_running(&objects, &state.nodes.output_sink);

    if filters_loaded && !input_running {
        // The input capture node has node.passive = true — it suspends when no app
        // is actively recording from deepfilter_mic. This is expected, not an error.
        // Only flag it if broadcast is actively enabled and we expect it to be running.
        // For now we track it for information purposes but don't raise an issue.
    }
    if filters_loaded && !output_running {
        // The output filter sink suspends when no apps are currently playing through it.
        // This is expected passive behaviour — it wakes up as soon as audio is routed to it.
        // Only flag it as an error if the node is missing entirely (handled above).
    }

    // Check that the default source is the clean mic
    let expected_source = &state.nodes.input_capture.replace("capture.", "");
    let default_source_correct = match backend.get_default_source() {
        Ok(source) => {
            // Accept either the playback node name or an exact match
            source == *expected_source || source.contains(expected_source.as_str())
        }
        Err(_) => false,
    };
    if !default_source_correct {
        let current = backend
            .get_default_source()
            .unwrap_or_else(|_| "(unknown)".to_string());
        issues.push(format!(
            "Default source is '{current}' — should be '{expected_source}' (filtered mic). \
             Run: broadcast-ctl fix-routing"
        ));
    }

    FilterHealth {
        filters_loaded,
        input_running,
        output_running,
        default_source_correct,
        issues,
    }
}

/// Returns true if the named node is present and not in a suspended state.
fn check_node_running(objects: &[Value], node_name: &str) -> bool {
    for obj in objects {
        if obj.get("type").and_then(|t| t.as_str()) != Some("PipeWire:Interface:Node") {
            continue;
        }
        let props = match obj.pointer("/info/props").and_then(|p| p.as_object()) {
            Some(p) => p,
            None => continue,
        };
        let name = props
            .get("node.name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if name != node_name {
            continue;
        }
        // If the node has a state field, check it's not "suspended"
        if let Some(state_str) = obj.pointer("/info/state").and_then(|v| v.as_str()) {
            return state_str != "suspended";
        }
        // Node exists but no state field — assume running
        return true;
    }
    false
}

/// Set the DeepFilterNet attenuation on a filter chain node found in `objects`.
/// 0.0 = passthrough (no filtering), 100.0 = full suppression.
pub fn set_attenuation(
    backend: &dyn PipeWireBackend,
    objects: &[Value],
    node_name: &str,
    attenuation: f64,
) -> Result<()> {
    for obj in objects {
        if obj.get("type").and_then(|t| t.as_str()) != Some("PipeWire:Interface:Node") {
            continue;
        }
        let props = match obj.pointer("/info/props").and_then(|p| p.as_object()) {
            Some(p) => p,
            None => continue,
        };
        let name = props
            .get("node.name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if name != node_name {
            continue;
        }

        let id = obj.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        if id == 0 {
            continue;
        }

        let param = format!(
            r#"{{ "Spa:Pod:Object:Param:Props" = {{ "params" = [ "Spa:Float" {} ] }} }}"#,
            attenuation
        );
        backend.set_param(id, "Props", &param)?;

        return Ok(());
    }
    Ok(())
}

/// Set attenuation on both input and output filters using configured node names.
pub fn set_filter_active(
    backend: &dyn PipeWireBackend,
    state: &BroadcastState,
    active: bool,
) -> Result<()> {
    #[cfg(not(test))]
    // Ensure Maxine configs are enabled/disabled when using the Maxine backend.
    // Enabling/disabling may restart PipeWire, so do this before manipulating node params.
    {
        if state.backend == crate::state::Backend::Maxine {
            crate::pipewire::set_maxine_enabled(active)?;
        } else {
            // Best-effort: disable any Maxine configs when not using Maxine
            let _ = crate::pipewire::set_maxine_enabled(false);
        }
    }

    let objects = backend.pw_dump()?;
    let attenuation = if active { 100.0 } else { 0.0 };
    // It's possible the nodes aren't immediately present after a restart; set_attenuation
    // will silently do nothing if the node isn't found.
    set_attenuation(backend, &objects, &state.nodes.input_capture, attenuation)?;
    set_attenuation(backend, &objects, &state.nodes.output_sink, attenuation)?;
    Ok(())
}

/// Set just the input (mic) filter attenuation.
pub fn set_input_attenuation(
    backend: &dyn PipeWireBackend,
    state: &BroadcastState,
    attenuation: f64,
) -> Result<()> {
    let objects = backend.pw_dump()?;
    set_attenuation(backend, &objects, &state.nodes.input_capture, attenuation)
}

/// Set just the output (speaker) filter attenuation.
pub fn set_output_attenuation(
    backend: &dyn PipeWireBackend,
    state: &BroadcastState,
    attenuation: f64,
) -> Result<()> {
    let objects = backend.pw_dump()?;
    set_attenuation(backend, &objects, &state.nodes.output_sink, attenuation)
}

/// Check if the filter chain nodes exist in PipeWire.
pub fn filters_loaded(backend: &dyn PipeWireBackend, state: &BroadcastState) -> Result<bool> {
    let objects = backend.pw_dump()?;
    let input = pipewire::find_node_id_in(&objects, &state.nodes.input_capture);
    let output = pipewire::find_node_id_in(&objects, &state.nodes.output_sink);
    Ok(input.is_some() && output.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::MockBackend;
    use serde_json::json;

    fn default_state() -> BroadcastState {
        BroadcastState::default()
    }

    fn pw_dump_with_both_nodes() -> Vec<serde_json::Value> {
        vec![
            json!({
                "id": 42,
                "type": "PipeWire:Interface:Node",
                "info": { "props": { "node.name": "capture.deepfilter_mic" } }
            }),
            json!({
                "id": 43,
                "type": "PipeWire:Interface:Node",
                "info": { "props": { "node.name": "broadcast_filter_sink" } }
            }),
        ]
    }

    #[test]
    fn test_set_filter_active_on() {
        let backend = MockBackend::new();
        *backend.pw_dump_result.borrow_mut() = pw_dump_with_both_nodes();
        let state = default_state();

        set_filter_active(&backend, &state, true).unwrap();

        let params = backend.set_params.borrow();
        assert_eq!(params.len(), 2);
        // Both should have attenuation 100.0
        assert!(params[0].2.contains("100"));
        assert!(params[1].2.contains("100"));
        // Node IDs should be 42 (input) and 43 (output)
        assert_eq!(params[0].0, 42);
        assert_eq!(params[1].0, 43);
    }

    #[test]
    fn test_set_filter_active_off() {
        let backend = MockBackend::new();
        *backend.pw_dump_result.borrow_mut() = pw_dump_with_both_nodes();
        let state = default_state();

        set_filter_active(&backend, &state, false).unwrap();

        let params = backend.set_params.borrow();
        assert_eq!(params.len(), 2);
        assert!(params[0].2.contains("0"));
        assert!(params[1].2.contains("0"));
    }

    #[test]
    fn test_set_attenuation_node_not_found() {
        let backend = MockBackend::new();
        // Empty pw-dump — no nodes exist
        *backend.pw_dump_result.borrow_mut() = vec![];

        // Should succeed without error, just not call set_param
        set_attenuation(&backend, &[], "nonexistent_node", 50.0).unwrap();
        assert!(backend.set_params.borrow().is_empty());
    }

    #[test]
    fn test_filters_loaded_both_present() {
        let backend = MockBackend::new();
        *backend.pw_dump_result.borrow_mut() = pw_dump_with_both_nodes();
        let state = default_state();

        assert!(filters_loaded(&backend, &state).unwrap());
    }

    #[test]
    fn test_filters_loaded_missing() {
        let backend = MockBackend::new();
        // Only the input node, missing the output
        *backend.pw_dump_result.borrow_mut() = vec![json!({
            "id": 42,
            "type": "PipeWire:Interface:Node",
            "info": { "props": { "node.name": "capture.deepfilter_mic" } }
        })];
        let state = default_state();

        assert!(!filters_loaded(&backend, &state).unwrap());
    }

    // ── filter_health ─────────────────────────────────────────────────

    fn pw_dump_with_running_nodes() -> Vec<serde_json::Value> {
        vec![
            json!({
                "id": 42,
                "type": "PipeWire:Interface:Node",
                "info": {
                    "state": "running",
                    "props": { "node.name": "capture.deepfilter_mic" }
                }
            }),
            json!({
                "id": 43,
                "type": "PipeWire:Interface:Node",
                "info": {
                    "state": "running",
                    "props": { "node.name": "broadcast_filter_sink" }
                }
            }),
        ]
    }

    #[test]
    fn test_filter_health_all_ok() {
        let backend = MockBackend::new();
        *backend.pw_dump_result.borrow_mut() = pw_dump_with_running_nodes();
        *backend.default_source_name.borrow_mut() = "deepfilter_mic".to_string();
        let state = default_state();

        let h = filter_health(&backend, &state);
        assert!(h.filters_loaded);
        assert!(h.input_running);
        assert!(h.output_running);
        assert!(h.default_source_correct);
        assert!(h.is_ok());
        assert!(h.issues.is_empty());
    }

    #[test]
    fn test_filter_health_wrong_default_source() {
        let backend = MockBackend::new();
        *backend.pw_dump_result.borrow_mut() = pw_dump_with_running_nodes();
        *backend.default_source_name.borrow_mut() = "alsa_input.usb-Razer_something".to_string();
        let state = default_state();

        let h = filter_health(&backend, &state);
        assert!(!h.default_source_correct);
        assert!(!h.is_ok());
        assert!(h.issues.iter().any(|i| i.contains("Default source")));
    }

    #[test]
    fn test_filter_health_suspended_nodes() {
        let backend = MockBackend::new();
        *backend.pw_dump_result.borrow_mut() = vec![
            json!({
                "id": 42,
                "type": "PipeWire:Interface:Node",
                "info": { "state": "suspended", "props": { "node.name": "capture.deepfilter_mic" } }
            }),
            json!({
                "id": 43,
                "type": "PipeWire:Interface:Node",
                "info": { "state": "suspended", "props": { "node.name": "broadcast_filter_sink" } }
            }),
        ];
        *backend.default_source_name.borrow_mut() = "deepfilter_mic".to_string();
        let state = default_state();

        let h = filter_health(&backend, &state);
        assert!(h.filters_loaded);
        assert!(!h.input_running);
        assert!(!h.output_running);
        // Both input and output suspended are fine — passive nodes that wake on demand.
        // No issues should be reported when only the filter chains are suspended.
        assert!(
            h.is_ok(),
            "suspended filters are expected passive behaviour, not an error"
        );
    }

    #[test]
    fn test_filter_health_nodes_missing() {
        let backend = MockBackend::new();
        *backend.pw_dump_result.borrow_mut() = vec![];
        *backend.default_source_name.borrow_mut() = "deepfilter_mic".to_string();
        let state = default_state();

        let h = filter_health(&backend, &state);
        assert!(!h.filters_loaded);
        assert!(!h.is_ok());
        assert!(h.issues.len() >= 2); // one per missing node
    }
}
