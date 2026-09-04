//! Embedding a VST3 plugin's own window into one of ours.
//!
//! Unlike AU, VST3 has a standard way for a plugin to ask the host to resize:
//! `IPlugFrame::resizeView`. Implementing it is what makes a plugin's own zoom
//! controls work.

#![allow(non_snake_case)]

use std::ffi::c_void;

use vst3::Steinberg::{
    kResultOk, kResultTrue, tresult, FIDString, IPlugFrame, IPlugFrameTrait, IPlugView,
    IPlugViewTrait, ViewRect,
};
use vst3::{Class, ComPtr, ComWrapper};

#[cfg(target_os = "macos")]
use crate::audio::plugins::editor;
use crate::audio::plugins::host_api::EditorSize;
#[cfg(target_os = "linux")]
use crate::audio::plugins::vst3_runloop;

/// The window system the plugin is asked to render into. One per OS: a plugin
/// that does not support its platform's own type has no embeddable editor.
pub fn platform_type() -> FIDString {
    #[cfg(target_os = "macos")]
    return vst3::Steinberg::kPlatformTypeNSView;
    #[cfg(target_os = "windows")]
    return vst3::Steinberg::kPlatformTypeHWND;
    #[cfg(target_os = "linux")]
    return vst3::Steinberg::kPlatformTypeX11EmbedWindowID;
}

/// Called when the plugin wants its window a different size.
pub type ResizeRequest = Box<dyn Fn(u32, u32) + Send + Sync>;

/// The host side of the resize conversation. The plugin holds a borrowed
/// reference to it, so it lives as long as the view does.
pub struct PlugFrame {
    resize: ResizeRequest,
}

/// On X11 the plugin drives its editor from the host's event loop, so the frame
/// it is given must also answer as `IRunLoop`.
#[cfg(target_os = "linux")]
impl Class for PlugFrame {
    type Interfaces = (IPlugFrame, vst3::Steinberg::Linux::IRunLoop);
}

#[cfg(not(target_os = "linux"))]
impl Class for PlugFrame {
    type Interfaces = (IPlugFrame,);
}

#[cfg(target_os = "linux")]
impl vst3::Steinberg::Linux::IRunLoopTrait for PlugFrame {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut vst3::Steinberg::Linux::IEventHandler,
        fd: vst3::Steinberg::Linux::FileDescriptor,
    ) -> tresult {
        vst3_runloop::register_event_handler(handler, fd)
    }

    unsafe fn unregisterEventHandler(
        &self,
        handler: *mut vst3::Steinberg::Linux::IEventHandler,
    ) -> tresult {
        vst3_runloop::unregister_event_handler(handler)
    }

    unsafe fn registerTimer(
        &self,
        handler: *mut vst3::Steinberg::Linux::ITimerHandler,
        milliseconds: vst3::Steinberg::Linux::TimerInterval,
    ) -> tresult {
        vst3_runloop::register_timer(handler, milliseconds)
    }

    unsafe fn unregisterTimer(
        &self,
        handler: *mut vst3::Steinberg::Linux::ITimerHandler,
    ) -> tresult {
        vst3_runloop::unregister_timer(handler)
    }
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, newSize: *mut ViewRect) -> tresult {
        if view.is_null() || newSize.is_null() {
            return vst3::Steinberg::kInvalidArgument;
        }
        let rect = &*newSize;
        let (width, height) = (
            (rect.right - rect.left).max(1) as u32,
            (rect.bottom - rect.top).max(1) as u32,
        );

        // Resize our window first: the plugin measures its own view against the
        // parent right after this returns.
        (self.resize)(width, height);

        let view = vst3::ComRef::from_raw(view).expect("checked above");
        view.onSize(newSize)
    }
}

/// A plugin editor attached to one of our windows.
pub struct EditorView {
    view: ComPtr<IPlugView>,
    /// Borrowed by the plugin for the life of the view.
    _frame: ComWrapper<PlugFrame>,
}

