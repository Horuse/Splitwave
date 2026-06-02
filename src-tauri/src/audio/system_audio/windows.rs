use std::collections::{HashMap, HashSet};

use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::{CloseHandle, MAX_PATH, S_OK};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::AudioApplication;
use crate::error::{AppError, AppResult};

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
        let Some(exe) = process_exe_name(pid) else {
            continue;
        };
        if !seen.insert(exe.clone()) {
            continue;
        }
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

unsafe fn process_exe_name(pid: u32) -> Option<String> {
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; MAX_PATH as usize];
    let mut len = buf.len() as u32;
    let res = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
    let _ = CloseHandle(handle);
    res.ok()?;
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit(['\\', '/']).next().map(str::to_string)
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
            if pid != 0 && process_exe_name(pid).as_deref() == Some(target) {
                return Some(pid);
            }
        }
        None
    }
}

pub fn load_app_icons(_bundle_ids: Vec<String>) -> HashMap<String, String> {
    HashMap::new()
}
