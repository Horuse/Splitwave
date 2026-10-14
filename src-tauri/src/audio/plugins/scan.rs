use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use super::au_backend::AuBackend;
use super::clap_backend::ClapBackend;
use super::vst3_backend::Vst3Backend;
use super::{PluginBackend, PluginDescriptor};

const CACHE_FILE: &str = "plugins.json";
/// Names the bundle being opened, for as long as it is open. Found on the next
/// scan only when that bundle took the app down with it.
const OPENING_FILE: &str = "plugins.opening";
/// How stale the saved cache may get during a long scan, so a crash midway
/// keeps the work done before it.
const SAVE_INTERVAL: Duration = Duration::from_secs(1);
/// A cache written by another release is dropped whole: that release may have
/// described or rejected bundles differently.
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

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
    // Empty for a bundle that holds no effects or crashed the app while being
    // opened, so it is not reopened on every scan either. A bundle that failed
    // to load is never cached.
    plugins: Vec<PluginDescriptor>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    app_version: String,
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
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(|| cache_dir.map(load).unwrap_or_default());
    if let Some(dir) = cache_dir {
        if take_crashed(dir, cache) {
            save(dir, cache);
        }
    }
    let backends: [&dyn PluginBackend; 2] = [&ClapBackend, &Vst3Backend];
    let files = scan_files(cache, &backends, full, cache_dir);
    drop(guard);

    #[cfg(target_os = "macos")]
    return files.into_iter().chain(AuBackend.scan()).collect();
    #[cfg(not(target_os = "macos"))]
    files
}

/// Answers each bundle found under `backends`' search dirs from `cache` when
/// its stamp is unchanged and opens it otherwise. Bundles no longer on disk
/// leave the cache. With a `cache_dir`, progress is saved as it goes and each
/// opened bundle is announced in [`OPENING_FILE`] first.
fn scan_files(
    cache: &mut Cache,
    backends: &[&dyn PluginBackend],
    full: bool,
    cache_dir: Option<&Path>,
) -> Vec<PluginDescriptor> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut dirty = false;
    let mut saved_at = Instant::now();
    for backend in backends {
        for dir in backend.search_dirs() {
            walk(&dir, backend.extension(), &mut |bundle| {
                let key = bundle.to_string_lossy().into_owned();
                if !seen.insert(key.clone()) {
                    return;
                }
                let stamp = stamp(bundle);
                if let Some(cached) = cache.bundles.get(&key) {
                    if !full && cached.stamp == stamp {
                        out.extend(cached.plugins.iter().cloned());
                        return;
                    }
                }

                let opening = cache_dir.map(|dir| dir.join(OPENING_FILE));
                if let Some(path) = &opening {
                    let _ = std::fs::write(path, &key);
                }
                let result = catch_unwind(AssertUnwindSafe(|| backend.scan_bundle(bundle)))
                    .unwrap_or_else(|_| Err("panicked while scanning".into()));
                if let Some(path) = &opening {
                    let _ = std::fs::remove_file(path);
                }

                match result {
                    Ok(plugins) => {
                        out.extend(plugins.iter().cloned());
                        cache.bundles.insert(key, CachedBundle { stamp, plugins });
                        dirty = true;
                    }
                    // Not cached, so the next scan tries the bundle again.
                    Err(err) => {
                        tracing::warn!("{}: {err}", bundle.display());
                        dirty |= cache.bundles.remove(&key).is_some();
                    }
                }
                if let Some(dir) = cache_dir {
                    if dirty && saved_at.elapsed() >= SAVE_INTERVAL {
                        save(dir, cache);
                        dirty = false;
                        saved_at = Instant::now();
                    }
                }
            });
        }
    }
    let before = cache.bundles.len();
    cache.bundles.retain(|key, _| seen.contains(key));
    dirty |= cache.bundles.len() != before;
    if let Some(dir) = cache_dir {
        if dirty {
            save(dir, cache);
        }
    }
    out
}