impl EditorView {
    /// Builds the plugin's view into `parent` -- an `NSView` on macOS, an `HWND`
    /// on Windows, an X11 window id on Linux -- and returns the size the plugin
    /// asked for.
    ///
    /// Must run on the main thread.
    pub fn attach(
        view: ComPtr<IPlugView>,
        parent: *mut c_void,
        titlebar: f64,
        resize: ResizeRequest,
    ) -> Result<(Self, EditorSize), String> {
        // SAFETY: main thread, and `parent` is a live window handle owned by a
        // window that outlives this editor.
        unsafe {
            let platform = platform_type();
            if view.isPlatformTypeSupported(platform) != kResultTrue {
                return Err(format!(
                    "editor does not support {}",
                    std::ffi::CStr::from_ptr(platform).to_string_lossy()
                ));
            }

            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if view.getSize(&mut rect) != kResultOk {
                return Err("editor reported no size".into());
            }

            #[cfg(target_os = "macos")]
            {
                use objc2::msg_send;
                use objc2::runtime::AnyObject;
                let parent_obj = parent as *mut AnyObject;
                let _: () = msg_send![parent_obj, setWantsLayer: true];
            }

            let frame = ComWrapper::new(PlugFrame { resize });
            let frame_ptr = frame
                .as_com_ref::<IPlugFrame>()
                .map(|r| r.as_ptr())
                .ok_or("PlugFrame implements IPlugFrame")?;
            // Before `attached`, so a plugin that resizes on open has somewhere
            // to send the request.
            view.setFrame(frame_ptr);

            if view.attached(parent, platform) != kResultOk {
                view.setFrame(std::ptr::null_mut());
                return Err("editor refused to attach to the window".into());
            }

            // Inform the view of its initial size so it initializes layout and rendering.
            let _ = view.onSize(&mut rect);

            // Notify the view that it has focus so timers and render loops start.
            let _ = view.onFocus(1);

            #[cfg(target_os = "macos")]
            {
                use objc2::msg_send;
                use objc2::runtime::AnyObject;
                let parent_obj = parent as *mut AnyObject;
                let _: () = msg_send![parent_obj, setNeedsDisplay: true];
                if let Some(view) = editor::last_subview(parent) {
                    let _: () = msg_send![view, setNeedsDisplay: true];
                }
            }
            let _ = titlebar;

            let size = (
                (rect.right - rect.left).max(1) as u32,
                (rect.bottom - rect.top).max(1) as u32,
            );
            Ok((
                Self {
                    view,
                    _frame: frame,
                },
                size,
            ))
        }
    }

    pub fn on_focus(&self, state: bool) {
        unsafe {
            let _ = self.view.onFocus(if state { 1 } else { 0 });
        }
    }

    pub fn on_size(&self, width: u32, height: u32) {
        unsafe {
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            let _ = self.view.onSize(&mut rect);
        }
    }
}

impl Drop for EditorView {
    fn drop(&mut self) {
        // Order matters: the plugin must let go of the parent view before the
        // window closes, and of our frame before it is freed.
        unsafe {
            let _ = self.view.removed();
            let _ = self.view.setFrame(std::ptr::null_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::plugins::vst3_backend::{Vst3Backend, Vst3Module};
    use crate::audio::plugins::vst3_host::Vst3Instance;
    use crate::audio::plugins::PluginBackend;
    use vst3::Steinberg::Vst::IEditControllerTrait;
    use vst3::Steinberg::Vst::ViewType::kEditor;

    fn skipped(what: &str) {
        println!("SKIPPED: no vst3 plugins installed, cannot check {what}");
    }

    #[test]
    fn every_installed_plugin_offers_an_embeddable_editor() {
        let installed = Vst3Backend.scan();
        if installed.is_empty() {
            return skipped("editor creation");
        }
        for plugin in installed {
            let module = Vst3Module::open(std::path::Path::new(&plugin.path)).unwrap();
            let instance = Vst3Instance::new(module, &plugin.plugin_id).unwrap();
            unsafe {
                let raw = instance.controller.createView(kEditor);
                let Some(view) = ComPtr::from_raw(raw) else {
                    println!("{}: no editor", plugin.name);
                    continue;
                };
                let platform = platform_type();
                let supported = view.isPlatformTypeSupported(platform);
                let mut rect = ViewRect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                let sized = view.getSize(&mut rect);
                let platform_name = std::ffi::CStr::from_ptr(platform).to_string_lossy();
                println!(
                    "{}: {platform_name} {}, size {}x{}",
                    plugin.name,
                    supported == kResultTrue,
                    rect.right - rect.left,
                    rect.bottom - rect.top
                );
                assert_eq!(
                    supported, kResultTrue,
                    "{} cannot host in a {platform_name}",
                    plugin.name
                );
                assert_eq!(sized, kResultOk, "{} reported no size", plugin.name);
            }
        }
    }

    /// A plugin asking to resize must reach our window, and the plugin must be
    /// told the size it ended up with. This is the path AU has no equivalent of.
    #[test]
    fn a_resize_request_reaches_the_host_and_returns_to_the_plugin() {
        let Some(plugin) = Vst3Backend.scan().into_iter().next() else {
            return skipped("editor resize requests");
        };
        let module = Vst3Module::open(std::path::Path::new(&plugin.path)).unwrap();
        let instance = Vst3Instance::new(module, &plugin.plugin_id).unwrap();

        let asked = std::sync::Arc::new(std::sync::Mutex::new(None));
        let seen = asked.clone();
        let frame = ComWrapper::new(PlugFrame {
            resize: Box::new(move |w, h| *seen.lock().unwrap() = Some((w, h))),
        });

        unsafe {
            let view = ComPtr::from_raw(instance.controller.createView(kEditor)).unwrap();
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: 640,
                bottom: 480,
            };
            let f = frame.to_com_ptr::<IPlugFrame>().unwrap();
            assert_eq!(f.resizeView(view.as_ptr(), &mut rect), kResultOk);
        }
        assert_eq!(*asked.lock().unwrap(), Some((640, 480)));
    }
}
