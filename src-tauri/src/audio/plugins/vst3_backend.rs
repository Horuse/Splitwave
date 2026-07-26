//! VST3 discovery, plus the module handle instantiation reuses.

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use vst3::Steinberg::{
    IPluginFactory, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo, PClassInfo2,
    PFactoryInfo, TUID,
};
use vst3::ComPtr;

use super::{PluginBackend, PluginDescriptor, PluginFormat};

/// `PClassInfo::category` of a class that is an audio effect, as opposed to the
/// controllers and other helper classes a factory also lists.
const AUDIO_MODULE_CLASS: &str = "Audio Module Class";

pub struct Vst3Backend;

impl PluginBackend for Vst3Backend {
    fn format(&self) -> PluginFormat {
        PluginFormat::Vst3
    }

    fn extension(&self) -> &'static str {
        "vst3"
    }

    /// The locations the VST3 spec reserves for installed plugins on each OS.
    fn search_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        #[cfg(target_os = "macos")]
        {
            dirs.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
            if let Some(home) = std::env::var_os("HOME") {
                dirs.push(Path::new(&home).join("Library/Audio/Plug-Ins/VST3"));
            }
        }
        #[cfg(target_os = "windows")]
        {
            // CommonProgramW6432 is the 64-bit location seen from any process;
            // CommonProgramFiles follows the host process's own bitness.
            for var in ["CommonProgramW6432", "CommonProgramFiles"] {
                if let Some(common) = std::env::var_os(var) {
                    dirs.push(Path::new(&common).join("VST3"));
                }
            }
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                dirs.push(Path::new(&local).join("Programs/Common/VST3"));
            }
        }
        #[cfg(target_os = "linux")]
        {
            if let Some(home) = std::env::var_os("HOME") {
                dirs.push(Path::new(&home).join(".vst3"));
            }
            dirs.push(PathBuf::from("/usr/lib/vst3"));
            dirs.push(PathBuf::from("/usr/local/lib/vst3"));
        }
        dirs.retain(|dir| dir.is_dir());
        dirs.dedup();
        dirs
    }

    fn scan_bundle(&self, path: &Path) -> Vec<PluginDescriptor> {
        let module = match Vst3Module::open(path) {
            Ok(module) => module,
            Err(err) => {
                tracing::warn!("vst3: {err}");
                return Vec::new();
            }
        };
        module.descriptors()
    }
}

/// A handle to a loaded bundle. Cheap to clone; the bundle unloads when the
/// last handle goes.
#[derive(Clone)]
pub struct Vst3Module(Arc<Module>);

/// A loaded VST3 bundle and its factory, kept together because the factory
/// points into code the bundle owns: releasing the bundle first leaves it
/// dangling. Dropping this unloads both, in that order.
struct Module {
    factory: Option<ComPtr<IPluginFactory>>,
    binary: Binary,
    path: String,
}

/// One `Module` per path, process-wide. `bundleEntry` and `bundleExit` act on
/// code shared by everything in the process, so a second load of the same
/// bundle hands out the same factory, and one scan finishing must not unload
/// the code another instance is still running.
fn loaded() -> &'static Mutex<HashMap<String, Weak<Module>>> {
    static LOADED: OnceLock<Mutex<HashMap<String, Weak<Module>>>> = OnceLock::new();
    LOADED.get_or_init(|| Mutex::new(HashMap::new()))
}

type GetFactory = unsafe extern "C" fn() -> *mut IPluginFactory;

