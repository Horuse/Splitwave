use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use super::au_backend::AuBackend;
use super::clap_backend::ClapBackend;
use super::vst3_backend::Vst3Backend;
use super::{PluginBackend, PluginDescriptor};

const CACHE_FILE: &str = "plugins.json";
/// Bump when `CachedBundle` or `PluginDescriptor` changes shape. A cache of
/// another version is dropped whole and rebuilt by the next scan.
const CACHE_VERSION: u32 = 1;

/// What a bundle looked like on disk when it was last opened. A bundle whose
/// stamp still matches is answered from the cache without loading its binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    modified_ms: u64,
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedBundle {
    stamp: Stamp,
    // Empty for a bundle that failed to load or holds no effects, so it is not
    // reopened on every scan either.
    plugins: Vec<PluginDescriptor>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    version: u32,
    bundles: HashMap<String, CachedBundle>,
}

/// Loaded from disk on the first scan. Held for a whole scan, so concurrent
/// callers queue up and then find everything already cached.
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// Every plugin of every registered format. File-based formats are answered from
/// the on-disk cache for bundles unchanged since they were last opened; `full`
/// reopens every bundle. Audio Units come from the system registry, which lists
/// them without loading them, so they are never cached.
pub fn scan_all(cache_dir: Option<&Path>, full: bool) -> Vec<PluginDescriptor> {
    let cache_path = cache_dir.map(|dir| dir.join(CACHE_FILE));
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(|| cache_path.as_deref().map(load).unwrap_or_default());

    let backends: [&dyn PluginBackend; 2] = [&ClapBackend, &Vst3Backend];
    let (mut out, dirty) = scan_files(cache, &backends, full);
    if dirty {
        if let Some(path) = &cache_path {
            save(path, cache);
        }
    }

    #[cfg(target_os = "macos")]
    out.extend(AuBackend.scan());
    out
}

/// Answers each bundle found under `backends`' search dirs from `cache` when
/// its stamp is unchanged and opens it otherwise. Bundles no longer on disk
/// leave the cache. Returns whether the cache changed.
fn scan_files(
    cache: &mut Cache,
    backends: &[&dyn PluginBackend],
    full: bool,
) -> (Vec<PluginDescriptor>, bool) {
    let mut fresh = HashMap::with_capacity(cache.bundles.len());
    let mut out = Vec::new();
    let mut dirty = false;
    for backend in backends {
        for dir in backend.search_dirs() {
            walk(&dir, backend.extension(), &mut |bundle| {
                let key = bundle.to_string_lossy().into_owned();
                if fresh.contains_key(&key) {
                    return;
                }
                let stamp = stamp(bundle);
                let entry = match cache.bundles.remove(&key) {
                    Some(cached) if !full && cached.stamp == stamp => cached,
                    _ => {
                        dirty = true;
                        CachedBundle {
                            stamp,
                            plugins: backend.scan_bundle(bundle),
                        }
                    }
                };
                out.extend(entry.plugins.iter().cloned());
                fresh.insert(key, entry);
            });
        }
    }
    dirty |= !cache.bundles.is_empty();
    cache.bundles = fresh;
    (out, dirty)
}

/// Latest modification time and total size of the files that make up the
/// bundle. Installers update a bundle's binary and Info.plist without touching
/// the bundle directory itself, so the directory's own mtime is not enough.
/// `Resources` is skipped: it can hold thousands of files and carries no code.
fn stamp(bundle: &Path) -> Stamp {
    let mut acc = Stamp {
        modified_ms: 0,
        size: 0,
    };
    stamp_into(bundle, &mut acc);
    acc
}

fn stamp_into(path: &Path, acc: &mut Stamp) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if let Some(ms) = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    {
        acc.modified_ms = acc.modified_ms.max(ms.as_millis() as u64);
    }
    if !meta.is_dir() {
        acc.size += meta.len();
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != "Resources" {
            stamp_into(&entry.path(), acc);
        }
    }
}

fn load(path: &Path) -> Cache {
    let Ok(bytes) = std::fs::read(path) else {
        return Cache::default();
    };
    match serde_json::from_slice::<Cache>(&bytes) {
        Ok(cache) if cache.version == CACHE_VERSION => cache,
        Ok(cache) => {
            tracing::info!(
                version = cache.version,
                "plugin cache from another version, rebuilding"
            );
            Cache::default()
        }
        Err(err) => {
            tracing::warn!("plugin cache unreadable, rebuilding: {err}");
            Cache::default()
        }
    }
}

