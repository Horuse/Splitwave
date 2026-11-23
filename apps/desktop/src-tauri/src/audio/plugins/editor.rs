//! The native window a plugin editor is embedded into, and the geometry rules
//! around it. Nothing here knows which format it is hosting: the window is
//! ours, the view inside it is the plugin's, and `PluginHost::embed_editor` is
//! the only seam between them.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tauri::Emitter;

use super::host_api::EditorSize;

/// Fallback editor size for plugins that report a nonsensical one.
pub const FALLBACK_EDITOR_SIZE: EditorSize = (800, 600);

/// The native window a plugin view is parented into. A Tauri window on macOS
/// and Windows; on Linux a bare X11 one, since plugins there can only embed
/// into X11 and the app itself may be on native Wayland.
#[derive(Clone)]
pub struct EditorWindow {
    #[cfg(not(target_os = "linux"))]
    window: tauri::Window,
    #[cfg(target_os = "linux")]
    window: super::editor_x11::X11Window,
}

impl EditorWindow {
    #[cfg(target_os = "macos")]
    pub fn tauri(&self) -> &tauri::Window {
        &self.window
    }

    pub fn raw_handle(&self) -> Result<raw_window_handle::RawWindowHandle, String> {
        #[cfg(not(target_os = "linux"))]
        {
            use raw_window_handle::HasWindowHandle;
            self.window
                .window_handle()
                .map(|h| h.as_raw())
                .map_err(|e| format!("window handle: {e}"))
        }
        #[cfg(target_os = "linux")]
        {
            Ok(raw_window_handle::XlibWindowHandle::new(self.window.id() as _).into())
        }
    }

    pub fn scale_factor(&self) -> f64 {
        #[cfg(not(target_os = "linux"))]
        return self.window.scale_factor().unwrap_or(1.0);
        #[cfg(target_os = "linux")]
        return self.window.scale_factor();
    }

    /// Content area size in the unit plugins report: logical px through Tauri,
    /// X11 px on Linux.
    fn content_size(&self) -> Option<EditorSize> {
        #[cfg(not(target_os = "linux"))]
        {
            let inner = self.window.inner_size().ok()?;
            let scale = self.scale_factor();
            Some((
                (inner.width as f64 / scale).round() as u32,
                (inner.height as f64 / scale).round() as u32,
            ))
        }
        #[cfg(target_os = "linux")]
        return self.window.size();
    }

    fn set_size(&self, w: f64, h: f64) {
        #[cfg(not(target_os = "linux"))]
        let _ = self.window.set_size(tauri::LogicalSize::new(w, h));
        #[cfg(target_os = "linux")]
        self.window.set_size(w.round() as u32, h.round() as u32);
    }

    fn show(&self) {
        #[cfg(not(target_os = "linux"))]
        let _ = self.window.show();
        #[cfg(target_os = "linux")]
        self.window.show();
    }

    fn hide(&self) {
        #[cfg(not(target_os = "linux"))]
        let _ = self.window.hide();
        #[cfg(target_os = "linux")]
        self.window.hide();
    }

    fn focus(&self) {
        #[cfg(not(target_os = "linux"))]
        let _ = self.window.set_focus();
        #[cfg(target_os = "linux")]
        self.window.raise();
    }

    fn is_visible(&self) -> bool {
        #[cfg(not(target_os = "linux"))]
        return self.window.is_visible().unwrap_or(false);
        #[cfg(target_os = "linux")]
        return self.window.is_visible();
    }

    fn destroy(&self) {
        #[cfg(not(target_os = "linux"))]
        let _ = self.window.destroy();
        #[cfg(target_os = "linux")]
        self.window.destroy();
    }
}

/// Native host windows that plugin editors are embedded into, keyed by node id.
/// `EditorWindow` is `Send + Sync`, so this lives outside any main-thread state
/// and can be created/closed from the command thread.
fn windows() -> &'static Mutex<HashMap<String, EditorWindow>> {
    static WINDOWS: OnceLock<Mutex<HashMap<String, EditorWindow>>> = OnceLock::new();
    WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The window hosting this node's editor, if one is open.
