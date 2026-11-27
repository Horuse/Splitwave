//! Tray icon and the webview panel it opens.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
#[cfg(not(target_os = "linux"))]
use std::time::Duration;
use std::time::Instant;

use tauri::image::Image;
use tauri::{
    AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent,
};
use tracing::error;

const TRAY_ID: &str = "main-tray";
const MAIN_WINDOW: &str = "main";
const PANEL_WINDOW: &str = "tray";
const PANEL_WIDTH: f64 = 320.0;
const PANEL_HEIGHT: f64 = 420.0;
#[cfg(not(target_os = "linux"))]
const PANEL_GAP: f64 = 6.0;
/// Pressing the tray icon blurs the open panel before the click arrives; a click
/// this soon after the blur means "close", not "reopen".
#[cfg(not(target_os = "linux"))]
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// What the tray icon says about the engine.
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayStatus {
    Idle,
    Running,
    /// Running without a device, app or file the pipeline asks for.
    Warning,
}

impl TrayStatus {
    fn load(cell: &AtomicU8) -> Self {
        match cell.load(Ordering::Relaxed) {
            value if value == Self::Running as u8 => Self::Running,
            value if value == Self::Warning as u8 => Self::Warning,
            _ => Self::Idle,
        }
    }
}

pub struct TrayState {
    close_to_tray: AtomicBool,
    available: AtomicBool,
    status: AtomicU8,
    blurred_at: Mutex<Option<Instant>>,
}

impl Default for TrayState {
    fn default() -> Self {
        Self {
            close_to_tray: AtomicBool::new(true),
            available: AtomicBool::new(false),
            status: AtomicU8::new(TrayStatus::Idle as u8),
            blurred_at: Mutex::new(None),
        }
    }
}

pub fn init(app: &AppHandle) {
    match create(app) {
        Ok(()) => app
            .state::<TrayState>()
            .available
            .store(true, Ordering::Relaxed),
        Err(error) => error!(%error, "failed to create the tray icon"),
    }
}

fn create(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    ensure_appindicator()?;
    create_panel(app).map_err(|e| e.to_string())?;
    create_icon(app).map_err(|e| e.to_string())
}

/// The appindicator bindings panic when the library is absent, so probe first.
#[cfg(target_os = "linux")]
fn ensure_appindicator() -> Result<(), String> {
    let found = [
        c"libayatana-appindicator3.so.1",
        c"libappindicator3.so.1",
    ]
    .iter()
    // SAFETY: the name is NUL-terminated; the handle is deliberately kept open.
    .any(|name| !unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_LAZY) }.is_null());
    if found {
        Ok(())
    } else {
        Err("libayatana-appindicator3 is not installed".into())
    }
}

fn create_panel(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, PANEL_WINDOW, WebviewUrl::App("tray".into()))
        .title("Splitwave")
        .inner_size(PANEL_WIDTH, PANEL_HEIGHT)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        // The page draws the rounded shape; the window around it stays clear.
        .transparent(true)
        // Elsewhere than macOS the shadow outlines the square window.
        .shadow(cfg!(target_os = "macos"))
        .visible(false)
        .build()?;
    Ok(())
}

fn create_icon(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = tauri::tray::TrayIconBuilder::with_id(TRAY_ID).tooltip("Splitwave");
    builder = builder
        .icon(tray_image(TrayStatus::Idle))
        // macOS tints a template image to match the menu bar.
        .icon_as_template(cfg!(target_os = "macos"));

    // Linux reports no tray clicks, so the panel opens from a menu entry there.
    #[cfg(target_os = "linux")]
    {
        use tauri::menu::{MenuBuilder, MenuItemBuilder};
        let open = MenuItemBuilder::with_id("tray_open_panel", "Open panel").build(app)?;
        let quit = MenuItemBuilder::with_id("tray_quit", "Quit Splitwave").build(app)?;
        let menu = MenuBuilder::new(app)
            .item(&open)
            .separator()
            .item(&quit)
            .build()?;
        builder = builder
            .menu(&menu)
            .on_menu_event(|app, event| match event.id().0.as_str() {
                "tray_open_panel" => open_panel_centered(app),
                "tray_quit" => app.exit(0),
                _ => {}
            });
    }

    #[cfg(not(target_os = "linux"))]
    {
        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
        builder = builder.on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left | MouseButton::Right,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_panel(tray.app_handle(), rect);
            }
        });
    }

    builder.build(app)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn tray_image(status: TrayStatus) -> Image<'static> {
    match status {
        TrayStatus::Idle => tauri::include_image!("icons/tray/menubar.png"),
        TrayStatus::Running => tauri::include_image!("icons/tray/menubar-running.png"),
        TrayStatus::Warning => tauri::include_image!("icons/tray/menubar-warning.png"),
    }
}

#[cfg(target_os = "linux")]
fn tray_image(status: TrayStatus) -> Image<'static> {
    match status {
        TrayStatus::Idle => tauri::include_image!("icons/tray/white.png"),
        TrayStatus::Running => tauri::include_image!("icons/tray/white-running.png"),
        TrayStatus::Warning => tauri::include_image!("icons/tray/white-warning.png"),
    }
}

