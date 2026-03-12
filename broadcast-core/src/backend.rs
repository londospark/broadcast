use anyhow::{Context, Result};
use std::process::Command;

use crate::pipewire::{self, SinkInput};

/// Abstraction over PipeWire/PulseAudio system commands.
/// Real implementation calls pw-dump, pactl, pw-cli, wpctl.
/// Test implementation returns canned data.
pub trait PipeWireBackend {
    /// Get all PipeWire objects as JSON (pw-dump).
    fn pw_dump(&self) -> Result<Vec<serde_json::Value>>;
    /// List all sink inputs (pactl list sink-inputs).
    fn list_sink_inputs(&self) -> Result<Vec<SinkInput>>;
    /// Get the PulseAudio sink index for a node name.
    fn get_sink_index(&self, node_name: &str) -> Result<Option<u32>>;
    /// Move a sink-input to a different sink.
    fn move_sink_input(&self, input_id: u32, sink_id: u32) -> Result<()>;
    /// Set a PipeWire node parameter (pw-cli set-param).
    fn set_param(&self, node_id: u64, param_type: &str, param_value: &str) -> Result<()>;
    /// Get the default audio sink name.
    fn get_default_sink(&self) -> Result<String>;
    /// List all sinks as JSON (pactl list sinks).
    fn list_sinks(&self) -> Result<Vec<serde_json::Value>>;
    /// List all sources as JSON (pactl list sources).
    fn list_sources(&self) -> Result<Vec<serde_json::Value>>;
}

/// Real implementation that shells out to pw-dump, pactl, pw-cli, wpctl.
pub struct RealBackend;

impl PipeWireBackend for RealBackend {
    fn pw_dump(&self) -> Result<Vec<serde_json::Value>> {
        let output = Command::new("pw-dump")
            .output()
            .context("Failed to run pw-dump")?;
        let objects: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).context("Failed to parse pw-dump JSON")?;
        Ok(objects)
    }

    fn list_sink_inputs(&self) -> Result<Vec<SinkInput>> {
        let output = Command::new("pactl")
            .args(["--format=json", "list", "sink-inputs"])
            .output()
            .context("Failed to run pactl list sink-inputs")?;

        if !output.status.success() {
            anyhow::bail!("pactl list sink-inputs failed");
        }

        let items: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).unwrap_or_default();

        Ok(pipewire::parse_sink_inputs(&items))
    }

    fn get_sink_index(&self, node_name: &str) -> Result<Option<u32>> {
        let sinks = self.list_sinks()?;
        Ok(pipewire::find_sink_index_in(&sinks, node_name))
    }

    fn move_sink_input(&self, input_id: u32, sink_id: u32) -> Result<()> {
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

    fn set_param(&self, node_id: u64, param_type: &str, param_value: &str) -> Result<()> {
        Command::new("pw-cli")
            .args(["set-param", &node_id.to_string(), param_type, param_value])
            .output()
            .context("Failed to run pw-cli set-param")?;
        Ok(())
    }

    fn get_default_sink(&self) -> Result<String> {
        let output = Command::new("wpctl")
            .args(["inspect", "@DEFAULT_AUDIO_SINK@"])
            .output()
            .context("Failed to run wpctl inspect")?;
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(pipewire::parse_default_sink(&text))
    }

    fn list_sinks(&self) -> Result<Vec<serde_json::Value>> {
        let output = Command::new("pactl")
            .args(["--format=json", "list", "sinks"])
            .output()
            .context("Failed to run pactl list sinks")?;
        let sinks: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).unwrap_or_default();
        Ok(sinks)
    }

    fn list_sources(&self) -> Result<Vec<serde_json::Value>> {
        let output = Command::new("pactl")
            .args(["--format=json", "list", "sources"])
            .output()
            .context("Failed to run pactl list sources")?;
        let sources: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).unwrap_or_default();
        Ok(sources)
    }
}
