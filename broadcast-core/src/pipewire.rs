use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::backend::{PipeWireBackend, RealBackend};

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
    /// PipeWire node.name (e.g. "broadcast_filter_output" for filter chain playback)
    pub node_name: String,
}

/// An available audio device (sink or source) with a user-friendly description.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioDevice {
    /// PipeWire node.name (stable identifier, e.g. "alsa_output.pci-0000_0c_00.4.analog-stereo")
    pub name: String,
    /// Human-readable description (e.g. "Starship/Matisse HD Audio Controller Analog Stereo")
    pub description: String,
}

/// Parse pactl JSON sinks into a list of available hardware output devices,
/// filtering out broadcast filter sinks and virtual sinks.
pub fn parse_sinks_as_devices(
    sinks: &[serde_json::Value],
    filter_sink_name: &str,
) -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    for sink in sinks {
        let props = sink.get("properties").and_then(|v| v.as_object());
        let name = props
            .and_then(|p| p.get("node.name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if name.is_empty() || name == filter_sink_name || name.contains("broadcast_filter") {
            continue;
        }
        let media_class = props
            .and_then(|p| p.get("media.class"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if media_class.contains("Virtual") {
            continue;
        }
        let description = props
            .and_then(|p| p.get("node.description"))
            .and_then(|v| v.as_str())
            .or_else(|| sink.get("description").and_then(|v| v.as_str()))
            .or_else(|| {
                props
                    .and_then(|p| p.get("node.nick"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or(name)
            .to_string();
        devices.push(AudioDevice {
            name: name.to_string(),
            description,
        });
    }
    devices
}

/// Parse pactl JSON sources into a list of available hardware input devices,
/// filtering out monitors, virtual sources, and broadcast filter sources.
pub fn parse_sources_as_devices(sources: &[serde_json::Value]) -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    for source in sources {
        let props = source.get("properties").and_then(|v| v.as_object());
        let name = props
            .and_then(|p| p.get("node.name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let media_class = props
            .and_then(|p| p.get("media.class"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // Skip monitor sources (they mirror sink output) and virtual sources
        if media_class.contains("Virtual") {
            continue;
        }
        // Skip monitor sources — they have media.class "Audio/Sink" rather than "Audio/Source"
        if media_class.contains("Sink") {
            continue;
        }
        // Skip deepfilter virtual mic
        if name.contains("deepfilter") {
            continue;
        }
        let description = props
            .and_then(|p| p.get("node.description"))
            .and_then(|v| v.as_str())
            .or_else(|| source.get("description").and_then(|v| v.as_str()))
            .or_else(|| {
                props
                    .and_then(|p| p.get("node.nick"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or(name)
            .to_string();
        devices.push(AudioDevice {
            name: name.to_string(),
            description,
        });
    }
    devices
}

// ---------------------------------------------------------------------------
// Pure parsing / search functions (no I/O, fully testable)
// ---------------------------------------------------------------------------

/// Search pw-dump JSON objects for a node ID by node.name.
pub fn find_node_id_in(objects: &[serde_json::Value], node_name: &str) -> Option<u32> {
    for obj in objects {
        if obj.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Node") {
            if let Some(props) = obj.pointer("/info/props").and_then(|p| p.as_object()) {
                if props.get("node.name").and_then(|v| v.as_str()) == Some(node_name) {
                    if let Some(id) = obj.get("id").and_then(|v| v.as_u64()) {
                        return Some(id as u32);
                    }
                }
            }
        }
    }
    None
}

/// Parse pactl JSON sink-input items into `SinkInput` structs.
pub fn parse_sink_inputs(items: &[serde_json::Value]) -> Vec<SinkInput> {
    let mut inputs = Vec::new();
    for item in items {
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
        let node_name = props
            .and_then(|p| p.get("node.name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        inputs.push(SinkInput {
            id,
            sink_name,
            client_name,
            app_binary,
            media_name,
            node_name,
        });
    }
    inputs
}

/// Parse the node.name from wpctl inspect output.
pub fn parse_default_sink(text: &str) -> String {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("node.name") {
            if let Some(val) = line.split('=').nth(1) {
                return val.trim().trim_matches('"').to_string();
            }
        }
    }
    "default".to_string()
}

/// Search pactl JSON sinks for the index matching a given node.name.
pub fn find_sink_index_in(sinks: &[serde_json::Value], node_name: &str) -> Option<u32> {
    for sink in sinks {
        let props = sink.get("properties").and_then(|v| v.as_object());
        let name = props
            .and_then(|p| p.get("node.name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if name == node_name {
            if let Some(idx) = sink.get("index").and_then(|v| v.as_u64()) {
                return Some(idx as u32);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Convenience wrappers using RealBackend (preserve original public API)
// ---------------------------------------------------------------------------

/// Get all PipeWire objects as JSON via pw-dump.
pub fn pw_dump() -> Result<Vec<serde_json::Value>> {
    RealBackend.pw_dump()
}

/// Find PipeWire node IDs by node.name property.
pub fn find_node_id(node_name: &str) -> Result<Option<u32>> {
    let objects = RealBackend.pw_dump()?;
    Ok(find_node_id_in(&objects, node_name))
}

/// List all sink inputs (app audio output streams) via pactl.
pub fn list_sink_inputs() -> Result<Vec<SinkInput>> {
    RealBackend.list_sink_inputs()
}

/// Get the default sink name.
pub fn get_default_sink() -> Result<String> {
    RealBackend.get_default_sink()
}

/// Get the PulseAudio sink index for a node name.
pub fn get_sink_index(node_name: &str) -> Result<Option<u32>> {
    RealBackend.get_sink_index(node_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── find_node_id_in ────────────────────────────────────────────────

    #[test]
    fn test_find_node_id_in_found() {
        let objects = vec![
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
        ];
        assert_eq!(find_node_id_in(&objects, "broadcast_filter_sink"), Some(43));
    }

    #[test]
    fn test_find_node_id_in_not_found() {
        let objects = vec![json!({
            "id": 42,
            "type": "PipeWire:Interface:Node",
            "info": { "props": { "node.name": "some_other_node" } }
        })];
        assert_eq!(find_node_id_in(&objects, "nonexistent"), None);
    }

    #[test]
    fn test_find_node_id_in_wrong_type() {
        let objects = vec![
            json!({
                "id": 99,
                "type": "PipeWire:Interface:Link",
                "info": { "props": { "node.name": "broadcast_filter_sink" } }
            }),
            json!({
                "id": 43,
                "type": "PipeWire:Interface:Node",
                "info": { "props": { "node.name": "broadcast_filter_sink" } }
            }),
        ];
        // Should skip the Link and find the Node
        assert_eq!(find_node_id_in(&objects, "broadcast_filter_sink"), Some(43));
    }

    // ── parse_sink_inputs ──────────────────────────────────────────────

    #[test]
    fn test_parse_sink_inputs_basic() {
        let items = vec![
            json!({
                "index": 100,
                "sink": 5,
                "properties": {
                    "application.name": "Brave Browser",
                    "application.process.binary": "brave",
                    "media.name": "Playback",
                    "node.name": "Brave Browser"
                }
            }),
            json!({
                "index": 101,
                "sink": 8,
                "properties": {
                    "application.name": "Spotify",
                    "application.process.binary": "spotify",
                    "media.name": "Music",
                    "node.name": "Spotify"
                }
            }),
        ];
        let inputs = parse_sink_inputs(&items);
        assert_eq!(inputs.len(), 2);

        assert_eq!(inputs[0].id, 100);
        assert_eq!(inputs[0].sink_name, "5");
        assert_eq!(inputs[0].client_name, "Brave Browser");
        assert_eq!(inputs[0].app_binary, "brave");
        assert_eq!(inputs[0].media_name, "Playback");
        assert_eq!(inputs[0].node_name, "Brave Browser");

        assert_eq!(inputs[1].id, 101);
        assert_eq!(inputs[1].sink_name, "8");
        assert_eq!(inputs[1].app_binary, "spotify");
        assert_eq!(inputs[1].node_name, "Spotify");
    }

    #[test]
    fn test_parse_sink_inputs_empty() {
        let inputs = parse_sink_inputs(&[]);
        assert!(inputs.is_empty());
    }

    #[test]
    fn test_parse_sink_inputs_missing_props() {
        let items = vec![json!({
            "index": 200,
            "sink": 3
            // no "properties" key at all
        })];
        let inputs = parse_sink_inputs(&items);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, 200);
        assert_eq!(inputs[0].client_name, "");
        assert_eq!(inputs[0].app_binary, "");
    }

    // ── parse_default_sink ─────────────────────────────────────────────

    #[test]
    fn test_parse_default_sink() {
        let text = r#"
  id 48, type PipeWire:Interface:Node
    media.class = "Audio/Sink"
    node.name = "alsa_output.pci-0000_00_1f.3.analog-stereo"
    node.nick = "ALC295 Analog"
"#;
        assert_eq!(
            parse_default_sink(text),
            "alsa_output.pci-0000_00_1f.3.analog-stereo"
        );
    }

    #[test]
    fn test_parse_default_sink_missing() {
        let text = "id 48, type PipeWire:Interface:Node\n  media.class = Audio/Sink\n";
        assert_eq!(parse_default_sink(text), "default");
    }

    // ── find_sink_index_in ─────────────────────────────────────────────

    #[test]
    fn test_find_sink_index_in_found() {
        let sinks = vec![
            json!({
                "index": 5,
                "properties": {
                    "node.name": "alsa_output.pci-0000_00_1f.3.analog-stereo",
                    "media.class": "Audio/Sink"
                }
            }),
            json!({
                "index": 8,
                "properties": {
                    "node.name": "broadcast_filter_sink",
                    "media.class": "Audio/Sink"
                }
            }),
        ];
        assert_eq!(find_sink_index_in(&sinks, "broadcast_filter_sink"), Some(8));
    }

    #[test]
    fn test_find_sink_index_in_not_found() {
        let sinks = vec![json!({
            "index": 5,
            "properties": {
                "node.name": "alsa_output.pci-0000_00_1f.3.analog-stereo",
                "media.class": "Audio/Sink"
            }
        })];
        assert_eq!(find_sink_index_in(&sinks, "nonexistent_sink"), None);
    }

    // ── parse_sinks_as_devices ─────────────────────────────────────────

    #[test]
    fn test_parse_sinks_as_devices_filters_correctly() {
        let sinks = vec![
            json!({
                "index": 42,
                "properties": {
                    "node.name": "broadcast_filter_sink",
                    "node.description": "Broadcast Filter",
                    "media.class": "Audio/Sink"
                }
            }),
            json!({
                "index": 71,
                "properties": {
                    "node.name": "alsa_output.pci-0000_0a_00.1.hdmi-stereo",
                    "node.description": "GPU HDMI",
                    "media.class": "Audio/Sink"
                }
            }),
            json!({
                "index": 75,
                "properties": {
                    "node.name": "alsa_output.pci-0000_0c_00.4.analog-stereo",
                    "node.description": "Motherboard Audio",
                    "media.class": "Audio/Sink"
                }
            }),
            json!({
                "index": 12,
                "properties": {
                    "node.name": "virtual_sink",
                    "node.description": "Virtual Sink",
                    "media.class": "Audio/Sink/Virtual"
                }
            }),
        ];
        let devices = parse_sinks_as_devices(&sinks, "broadcast_filter_sink");
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "alsa_output.pci-0000_0a_00.1.hdmi-stereo");
        assert_eq!(devices[0].description, "GPU HDMI");
        assert_eq!(
            devices[1].name,
            "alsa_output.pci-0000_0c_00.4.analog-stereo"
        );
        assert_eq!(devices[1].description, "Motherboard Audio");
    }

    #[test]
    fn test_parse_sinks_as_devices_empty() {
        let devices = parse_sinks_as_devices(&[], "broadcast_filter_sink");
        assert!(devices.is_empty());
    }

    #[test]
    fn test_parse_sinks_description_fallback_chain() {
        let sinks = vec![
            // Has node.description → use it
            json!({
                "properties": {
                    "node.name": "sink_a",
                    "node.description": "Friendly A",
                    "node.nick": "Nick A",
                    "media.class": "Audio/Sink"
                },
                "description": "Top-level A"
            }),
            // Missing node.description → fall back to top-level description
            json!({
                "properties": {
                    "node.name": "sink_b",
                    "node.nick": "Nick B",
                    "media.class": "Audio/Sink"
                },
                "description": "Top-level B"
            }),
            // Missing both → fall back to node.nick
            json!({
                "properties": {
                    "node.name": "sink_c",
                    "node.nick": "Nick C",
                    "media.class": "Audio/Sink"
                }
            }),
            // Only node.name → use it as last resort
            json!({
                "properties": {
                    "node.name": "sink_d",
                    "media.class": "Audio/Sink"
                }
            }),
        ];
        let devices = parse_sinks_as_devices(&sinks, "");
        assert_eq!(devices.len(), 4);
        assert_eq!(devices[0].description, "Friendly A");
        assert_eq!(devices[1].description, "Top-level B");
        assert_eq!(devices[2].description, "Nick C");
        assert_eq!(devices[3].description, "sink_d");
    }

    // ── parse_sources_as_devices ───────────────────────────────────────

    #[test]
    fn test_parse_sources_as_devices_filters_correctly() {
        let sources = vec![
            json!({
                "index": 41,
                "properties": {
                    "node.name": "deepfilter_mic",
                    "node.description": "Clean Mic (DeepFilter)",
                    "media.class": "Audio/Source"
                }
            }),
            json!({
                "index": 71,
                "properties": {
                    "node.name": "alsa_output.pci-0000_0a_00.1.hdmi-stereo",
                    "node.description": "Monitor of GPU HDMI",
                    "media.class": "Audio/Sink"
                }
            }),
            json!({
                "index": 76,
                "properties": {
                    "node.name": "alsa_input.pci-0000_0c_00.4.analog-stereo",
                    "node.description": "Motherboard Mic",
                    "media.class": "Audio/Source"
                }
            }),
        ];
        let devices = parse_sources_as_devices(&sources);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "alsa_input.pci-0000_0c_00.4.analog-stereo");
        assert_eq!(devices[0].description, "Motherboard Mic");
    }

    #[test]
    fn test_parse_sources_as_devices_empty() {
        let devices = parse_sources_as_devices(&[]);
        assert!(devices.is_empty());
    }
}