pub fn window_for(node_id: &str) -> Option<EditorWindow> {
    windows().lock().unwrap().get(node_id).cloned()
}

/// Returns whether this node's editor window currently exists and is visible.
pub fn is_open(node_id: &str) -> bool {
    windows()
        .lock()
        .unwrap()
        .get(node_id)
        .is_some_and(EditorWindow::is_visible)
}

/// Closes a node's editor window if one is open. Shared with the format hosts,
/// which have to take the window down alongside the instance it belongs to.
pub fn close_window(node_id: &str) {
    if let Some(host) = super::registry::for_node(node_id) {
        host.destroy_editor(node_id);
    }
    if let Some(w) = windows().lock().unwrap().remove(node_id) {
        w.destroy();
    }
}

/// Rejects the degenerate sizes plugins report before their view exists (0x0)
/// or absurd values, so the window is never opened invisibly small or huge.
pub fn valid_gui_size(w: u32, h: u32) -> Option<EditorSize> {
    (w >= 100 && h >= 100 && w <= 8000 && h <= 8000).then_some((w, h))
}

/// Returns window decoration overhead. Because `window.set_size` in Tauri 2
/// sets the inner (client) size directly across all platforms, content area
/// matches the requested dimensions 1:1 without adding synthetic padding.
pub fn decoration_overhead(_window: &EditorWindow) -> (f64, f64) {
    (0.0, 0.0)
}

/// Sizes the window so its content area is `w` x `h`.
pub fn set_content_size(window: &EditorWindow, w: f64, h: f64) {
    window.set_size(w, h);
}

/// Requests a window resize from a plugin. If the window's content area already
/// matches the requested dimensions, this is a no-op, preventing redundant OS
/// resizing calls and feedback loops. When dimensions differ, the window is
/// resized synchronously on the main thread so plugin internal layout engines
/// (JUCE, VST3, AU) immediately see the updated parent bounds without frame
/// tearing or asynchronous jitter.
pub fn request_resize(window: &EditorWindow, _node_id: &str, w: u32, h: u32) {
    if window.content_size() == Some((w, h)) {
        return;
    }
    window.set_size(w as f64, h as f64);
}

const MIN_EDITOR_SIZE: EditorSize = (200, 150);

#[cfg(not(target_os = "linux"))]
fn create_window(node_id: &str, title: &str) -> Result<EditorWindow, String> {
    let app = crate::app_handle().ok_or("app handle not ready")?;
    let builder = tauri::WindowBuilder::new(app, format!("plugin-editor-{node_id}"))
        .title(title)
        .inner_size(FALLBACK_EDITOR_SIZE.0 as f64, FALLBACK_EDITOR_SIZE.1 as f64)
        .visible(false)
        // Always resizable with a small floor: even when a plugin reports a bad
        // size or does not reflow, the user can enlarge the window to reveal it.
        .resizable(true)
        .min_inner_size(MIN_EDITOR_SIZE.0 as f64, MIN_EDITOR_SIZE.1 as f64);

    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Visible);

    let window = builder
        .build()
        .map_err(|e| format!("editor window for {node_id}: {e}"))?;

    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        if let Ok(ns_window) = window.ns_window() {
            let nsw = ns_window as *mut AnyObject;
            let mut mask: usize = msg_send![nsw, styleMask];
            // Clear NSWindowStyleMaskFullSizeContentView (1 << 15 = 32768) so
            // the content view stays strictly below the titlebar and plugin
            // headers never overlap window controls.
            mask &= !(1 << 15);
            let _: () = msg_send![nsw, setStyleMask: mask];
            let _: () = msg_send![nsw, setTitlebarAppearsTransparent: false];
        }
    }

    let nid = node_id.to_string();
    window.on_window_event(move |ev| {
        // The user closed the window: keep the native view parented to avoid
        // COM/lifecycle destruction churn in plugins, just hide the window and
        // notify the FE node that its editor is closed.
        if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
            api.prevent_close();
            let nid = nid.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::audio::plugins::main_thread::run(move || {
                    let _ = close(&nid);
                });
            });
        }
    });
    Ok(EditorWindow { window })
}

