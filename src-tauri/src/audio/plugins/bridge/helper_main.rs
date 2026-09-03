//! Standalone helper process runner for Out-of-Process VST3 plugins on Windows.

#![cfg(target_os = "windows")]

use std::ffi::c_void;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, UpdateWindow, HBRUSH, PAINTSTRUCT};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClassNameW, GetSystemMetrics, GetWindow, GetWindowLongPtrW, IsWindow, LoadCursorW,
    MoveWindow, PeekMessageW, PostThreadMessageW, RegisterClassExW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, CREATESTRUCTW, CS_DBLCLKS,
    CS_HREDRAW, CS_OWNDC, CS_VREDRAW, GWLP_USERDATA, GW_CHILD, GW_HWNDNEXT, IDC_ARROW, PM_REMOVE,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_HIDE, SW_SHOW,
    WINDOW_EX_STYLE, WM_CLOSE, WM_CREATE, WM_ERASEBKGND, WM_NCCREATE, WM_NCDESTROY, WM_PAINT,
    WM_SETFOCUS, WM_SIZE, WM_USER, WNDCLASSEXW, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    WS_OVERLAPPEDWINDOW,
};

use crate::audio::effects::Effect;
use crate::audio::plugins::bridge::protocol::{HelperEvent, HostCommand};
use crate::audio::plugins::bridge::shm_audio::ShmHelper;
use crate::audio::plugins::vst3_backend::Vst3Module;
use crate::audio::plugins::vst3_com::EditListener;
use crate::audio::plugins::vst3_editor::{EditorView, PlugFrame};
use crate::audio::plugins::vst3_host::Vst3Instance;
use crate::audio::plugins::vst3_node::Vst3Node;
use crate::audio::plugins::ParamRing;

use vst3::ComWrapper;
use vst3::Steinberg::{
    kPlatformTypeHWND, kResultOk, kResultTrue, IPlugFrame, IPlugViewTrait, ViewRect,
};

const WM_HOST_COMMAND: u32 = WM_USER + 101;
const CLASS_NAME: PCWSTR = w!("SplitwaveBridgePluginWindow");

struct BridgeWindowState {
    editor_view: Option<EditorView>,
    event_tx: Sender<HelperEvent>,
}

