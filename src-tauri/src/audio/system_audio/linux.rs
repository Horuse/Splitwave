use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use freedesktop_desktop_entry::{
    desktop_entries, find_app_by_id, get_languages_from_env, unicase::Ascii,
};
use freedesktop_icons::lookup;
use pipewire::{context::ContextRc, main_loop::MainLoopRc, types::ObjectType};

use super::AudioApplication;
use crate::error::{AppError, AppResult};

const ICON_SIZE: u32 = 128;

pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    // pipewire objects are !Send, so the whole snapshot lives on one thread.
    std::thread::spawn(snapshot)
        .join()
        .map_err(|_| AppError::Host("pipewire enumeration thread panicked".into()))?
}

pub fn load_app_icons(bundle_ids: Vec<String>) -> HashMap<String, String> {
    bundle_ids
        .into_iter()
        .filter_map(|binary| Some((binary.clone(), STANDARD.encode(resolve_icon(&binary)?))))
        .collect()
}

fn resolve_icon(binary: &str) -> Option<Vec<u8>> {
    let locales = get_languages_from_env();
    let entries = desktop_entries(&locales);
    let entry = find_app_by_id(&entries, Ascii::new(binary))?;
    let icon = entry.icon()?;

    if icon.starts_with('/') {
        return read_icon_file(Path::new(icon));
    }

    let theme = current_icon_theme();
    let path = lookup(icon)
        .with_size(ICON_SIZE as u16)
        .with_scale(1)
        .with_theme(&theme)
        .find()
        .or_else(|| lookup(icon).with_size(ICON_SIZE as u16).with_scale(1).find())?;
    read_icon_file(&path)
}

fn read_icon_file(path: &Path) -> Option<Vec<u8>> {
    let svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    if svg {
        rasterize_svg(path)
    } else {
        std::fs::read(path).ok()
    }
}

fn rasterize_svg(path: &Path) -> Option<Vec<u8>> {
    let data = std::fs::read(path).ok()?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
    let orig = tree.size().to_int_size();
    let scale = ICON_SIZE as f32 / orig.width().max(orig.height()) as f32;
    let w = (orig.width() as f32 * scale) as u32;
    let h = (orig.height() as f32 * scale) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().ok()
}

// The icon name resolves against the user's chosen theme, so read it from the
// GTK settings; fall back to hicolor, which every theme inherits from.
fn current_icon_theme() -> String {
    let Some(config) = dirs::config_dir() else {
        return "hicolor".into();
    };
    for sub in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
        let Ok(content) = std::fs::read_to_string(config.join(sub)) else {
            continue;
        };
        for line in content.lines() {
            if let Some(val) = line.trim().strip_prefix("gtk-icon-theme-name") {
                let val = val.trim_start_matches('=').trim();
                if !val.is_empty() {
                    return val.to_string();
                }
            }
        }
    }
    "hicolor".into()
}

fn snapshot() -> AppResult<Vec<AudioApplication>> {
    let mainloop = MainLoopRc::new(None).map_err(pw_err)?;
    let context = ContextRc::new(&mainloop, None).map_err(pw_err)?;
    let core = context.connect_rc(None).map_err(pw_err)?;
    let registry = core.get_registry_rc().map_err(pw_err)?;

    let apps: Rc<RefCell<Vec<AudioApplication>>> = Rc::new(RefCell::new(Vec::new()));
    let apps_cb = apps.clone();

    let _reg_listener = registry
        .add_listener_local()
        .global(move |global| {
            if global.type_ != ObjectType::Node {
                return;
            }
            let Some(props) = &global.props else { return };
            if props.get("media.class") != Some("Stream/Output/Audio") {
                return;
            }
            let name = props
                .get("application.name")
                .or_else(|| props.get("node.name"))
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                return;
            }
            // binary is the stable handle for matching a .desktop entry; fall
            // back to the display name when a stream doesn't report one.
            let bundle_id = props
                .get("application.process.binary")
                .filter(|b| !b.is_empty())
                .unwrap_or(&name)
                .to_string();
            apps_cb.borrow_mut().push(AudioApplication {
                bundle_id,
                name,
                icon: None,
            });
        })
        .register();

    let pending = core.sync(0).map_err(pw_err)?;
    let ml_quit = mainloop.clone();
    let _core_listener = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == 0 && seq == pending {
                ml_quit.quit();
            }
        })
        .register();

    mainloop.run();

    // The registry listener still holds a clone of `apps`, so take the contents
    // out of the RefCell rather than unwrapping the Rc.
    let collected = std::mem::take(&mut *apps.borrow_mut());
    let mut seen = HashSet::new();
    let mut out: Vec<AudioApplication> = collected
        .into_iter()
        .filter(|a| seen.insert(a.bundle_id.clone()))
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

fn pw_err(e: impl std::fmt::Display) -> AppError {
    AppError::Host(format!("pipewire: {e}"))
}
