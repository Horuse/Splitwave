use std::path::PathBuf;

use pipewire as pw;
use tauri::AppHandle;

use crate::audio::pw_enum::nodes_by_class;

use super::{VirtualDeviceConfig, VirtualDriverStatus};

const CONF_NAME: &str = "50-splitwave-sinks.conf";
const NODE_PREFIX: &str = "splitwave";

fn conf_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("pipewire/pipewire.conf.d")
            .join(CONF_NAME),
    )
}

// node.name is the stable handle, node.description is the label shown in
// system settings. Quotes would break both the .conf and the created node
// props, so drop them from the label.
fn clean_label(name: &str) -> String {
    name.replace(['"', '\''], "")
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
    unload_runtime_sinks();
    if let Some(p) = conf_path() {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

use std::sync::Mutex;

static CACHED_DEVICES: Mutex<Option<Vec<VirtualDeviceConfig>>> = Mutex::new(None);

pub fn find_virtual_device(id_or_name: &str) -> Option<VirtualDeviceConfig> {
    let clean = id_or_name.strip_prefix("monitor:").unwrap_or(id_or_name);
    let mut guard = CACHED_DEVICES.lock().unwrap();
    if guard.is_none() {
        *guard = Some(load_devices_from_conf());
    }
    guard.as_ref()?.iter().find(|d| {
        d.id == clean || format!("{NODE_PREFIX}.{}", d.id) == clean
    }).cloned()
}

fn load_devices_from_conf() -> Vec<VirtualDeviceConfig> {
    let Some(path) = conf_path() else { return Vec::new() };
    let Ok(content) = std::fs::read_to_string(path) else { return Vec::new() };
    parse_conf_devices(&content)
}

fn parse_conf_devices(content: &str) -> Vec<VirtualDeviceConfig> {
    let mut devices = Vec::new();
    let mut cur_id = None;
    let mut cur_name = None;
    let mut cur_channels = 2;
    let mut cur_rate = 48_000;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("node.name") {
            if let Some(val) = trimmed.split('=').nth(1) {
                let name = val.trim().trim_matches('"');
                if let Some(id) = name.strip_prefix(&format!("{NODE_PREFIX}.")) {
                    cur_id = Some(id.to_string());
                }
            }
        } else if trimmed.starts_with("node.description") {
            if let Some(val) = trimmed.split('=').nth(1) {
                cur_name = Some(val.trim().trim_matches('"').to_string());
            }
        } else if trimmed.starts_with("audio.channels") {
            if let Some(val) = trimmed.split('=').nth(1) {
                if let Ok(ch) = val.trim().parse::<u32>() {
                    cur_channels = ch;
                }
            }
        } else if trimmed.starts_with("audio.rate") {
            if let Some(val) = trimmed.split('=').nth(1) {
                if let Ok(r) = val.trim().parse::<u32>() {
                    cur_rate = r;
                }
            }
        } else if trimmed == "}" {
            if let (Some(id), Some(name)) = (cur_id.take(), cur_name.take()) {
                devices.push(VirtualDeviceConfig {
                    id,
                    name,
                    channels: cur_channels,
                    sample_rate: cur_rate,
                });
                cur_channels = 2;
                cur_rate = 48_000;
            }
        }
    }
    devices
}

pub fn apply_virtual_devices(devices: Vec<VirtualDeviceConfig>) -> Result<(), String> {
    *CACHED_DEVICES.lock().unwrap() = Some(devices.clone());

    unload_runtime_sinks();

    let conf = conf_path().ok_or("no config directory")?;
    if devices.is_empty() {
        let _ = std::fs::remove_file(&conf);
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "restart", "pipewire"])
            .output();
        return Ok(());
    }

    if let Some(parent) = conf.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create conf dir: {e}"))?;
    }
    std::fs::write(&conf, conf_contents(&devices)).map_err(|e| format!("write conf: {e}"))?;

    // Try restarting pipewire user service if systemd is available so that PipeWire
    // recreates all sinks cleanly from the updated config file at the new sample rates.
    let restarted = std::process::Command::new("systemctl")
        .args(["--user", "restart", "pipewire"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !restarted {
        for d in &devices {
            create_runtime_sink(&d.id, &clean_label(&d.name), d.channels, d.sample_rate)?;
        }
    }
    Ok(())
}

fn conf_contents(devices: &[VirtualDeviceConfig]) -> String {
    let mut out =
        String::from("# Auto-generated by Splitwave. Do not edit.\n\ncontext.objects = [\n");
    for d in devices {
        let desc = clean_label(&d.name);
        out.push_str("  {\n");
        out.push_str("    factory = adapter\n");
        out.push_str("    args = {\n");
        out.push_str("      factory.name     = support.null-audio-sink\n");
        out.push_str(&format!(
            "      node.name        = \"{NODE_PREFIX}.{}\"\n",
            d.id
        ));
        out.push_str(&format!("      node.description = \"{desc}\"\n"));
        out.push_str("      media.class      = Audio/Sink\n");
        out.push_str(&format!("      audio.channels   = {}\n", d.channels));
        out.push_str(&format!(
            "      audio.position   = [ {} ]\n",
            positions(d.channels)
        ));
        out.push_str(&format!("      audio.rate       = {}\n", d.sample_rate));
        out.push_str(&format!("      node.rate        = 1/{}\n", d.sample_rate));
        out.push_str("      object.linger    = true\n");
        out.push_str("    }\n");
        out.push_str("  }\n");
    }
    out.push_str("]\n");
    out
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

// Create the sink in the running session so it shows up immediately. The .conf
// only takes effect on the next PipeWire start; object.linger keeps the node
// alive after we disconnect.
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
        props.insert("audio.channels", channels.to_string());
        props.insert("audio.position", format!("[ {} ]", positions(channels)));
        props.insert("audio.rate", sample_rate.to_string());
        props.insert("node.rate", format!("1/{sample_rate}"));
        let _node: pw::node::Node = core
            .create_object("adapter", &props)
            .map_err(|e| format!("create null sink: {e}"))?;
        roundtrip(core, mainloop)
    })
}

fn unload_runtime_sinks() {
    let prefix = format!("{NODE_PREFIX}.");
    let Ok(nodes) = nodes_by_class("Audio/Sink") else {
        return;
    };
    let mine: Vec<u32> = nodes
        .iter()
        .filter(|n| n.name.starts_with(&prefix))
        .map(|n| n.id)
        .collect();
    if mine.is_empty() {
        return;
    }
    let _ = with_session(|core, mainloop| {
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
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_conf_devices_correctly() {
        let conf = r#"
# Auto-generated by Splitwave. Do not edit.

context.objects = [
  {
    factory = adapter
    args = {
      factory.name     = support.null-audio-sink
      node.name        = "splitwave.test123"
      node.description = "Test Virtual Device"
      media.class      = Audio/Sink
      audio.channels   = 6
      audio.position   = [ FL FR FC LFE SL SR ]
      audio.rate       = 96000
      node.rate        = 1/96000
      object.linger    = true
    }
  }
]
"#;
        let devices = parse_conf_devices(conf);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "test123");
        assert_eq!(devices[0].name, "Test Virtual Device");
        assert_eq!(devices[0].channels, 6);
        assert_eq!(devices[0].sample_rate, 96000);
    }
}
