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
    pub master: bool,
    pub input_filter: bool,
    pub output_filter: bool,
    pub default_route: AppRoute,
    #[serde(default)]
    pub app_routes: HashMap<String, AppRoute>,
    #[serde(default)]
    pub nodes: NodeNames,
}

impl Default for BroadcastState {
    fn default() -> Self {
        Self {
            master: true,
            input_filter: true,
            output_filter: true,
            default_route: AppRoute::Direct,
            app_routes: HashMap::new(),
            nodes: NodeNames::default(),
        }
    }
}

impl BroadcastState {
    fn state_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(STATE_DIR).join(STATE_FILE)
    }

    pub fn load() -> Result<Self> {
        let path = Self::state_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path).context("Failed to read state file")?;
        let state: Self = serde_json::from_str(&data).context("Failed to parse state file")?;
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create state directory")?;
        }
        let data = serde_json::to_string_pretty(self)?;
        fs::write(&path, data).context("Failed to write state file")?;
        Ok(())
    }

    /// Get the route for a specific app, falling back to default.
    pub fn route_for(&self, app_binary: &str) -> AppRoute {
        let key = app_binary.to_lowercase();
        self.app_routes.get(&key).copied().unwrap_or(self.default_route)
    }

    /// Set the route for a specific app.
    pub fn set_app_route(&mut self, app_binary: &str, route: AppRoute) {
        self.app_routes.insert(app_binary.to_lowercase(), route);
    }
}
