//! Third-party audio plugin hosting. Each format backend (CLAP now; AU / VST3
//! later) implements `PluginBackend`, so the node graph and the UI reference
//! plugins through one format-agnostic descriptor.

#[cfg(target_os = "macos")]
mod au_backend;
#[cfg(target_os = "macos")]
pub mod au_host;
mod clap_backend;
#[cfg(target_os = "macos")]
pub mod vst3_backend;
pub mod host;
pub mod host_api;
mod node;
pub mod registry;
pub mod param_ring;
pub mod scan;

#[cfg(target_os = "macos")]
pub use au_host::AuNode;
pub use host_api::{HostedNode, PluginParamInfo, PluginStatus};
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
    #[cfg(target_os = "macos")]
    Au,
    #[cfg(target_os = "macos")]
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
