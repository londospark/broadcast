use anyhow::{Context, Result};
use std::process::Command;

use crate::pipewire;
use crate::state::BroadcastState;

/// Set the DeepFilterNet attenuation on a filter chain node.
/// 0.0 = passthrough (no filtering), 100.0 = full suppression.
fn set_attenuation(node_name: &str, attenuation: f64) -> Result<()> {
    let objects = pipewire::pw_dump()?;
    for obj in &objects {
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
        Command::new("pw-cli")
            .args(["set-param", &id.to_string(), "Props", &param])
            .output()
            .context("Failed to run pw-cli set-param")?;

        return Ok(());
    }
    Ok(())
}

/// Set attenuation on both input and output filters using configured node names.
pub fn set_filter_active(active: bool) -> Result<()> {
    let state = BroadcastState::load().unwrap_or_default();
    let attenuation = if active { 100.0 } else { 0.0 };
    set_attenuation(&state.nodes.input_capture, attenuation)?;
    set_attenuation(&state.nodes.output_sink, attenuation)?;
    Ok(())
}

/// Set just the input (mic) filter attenuation.
pub fn set_input_attenuation(attenuation: f64) -> Result<()> {
    let state = BroadcastState::load().unwrap_or_default();
    set_attenuation(&state.nodes.input_capture, attenuation)
}

/// Set just the output (speaker) filter attenuation.
pub fn set_output_attenuation(attenuation: f64) -> Result<()> {
    let state = BroadcastState::load().unwrap_or_default();
    set_attenuation(&state.nodes.output_sink, attenuation)
}

/// Check if the filter chain nodes exist in PipeWire.
pub fn filters_loaded() -> Result<bool> {
    let state = BroadcastState::load().unwrap_or_default();
    let input = pipewire::find_node_id(&state.nodes.input_capture)?;
    let output = pipewire::find_node_id(&state.nodes.output_sink)?;
    Ok(input.is_some() && output.is_some())
}