#[cfg(target_os = "windows")]
fn tray_image(status: TrayStatus) -> Image<'static> {
    match (windows_light_taskbar(), status) {
        (true, TrayStatus::Idle) => tauri::include_image!("icons/tray/black.png"),
        (true, TrayStatus::Running) => tauri::include_image!("icons/tray/black-running.png"),
        (true, TrayStatus::Warning) => tauri::include_image!("icons/tray/black-warning.png"),
        (false, TrayStatus::Idle) => tauri::include_image!("icons/tray/white.png"),
        (false, TrayStatus::Running) => tauri::include_image!("icons/tray/white-running.png"),
        (false, TrayStatus::Warning) => tauri::include_image!("icons/tray/white-warning.png"),
    }
}

fn refresh_icon(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let status = TrayStatus::load(&app.state::<TrayState>().status);
    report(tray.set_icon_with_as_template(Some(tray_image(status)), cfg!(target_os = "macos")));
}

/// The taskbar has its own theme, separate from the one apps get.
#[cfg(target_os = "windows")]
fn windows_light_taskbar() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("SystemUsesLightTheme"))
        .map_or_else(
            |error| {
                error!(%error, "failed to read the taskbar theme");
                false
            },
            |light| light != 0,
        )
}

#[cfg(not(target_os = "linux"))]
fn toggle_panel(app: &AppHandle, icon: tauri::Rect) {
    let Some(panel) = app.get_webview_window(PANEL_WINDOW) else {
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        report(panel.hide());
        return;
    }
    let blurred = app
        .state::<TrayState>()
        .blurred_at
        .lock()
        .ok()
        .and_then(|at| *at);
    if blurred.is_some_and(|at| at.elapsed() < REOPEN_GUARD) {
        return;
    }
    report(place_by_icon(&panel, icon));
    show_panel(&panel);
}

/// Below the icon for a top bar, above it for a bottom taskbar, kept inside the
/// monitor's work area.
#[cfg(not(target_os = "linux"))]
fn place_by_icon(panel: &WebviewWindow, icon: tauri::Rect) -> tauri::Result<()> {
    let scale = panel.scale_factor()?;
    let position = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    let center_x = position.x + size.width / 2.0;
    let center_y = position.y + size.height / 2.0;

    let monitor = panel.available_monitors()?.into_iter().find(|m| {
        let origin = m.position();
        let extent = m.size();
        center_x >= f64::from(origin.x)
            && center_x < f64::from(origin.x) + f64::from(extent.width)
            && center_y >= f64::from(origin.y)
            && center_y < f64::from(origin.y) + f64::from(extent.height)
    });
    let Some(monitor) = monitor else {
        return panel.center();
    };

    let factor = monitor.scale_factor();
    let width = PANEL_WIDTH * factor;
    let height = PANEL_HEIGHT * factor;
    let gap = PANEL_GAP * factor;
    let area = monitor.work_area();
    let left = f64::from(area.position.x);
    let top = f64::from(area.position.y);
    let right = left + f64::from(area.size.width);
    let bottom = top + f64::from(area.size.height);
    let upper_half =
        center_y < f64::from(monitor.position().y) + f64::from(monitor.size().height) / 2.0;
    let y = if upper_half {
        position.y + size.height + gap
    } else {
        position.y - height - gap
    };
    let x = (center_x - width / 2.0)
        .min(right - width - gap)
        .max(left + gap);
    let y = y.min(bottom - height - gap).max(top + gap);
    panel.set_position(tauri::PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    ))
}

#[cfg(target_os = "linux")]
fn open_panel_centered(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL_WINDOW) {
        report(panel.center());
        show_panel(&panel);
    }
}

fn show_panel(panel: &WebviewWindow) {
    report(panel.show());
    report(panel.set_focus());
}

pub fn show_main(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL_WINDOW) {
        report(panel.hide());
    }
    if let Some(main) = app.get_webview_window(MAIN_WINDOW) {
        report(main.show());
        report(main.unminimize());
        report(main.set_focus());
    }
}

fn report(result: tauri::Result<()>) {
    if let Err(error) = result {
        error!(%error, "tray window operation failed");
    }
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let app = window.app_handle();
    let state = app.state::<TrayState>();
    match (window.label(), event) {
        (MAIN_WINDOW, WindowEvent::CloseRequested { api, .. }) => {
            // Hiding without a tray icon would leave no way back to the window.
            if state.available.load(Ordering::Relaxed)
                && state.close_to_tray.load(Ordering::Relaxed)
            {
                api.prevent_close();
                report(window.hide());
            } else {
                // The hidden panel window would otherwise keep the process alive.
                app.exit(0);
            }
        }
        (PANEL_WINDOW, WindowEvent::Focused(false)) => {
            if let Ok(mut at) = state.blurred_at.lock() {
                *at = Some(Instant::now());
            }
            report(window.hide());
        }
        #[cfg(target_os = "windows")]
        (MAIN_WINDOW, WindowEvent::ThemeChanged(_)) => {
            refresh_icon(app);
        }
        _ => {}
    }
}

#[tauri::command]
pub fn set_close_to_tray(enabled: bool, state: State<'_, TrayState>) {
    state.close_to_tray.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
pub fn set_tray_status(status: TrayStatus, app: AppHandle, state: State<'_, TrayState>) {
    if state.status.swap(status as u8, Ordering::Relaxed) != status as u8 {
        refresh_icon(&app);
    }
}

#[tauri::command]
pub fn tray_available(state: State<'_, TrayState>) -> bool {
    state.available.load(Ordering::Relaxed)
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    show_main(&app);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
