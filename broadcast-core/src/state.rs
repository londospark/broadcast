use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const STATE_DIR: &str = ".local/state/broadcast";
const STATE_FILE: &str = "config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppRoute {
    Filtered,
    Direct,
}

impl std::fmt::Display for AppRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AppRoute::Filtered => write!(f, "filtered"),
            AppRoute::Direct => write!(f, "direct"),
        }
    }
}

impl std::str::FromStr for AppRoute {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "filtered" | "filter" | "on" => Ok(AppRoute::Filtered),
            "direct" | "off" => Ok(AppRoute::Direct),
            _ => anyhow::bail!("Invalid route: {s}. Use 'filtered' or 'direct'"),
        }
    }
}

/// PipeWire node names used by the filter chains.
/// These match the names in the PipeWire filter chain config files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeNames {
    /// The capture node of the input (mic) filter chain
    pub input_capture: String,
    /// The capture node of the output (speaker) filter chain (the virtual sink)
    pub output_sink: String,
}

impl Default for NodeNames {
    fn default() -> Self {
        Self {
            input_capture: "capture.deepfilter_mic".into(),
            output_sink: "broadcast_filter_sink".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastState {
    /// Global enable/disable for all filtering.
    /// Alias "master" for backward-compat with older state files.
    #[serde(alias = "master")]
    pub active: bool,
    pub default_route: AppRoute,
    #[serde(default)]
    pub app_routes: HashMap<String, AppRoute>,
    #[serde(default)]
    pub nodes: NodeNames,
    /// Preferred output sink node.name (None = auto-detect first hardware sink)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_output_sink: Option<String>,
    /// Preferred input source node.name (None = auto-detect)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_input_source: Option<String>,
}

impl Default for BroadcastState {
    fn default() -> Self {
        Self {
            active: true,
            default_route: AppRoute::Direct,
            app_routes: HashMap::new(),
            nodes: NodeNames::default(),
            preferred_output_sink: None,
            preferred_input_source: None,
        }
    }
}

impl BroadcastState {
    fn state_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(STATE_DIR).join(STATE_FILE)
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::state_path())
    }

    pub fn load_from(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(path).context("Failed to read state file")?;
        let state: Self = serde_json::from_str(&data).context("Failed to parse state file")?;
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::state_path())
    }

