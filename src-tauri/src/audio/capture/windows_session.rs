//! Per-session mute for WASAPI capture.
//!
//! Process loopback is passive: the captured app keeps rendering to its
//! endpoint, so its audio and the graph's copy both reach the speakers. The
//! session volume API is the only documented way to silence the original, and
//! it is entirely separate from the loopback client.
//!
//! Sessions are matched the same way the app picker names them -- by exe base
//! name -- so a multi-process app (browser tabs, helpers) is covered as a whole,
//! which mirrors `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`.

use tracing::warn;
use windows::core::Interface;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Threading::GetCurrentProcessId;

use crate::audio::system_audio::exe_base_name;

/// Which sessions a mute covers.
pub enum Target {
    /// Every session whose exe base name matches, e.g. all `chrome.exe`.
    Exe(String),
    /// The whole render endpoint except our own process, mirroring the
    /// exclude-current-app tap on macOS.
    AllButSelf,
    /// Exactly the sessions a previous `apply` changed, used to restore them.
    Pids(Vec<u32>),
}

/// Restores on drop the sessions it muted, and only those -- a session the user
/// had already muted is left alone so we never unmute it behind their back.
pub struct SessionMute {
    muted: Vec<u32>,
}

impl SessionMute {
    pub fn apply(target: Target) -> Self {
        let muted = match set_mute(&target, true) {
            Ok(pids) => pids,
            Err(e) => {
                warn!(error = %e, "failed to mute captured sessions; original audio stays audible");
                Vec::new()
            }
        };
        SessionMute { muted }
    }
}

impl Drop for SessionMute {
    fn drop(&mut self) {
        if self.muted.is_empty() {
            return;
        }
        if let Err(e) = set_mute(&Target::Pids(std::mem::take(&mut self.muted)), false) {
            warn!(error = %e, "failed to restore muted sessions; they stay silent until the app is restarted");
        }
    }
}

/// Applies `mute` to every session the target covers, returning the PIDs whose
/// state this call actually changed.
fn set_mute(target: &Target, mute: bool) -> windows::core::Result<Vec<u32>> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let sessions = manager.GetSessionEnumerator()?;
        let own_pid = GetCurrentProcessId();

        let mut changed = Vec::new();
        for i in 0..sessions.GetCount()? {
            let session = sessions.GetSession(i)?;
            let ctrl2: IAudioSessionControl2 = session.cast()?;
            if ctrl2.IsSystemSoundsSession() == S_OK {
                continue;
            }
            let pid = ctrl2.GetProcessId()?;
            if pid == 0 || pid == own_pid {
                continue;
            }
            if !target.covers(pid) {
                continue;
            }
            let volume: ISimpleAudioVolume = session.cast()?;
            if volume.GetMute()?.as_bool() == mute {
                continue;
            }
            volume.SetMute(mute, std::ptr::null())?;
            changed.push(pid);
        }
        Ok(changed)
    }
}

impl Target {
    fn covers(&self, pid: u32) -> bool {
        match self {
            Target::Exe(exe) => exe_base_name(pid).as_deref() == Some(exe.as_str()),
            Target::AllButSelf => true,
            Target::Pids(pids) => pids.contains(&pid),
        }
    }
}
