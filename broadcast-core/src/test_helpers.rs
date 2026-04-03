use std::cell::RefCell;
use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use crate::backend::PipeWireBackend;
use crate::pipewire::SinkInput;

/// Records all calls made to the backend for assertion.
#[derive(Debug, Default)]
pub struct MockBackend {
    pub pw_dump_result: RefCell<Vec<Value>>,
    pub sink_inputs: RefCell<Vec<SinkInput>>,
    pub sink_indices: RefCell<HashMap<String, u32>>,
    pub sinks: RefCell<Vec<Value>>,
    pub sources: RefCell<Vec<Value>>,
    pub default_sink_name: RefCell<String>,
    pub default_source_name: RefCell<String>,
    /// Tracks (input_id, sink_id) for each move_sink_input call.
    pub moved_inputs: RefCell<Vec<(u32, u32)>>,
    /// Tracks (node_id, param_type, param_value) for each set_param call.
    pub set_params: RefCell<Vec<(u64, String, String)>>,
    /// Tracks input_ids that were unmuted.
    pub unmuted_inputs: RefCell<Vec<u32>>,
    /// Tracks source names set as default.
    pub set_default_sources: RefCell<Vec<String>>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PipeWireBackend for MockBackend {
    fn pw_dump(&self) -> Result<Vec<Value>> {
        Ok(self.pw_dump_result.borrow().clone())
    }

    fn list_sink_inputs(&self) -> Result<Vec<SinkInput>> {
        Ok(self.sink_inputs.borrow().clone())
    }

    fn get_sink_index(&self, node_name: &str) -> Result<Option<u32>> {
        Ok(self.sink_indices.borrow().get(node_name).copied())
    }

    fn move_sink_input(&self, input_id: u32, sink_id: u32) -> Result<()> {
        self.moved_inputs.borrow_mut().push((input_id, sink_id));
        Ok(())
    }

    fn set_param(&self, node_id: u64, param_type: &str, param_value: &str) -> Result<()> {
        self.set_params.borrow_mut().push((
            node_id,
            param_type.to_string(),
            param_value.to_string(),
        ));
        Ok(())
    }

    fn get_default_sink(&self) -> Result<String> {
        Ok(self.default_sink_name.borrow().clone())
    }

    fn get_default_source(&self) -> Result<String> {
        Ok(self.default_source_name.borrow().clone())
    }

    fn set_default_source(&self, source_name: &str) -> Result<()> {
        self.set_default_sources
            .borrow_mut()
            .push(source_name.to_string());
        *self.default_source_name.borrow_mut() = source_name.to_string();
        Ok(())
    }

    fn list_sinks(&self) -> Result<Vec<Value>> {
        Ok(self.sinks.borrow().clone())
    }

    fn list_sources(&self) -> Result<Vec<Value>> {
        Ok(self.sources.borrow().clone())
    }

    fn ensure_sink_input_unmuted(&self, input_id: u32) -> Result<()> {
        self.unmuted_inputs.borrow_mut().push(input_id);
        Ok(())
    }
}
