use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eRender, EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

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

pub fn device_volume(kind: DeviceKind, name: &str) -> Option<f32> {
    unsafe {
        let vol = endpoint_volume(kind, name)?;
        if vol.GetMute().ok()?.as_bool() {
            return Some(0.0);
        }
        Some(vol.GetMasterVolumeLevelScalar().ok()?.clamp(0.0, 1.0))
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