    pub fn save_to(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create state directory")?;
        }
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data).context("Failed to write state file")?;
        Ok(())
    }

    /// Get the route for a specific app, falling back to default.
    pub fn route_for(&self, app_binary: &str) -> AppRoute {
        let key = app_binary.to_lowercase();
        self.app_routes
            .get(&key)
            .copied()
            .unwrap_or(self.default_route)
    }

    /// Set the route for a specific app.
    pub fn set_app_route(&mut self, app_binary: &str, route: AppRoute) {
        self.app_routes.insert(app_binary.to_lowercase(), route);
    }

    /// Set the preferred output sink (None to auto-detect).
    pub fn set_preferred_output_sink(&mut self, sink_name: Option<String>) {
        self.preferred_output_sink = sink_name;
    }

    /// Set the preferred input source (None to auto-detect).
    pub fn set_preferred_input_source(&mut self, source_name: Option<String>) {
        self.preferred_input_source = source_name;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AppRoute::FromStr ──────────────────────────────────────────────

    #[test]
    fn test_app_route_from_str_filtered() {
        for s in &["filtered", "filter", "on"] {
            assert_eq!(s.parse::<AppRoute>().unwrap(), AppRoute::Filtered);
        }
    }

    #[test]
    fn test_app_route_from_str_direct() {
        for s in &["direct", "off"] {
            assert_eq!(s.parse::<AppRoute>().unwrap(), AppRoute::Direct);
        }
    }

    #[test]
    fn test_app_route_from_str_case_insensitive() {
        assert_eq!("FILTERED".parse::<AppRoute>().unwrap(), AppRoute::Filtered);
        assert_eq!("Direct".parse::<AppRoute>().unwrap(), AppRoute::Direct);
        assert_eq!("ON".parse::<AppRoute>().unwrap(), AppRoute::Filtered);
        assert_eq!("OFF".parse::<AppRoute>().unwrap(), AppRoute::Direct);
    }

    #[test]
    fn test_app_route_from_str_invalid() {
        assert!("invalid".parse::<AppRoute>().is_err());
        assert!("".parse::<AppRoute>().is_err());
        assert!("yes".parse::<AppRoute>().is_err());
    }

    // ── AppRoute::Display ──────────────────────────────────────────────

    #[test]
    fn test_app_route_display() {
        assert_eq!(AppRoute::Filtered.to_string(), "filtered");
        assert_eq!(AppRoute::Direct.to_string(), "direct");
    }

    // ── Defaults ───────────────────────────────────────────────────────

    #[test]
    fn test_default_state() {
        let s = BroadcastState::default();
        assert!(s.active);
        assert_eq!(s.default_route, AppRoute::Direct);
        assert!(s.app_routes.is_empty());
    }

    #[test]
    fn test_node_names_default() {
        let n = NodeNames::default();
        assert_eq!(n.input_capture, "capture.deepfilter_mic");
        assert_eq!(n.output_sink, "broadcast_filter_sink");
    }

    // ── route_for / set_app_route ──────────────────────────────────────

    #[test]
    fn test_route_for_known_app() {
        let mut s = BroadcastState::default();
        s.set_app_route("brave", AppRoute::Filtered);
        assert_eq!(s.route_for("brave"), AppRoute::Filtered);
    }

    #[test]
    fn test_route_for_unknown_app() {
        let s = BroadcastState::default(); // default_route = Direct
        assert_eq!(s.route_for("unknown_app"), AppRoute::Direct);
    }

    #[test]
    fn test_route_for_case_insensitive() {
        let mut s = BroadcastState::default();
        s.set_app_route("brave", AppRoute::Filtered);
        // Lookup with different case should still match (key lowered on set and get)
        assert_eq!(s.route_for("Brave"), AppRoute::Filtered);
        assert_eq!(s.route_for("BRAVE"), AppRoute::Filtered);
    }

    #[test]
    fn test_set_app_route() {
        let mut s = BroadcastState::default();
        s.set_app_route("spotify", AppRoute::Direct);
        assert_eq!(s.route_for("spotify"), AppRoute::Direct);
        // Overwrite
        s.set_app_route("spotify", AppRoute::Filtered);
        assert_eq!(s.route_for("spotify"), AppRoute::Filtered);
    }

    // ── Serde ──────────────────────────────────────────────────────────

    #[test]
    fn test_serde_roundtrip() {
        let mut s = BroadcastState {
            active: false,
            ..Default::default()
        };
        s.set_app_route("brave", AppRoute::Filtered);

        let json = serde_json::to_string(&s).unwrap();
        let s2: BroadcastState = serde_json::from_str(&json).unwrap();

        assert!(!s2.active);
        assert_eq!(s2.route_for("brave"), AppRoute::Filtered);
        assert_eq!(s2.nodes.input_capture, s.nodes.input_capture);
    }

    #[test]
    fn test_serde_roundtrip_with_preferred_devices() {
        let mut s = BroadcastState::default();
        s.set_preferred_output_sink(Some("alsa_output.pci-0000_0c_00.4.analog-stereo".into()));
        s.set_preferred_input_source(Some("alsa_input.pci-0000_0c_00.4.analog-stereo".into()));

        let json = serde_json::to_string(&s).unwrap();
        let s2: BroadcastState = serde_json::from_str(&json).unwrap();

        assert_eq!(
            s2.preferred_output_sink.as_deref(),
            Some("alsa_output.pci-0000_0c_00.4.analog-stereo")
        );
        assert_eq!(
            s2.preferred_input_source.as_deref(),
            Some("alsa_input.pci-0000_0c_00.4.analog-stereo")
        );
    }

    #[test]
    fn test_serde_missing_optional_fields() {
        // Minimal JSON without nodes, app_routes, or preferred devices — serde defaults should kick in
        let json = r#"{"active":true,"default_route":"filtered"}"#;
        let s: BroadcastState = serde_json::from_str(json).unwrap();
        assert!(s.app_routes.is_empty());
        assert_eq!(s.nodes.output_sink, "broadcast_filter_sink");
        assert_eq!(s.default_route, AppRoute::Filtered);
        assert!(s.preferred_output_sink.is_none());
        assert!(s.preferred_input_source.is_none());
    }

    #[test]
    fn test_serde_backward_compat_old_state_file() {
        // Old state files used "master", "input_filter", "output_filter"
        let json = r#"{"master":false,"input_filter":true,"output_filter":false,"default_route":"direct"}"#;
        let s: BroadcastState = serde_json::from_str(json).unwrap();
        assert!(!s.active); // "master" alias maps to "active"
        assert_eq!(s.default_route, AppRoute::Direct);
    }

    #[test]
    fn test_preferred_devices_not_serialized_when_none() {
        let s = BroadcastState::default();
        let json = serde_json::to_string_pretty(&s).unwrap();
        assert!(!json.contains("preferred_output_sink"));
        assert!(!json.contains("preferred_input_source"));
    }

    // ── File I/O (load / save) ─────────────────────────────────────────

    #[test]
    fn test_load_missing_file() {
        let path =
            std::env::temp_dir().join(format!("broadcast_test_{}/config.json", std::process::id()));
        let state = BroadcastState::load_from(&path).unwrap();
        assert_eq!(state.default_route, AppRoute::Direct);
    }

    #[test]
    fn test_save_and_load() {
        let dir = std::env::temp_dir().join(format!("broadcast_save_{}", std::process::id()));
        let path = dir.join("config.json");

        let mut s = BroadcastState {
            active: false,
            ..Default::default()
        };
        s.set_app_route("brave", AppRoute::Filtered);
        s.save_to(&path).unwrap();

        let loaded = BroadcastState::load_from(&path).unwrap();
        assert!(!loaded.active);
        assert_eq!(loaded.route_for("brave"), AppRoute::Filtered);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_corrupt_json() {
        let dir = std::env::temp_dir().join(format!("broadcast_corrupt_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, "NOT JSON {{{{").unwrap();

        assert!(BroadcastState::load_from(&path).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }
}