/// The executable half of a VST3 module: a CFBundle on macOS, a plain shared
/// library everywhere else. Loading it, finding a symbol in it and running the
/// module's entry/exit hooks is all the rest of this file needs from it.
#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::path::Path;

    use objc2_core_foundation::{CFBundle, CFRetained, CFString, CFURL};

    type BundleEntry = unsafe extern "C" fn(*mut c_void) -> bool;
    type BundleExit = unsafe extern "C" fn() -> bool;

    pub struct Binary(CFRetained<CFBundle>);

    impl Binary {
        pub fn open(path: &Path) -> Result<Self, String> {
            let bytes = path.as_os_str().as_encoded_bytes();
            // SAFETY: the buffer outlives the call, which copies what it needs.
            let url = unsafe {
                CFURL::from_file_system_representation(
                    None,
                    bytes.as_ptr(),
                    bytes.len() as isize,
                    true,
                )
            }
            .ok_or("path is not representable as a file URL")?;

            let bundle = CFBundle::new(None, Some(&url))
                .ok_or("not a loadable bundle (no Contents/Info.plist?)")?;

            // SAFETY: loads foreign code, which can do anything on load. Inherent
            // to hosting third-party plugins and accepted for every format here.
            if !unsafe { bundle.load_executable() } {
                return Err("bundle executable failed to load (wrong architecture?)".into());
            }
            Ok(Self(bundle))
        }

        pub fn symbol(&self, name: &str) -> *mut c_void {
            self.0
                .function_pointer_for_name(Some(&CFString::from_str(name)))
        }

        pub fn enter(&self) -> Result<(), String> {
            let entry = self.symbol("bundleEntry");
            if entry.is_null() {
                return Err("exports no bundleEntry, so it is not a VST3 module".into());
            }
            // SAFETY: the symbol's signature is fixed by the VST3 macOS ABI.
            let entered = unsafe {
                std::mem::transmute::<*mut c_void, BundleEntry>(entry)(
                    CFRetained::as_ptr(&self.0).as_ptr().cast(),
                )
            };
            entered
                .then_some(())
                .ok_or_else(|| "bundleEntry refused to initialise the module".into())
        }

        pub fn exit(&self) {
            let exit = self.symbol("bundleExit");
            if !exit.is_null() {
                // SAFETY: signature fixed by the VST3 macOS ABI.
                unsafe { std::mem::transmute::<*mut c_void, BundleExit>(exit)() };
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};

    /// Windows takes no argument, Linux takes the `dlopen` handle. Both return
    /// success, and both are optional: a module that exports neither is still a
    /// valid VST3 as long as `GetPluginFactory` is there.
    #[cfg(target_os = "windows")]
    type ModuleEntry = unsafe extern "C" fn() -> bool;
    #[cfg(not(target_os = "windows"))]
    type ModuleEntry = unsafe extern "C" fn(*mut c_void) -> bool;
    type ModuleExit = unsafe extern "C" fn() -> bool;

    #[cfg(target_os = "windows")]
    const ENTRY: &str = "InitDll";
    #[cfg(target_os = "windows")]
    const EXIT: &str = "ExitDll";
    #[cfg(not(target_os = "windows"))]
    const ENTRY: &str = "ModuleEntry";
    #[cfg(not(target_os = "windows"))]
    const EXIT: &str = "ModuleExit";

    /// Extension of the loadable file inside a bundle, which is not the `.vst3`
    /// of the bundle directory itself on Linux.
    #[cfg(target_os = "windows")]
    const BINARY_EXT: &str = "vst3";
    #[cfg(not(target_os = "windows"))]
    const BINARY_EXT: &str = "so";

    /// `Contents` subdirectory holding the build for this machine. The names are
    /// fixed by the VST3 bundle layout, not by us.
    const ARCH_DIR: &str = if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "arm64-win"
        } else if cfg!(target_arch = "x86_64") {
            "x86_64-win"
        } else {
            "x86-win"
        }
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-linux"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64-linux"
    } else {
        "i386-linux"
    };

    /// The loaded library. `Module` owns exactly one and unloads it on drop, so
    /// the handle is never copied out of here.
    pub struct Binary(*mut c_void);

    // SAFETY: the handle names code the loader keeps loaded process-wide, not
    // thread-owned state; `Module` is the only owner and its registry
    // serialises load and unload.
    unsafe impl Send for Binary {}
    unsafe impl Sync for Binary {}

    impl Binary {
        pub fn open(path: &Path) -> Result<Self, String> {
            let binary = binary_in(path)?;
            let handle = load(&binary)?;
            Ok(Self(handle))
        }

        pub fn symbol(&self, name: &str) -> *mut c_void {
            let Ok(name) = std::ffi::CString::new(name) else {
                return std::ptr::null_mut();
            };
            // SAFETY: a missing symbol comes back as null rather than as UB; the
            // caller transmutes to the signature the VST3 ABI fixes for it.
            #[cfg(target_os = "windows")]
            unsafe {
                use windows::core::PCSTR;
                use windows::Win32::Foundation::HMODULE;
                use windows::Win32::System::LibraryLoader::GetProcAddress;

                GetProcAddress(HMODULE(self.0), PCSTR(name.as_ptr().cast()))
                    .map(|p| p as *mut c_void)
                    .unwrap_or(std::ptr::null_mut())
            }
            #[cfg(not(target_os = "windows"))]
            unsafe {
                libc::dlsym(self.0, name.as_ptr())
            }
        }

        pub fn enter(&self) -> Result<(), String> {
            let entry = self.symbol(ENTRY);
            // Both hooks are optional in the spec; `GetPluginFactory` is what
            // makes a library a VST3 module, and the caller checks for that.
            if entry.is_null() {
                return Ok(());
            }
            // SAFETY: signature fixed by the VST3 ABI for this platform.
            let entered = unsafe {
                let entry = std::mem::transmute::<*mut c_void, ModuleEntry>(entry);
                #[cfg(target_os = "windows")]
                {
                    entry()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    entry(self.0)
                }
            };
            entered
                .then_some(())
                .ok_or_else(|| format!("{ENTRY} refused to initialise the module"))
        }

        pub fn exit(&self) {
            let exit = self.symbol(EXIT);
            if !exit.is_null() {
                // SAFETY: signature fixed by the VST3 ABI.
                unsafe { std::mem::transmute::<*mut c_void, ModuleExit>(exit)() };
            }
        }
    }

    impl Drop for Binary {
        fn drop(&mut self) {
            // SAFETY: the handle came from the loader below and nothing else
            // holds it; `Module::drop` has already released the factory.
            unsafe {
                #[cfg(target_os = "windows")]
                {
                    use windows::Win32::Foundation::{FreeLibrary, HMODULE};
                    let _ = FreeLibrary(HMODULE(self.0));
                }
                #[cfg(not(target_os = "windows"))]
                {
                    libc::dlclose(self.0);
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn load(binary: &Path) -> Result<*mut c_void, String> {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::System::LibraryLoader::LoadLibraryW;

        let wide: Vec<u16> = binary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: loads foreign code, which runs its own initialisers. Inherent
        // to hosting third-party plugins and accepted for every format here.
        unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }
            .map(|m| m.0)
            .map_err(|e| format!("{} failed to load: {e}", binary.display()))
    }

    #[cfg(not(target_os = "windows"))]
    fn load(binary: &Path) -> Result<*mut c_void, String> {
        use std::os::unix::ffi::OsStrExt;

        let path = std::ffi::CString::new(binary.as_os_str().as_bytes())
            .map_err(|_| format!("{}: path contains a NUL", binary.display()))?;
        // SAFETY: loads foreign code, which runs its own initialisers. Inherent
        // to hosting third-party plugins and accepted for every format here.
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            // SAFETY: dlerror is valid straight after a failed dlopen.
            let err = unsafe {
                let msg = libc::dlerror();
                if msg.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(msg).to_string_lossy().into_owned()
                }
            };
            return Err(format!("{} failed to load: {err}", binary.display()));
        }
        Ok(handle)
    }

    /// The loadable file for this machine inside a `.vst3` bundle. Windows also
    /// allows a bare DLL named `*.vst3`, which is already the file to load.
    fn binary_in(path: &Path) -> Result<PathBuf, String> {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        let arch = path.join("Contents").join(ARCH_DIR);
        let named = path
            .file_stem()
            .map(|stem| arch.join(stem).with_extension(BINARY_EXT));
        if let Some(named) = named.filter(|p| p.is_file()) {
            return Ok(named);
        }
        // The spec names the binary after the bundle, but not every vendor
        // obeys; a directory holding exactly one candidate is still unambiguous.
        let mut candidates = std::fs::read_dir(&arch)
            .map_err(|e| format!("{}: {e}", arch.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(BINARY_EXT));
        match (candidates.next(), candidates.next()) {
            (Some(only), None) => Ok(only),
            (Some(_), Some(_)) => Err(format!(
                "{} holds several binaries and none matches the bundle name",
                arch.display()
            )),
            _ => Err(format!("{} holds no loadable binary", arch.display())),
        }
    }
}

use platform::Binary;

impl Vst3Module {
    pub fn open(path: &Path) -> Result<Self, String> {
        let key = path.display().to_string();
        // Held across the load so two threads cannot both run `bundleEntry`.
        let mut loaded = loaded().lock().unwrap();
        if let Some(module) = loaded.get(&key).and_then(Weak::upgrade) {
            return Ok(Self(module));
        }
        let module = Arc::new(Module::load(path)?);
        loaded.insert(key, Arc::downgrade(&module));
        Ok(Self(module))
    }

    pub fn factory(&self) -> &ComPtr<IPluginFactory> {
        self.0.factory.as_ref().expect("set in load, cleared in drop")
    }

    /// Every audio effect class the factory advertises.
    pub fn descriptors(&self) -> Vec<PluginDescriptor> {
        self.0.descriptors()
    }
}

impl Module {
    fn load(path: &Path) -> Result<Self, String> {
        let display = path.display();
        let at = |step: &str| format!("{display}: {step}");

        let binary = Binary::open(path).map_err(|e| at(&e))?;
        binary.enter().map_err(|e| at(&e))?;

        let mut module = Self {
            factory: None,
            binary,
            path: display.to_string(),
        };

        let get_factory = module.binary.symbol("GetPluginFactory");
        if get_factory.is_null() {
            return Err(at("exports no GetPluginFactory"));
        }
        // SAFETY: signature fixed by the VST3 ABI; the returned factory carries
        // one reference, which `from_raw` takes over rather than adding to.
        let factory = unsafe {
            let raw = std::mem::transmute::<*mut c_void, GetFactory>(get_factory)();
            ComPtr::from_raw(raw)
        }
        .ok_or_else(|| at("GetPluginFactory returned nothing"))?;

        module.factory = Some(factory);
        Ok(module)
    }

    fn descriptors(&self) -> Vec<PluginDescriptor> {
        let factory = self.factory.as_ref().expect("set in load");
        let factory2 = factory.cast::<vst3::Steinberg::IPluginFactory2>();

        // Per-class vendor is optional and FabFilter leaves it empty; the
        // factory's own vendor is the authoritative one.
        let mut factory_info: PFactoryInfo = unsafe { std::mem::zeroed() };
        let factory_vendor = unsafe {
            if factory.getFactoryInfo(&mut factory_info) == vst3::Steinberg::kResultOk {
                c_str(&factory_info.vendor)
            } else {
                String::new()
            }
        };

        let mut out = Vec::new();
        // SAFETY: indices below the count the factory itself reports.
        unsafe {
            for index in 0..factory.countClasses() {
                let mut info: PClassInfo = std::mem::zeroed();
                if factory.getClassInfo(index, &mut info) != vst3::Steinberg::kResultOk {
                    continue;
                }
                if c_str(&info.category) != AUDIO_MODULE_CLASS {
                    continue;
                }

                let mut info2: PClassInfo2 = std::mem::zeroed();
                let extended = factory2.as_ref().is_some_and(|f| {
                    f.getClassInfo2(index, &mut info2) == vst3::Steinberg::kResultOk
                });
                // Splitwave routes audio and sends no notes, so an instrument
                // would sit silent in the graph. Mirrors the AU scan, which
                // takes only `aufx` and `aumf`.
                if !extended || !c_str(&info2.subCategories).split('|').any(|c| c == "Fx") {
                    continue;
                }

                let cid = format_cid(&info.cid);
                out.push(PluginDescriptor {
                    uid: format!("vst3:{}:{cid}", self.path),
                    format: PluginFormat::Vst3,
                    path: self.path.clone(),
                    plugin_id: cid,
                    name: c_str(&info.name),
                    vendor: match c_str(&info2.vendor) {
                        v if !v.is_empty() => v,
                        _ => factory_vendor.clone(),
                    },
                    version: c_str(&info2.version),
                });
            }
        }
        out
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        // Release the factory before unloading the code that implements it.
        self.factory = None;
        self.binary.exit();
    }
}

/// A class id as 32 hex digits, the form used to address a plugin inside its
/// bundle and to reference it from a saved project.
pub fn format_cid(cid: &TUID) -> String {
    cid.iter().map(|b| format!("{:02X}", *b as u8)).collect()
}

/// Inverse of `format_cid`; `None` when the text is not 32 hex digits.
// Used by the host to resolve a saved plugin_id back to a class id.
#[allow(dead_code)]
pub fn parse_cid(text: &str) -> Option<TUID> {
    if text.len() != 32 {
        return None;
    }
    let mut cid: TUID = [0; 16];
    for (byte, pair) in cid.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()? as _;
    }
    Some(cid)
}