unsafe extern "system" fn bridge_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CREATE => DefWindowProcW(hwnd, msg, wparam, lparam),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_SETFOCUS => {
            let current = GetWindow(hwnd, GW_CHILD).ok();
            if let Some(child) = current {
                let _ = SetFocus(Some(child));
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BridgeWindowState;
            if !ptr.is_null() {
                let data = &mut *ptr;
                let w = (lparam.0 & 0xffff) as i32;
                let h = ((lparam.0 >> 16) & 0xffff) as i32;

                let mut current = GetWindow(hwnd, GW_CHILD).ok();
                while let Some(child) = current {
                    let _ = MoveWindow(child, 0, 0, w, h, true);
                    current = GetWindow(child, GW_HWNDNEXT).ok();
                }

                if let Some(ref mut editor) = data.editor_view {
                    let mut rect = ViewRect {
                        left: 0,
                        top: 0,
                        right: w.max(1),
                        bottom: h.max(1),
                    };
                    let _ = editor.view().onSize(&mut rect);
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BridgeWindowState;
            if !ptr.is_null() {
                let data = &*ptr;
                let _ = data.event_tx.send(HelperEvent::EditorClosed);
            }
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut BridgeWindowState;
            if !ptr.is_null() {
                let _ = Box::from_raw(ptr);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn register_window_class() -> Result<(), String> {
    unsafe {
        let h_instance = GetModuleHandleW(None).map_err(|e| format!("{e}"))?;
        let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_OWNDC | CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(bridge_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_instance.into(),
            hIcon: Default::default(),
            hCursor: cursor,
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: CLASS_NAME,
            hIconSm: Default::default(),
        };
        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            // Already registered or failed
        }
        Ok(())
    }
}

struct BridgeListener {
    tx: Sender<HelperEvent>,
    param_ring: Arc<ParamRing>,
}

impl EditListener for BridgeListener {
    fn param_edited(&self, id: u32, value: f64) {
        self.param_ring.push(id, value);
        let _ = self.tx.send(HelperEvent::ParamEdited { id, value });
    }

    fn restart(&self, _flags: i32) {}
}

/// Entry point executed when `--plugin-bridge <session_id>` is passed.
pub fn run_helper(session_id: &str) -> i32 {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let _ = register_window_class();

    // Force creation of Win32 message queue on this thread before other threads post messages
    unsafe {
        let mut dummy = std::mem::zeroed();
        let _ = PeekMessageW(&mut dummy, None, 0, 0, PM_REMOVE);
    }

    let main_thread_id = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };

    let (event_tx, event_rx) = channel::<HelperEvent>();
    let (cmd_tx, cmd_rx) = channel::<HostCommand>();

    // Stdout event writer thread
    thread::spawn(move || {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        while let Ok(event) = event_rx.recv() {
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = writeln!(handle, "{json}");
                let _ = handle.flush();
            }
        }
    });

    // Stdin command reader thread
    let cmd_tx_clone = cmd_tx.clone();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let reader = BufReader::new(stdin);
        for line in reader.lines() {
            match line {
                Ok(text) if !text.trim().is_empty() => {
                    if let Ok(cmd) = serde_json::from_str::<HostCommand>(&text) {
                        let is_shutdown = matches!(cmd, HostCommand::Shutdown);
                        let _ = cmd_tx_clone.send(cmd);
                        unsafe {
                            let _ = PostThreadMessageW(
                                main_thread_id,
                                WM_HOST_COMMAND,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                        if is_shutdown {
                            break;
                        }
                    }
                }
                _ => {
                    // Stdin EOF (host closed)
                    let _ = cmd_tx_clone.send(HostCommand::Shutdown);
                    unsafe {
                        let _ = PostThreadMessageW(
                            main_thread_id,
                            WM_HOST_COMMAND,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                    break;
                }
            }
        }
    });

    let mut instance: Option<Vst3Instance> = None;
    let param_ring = Arc::new(ParamRing::new());
    let mut window_hwnd: Option<HWND> = None;

    // Start DSP audio thread
    let dsp_session_id = session_id.to_string();
    let (dsp_node_tx, dsp_node_rx) = channel::<Vst3Node>();
    let node_sender = dsp_node_tx;

    thread::Builder::new()
        .name("bridge:dsp".to_string())
        .spawn(move || {
            let mut shm = match ShmHelper::open(&dsp_session_id) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(session = %dsp_session_id, error = %e, "bridge: ShmHelper::open failed");
                    return;
                }
            };

            let mut node: Option<Vst3Node> = None;
            let mut scratch = vec![0.0f32; 4096 * 8];

            while shm.is_alive() {
                // Check if a new node was provided
                while let Ok(new_node) = dsp_node_rx.try_recv() {
                    node = Some(new_node);
                }

                match shm.wait_for_input(20) {
                    Ok(Some((frames, channels))) => {
                        let total_samples = frames * channels;
                        if let Some(ref mut active_node) = node {
                            if scratch.len() < total_samples {
                                scratch.resize(total_samples, 0.0);
                            }
                            let input = shm.read_input(total_samples);
                            scratch[..total_samples].copy_from_slice(input);
                            active_node.process(&mut scratch[..total_samples], frames);
                            let latency = active_node.latency_frames();
                            shm.write_output_and_signal(&scratch[..total_samples], frames, latency);
                        } else {
                            // No node active yet, echo zeros
                            scratch[..total_samples].fill(0.0);
                            shm.write_output_and_signal(&scratch[..total_samples], frames, 0);
                        }
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        })
        .expect("failed to spawn bridge DSP thread");

    // Main UI + Command event loop
    unsafe {
        let mut msg = std::mem::zeroed();
        loop {
            let mut got_msg = false;
            // Process pending Win32 messages
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                got_msg = true;
                if msg.message == WM_HOST_COMMAND {
                    // Handled in command drain below
                } else {
                    let _ = TranslateMessage(&msg);
                    let _ = DispatchMessageW(&msg);
                }
            }

            let mut had_cmd = false;

            // Drain incoming host commands
            while let Ok(cmd) = cmd_rx.try_recv() {
                had_cmd = true;
                match cmd {
                    HostCommand::Init { path, plugin_id } => {
                        match Vst3Module::open(std::path::Path::new(&path)) {
                            Ok(module) => match Vst3Instance::new(module, &plugin_id) {
                                Ok(mut inst) => {
                                    inst.listen(Box::new(BridgeListener {
                                        tx: event_tx.clone(),
                                        param_ring: param_ring.clone(),
                                    }));
                                    let has_editor = inst.has_editor();
                                    let params = inst.params();
                                    instance = Some(inst);
                                    let _ =
                                        event_tx.send(HelperEvent::Loaded { params, has_editor });
                                }
                                Err(e) => {
                                    let _ = event_tx.send(HelperEvent::Error { message: e });
                                }
                            },
                            Err(e) => {
                                let _ = event_tx.send(HelperEvent::Error { message: e });
                            }
                        }
                    }
                    HostCommand::Activate {
                        sample_rate,
                        max_frames,
                        channels,
                        state,
                    } => {
                        if let Some(ref mut inst) = instance {
                            if let Some(ref blob) = state {
                                let _ = inst.restore_state(blob);
                            }
                            let alive = Arc::new(AtomicBool::new(true));
                            match inst.activate(
                                sample_rate,
                                max_frames,
                                channels,
                                param_ring.clone(),
                                alive,
                            ) {
                                Ok(node) => {
                                    let accepted_channels = node.channels();
                                    let latency_frames = node.latency_frames();
                                    let _ = node_sender.send(node);
                                    let _ = event_tx.send(HelperEvent::Activated {
                                        accepted_channels,
                                        latency_frames,
                                    });
                                }
                                Err(e) => {
                                    let _ = event_tx.send(HelperEvent::Error { message: e });
                                }
                            }
                        } else {
                            let _ = event_tx.send(HelperEvent::Error {
                                message: "instance not initialized".into(),
                            });
                        }
                    }
                    HostCommand::OpenEditor { title } => {
                        if let Some(hwnd) = window_hwnd {
                            if IsWindow(Some(hwnd)).as_bool() {
                                let _ = ShowWindow(hwnd, SW_SHOW);
                                let _ = SetForegroundWindow(hwnd);
                                let _ = event_tx.send(HelperEvent::Ok);
                                continue;
                            }
                        }

                        if let Some(ref mut inst) = instance {
                            let Some(view) = inst.take_view() else {
                                let _ = event_tx.send(HelperEvent::Error {
                                    message: "plugin has no editor".into(),
                                });
                                continue;
                            };

                            if view.isPlatformTypeSupported(kPlatformTypeHWND) != kResultTrue {
                                let _ = event_tx.send(HelperEvent::Error {
                                    message: "plugin editor does not support HWND".into(),
                                });
                                continue;
                            }

                            let mut rect = ViewRect {
                                left: 0,
                                top: 0,
                                right: 0,
                                bottom: 0,
                            };
                            if view.getSize(&mut rect) != kResultOk {
                                let _ = event_tx.send(HelperEvent::Error {
                                    message: "plugin editor reported no size".into(),
                                });
                                continue;
                            }

                            let width = (rect.right - rect.left).max(200);
                            let height = (rect.bottom - rect.top).max(150);

                            let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_CLIPSIBLINGS;
                            let mut win_rect = RECT {
                                left: 0,
                                top: 0,
                                right: width,
                                bottom: height,
                            };
                            let _ = AdjustWindowRectEx(
                                &mut win_rect,
                                style,
                                false,
                                WINDOW_EX_STYLE::default(),
                            );
                            let win_w = win_rect.right - win_rect.left;
                            let win_h = win_rect.bottom - win_rect.top;

                            let screen_w = GetSystemMetrics(SM_CXSCREEN);
                            let screen_h = GetSystemMetrics(SM_CYSCREEN);
                            let pos_x = ((screen_w - win_w) / 2).max(50);
                            let pos_y = ((screen_h - win_h) / 2).max(50);

                            let user_data = Box::new(BridgeWindowState {
                                editor_view: None,
                                event_tx: event_tx.clone(),
                            });
                            let user_data_ptr = Box::into_raw(user_data);

                            let title_hstring = HSTRING::from(&title);
                            let h_instance = GetModuleHandleW(None).unwrap_or_default();

                            let hwnd_res = CreateWindowExW(
                                WINDOW_EX_STYLE::default(),
                                CLASS_NAME,
                                PCWSTR(title_hstring.as_ptr()),
                                style,
                                pos_x,
                                pos_y,
                                win_w,
                                win_h,
                                None,
                                None,
                                Some(h_instance.into()),
                                Some(user_data_ptr as *mut _),
                            );

                            let hwnd = match hwnd_res {
                                Ok(h) => h,
                                Err(e) => {
                                    let _ = Box::from_raw(user_data_ptr);
                                    let _ = event_tx.send(HelperEvent::Error {
                                        message: format!("CreateWindowExW failed: {e}"),
                                    });
                                    continue;
                                }
                            };

                            let resize_hwnd_val = hwnd.0 as isize;
                            let resize_cb = Box::new(move |new_w: u32, new_h: u32| {
                                let resize_hwnd = HWND(resize_hwnd_val as *mut c_void);
                                let mut r = RECT {
                                    left: 0,
                                    top: 0,
                                    right: new_w as i32,
                                    bottom: new_h as i32,
                                };
                                let _ = AdjustWindowRectEx(
                                    &mut r,
                                    style,
                                    false,
                                    WINDOW_EX_STYLE::default(),
                                );
                                let _ = SetWindowPos(
                                    resize_hwnd,
                                    None,
                                    0,
                                    0,
                                    r.right - r.left,
                                    r.bottom - r.top,
                                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                                );
                            });

                            let frame = ComWrapper::new(PlugFrame::new(resize_cb));
                            let frame_ptr = frame.as_com_ref::<IPlugFrame>().map(|r| r.as_ptr());
                            if let Some(fptr) = frame_ptr {
                                view.setFrame(fptr);
                            }

                            if view.attached(hwnd.0 as *mut c_void, kPlatformTypeHWND) != kResultOk
                            {
                                let _ = DestroyWindow(hwnd);
                                let _ = event_tx.send(HelperEvent::Error {
                                    message: "plugin view refused to attach".into(),
                                });
                                continue;
                            }

                            let _ = view.onSize(&mut rect);

                            let mut current = GetWindow(hwnd, GW_CHILD).ok();
                            while let Some(child) = current {
                                let mut class_buf = [0u16; 128];
                                let len = GetClassNameW(child, &mut class_buf);
                                let _ = String::from_utf16_lossy(&class_buf[..len as usize]);
                                let _ = MoveWindow(child, 0, 0, width, height, true);
                                let _ = ShowWindow(child, SW_SHOW);
                                let _ = UpdateWindow(child);
                                current = GetWindow(child, GW_HWNDNEXT).ok();
                            }

                            (*user_data_ptr).editor_view =
                                Some(EditorView::from_raw_parts(view, frame));
                            window_hwnd = Some(hwnd);

                            let _ = ShowWindow(hwnd, SW_SHOW);
                            let _ = UpdateWindow(hwnd);
                            let _ = SetForegroundWindow(hwnd);

                            let _ = event_tx.send(HelperEvent::EditorOpened {
                                width: width as u32,
                                height: height as u32,
                            });
                        }
                    }
                    HostCommand::CloseEditor => {
                        if let Some(hwnd) = window_hwnd {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        }
                        let _ = event_tx.send(HelperEvent::Ok);
                    }
                    HostCommand::SetParam { id, value } => {
                        if let Some(ref inst) = instance {
                            inst.set_param(id, value);
                            param_ring.push(id, value);
                        }
                    }
                    HostCommand::GetParams => {
                        if let Some(ref inst) = instance {
                            let _ = event_tx.send(HelperEvent::ParamsList {
                                params: inst.params(),
                            });
                        }
                    }
                    HostCommand::SaveState => {
                        let blob = instance.as_ref().and_then(|inst| inst.save_state());
                        let _ = event_tx.send(HelperEvent::StateSaved { blob });
                    }
                    HostCommand::RestoreState { blob } => {
                        if let Some(ref inst) = instance {
                            let _ = inst.restore_state(&blob);
                        }
                        let _ = event_tx.send(HelperEvent::Ok);
                    }
                    HostCommand::Shutdown => {
                        if let Some(hwnd) = window_hwnd {
                            let _ = DestroyWindow(hwnd);
                        }
                        drop(instance);
                        return 0;
                    }
                }
            }

            if !got_msg && !had_cmd {
                windows::Win32::System::Threading::Sleep(5);
            }
        }
    }
}
