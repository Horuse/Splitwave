use std::sync::{OnceLock, RwLock};

use pipewire as pw;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::audio::pw_enum::nodes_by_class;

use super::{VirtualDeviceConfig, VirtualDriverStatus};

const NODE_PREFIX: &str = "splitwave";
const STORE_NAME: &str = "virtual-devices.json";
const STORE_KEY: &str = "devices";
const LEGACY_CONF_NAME: &str = "50-splitwave-sinks.conf";

fn configs() -> &'static RwLock<Vec<VirtualDeviceConfig>> {
    static CONFIGS: OnceLock<RwLock<Vec<VirtualDeviceConfig>>> = OnceLock::new();
    CONFIGS.get_or_init(|| RwLock::new(Vec::new()))
}

fn clean_label(name: &str) -> String {
    name.replace('\0', "")
}

// PipeWire channel map. Standard names for mono/stereo; generic AUX for wider
// layouts so any channel count is accepted.
fn positions(channels: u32) -> String {
    let list: Vec<String> = match channels.clamp(1, 256) {
        1 => vec!["MONO".into()],
        2 => vec!["FL".into(), "FR".into()],
        n => (0..n).map(|i| format!("AUX{i}")).collect(),
    };
    list.join(" ")
}

pub fn status() -> VirtualDriverStatus {
    // Native PipeWire (no subprocess): a reachable session is all we need to
    // create null-sinks.
    let ok = nodes_by_class("Audio/Sink").is_ok();
    VirtualDriverStatus {
        installed: ok,
        installed_version: None,
        current_version: super::DRIVER_VERSION,
        needs_update: false,
    }
}

pub fn install(_app: &AppHandle) -> Result<(), String> {
    // No driver to install on PipeWire; sinks are created directly.
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    unload_runtime_sinks()?;
    remove_legacy_config()?;
    *configs()
        .write()
        .map_err(|_| "virtual device cache poisoned")? = Vec::new();
    Ok(())
}

pub fn apply_virtual_devices(devices: Vec<VirtualDeviceConfig>) -> Result<(), String> {
    for device in &devices {
        if !(1..=256).contains(&device.channels) {
            return Err(format!("invalid channel count for {:?}", device.name));
        }
        if device.sample_rate == 0 {
            return Err(format!("invalid sample rate for {:?}", device.name));
        }
    }
    remove_legacy_config()?;
    unload_runtime_sinks()?;
    for d in &devices {
        create_runtime_sink(&d.id, &clean_label(&d.name), d.channels, d.sample_rate)?;
    }
    *configs()
        .write()
        .map_err(|_| "virtual device cache poisoned")? = devices;
    Ok(())
}

fn remove_legacy_config() -> Result<(), String> {
    let Some(path) =
        dirs::config_dir().map(|dir| dir.join("pipewire/pipewire.conf.d").join(LEGACY_CONF_NAME))
    else {
        return Ok(());
    };
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove legacy PipeWire config: {error}")),
    }
}

pub fn restore(app: &AppHandle) -> Result<(), String> {
    let store = app
        .store(STORE_NAME)
        .map_err(|e| format!("open virtual device store: {e}"))?;
    let devices = store
        .get(STORE_KEY)
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| format!("read virtual device store: {e}"))?
        .unwrap_or_default();
    apply_virtual_devices(devices)
}

pub fn config_for_node(node_name: &str) -> Option<VirtualDeviceConfig> {
    let id = node_name.strip_prefix(&format!("{NODE_PREFIX}."))?;
    configs()
        .read()
        .ok()?
        .iter()
        .find(|device| device.id == id)
        .cloned()
}

// Connect a short-lived PipeWire session, apply `f`, and wait one round-trip so
// the server-side change is registered before we disconnect.
fn with_session(
    f: impl FnOnce(&pw::core::CoreRc, &pw::main_loop::MainLoopRc) -> Result<(), String>,
) -> Result<(), String> {
    pw::init();
    let mainloop =
        pw::main_loop::MainLoopRc::new(None).map_err(|e| format!("pipewire mainloop: {e}"))?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .map_err(|e| format!("pipewire context: {e}"))?;
    let core = context
        .connect_rc(None)
        .map_err(|e| format!("pipewire connect: {e}"))?;
    let res = f(&core, &mainloop);
    mainloop.quit();
    res
}

fn roundtrip(core: &pw::core::CoreRc, mainloop: &pw::main_loop::MainLoopRc) -> Result<(), String> {
    let pending = core.sync(0).map_err(|e| format!("pipewire sync: {e}"))?;
    let ml = mainloop.clone();
    let _l = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == 0 && seq == pending {
                ml.quit();
            }
        })
        .register();
    mainloop.run();
    Ok(())
}

fn create_runtime_sink(
    id: &str,
    label: &str,
    channels: u32,
    sample_rate: u32,
) -> Result<(), String> {
    with_session(|core, mainloop| {
        let mut props = pw::properties::properties! {
            *pw::keys::FACTORY_NAME => "support.null-audio-sink",
            *pw::keys::MEDIA_CLASS => "Audio/Sink",
            "object.linger" => "1",
        };
        props.insert(*pw::keys::NODE_NAME, format!("{NODE_PREFIX}.{id}"));
        props.insert(*pw::keys::NODE_DESCRIPTION, label.to_string());
        props.insert("audio.position", positions(channels));
        props.insert("audio.rate", sample_rate.to_string());
        let _node: pw::node::Node = core
            .create_object("adapter", &props)
            .map_err(|e| format!("create null sink: {e}"))?;
        roundtrip(core, mainloop)
    })
}

fn unload_runtime_sinks() -> Result<(), String> {
    let prefix = format!("{NODE_PREFIX}.");
    let nodes = nodes_by_class("Audio/Sink").map_err(|error| error.to_string())?;
    let mine: Vec<u32> = nodes
        .iter()
        .filter(|n| n.name.starts_with(&prefix))
        .map(|n| n.id)
        .collect();
    if mine.is_empty() {
        return Ok(());
    }
    with_session(|core, mainloop| {
        let registry = core
            .get_registry()
            .map_err(|e| format!("pipewire registry: {e}"))?;
        for id in mine {
            registry
                .destroy_global(id)
                .into_result()
                .map_err(|e| format!("destroy sink {id}: {e:?}"))?;
        }
        roundtrip(core, mainloop)
    })
}