#[cfg(target_os = "linux")]
fn create_window(node_id: &str, title: &str) -> Result<EditorWindow, String> {
    let window = super::editor_x11::X11Window::create(title, FALLBACK_EDITOR_SIZE, MIN_EDITOR_SIZE)
        .map_err(|e| format!("editor window for {node_id}: {e}"))?;
    Ok(EditorWindow { window })
}

/// Main thread, on the shared plugin tick. The user closing an X11 editor
/// window arrives as an event on our connection rather than through Tauri, so
/// it is turned into the same `close` the Tauri windows get.
#[cfg(target_os = "linux")]
pub fn tick() {
    for id in super::editor_x11::take_close_requests() {
        let node_id = windows()
            .lock()
            .unwrap()
            .iter()
            .find(|(_, w)| w.window.id() == id)
            .map(|(nid, _)| nid.clone());
        if let Some(node_id) = node_id {
            let _ = close(&node_id);
        }
    }
}

/// Opens the plugin editor embedded in a native host window. The tested plugins
/// only support embedded GUIs, so the host must own the window and hand its
/// native handle to the plugin.
pub fn open(node_id: &str, title: &str) -> Result<(), String> {
    tracing::debug!(node_id, title, "opening plugin editor");
    let existing = windows().lock().unwrap().get(node_id).cloned();
    if let Some(w) = existing {
        if let Some(host) = super::registry::for_node(node_id) {
            let _ = host.show_editor(node_id);
        }
        w.show();
        w.focus();
        return Ok(());
    }
    let window = create_window(node_id, if title.is_empty() { "Plugin" } else { title })?;
    windows()
        .lock()
        .unwrap()
        .insert(node_id.to_string(), window.clone());

    let embedded = match super::registry::for_node(node_id) {
        Some(host) => host.embed_editor(node_id, &window),
        None => Err(format!("{node_id}: no plugin is running on this node")),
    };

    // The window exists before the plugin view does, so a failed embed would
    // otherwise leave an empty one on screen and the caller none the wiser.
    let size = match embedded {
        Ok(size) => size,
        Err(e) => {
            tracing::error!(node_id, error = %e, "plugin editor embed failed");
            close_window(node_id);
            return Err(e);
        }
    };
    // Sanitised once, here, rather than by each host: a size is a size whoever
    // reported it.
    let (width, height) = valid_gui_size(size.0, size.1).unwrap_or(FALLBACK_EDITOR_SIZE);
    tracing::debug!(node_id, width, height, "plugin editor embedded");

    set_content_size(&window, width as f64, height as f64);

    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::msg_send;
        use objc2_foundation::{NSPoint, NSRect, NSSize};
        if let Ok(parent) = window.tauri().ns_view() {
            if let Some(view) = last_subview(parent) {
                let frame = NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(width as f64, height as f64),
                );
                let _: () = msg_send![view, setFrame: frame];
                let _: () = msg_send![view, setNeedsDisplay: true];
            }
        }
    }

    window.show();
    window.focus();
    Ok(())
}

/// Hides the plugin editor window (or tears it down if requested).
pub fn close(node_id: &str) -> Result<(), String> {
    if let Some(host) = super::registry::for_node(node_id) {
        let _ = host.hide_editor(node_id);
    }
    let existing = windows().lock().unwrap().get(node_id).cloned();
    if let Some(w) = existing {
        w.hide();
    }
    if let Some(app) = crate::app_handle() {
        let _ = app.emit(super::host_api::EDITOR_CLOSED_EVENT, node_id);
    }
    Ok(())
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