/// A fixed-width C string field, up to its first NUL.
fn c_str(field: &[std::ffi::c_char]) -> String {
    let bytes: Vec<u8> = field
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cid_round_trips() {
        let cid: TUID = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, -0x78, -0x67, -0x56, -0x45, -0x34,
            -0x23, -0x12, -0x01,
        ];
        let text = format_cid(&cid);
        assert_eq!(text, "00112233445566778899AABBCCDDEEFF");
        assert_eq!(parse_cid(&text), Some(cid));
        assert_eq!(parse_cid("nope"), None);
        assert_eq!(parse_cid(&text[..30]), None);
    }

    /// Two handles to one bundle must share a factory. Loading it twice would
    /// mean the first handle dropped unloads code the second is still calling.
    #[test]
    fn opening_a_bundle_twice_reuses_the_same_module() {
        let Some(plugin) = Vst3Backend.scan().into_iter().next() else {
            println!("SKIPPED: no vst3 plugins installed, cannot check module reuse");
            return;
        };
        let path = std::path::Path::new(&plugin.path);
        let first = Vst3Module::open(path).unwrap();
        let second = Vst3Module::open(path).unwrap();
        assert_eq!(
            first.factory().as_ptr(),
            second.factory().as_ptr(),
            "a second open loaded a separate copy of {}",
            plugin.path
        );

        drop(first);
        // Surviving handle still works: the drop must not have unloaded it.
        assert!(!second.descriptors().is_empty());
    }

    #[test]
    fn lists_installed_vst3_plugins() {
        let found = Vst3Backend.scan();
        for plugin in &found {
            assert_eq!(plugin.format, PluginFormat::Vst3);
            assert_eq!(plugin.plugin_id.len(), 32);
            assert!(!plugin.name.is_empty(), "{plugin:?} has no name");
            assert!(parse_cid(&plugin.plugin_id).is_some());
        }
        // A machine with no VST3 plugins installed is a valid state, so the
        // scan finding nothing is not a failure -- but it does mean this test
        // asserted nothing, which the log should not hide.
        if found.is_empty() {
            println!("SKIPPED: no vst3 plugins installed, scan asserted nothing");
            return;
        }
        println!("found {} vst3 plugins", found.len());
        for plugin in &found {
            println!("  {} by {} [{}]", plugin.name, plugin.vendor, plugin.path);
        }
    }
}
