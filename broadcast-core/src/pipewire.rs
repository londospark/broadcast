use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;

/// A PipeWire node as reported by pw-dump.
#[derive(Debug, Clone, Deserialize)]
pub struct PwNode {
    pub id: u32,
    #[serde(default)]
    pub info: Option<PwNodeInfo>,
    #[serde(rename = "type")]
    pub node_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PwNodeInfo {
    pub props: Option<serde_json::Value>,
    pub params: Option<serde_json::Value>,
}

/// A PulseAudio sink-input (app stream routed to a sink).
#[derive(Debug, Clone)]
pub struct SinkInput {
    pub id: u32,
    pub sink_name: String,
    pub client_name: String,
    pub app_binary: String,
    pub media_name: String,
}

/// Get all PipeWire objects as JSON via pw-dump.
pub fn pw_dump() -> Result<Vec<serde_json::Value>> {
    let output = Command::new("pw-dump")
        .output()
        .context("Failed to run pw-dump")?;
    let objects: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).context("Failed to parse pw-dump JSON")?;
    Ok(objects)
}

/// Find PipeWire node IDs by node.name property.
pub fn find_node_id(node_name: &str) -> Result<Option<u32>> {
    let objects = pw_dump()?;
    for obj in &objects {
        if obj.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Node") {
            if let Some(props) = obj
                .pointer("/info/props")
                .and_then(|p| p.as_object())
            {
                if props.get("node.name").and_then(|v| v.as_str()) == Some(node_name) {
                    if let Some(id) = obj.get("id").and_then(|v| v.as_u64()) {
                        return Ok(Some(id as u32));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// List all sink inputs (app audio output streams) via pactl.
pub fn list_sink_inputs() -> Result<Vec<SinkInput>> {
    let output = Command::new("pactl")
        .args(["--format=json", "list", "sink-inputs"])
        .output()
        .context("Failed to run pactl list sink-inputs")?;

    if !output.status.success() {
        anyhow::bail!("pactl list sink-inputs failed");
    }

    let items: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).unwrap_or_default();

    let mut inputs = Vec::new();
    for item in &items {
        let id = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let sink_name = item
            .get("sink")
            .and_then(|v| v.as_u64())
            .map(|v| v.to_string())
            .unwrap_or_default();

        let props = item.get("properties").and_then(|v| v.as_object());
        let client_name = props
            .and_then(|p| p.get("application.name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let app_binary = props
            .and_then(|p| p.get("application.process.binary"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let media_name = props
            .and_then(|p| p.get("media.name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        inputs.push(SinkInput {
            id,
            sink_name,
            client_name,
            app_binary,
            media_name,
        });
    }
    Ok(inputs)
}

/// Get the default sink name.
pub fn get_default_sink() -> Result<String> {
    let output = Command::new("wpctl")
        .args(["inspect", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .context("Failed to run wpctl inspect")?;
    let text = String::from_utf8_lossy(&output.stdout);
    // Extract node.name from output
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("node.name") {
            if let Some(val) = line.split('=').nth(1) {
                return Ok(val.trim().trim_matches('"').to_string());
            }
        }
    }
    Ok("default".to_string())
}

/// Get the PulseAudio sink index for a node name.
pub fn get_sink_index(node_name: &str) -> Result<Option<u32>> {
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
        if name == node_name {
            if let Some(idx) = sink.get("index").and_then(|v| v.as_u64()) {
                return Ok(Some(idx as u32));
            }
        }
    }
    Ok(None)
}