fn save(path: &Path, cache: &mut Cache) {
    cache.version = CACHE_VERSION;
    let bytes = match serde_json::to_vec(cache) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!("plugin cache not serialized: {err}");
            return;
        }
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Written aside and renamed over, so a crash mid-write never leaves a
    // truncated cache behind.
    let tmp = path.with_extension("json.tmp");
    if let Err(err) = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)) {
        tracing::warn!("plugin cache not saved to {}: {err}", path.display());
    }
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;

    use super::super::PluginFormat;
    use super::*;

    /// Describes every `.fake` bundle as one plugin and counts how many
    /// bundles it had to open.
    struct FakeBackend {
        dir: PathBuf,
        opened: Cell<usize>,
    }

    impl PluginBackend for FakeBackend {
        fn format(&self) -> PluginFormat {
            PluginFormat::Clap
        }
        fn search_dirs(&self) -> Vec<PathBuf> {
            vec![self.dir.clone()]
        }
        fn extension(&self) -> &'static str {
            "fake"
        }
        fn scan_bundle(&self, path: &Path) -> Vec<PluginDescriptor> {
            self.opened.set(self.opened.get() + 1);
            let path = path.to_string_lossy().into_owned();
            vec![PluginDescriptor {
                uid: format!("fake:{path}"),
                format: PluginFormat::Clap,
                path,
                plugin_id: "id".into(),
                name: "name".into(),
                vendor: String::new(),
                version: String::new(),
            }]
        }
    }

    fn fixture(name: &str) -> FakeBackend {
        let dir =
            std::env::temp_dir().join(format!("splitwave-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for bundle in ["a.fake", "b.fake"] {
            std::fs::write(dir.join(bundle), b"v1").unwrap();
        }
        FakeBackend {
            dir,
            opened: Cell::new(0),
        }
    }

    fn paths(found: &[PluginDescriptor]) -> Vec<String> {
        let mut paths: Vec<String> = found.iter().map(|p| p.path.clone()).collect();
        paths.sort();
        paths
    }

    #[test]
    fn unchanged_bundles_are_not_reopened() {
        let backend = fixture("unchanged");
        let mut cache = Cache::default();

        let (first, dirty) = scan_files(&mut cache, &[&backend], false);
        assert_eq!(backend.opened.get(), 2);
        assert!(dirty);

        let (again, dirty) = scan_files(&mut cache, &[&backend], false);
        assert_eq!(backend.opened.get(), 2, "a cached bundle was reopened");
        assert!(!dirty);
        assert_eq!(paths(&first), paths(&again));
    }

    #[test]
    fn a_changed_bundle_is_reopened_alone() {
        let backend = fixture("changed");
        let mut cache = Cache::default();
        scan_files(&mut cache, &[&backend], false);

        std::fs::write(backend.dir.join("a.fake"), b"v2 is longer").unwrap();
        let (_, dirty) = scan_files(&mut cache, &[&backend], false);
        assert_eq!(backend.opened.get(), 3);
        assert!(dirty);
    }

    #[test]
    fn added_and_removed_bundles_are_tracked() {
        let backend = fixture("added-removed");
        let mut cache = Cache::default();
        scan_files(&mut cache, &[&backend], false);

        std::fs::remove_file(backend.dir.join("b.fake")).unwrap();
        std::fs::write(backend.dir.join("c.fake"), b"v1").unwrap();
        let (found, dirty) = scan_files(&mut cache, &[&backend], false);
        assert_eq!(backend.opened.get(), 3, "only the new bundle is opened");
        assert!(dirty);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|p| !p.path.ends_with("b.fake")));
        assert!(!cache.bundles.keys().any(|k| k.ends_with("b.fake")));
    }

    #[test]
    fn a_full_scan_reopens_everything() {
        let backend = fixture("full");
        let mut cache = Cache::default();
        scan_files(&mut cache, &[&backend], false);
        scan_files(&mut cache, &[&backend], true);
        assert_eq!(backend.opened.get(), 4);
    }

    #[test]
    fn the_cache_round_trips_through_disk() {
        let backend = fixture("disk");
        let path = backend.dir.join(CACHE_FILE);
        let mut cache = Cache::default();
        scan_files(&mut cache, &[&backend], false);
        save(&path, &mut cache);

        let mut loaded = load(&path);
        scan_files(&mut loaded, &[&backend], false);
        assert_eq!(
            backend.opened.get(),
            2,
            "a bundle cached on disk was reopened"
        );
    }
}
