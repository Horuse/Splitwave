//! The native window a plugin editor is embedded into, and the geometry rules
//! around it. Nothing here knows which format it is hosting: the window is
//! ours, the view inside it is the plugin's, and `PluginHost::embed_editor` is
//! the only seam between them.

#[cfg(not(target_os = "windows"))]
use std::collections::HashMap;
#[cfg(not(target_os = "windows"))]
use std::sync::{Mutex, OnceLock};

#[cfg(not(target_os = "windows"))]
use tauri::Emitter;

use super::host_api::EditorSize;

/// Fallback editor size for plugins that report a nonsensical one.
pub const FALLBACK_EDITOR_SIZE: EditorSize = (800, 600);

/// Standard title-bar height (logical px) used when the window reports no
/// decoration overhead, which tao does on macOS (`outer_size == inner_size`).
#[cfg(target_os = "macos")]
const TITLEBAR_LOGICAL: f64 = 28.0;
#[cfg(not(target_os = "macos"))]
const TITLEBAR_LOGICAL: f64 = 32.0;

/// Native host windows that plugin editors are embedded into, keyed by node id.
/// `tauri::Window` is `Send + Sync`, so this lives outside any main-thread state
/// and can be created/closed from the command thread.
#[cfg(not(target_os = "windows"))]
fn windows() -> &'static Mutex<HashMap<String, tauri::Window>> {
    static WINDOWS: OnceLock<Mutex<HashMap<String, tauri::Window>>> = OnceLock::new();
    WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The window hosting this node's editor, if one is open.
pub fn window_for(node_id: &str) -> Option<tauri::Window> {
    #[cfg(target_os = "windows")]
    {
        let _ = node_id;
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        windows().lock().unwrap().get(node_id).cloned()
    }
}

/// Closes a node's editor window if one is open. Shared with the format hosts,
/// which have to take the window down alongside the instance it belongs to.
pub fn close_window(node_id: &str) {
    #[cfg(target_os = "windows")]
    {
        if let Some(host) = super::registry::for_node(node_id) {
            host.destroy_editor(node_id);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(w) = windows().lock().unwrap().remove(node_id) {
            let _ = w.close();
        }
    }
}


/// Rejects the degenerate sizes plugins report before their view exists (0x0)
/// or absurd values, so the window is never opened invisibly small or huge.
pub fn valid_gui_size(w: u32, h: u32) -> Option<EditorSize> {
    (w >= 100 && h >= 100 && w <= 8000 && h <= 8000).then_some((w, h))
}

/// Logical px the window decoration takes beyond its content, as (width,
/// height). The content view runs the full height of the window, under the
/// title bar, so this is also how far down a child view must start to clear it.
pub fn decoration_overhead(window: &tauri::Window) -> (f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let (dw, measured_dh) = match (window.inner_size(), window.outer_size()) {
        (Ok(inner), Ok(outer)) => (
            outer.width.saturating_sub(inner.width) as f64 / scale,
            outer.height.saturating_sub(inner.height) as f64 / scale,
        ),
        _ => (0.0, 0.0),
    };
    // tao returns outer == inner on macOS, so the measurement is 0; fall back to
    // the platform title-bar height so the plugin renders below the bar.
    let dh = if measured_dh > 0.5 {
        measured_dh
    } else {
        TITLEBAR_LOGICAL
    };
    (dw, dh)
}

/// Sizes the window so its content area is `w` x `h` logical px.
pub fn set_content_size(window: &tauri::Window, w: f64, h: f64) {
    #[cfg(target_os = "macos")]
    {
        let (dw, dh) = decoration_overhead(window);
        let _ = window.set_size(tauri::LogicalSize::new(w + dw, h + dh));
    }
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_size(tauri::PhysicalSize::new(w as u32, h as u32));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = window.set_size(tauri::LogicalSize::new(w, h));
    }
}

/// Opens the plugin editor embedded in a native host window. The tested plugins
/// only support embedded GUIs, so the host must own the window and hand its
/// native handle to the plugin.
#[cfg(target_os = "windows")]
pub fn open(node_id: &str, title: &str) -> Result<(), String> {
    tracing::debug!(node_id, title, "opening plugin editor via bridge");
    let (cmd_tx, reply_rx) = super::bridge::bridge_host::with_slot(node_id, |slot| {
        (slot.cmd_tx.clone(), slot.reply_rx.clone())
    })
    .ok_or_else(|| format!("{node_id}: no plugin is running on this node"))?;

    cmd_tx
        .send(super::bridge::protocol::HostCommand::OpenEditor {
            title: if title.is_empty() {
                "Plugin Editor".to_string()
            } else {
                title.to_string()
            },
        })
        .map_err(|e| format!("failed to send OpenEditor: {e}"))?;

    let rx = reply_rx.lock().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(3000);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(super::bridge::protocol::HelperEvent::EditorOpened { .. })
            | Ok(super::bridge::protocol::HelperEvent::Ok) => return Ok(()),
            Ok(super::bridge::protocol::HelperEvent::Error { message }) => return Err(message),
            Ok(_) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("helper process disconnected".to_string());
            }
        }
    }
    Err("timed out waiting for plugin editor to open".to_string())
}

