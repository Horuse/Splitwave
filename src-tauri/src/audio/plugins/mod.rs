//! Third-party audio plugin hosting. Each format backend (CLAP now; AU / VST3
//! later) implements `PluginBackend`, so the node graph and the UI reference
//! plugins through one format-agnostic descriptor.

mod clap_backend;
pub mod host;
mod node;
pub mod param_ring;
pub mod scan;

pub use node::PluginNode;
pub use param_ring::ParamRing;

use std::path::{Path, PathBuf};

use serde::Serialize;

pub use scan::scan_all;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginFormat {
    Clap,
}

/// One instantiable plugin found by a scan. `uid` is stable across scans so the
/// graph can store a reference and re-resolve the plugin when the project loads.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptor {
    pub uid: String,
    pub format: PluginFormat,
    pub path: String,
    // A single bundle may expose several plugins, keyed by this format-native id.
    pub plugin_id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
}

/// A pluggable format backend. One deterministic scan path per format; a bundle
/// that fails to load is skipped and logged, never aborts the whole scan.
pub trait PluginBackend {
    #[allow(dead_code)]
    fn format(&self) -> PluginFormat;
    /// Standard install directories to search for this format.
    fn search_dirs(&self) -> Vec<PathBuf>;
    /// Bundle / library extension that marks a plugin, without the leading dot.
    fn extension(&self) -> &'static str;
    /// Enumerate the plugins inside one bundle / library path.
    fn scan_bundle(&self, path: &Path) -> Vec<PluginDescriptor>;
}
