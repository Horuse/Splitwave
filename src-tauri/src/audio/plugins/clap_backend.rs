use std::path::{Path, PathBuf};

use clack_host::prelude::PluginEntry;

use super::{PluginBackend, PluginDescriptor, PluginFormat};

/// `CLAP_PLUGIN_FEATURE_AUDIO_EFFECT`: the plugin processes audio it is given,
/// as opposed to generating it from notes.
const AUDIO_EFFECT: &std::ffi::CStr = c"audio-effect";

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
            // Splitwave routes audio and sends no notes, so an instrument would
            // sit silent in the graph. Mirrors the VST3 scan's `Fx` filter and
            // the AU scan, which takes only `aufx` and `aumf`.
            if !desc.features().any(|f| f == AUDIO_EFFECT) {
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The CLAP scan must survive whatever is installed and describe every
    /// plugin well enough to re-resolve it later. Loading a foreign dylib is
    /// the risky half; a bundle that misbehaves is skipped, never fatal.
    #[test]
    fn lists_installed_clap_plugins() {
        let found = ClapBackend.scan();
        if found.is_empty() {
            println!("SKIPPED: no clap plugins installed, scan asserted nothing");
            return;
        }
        for plugin in &found {
            assert_eq!(plugin.format, PluginFormat::Clap);
            assert!(!plugin.plugin_id.is_empty(), "{plugin:?} has no id");
            assert!(!plugin.name.is_empty(), "{plugin:?} has no name");
            assert!(plugin.uid.starts_with("clap:"), "{plugin:?} has a foreign uid");
            assert!(plugin.path.ends_with(".clap"), "{plugin:?} is not a bundle");
        }
        println!("found {} clap plugins", found.len());
        for plugin in &found {
            println!("  {} by {}", plugin.name, plugin.vendor);
        }
    }

    /// A rescan must describe the same plugins the same way, since a saved
    /// project stores the uid and re-resolves it on load.
    #[test]
    fn a_rescan_is_stable() {
        let first = ClapBackend.scan();
        if first.is_empty() {
            println!("SKIPPED: no clap plugins installed, cannot check rescan stability");
            return;
        }
        let again = ClapBackend.scan();
        let uids = |v: &[PluginDescriptor]| {
            let mut ids: Vec<String> = v.iter().map(|p| p.uid.clone()).collect();
            ids.sort();
            ids
        };
        assert_eq!(uids(&first), uids(&again));
    }
}
