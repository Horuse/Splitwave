use std::path::Path;

use super::clap_backend::ClapBackend;
use super::{PluginBackend, PluginDescriptor};

/// Scan every registered format's install directories and return all plugins.
pub fn scan_all() -> Vec<PluginDescriptor> {
    let backends: [Box<dyn PluginBackend>; 1] = [Box::new(ClapBackend)];
    let mut out = Vec::new();
    for backend in backends {
        let ext = backend.extension();
        for dir in backend.search_dirs() {
            walk(&dir, ext, &mut |bundle| out.extend(backend.scan_bundle(bundle)));
        }
    }
    out
}

/// Recurse `dir`, invoking `on_bundle` for every entry whose extension matches
/// `ext`. A matched bundle is not descended into (on macOS a `.clap` is itself
/// a directory).
fn walk(dir: &Path, ext: &str, on_bundle: &mut impl FnMut(&Path)) {
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
