//! Plugin editor windows on Linux, created straight on the X server.
//!
//! VST3 and CLAP editors can only be parented into an X11 window, but the app
//! itself may run on native Wayland, where GTK has no X11 window to give. So
//! editor windows bypass GTK and live on their own connection to `$DISPLAY`,
//! which under a Wayland session is XWayland. The rest of the UI stays native.
//!
//! Sizes here are X11 pixels, the unit plugins use on this platform.

use std::sync::OnceLock;

use x11rb::connection::Connection;
use x11rb::properties::WmSizeHints;
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, MapState,
    PropMode, StackMode, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        _NET_WM_NAME,
        UTF8_STRING,
    }
}

struct Display {
    conn: RustConnection,
    root: u32,
    black_pixel: u32,
    atoms: Atoms,
}

/// X11's own baseline DPI, which `Xft.dpi` is a multiple of when the desktop
/// asks X11 clients to scale themselves.
const BASE_DPI: f64 = 96.0;

fn connect() -> Result<Display, String> {
    let (conn, screen_num) = RustConnection::connect(None)
        .map_err(|e| format!("plugin editors need an X11 display (XWayland under Wayland): {e}"))?;
    let screen = &conn.setup().roots[screen_num];
    let (root, black_pixel) = (screen.root, screen.black_pixel);
    let atoms = Atoms::new(&conn)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    Ok(Display {
        conn,
        root,
        black_pixel,
        atoms,
    })
}

/// One connection for the whole process, opened by the first editor. `$DISPLAY`
/// does not change under a running app, so a failure is final too.
static DISPLAY: OnceLock<Result<Display, String>> = OnceLock::new();

fn display() -> Result<&'static Display, String> {
    DISPLAY.get_or_init(connect).as_ref().map_err(Clone::clone)
}

#[derive(Clone, Copy, Debug)]
pub struct X11Window {
    id: u32,
}

impl X11Window {
    pub fn create(
        title: &str,
        (w, h): (u32, u32),
        (min_w, min_h): (u32, u32),
    ) -> Result<Self, String> {
        let d = display()?;
        let err = |e: &dyn std::fmt::Display| format!("x11 editor window: {e}");
        let id = d.conn.generate_id().map_err(|e| err(&e))?;
        d.conn
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                id,
                d.root,
                0,
                0,
                w as u16,
                h as u16,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::NONE,
                &CreateWindowAux::new()
                    .background_pixel(d.black_pixel)
                    .event_mask(EventMask::STRUCTURE_NOTIFY),
            )
            .map_err(|e| err(&e))?;
        d.conn
            .change_property32(
                PropMode::REPLACE,
                id,
                d.atoms.WM_PROTOCOLS,
                AtomEnum::ATOM,
                &[d.atoms.WM_DELETE_WINDOW],
            )
            .map_err(|e| err(&e))?;
        d.conn
            .change_property8(
                PropMode::REPLACE,
                id,
                d.atoms._NET_WM_NAME,
                d.atoms.UTF8_STRING,
                title.as_bytes(),
            )
            .map_err(|e| err(&e))?;
        let hints = WmSizeHints {
            min_size: Some((min_w as i32, min_h as i32)),
            ..Default::default()
        };
        hints.set_normal_hints(&d.conn, id).map_err(|e| err(&e))?;
        // The plugin reparents into this window over its own connection, so the
        // window has to exist on the server before its id is handed out.
        d.conn.sync().map_err(|e| err(&e))?;
        Ok(Self { id })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    /// `Xft.dpi / 96`. A desktop that scales X11 clients itself leaves it at 96,
    /// so the plugin draws at 1x; one that wants clients to scale raises it.
    pub fn scale_factor(&self) -> f64 {
        let Ok(d) = display() else { return 1.0 };
        let dpi = d
            .conn
            .get_property(
                false,
                d.root,
                AtomEnum::RESOURCE_MANAGER,
                AtomEnum::STRING,
                0,
                u32::MAX,
            )
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| xft_dpi(&String::from_utf8_lossy(&r.value)));
        dpi.map_or(1.0, |dpi| dpi / BASE_DPI)
    }

    pub fn size(&self) -> Option<(u32, u32)> {
        let d = display().ok()?;
        let g = d.conn.get_geometry(self.id).ok()?.reply().ok()?;
        Some((g.width as u32, g.height as u32))
    }

    pub fn set_size(&self, w: u32, h: u32) {
        self.request(|d| {
            d.conn
                .configure_window(self.id, &ConfigureWindowAux::new().width(w).height(h))
                .map(drop)
        });
    }

    pub fn show(&self) {
        self.request(|d| d.conn.map_window(self.id).map(drop));
    }

    pub fn hide(&self) {
        self.request(|d| d.conn.unmap_window(self.id).map(drop));
    }

    pub fn raise(&self) {
        self.request(|d| {
            d.conn
                .configure_window(
                    self.id,
                    &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
                )
                .map(drop)
        });
    }

    pub fn is_visible(&self) -> bool {
        display()
            .ok()
            .and_then(|d| d.conn.get_window_attributes(self.id).ok())
            .and_then(|c| c.reply().ok())
            .is_some_and(|a| a.map_state != MapState::UNMAPPED)
    }

    pub fn destroy(&self) {
        self.request(|d| d.conn.destroy_window(self.id).map(drop));
    }

    fn request(&self, f: impl FnOnce(&Display) -> Result<(), x11rb::errors::ConnectionError>) {
        let Ok(d) = display() else { return };
        if let Err(e) = f(d).and_then(|()| d.conn.flush()) {
            tracing::warn!(window = self.id, error = %e, "x11 editor window request failed");
        }
    }
}

/// Drains pending events without blocking, returning the windows the user asked
/// to close. Main thread, on the shared plugin tick. No-op until an editor has
/// opened the connection.
pub fn take_close_requests() -> Vec<u32> {
    let Some(Ok(d)) = DISPLAY.get() else {
        return Vec::new();
    };
    let mut closed = Vec::new();
    while let Ok(Some(event)) = d.conn.poll_for_event() {
        if let Event::ClientMessage(msg) = event {
            if msg.type_ == d.atoms.WM_PROTOCOLS
                && msg.data.as_data32()[0] == d.atoms.WM_DELETE_WINDOW
            {
                closed.push(msg.window);
            }
        }
    }
    closed
}

fn xft_dpi(resources: &str) -> Option<f64> {
    resources.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "Xft.dpi").then(|| value.trim().parse().ok())?
    })
}

#[cfg(test)]
mod tests {
    use super::xft_dpi;

    #[test]
    fn reads_xft_dpi_among_other_resources() {
        assert_eq!(xft_dpi("Xft.antialias:\t1\nXft.dpi:\t192\n"), Some(192.0));
    }

    #[test]
    fn no_xft_dpi_means_none() {
        assert_eq!(xft_dpi("Xcursor.size:\t24\n"), None);
        assert_eq!(xft_dpi(""), None);
    }
}
