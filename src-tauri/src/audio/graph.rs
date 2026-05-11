use serde::Deserialize;

use crate::error::{AppError, AppResult};

/// Graph payload received from the frontend.
#[derive(Debug, Deserialize)]
pub struct GraphSpec {
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NodeSpec {
    Input { id: String, data: DeviceNodeData },
    Output { id: String, data: DeviceNodeData },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceNodeData {
    pub device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EdgeSpec {
    #[allow(dead_code)]
    pub id: String,
    pub source: String,
    pub target: String,
}

/// Result of `GraphSpec::validate` — all the info the engine needs to start.
#[derive(Debug)]
pub struct ValidGraph {
    pub input_device_id: String,
    pub output_device_id: String,
}

impl GraphSpec {
    /// Demo validation: exactly one Input and one Output, both with a device
    /// selected, connected by an Input → Output edge.
    pub fn validate(&self) -> AppResult<ValidGraph> {
        let mut input: Option<(&str, &DeviceNodeData)> = None;
        let mut output: Option<(&str, &DeviceNodeData)> = None;

        for node in &self.nodes {
            match node {
                NodeSpec::Input { id, data } => {
                    if input.is_some() {
                        return Err(AppError::Validation("multiple Input nodes".into()));
                    }
                    input = Some((id.as_str(), data));
                }
                NodeSpec::Output { id, data } => {
                    if output.is_some() {
                        return Err(AppError::Validation("multiple Output nodes".into()));
                    }
                    output = Some((id.as_str(), data));
                }
            }
        }

        let (input_id, input_data) =
            input.ok_or_else(|| AppError::Validation("missing Input node".into()))?;
        let (output_id, output_data) =
            output.ok_or_else(|| AppError::Validation("missing Output node".into()))?;

        let input_device = input_data
            .device_id
            .as_deref()
            .ok_or_else(|| AppError::Validation("Input has no device selected".into()))?;
        let output_device = output_data
            .device_id
            .as_deref()
            .ok_or_else(|| AppError::Validation("Output has no device selected".into()))?;

        let connected = self
            .edges
            .iter()
            .any(|e| e.source == input_id && e.target == output_id);
        if !connected {
            return Err(AppError::Validation(
                "Input is not connected to Output".into(),
            ));
        }

        Ok(ValidGraph {
            input_device_id: input_device.to_string(),
            output_device_id: output_device.to_string(),
        })
    }
}
