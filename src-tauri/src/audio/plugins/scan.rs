use std::path::Path;

#[cfg(target_os = "macos")]
use super::au_backend::AuBackend;
use super::clap_backend::ClapBackend;
use super::{PluginBackend, PluginDescriptor};

/// Scan every registered format and return all plugins found.
pub fn scan_all() -> Vec<PluginDescriptor> {
    let backends: Vec<Box<dyn PluginBackend>> = vec![
        Box::new(ClapBackend),
        #[cfg(target_os = "macos")]
        Box::new(AuBackend),
    ];
    backends.iter().flat_map(|b| b.scan()).collect()
}

/// Recurse `dir`, invoking `on_bundle` for every entry whose extension matches
/// `ext`. A matched bundle is not descended into (on macOS a `.clap` is itself
/// a directory).
pub(super) fn walk(dir: &Path, ext: &str, on_bundle: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            on_bundle(&path);
        } else if path.is_dir() {
            walk(&path, ext, on_bundle);
        }
    }
}
