//! IPC protocol for Out-of-Process Plugin Bridge on Windows.

#![cfg(target_os = "windows")]

use crate::audio::plugins::host_api::PluginParamInfo;
use serde::{Deserialize, Serialize};

/// Commands sent from Splitwave Host to the Plugin Bridge Helper process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HostCommand {
    /// Initialize the plugin instance inside the helper process.
    Init { path: String, plugin_id: String },
    /// Activate audio processing.
    Activate {
        sample_rate: u32,
        max_frames: usize,
        channels: usize,
        state: Option<String>,
    },
    /// Open the native editor window.
    OpenEditor { title: String },
    /// Close / hide the native editor window.
    CloseEditor,
    /// Set a parameter value from the host.
    SetParam { id: u32, value: f64 },
    /// Request current parameter list.
    GetParams,
    /// Request saved state blob.
    SaveState,
    /// Restore plugin state from blob.
    RestoreState { blob: String },
    /// Gracefully shutdown the helper process.
    Shutdown,
}

/// Events / Responses sent from the Helper process to Splitwave Host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HelperEvent {
    /// Plugin loaded and initialized successfully.
    Loaded {
        params: Vec<PluginParamInfo>,
        has_editor: bool,
    },
    /// Plugin activated and ready for audio processing.
    Activated {
        accepted_channels: usize,
        latency_frames: usize,
    },
    /// Parameter value was edited inside the plugin's own window.
    ParamEdited { id: u32, value: f64 },
    /// Editor window was opened with given dimensions.
    EditorOpened { width: u32, height: u32 },
    /// Editor window was closed by the user (e.g. WM_CLOSE / X button).
    EditorClosed,
    /// Serialized state blob response.
    StateSaved { blob: Option<String> },
    /// Current parameters list response.
    ParamsList { params: Vec<PluginParamInfo> },
    /// Operation succeeded.
    Ok,
    /// An error occurred in the helper process.
    Error { message: String },
}
