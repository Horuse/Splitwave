//! Third-party audio plugin hosting. Each format backend (CLAP and VST3
//! everywhere, AU on macOS) implements `PluginBackend`, so the node graph and
//! the UI reference plugins through one format-agnostic descriptor.

#[cfg(target_os = "macos")]
mod au_backend;
#[cfg(target_os = "macos")]
pub mod au_host;
mod clap_backend;
pub mod clap_host;
pub mod clap_registry;
pub mod editor;
pub mod host_api;
pub mod main_thread;
mod node;
pub mod param_ring;
pub mod registry;
pub mod scan;
pub mod vst3_backend;
pub mod vst3_com;
pub mod vst3_editor;
pub mod vst3_host;
pub mod vst3_node;
pub mod vst3_registry;
#[cfg(target_os = "linux")]
pub mod vst3_runloop;

#[cfg(target_os = "macos")]
pub use au_host::AuNode;
pub use host_api::{PluginParamInfo, PluginStatus};
pub use node::PluginNode;
pub use param_ring::ParamRing;

use std::path::{Path, PathBuf};

use serde::Serialize;

pub use scan::scan_all;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum PluginFormat {
    Clap,
    Au,
    Vst3,
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
    fn scan_bundle(&self, _path: &Path) -> Vec<PluginDescriptor> {
        Vec::new()
    }
    /// Full discovery for this format. The default walks `search_dirs` for
    /// `extension` bundles; a format whose registry is not the filesystem
    /// (Audio Units) overrides it.
    fn scan(&self) -> Vec<PluginDescriptor> {
        let mut out = Vec::new();
        for dir in self.search_dirs() {
            scan::walk(&dir, self.extension(), &mut |bundle| {
                out.extend(self.scan_bundle(bundle))
            });
        }
        out
    }
}
