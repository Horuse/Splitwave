use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, MAX_PATH, S_OK};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    MMDeviceEnumerator,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

use super::AudioApplication;
use crate::error::{AppError, AppResult};

fn path_cache() -> &'static Mutex<HashMap<String, String>> {
    static C: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn icon_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ensure_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    unsafe { enumerate() }.map_err(|e| AppError::Host(format!("wasapi sessions: {e}")))
}

unsafe fn enumerate() -> windows::core::Result<Vec<AudioApplication>> {
    ensure_com();
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
    let sessions = manager.GetSessionEnumerator()?;

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for i in 0..sessions.GetCount()? {
        let ctrl2: IAudioSessionControl2 = sessions.GetSession(i)?.cast()?;
        // S_OK means this is the system-sounds session; skip it.
        if ctrl2.IsSystemSoundsSession() == S_OK {
            continue;
        }
        let pid = ctrl2.GetProcessId()?;
        if pid == 0 {
            continue;
        }
        let Some(path) = process_exe_path(pid) else {
            continue;
        };
        let exe = base_name(&path);
        if !seen.insert(exe.clone()) {
            continue;
        }
        path_cache().lock().unwrap().insert(exe.clone(), path);
        let name = exe.strip_suffix(".exe").unwrap_or(&exe).to_string();
        out.push(AudioApplication {
            bundle_id: exe,
            name,
            icon: None,
        });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

// Resolve a live PID for an app the user picked by exe name. Process loopback
// targets a PID, but the saved pipeline carries the stable exe name.
pub fn pid_for_exe(target: &str) -> Option<u32> {
    unsafe {
        ensure_com();
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None).ok()?;
        let sessions = manager.GetSessionEnumerator().ok()?;
        for i in 0..sessions.GetCount().ok()? {
            let Ok(ctrl2) = sessions.GetSession(i).and_then(|c| c.cast::<IAudioSessionControl2>())
            else {
                continue;
            };
            let pid = ctrl2.GetProcessId().ok()?;
            if pid != 0
                && process_exe_path(pid).map(|p| base_name(&p)).as_deref() == Some(target)
            {
                return Some(pid);
            }
        }
        None
    }
}

pub fn load_app_icons(bundle_ids: Vec<String>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut to_load: Vec<(String, String)> = Vec::new();
    {
        let paths = path_cache().lock().unwrap();
        let icons = icon_cache().lock().unwrap();
        for id in &bundle_ids {
            if let Some(cached) = icons.get(id) {
                if let Some(png) = cached {
                    result.insert(id.clone(), png.clone());
                }
                continue;
            }
            if let Some(path) = paths.get(id) {
                to_load.push((id.clone(), path.clone()));
            }
        }
    }

    for (id, path) in to_load {
        let icon = unsafe { icon_png_base64(&path) };
        if let Some(ref png) = icon {
            result.insert(id.clone(), png.clone());
        }
        icon_cache().lock().unwrap().insert(id, icon);
    }
    result
}

unsafe fn process_exe_path(pid: u32) -> Option<String> {
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; MAX_PATH as usize];
    let mut len = buf.len() as u32;
    let res =
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
    let _ = CloseHandle(handle);
    res.ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

fn base_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

unsafe fn icon_png_base64(path: &str) -> Option<String> {
    let (w, h, rgba) = extract_icon_rgba(path)?;
    let png = encode_png(w, h, &rgba)?;
    Some(STANDARD.encode(&png))
}

unsafe fn extract_icon_rgba(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sfi = SHFILEINFOW::default();
    let ok = SHGetFileInfoW(
        PCWSTR(wide.as_ptr()),
        FILE_FLAGS_AND_ATTRIBUTES(0),
        Some(&mut sfi),
        std::mem::size_of::<SHFILEINFOW>() as u32,
        SHGFI_ICON | SHGFI_LARGEICON,
    );
    if ok == 0 || sfi.hIcon.is_invalid() {
        return None;
    }
    let result = hicon_to_rgba(sfi.hIcon);
    let _ = DestroyIcon(sfi.hIcon);
    result
}

unsafe fn hicon_to_rgba(hicon: HICON) -> Option<(u32, u32, Vec<u8>)> {
    let mut info = ICONINFO::default();
    GetIconInfo(hicon, &mut info).ok()?;

    let mut bm = BITMAP::default();
    GetObjectW(
        HGDIOBJ(info.hbmColor.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut std::ffi::c_void),
    );
    let w = bm.bmWidth;
    let h = bm.bmHeight;
    if w <= 0 || h <= 0 {
        let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        return None;
    }

    let mut bi = BITMAPINFO::default();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h; // top-down
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB.0 as u32;

    let mut buf = vec![0u8; (w * h * 4) as usize];
    let hdc = CreateCompatibleDC(None);
    let lines = GetDIBits(
        hdc,
        info.hbmColor,
        0,
        h as u32,
        Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
        &mut bi,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(hdc);
    let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
    let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
    if lines == 0 {
        return None;
    }

    // BGRA -> RGBA. Icons without an alpha channel come back fully transparent;
    // force opaque in that case.
    let mut any_alpha = false;
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2);
        if px[3] != 0 {
            any_alpha = true;
        }
    }
    if !any_alpha {
        for px in buf.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }
    Some((w as u32, h as u32, buf))
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}
