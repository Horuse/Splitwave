use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eRender, EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::AUDIO_VOLUME_NOTIFICATION_DATA;
use windows_core::implement;

use super::{DeviceVolume, Notify, MUTED_DB};
use crate::audio::device::DeviceKind;

fn flow(kind: DeviceKind) -> EDataFlow {
    match kind {
        DeviceKind::Input => eCapture,
        DeviceKind::Output => eRender,
    }
}

// COM may already be initialised on this thread; a second call returns
// S_FALSE/RPC_E_CHANGED_MODE, both harmless here.
fn ensure_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

// Match the endpoint cpal exposed by its friendly name (PKEY_Device_FriendlyName).
unsafe fn endpoint_volume(kind: DeviceKind, name: &str) -> Option<IAudioEndpointVolume> {
    ensure_com();
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
    let collection = enumerator
        .EnumAudioEndpoints(flow(kind), DEVICE_STATE_ACTIVE)
        .ok()?;
    for i in 0..collection.GetCount().ok()? {
        let dev = collection.Item(i).ok()?;
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        let Ok(mut prop) = store.GetValue(&PKEY_Device_FriendlyName) else {
            continue;
        };
        let dev_name = prop
            .Anonymous
            .Anonymous
            .Anonymous
            .pwszVal
            .to_string()
            .unwrap_or_default();
        let _ = PropVariantClear(&mut prop);
        if dev_name == name {
            return dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok();
        }
    }
    None
}

pub fn device_volume(kind: DeviceKind, name: &str) -> Option<DeviceVolume> {
    unsafe {
        let vol = endpoint_volume(kind, name)?;
        if vol.GetMute().ok()?.as_bool() {
            return Some(DeviceVolume {
                scalar: 0.0,
                db: Some(MUTED_DB),
            });
        }
        Some(DeviceVolume {
            scalar: vol.GetMasterVolumeLevelScalar().ok()?.clamp(0.0, 1.0),
            db: vol.GetMasterVolumeLevel().ok().filter(|db| db.is_finite()),
        })
    }
}

pub fn set_device_volume(kind: DeviceKind, name: &str, scalar: f32) -> bool {
    unsafe {
        let Some(vol) = endpoint_volume(kind, name) else {
            return false;
        };
        if scalar <= 0.0 {
            return vol.SetMute(true, std::ptr::null()).is_ok();
        }
        if vol.SetMute(false, std::ptr::null()).is_err() {
            return false;
        }
        vol.SetMasterVolumeLevelScalar(scalar.clamp(0.0, 1.0), std::ptr::null())
            .is_ok()
    }
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback {
    notify: Notify,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, _data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows_core::Result<()> {
        (self.notify)();
        Ok(())
    }
}

/// Holds the endpoint alive alongside its callback; dropping unregisters.
pub struct Watch {
    endpoint: IAudioEndpointVolume,
    callback: IAudioEndpointVolumeCallback,
}

// Both interfaces are activated in the MTA (`ensure_com` uses
// COINIT_MULTITHREADED), where COM allows calls and release from any thread.
unsafe impl Send for Watch {}

impl Drop for Watch {
    fn drop(&mut self) {
        unsafe {
            let _ = self.endpoint.UnregisterControlChangeNotify(&self.callback);
        }
    }
}

pub fn watch_device(kind: DeviceKind, name: &str, notify: Notify) -> Option<Watch> {
    unsafe {
        let endpoint = endpoint_volume(kind, name)?;
        let callback: IAudioEndpointVolumeCallback = VolumeCallback { notify }.into();
        endpoint.RegisterControlChangeNotify(&callback).ok()?;
        Some(Watch { endpoint, callback })
    }
}
