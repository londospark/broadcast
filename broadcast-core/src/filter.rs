use anyhow::Result;
use serde_json::Value;

use crate::backend::PipeWireBackend;
use crate::pipewire;
use crate::state::BroadcastState;

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
    let objects = backend.pw_dump()?;
    let attenuation = if active { 100.0 } else { 0.0 };
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
}
