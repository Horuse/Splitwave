use std::path::{Path, PathBuf};

use clack_host::prelude::PluginEntry;

use super::{PluginBackend, PluginDescriptor, PluginFormat};

pub struct ClapBackend;

impl PluginBackend for ClapBackend {
    fn format(&self) -> PluginFormat {
        PluginFormat::Clap
    }

    fn extension(&self) -> &'static str {
        "clap"
    }

    fn search_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(paths) = std::env::var_os("CLAP_PATH") {
            dirs.extend(std::env::split_paths(&paths));
        }
        #[cfg(target_os = "macos")]
        {
            dirs.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
            if let Some(home) = std::env::var_os("HOME") {
                dirs.push(Path::new(&home).join("Library/Audio/Plug-Ins/CLAP"));
            }
        }
        dirs
    }

    fn scan_bundle(&self, path: &Path) -> Vec<PluginDescriptor> {
        // Loading a foreign dylib is inherently unsafe: a non-compliant bundle
        // can trigger any behavior just on load. Accepted per format design.
        let entry = match unsafe { PluginEntry::load(path) } {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!("clap: failed to load {}: {err}", path.display());
                return Vec::new();
            }
        };
        let Some(factory) = entry.get_plugin_factory() else {
            return Vec::new();
        };
        let path_str = path.to_string_lossy().into_owned();
        let mut out = Vec::new();
        for desc in factory.plugin_descriptors() {
            let Some(id) = desc.id().and_then(|c| c.to_str().ok()) else {
                continue;
            };
            let name = desc
                .name()
                .and_then(|c| c.to_str().ok())
                .unwrap_or(id)
                .to_string();
            let vendor = desc
                .vendor()
                .and_then(|c| c.to_str().ok())
                .unwrap_or("")
                .to_string();
            let version = desc
                .version()
                .and_then(|c| c.to_str().ok())
                .unwrap_or("")
                .to_string();
            out.push(PluginDescriptor {
                uid: format!("clap:{path_str}:{id}"),
                format: PluginFormat::Clap,
                path: path_str.clone(),
                plugin_id: id.to_string(),
                name,
                vendor,
                version,
            });
        }
        out
    }
}