/// Opens the plugin editor embedded in a native host window. The tested plugins
/// only support embedded GUIs, so the host must own the window and hand its
/// native handle to the plugin.
#[cfg(not(target_os = "windows"))]
pub fn open(node_id: &str, title: &str) -> Result<(), String> {
    tracing::debug!(node_id, title, "opening plugin editor");
    let app = crate::app_handle().ok_or("app handle not ready")?;
    if let Some(w) = windows().lock().unwrap().get(node_id) {
        let _ = w.set_focus();
        return Ok(());
    }

    let nid = node_id.to_string();
    let title = if title.is_empty() { "Plugin" } else { title }.to_string();
    let app_handle = app.clone();

    // The host window and its embedded plugin GUI must be created and parented on
    // the UI thread so their event handling and teardown belong to the same thread.
    let (window, size) =
        super::main_thread::run(move || -> Result<(tauri::Window, EditorSize), String> {
            let window = tauri::WindowBuilder::new(&app_handle, format!("plugin-editor-{nid}"))
            .title(&title)
            .inner_size(FALLBACK_EDITOR_SIZE.0 as f64, FALLBACK_EDITOR_SIZE.1 as f64)
            // Always resizable with a small floor: even when a plugin reports a bad
            // size or does not reflow, the user can enlarge the window to reveal it.
            .resizable(true)
            .min_inner_size(200.0, 150.0)
            .build()
            .map_err(|e| format!("editor window for {nid}: {e}"))?;

            let event_nid = nid.clone();
            window.on_window_event(move |ev| {
                // The plugin's view is a child of this window: tear the GUI down before
                // the window goes away, and tell the FE node its editor button is stale.
                if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
                    api.prevent_close();
                    if let Some(host) = super::registry::for_node(&event_nid) {
                        host.destroy_editor(&event_nid);
                    }
                    if let Some(w) = windows().lock().unwrap().remove(&event_nid) {
                        let _ = w.destroy();
                    }
                    if let Some(app) = crate::app_handle() {
                        let _ = app.emit(super::host_api::EDITOR_CLOSED_EVENT, &event_nid);
                    }
                }
            });

            let embedded = match super::registry::for_node(&nid) {
                Some(host) => host.embed_editor(&nid, &window),
                None => Err(format!("{nid}: no plugin is running on this node")),
            };

            // The window exists before the plugin view does, so a failed embed would
            // otherwise leave an empty one on screen and the caller none the wiser.
            let size = match embedded {
                Ok(size) => size,
                Err(e) => {
                    tracing::error!(nid, error = %e, "plugin editor embed failed");
                    let _ = window.close();
                    return Err(e);
                }
            };

            let (width, height) = valid_gui_size(size.0, size.1).unwrap_or(FALLBACK_EDITOR_SIZE);
            set_content_size(&window, width as f64, height as f64);

            Ok((window, size))
        })??;

    windows()
        .lock()
        .unwrap()
        .insert(node_id.to_string(), window.clone());

    let (width, height) = valid_gui_size(size.0, size.1).unwrap_or(FALLBACK_EDITOR_SIZE);
    tracing::debug!(node_id, width, height, "plugin editor embedded");

    Ok(())
}

/// Tears down the plugin editor and closes its native window.
#[cfg(target_os = "windows")]
pub fn close(node_id: &str) -> Result<(), String> {
    if let Some(host) = super::registry::for_node(node_id) {
        host.destroy_editor(node_id);
    }
    Ok(())
}

/// Tears down the plugin editor and closes its native window.
#[cfg(not(target_os = "windows"))]
pub fn close(node_id: &str) -> Result<(), String> {
    // The plugin's view is a child of this window, so it goes first -- and it
    // goes on the main thread, which is the one place AppKit and every format
    // agree on.
    let nid = node_id.to_string();
    let _ = super::main_thread::run(move || {
        if let Some(host) = super::registry::for_node(&nid) {
            host.destroy_editor(&nid);
        }
    });
    close_window(node_id);
    Ok(())
}

/// `NSViewWidthSizable` / `NSViewHeightSizable`: the plugin view follows the
/// editor window's content area instead of staying pinned at its initial size.
#[cfg(target_os = "macos")]
const NS_VIEW_WIDTH_SIZABLE: usize = 1 << 1;
#[cfg(target_os = "macos")]
const NS_VIEW_HEIGHT_SIZABLE: usize = 1 << 4;

/// Frames `view` into the content view it was just added to, leaving `titlebar`
/// px clear at the top.
///
/// The content view runs the full window height, under the title bar, so
/// filling its bounds outright would put the top of the editor behind the bar.
/// Starting at the bottom-left origin of an unflipped NSView and stopping short
/// of the top is the arrangement a plugin ends up in when it parents its own
/// view. Margins stay fixed by default, so the mask preserves that gap through
/// every later resize.
///
/// SAFETY: `parent` is a live window content view and `view` one of its
/// subviews; main thread only.
#[cfg(target_os = "macos")]
pub unsafe fn inset_below_titlebar(
    parent: *mut std::ffi::c_void,
    view: *mut objc2::runtime::AnyObject,
    titlebar: f64,
) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    unsafe {
        let bounds: NSRect = msg_send![parent as *mut AnyObject, bounds];
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(bounds.size.width, (bounds.size.height - titlebar).max(1.0)),
        );
        let _: () = msg_send![view, setFrame: frame];
        let _: () =
            msg_send![view, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
    }
}

/// The last subview of `parent`, which is the one a plugin just added.
///
/// SAFETY: `parent` is a live NSView; main thread only.
#[cfg(target_os = "macos")]
pub unsafe fn last_subview(
    parent: *mut std::ffi::c_void,
) -> Option<*mut objc2::runtime::AnyObject> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    unsafe {
        let subviews: *mut AnyObject = msg_send![parent as *mut AnyObject, subviews];
        let count: usize = msg_send![subviews, count];
        (count > 0).then(|| msg_send![subviews, objectAtIndex: count - 1])
    }
}