/// Marks the bundle a previous run died opening, if any, as crashed. Returns
/// whether one was found.
fn take_crashed(cache_dir: &Path, cache: &mut Cache) -> bool {
    let marker = cache_dir.join(OPENING_FILE);
    let Ok(key) = std::fs::read_to_string(&marker) else {
        return false;
    };
    let _ = std::fs::remove_file(&marker);
    tracing::warn!("{key}: crashed the app while being scanned, skipped until it changes");
    cache.bundles.insert(
        key.clone(),
        CachedBundle {
            stamp: stamp(Path::new(&key)),
            plugins: Vec::new(),
        },
    );
    true
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
    // The bundle itself may be a symlink into another plugin folder; links
    // inside it are not followed (see `stamp_into`).
    if let Ok(meta) = std::fs::metadata(bundle) {
        stamp_entry(bundle, &meta, &mut acc);
    }
    acc
}

/// Links inside a bundle are skipped rather than followed: a framework's
/// `Versions/Current` points at a sibling already counted, and a link back up
/// the tree would recurse forever.
fn stamp_into(path: &Path, acc: &mut Stamp) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() {
        return;
    }
    stamp_entry(path, &meta, acc);
}

fn stamp_entry(path: &Path, meta: &std::fs::Metadata, acc: &mut Stamp) {
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

fn load(cache_dir: &Path) -> Cache {
    let Ok(bytes) = std::fs::read(cache_dir.join(CACHE_FILE)) else {
        return Cache::default();
    };
    match serde_json::from_slice::<Cache>(&bytes) {
        Ok(cache) if cache.app_version == APP_VERSION => cache,
        Ok(cache) => {
            tracing::info!(
                from = cache.app_version,
                "plugin cache from another release, rebuilding"
            );
            Cache::default()
        }
        Err(err) => {
            tracing::warn!("plugin cache unreadable, rebuilding: {err}");
            Cache::default()
        }
    }
}

fn save(cache_dir: &Path, cache: &mut Cache) {
    cache.app_version = APP_VERSION.to_string();
    let bytes = match serde_json::to_vec(cache) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!("plugin cache not serialized: {err}");
            return;
        }
    };
    let _ = std::fs::create_dir_all(cache_dir);
    let path = cache_dir.join(CACHE_FILE);
    // Written aside and renamed over, so a crash mid-write never leaves a
    // truncated cache behind.
    let tmp = path.with_extension("json.tmp");
    if let Err(err) = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &path)) {
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
        fn scan_bundle(&self, path: &Path) -> Result<Vec<PluginDescriptor>, String> {
            self.opened.set(self.opened.get() + 1);
            match std::fs::read(path).as_deref() {
                Ok(b"broken") => return Err("unreadable".into()),
                Ok(b"panics") => panic!("plugin panicked"),
                _ => {}
            }
            let path = path.to_string_lossy().into_owned();
            Ok(vec![PluginDescriptor {
                uid: format!("fake:{path}"),
                format: PluginFormat::Clap,
                path,
                plugin_id: "id".into(),
                name: "name".into(),
                vendor: String::new(),
                version: String::new(),
            }])
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

    fn scan(backend: &FakeBackend, cache: &mut Cache, full: bool) -> Vec<PluginDescriptor> {
        scan_files(cache, &[backend], full, None)
    }

    #[test]
    fn unchanged_bundles_are_not_reopened() {
        let backend = fixture("unchanged");
        let mut cache = Cache::default();

        let first = scan(&backend, &mut cache, false);
        assert_eq!(backend.opened.get(), 2);

        let again = scan(&backend, &mut cache, false);
        assert_eq!(backend.opened.get(), 2, "a cached bundle was reopened");
        assert_eq!(paths(&first), paths(&again));
    }

    #[test]
    fn a_changed_bundle_is_reopened_alone() {
        let backend = fixture("changed");
        let mut cache = Cache::default();
        scan(&backend, &mut cache, false);

        std::fs::write(backend.dir.join("a.fake"), b"v2 is longer").unwrap();
        scan(&backend, &mut cache, false);
        assert_eq!(backend.opened.get(), 3);
    }

    #[test]
    fn added_and_removed_bundles_are_tracked() {
        let backend = fixture("added-removed");
        let mut cache = Cache::default();
        scan(&backend, &mut cache, false);

        std::fs::remove_file(backend.dir.join("b.fake")).unwrap();
        std::fs::write(backend.dir.join("c.fake"), b"v1").unwrap();
        let found = scan(&backend, &mut cache, false);
        assert_eq!(backend.opened.get(), 3, "only the new bundle is opened");
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|p| !p.path.ends_with("b.fake")));
        assert!(!cache.bundles.keys().any(|k| k.ends_with("b.fake")));
    }

    #[test]
    fn a_full_scan_reopens_everything() {
        let backend = fixture("full");
        let mut cache = Cache::default();
        scan(&backend, &mut cache, false);
        scan(&backend, &mut cache, true);
        assert_eq!(backend.opened.get(), 4);
    }

    #[test]
    fn the_cache_round_trips_through_disk() {
        let backend = fixture("disk");
        let mut cache = Cache::default();
        scan_files(&mut cache, &[&backend], false, Some(&backend.dir));

        let mut loaded = load(&backend.dir);
        scan(&backend, &mut loaded, false);
        assert_eq!(
            backend.opened.get(),
            2,
            "a bundle cached on disk was reopened"
        );
    }

    #[test]
    fn a_cache_from_another_release_is_dropped() {
        let backend = fixture("release");
        let mut cache = Cache::default();
        scan_files(&mut cache, &[&backend], false, Some(&backend.dir));

        let path = backend.dir.join(CACHE_FILE);
        let stored = std::fs::read_to_string(&path).unwrap();
        let older = stored.replace(APP_VERSION, "0.0.1");
        std::fs::write(&path, older).unwrap();
        assert!(load(&backend.dir).bundles.is_empty());
    }

    #[test]
    fn a_failed_bundle_is_retried_not_cached() {
        let backend = fixture("failed");
        std::fs::write(backend.dir.join("a.fake"), b"broken").unwrap();
        let mut cache = Cache::default();

        let found = scan(&backend, &mut cache, false);
        assert_eq!(found.len(), 1);
        assert!(!cache.bundles.keys().any(|k| k.ends_with("a.fake")));

        scan(&backend, &mut cache, false);
        assert_eq!(backend.opened.get(), 3, "the failed bundle was not retried");
    }

    #[test]
    fn a_panicking_bundle_does_not_end_the_scan() {
        let backend = fixture("panics");
        std::fs::write(backend.dir.join("a.fake"), b"panics").unwrap();
        let mut cache = Cache::default();

        let found = scan(&backend, &mut cache, false);
        assert_eq!(paths(&found).len(), 1);
        assert!(found[0].path.ends_with("b.fake"));
        assert!(!cache.bundles.keys().any(|k| k.ends_with("a.fake")));
    }

    #[test]
    fn a_bundle_that_crashed_the_app_is_skipped_until_a_full_scan() {
        let backend = fixture("crashed");
        let crashed = backend.dir.join("a.fake").to_string_lossy().into_owned();
        // What a run that died inside `scan_bundle` leaves behind.
        std::fs::write(backend.dir.join(OPENING_FILE), &crashed).unwrap();
        let mut cache = Cache::default();

        assert!(take_crashed(&backend.dir, &mut cache));
        assert!(!backend.dir.join(OPENING_FILE).exists());
        let found = scan(&backend, &mut cache, false);
        assert_eq!(backend.opened.get(), 1, "the crashed bundle was reopened");
        assert!(found.iter().all(|p| p.path != crashed));

        let found = scan(&backend, &mut cache, true);
        assert_eq!(backend.opened.get(), 3);
        assert!(found.iter().any(|p| p.path == crashed));
    }

    #[test]
    fn a_completed_scan_leaves_no_opening_marker() {
        let backend = fixture("marker");
        let mut cache = Cache::default();
        scan_files(&mut cache, &[&backend], false, Some(&backend.dir));
        assert!(!backend.dir.join(OPENING_FILE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_loop_inside_a_bundle_is_not_followed() {
        let backend = fixture("symlink");
        let bundle = backend.dir.join("loop.fake");
        std::fs::create_dir_all(bundle.join("Versions")).unwrap();
        std::fs::write(bundle.join("binary"), b"code").unwrap();
        std::os::unix::fs::symlink(&bundle, bundle.join("Versions/Current")).unwrap();
        assert_eq!(stamp(&bundle).size, 4);
    }
}
