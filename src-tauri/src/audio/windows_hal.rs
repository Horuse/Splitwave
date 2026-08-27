//! WASAPI endpoint plumbing outside cpal. Same shape as `macos_hal`: register
//! a native listener, forward a pre-built notification from the callback and
//! let the audio control thread do the actual (potentially slow) work.

use tracing::{info, warn};
use windows::core::{implement, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    eRender, IMMDevice, IMMDeviceCollection, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, PKEY_AudioEngine_DeviceFormat,
    DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Variant::VT_LPWSTR;

use crate::audio::volume::Notify;

/// Registered listener for the device-format property of one render endpoint.
/// Windows fires `OnPropertyValueChanged` from its own thread when the user
/// changes the endpoint's default format (sample rate / bit depth); the
/// callback only forwards, stream reopen happens on the audio control thread.
pub struct EndpointRateListener {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

// SAFETY: the COM objects are used only for register (in the watch call, on
// the calling thread) and unregister (in Drop). Both MMDeviceEnumerator
// registration and IMMNotificationClient unregister are thread-safe Windows
// operations, same pattern as cpal's own notification monitor.
unsafe impl Send for EndpointRateListener {}
unsafe impl Sync for EndpointRateListener {}

impl Drop for EndpointRateListener {
    fn drop(&mut self) {
        ensure_com();
        unsafe {
            if let Err(e) = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.client)
            {
                warn!(error = %e, "failed to unregister endpoint rate listener");
            }
        }
    }
}

fn ensure_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn com_err(e: windows::core::Error) -> String {
    format!("wasapi: {e}")
}

// Endpoint display name matches cpal's WASAPI device naming (it also derives
// its `Device::name` from the friendly-name property), which is what the
// device model stores as `DeviceInfo.id`.
fn endpoint_name(device: &IMMDevice) -> Option<String> {
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let pv = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        if pv.Anonymous.Anonymous.vt != VT_LPWSTR {
            return None;
        }
        let ptr = pv.Anonymous.Anonymous.Anonymous.pwszVal;
        if ptr.is_null() {
            return None;
        }
        PCWSTR::from_raw(ptr.0).to_string().ok()
    }
}

fn endpoint_id(device: &IMMDevice) -> Option<String> {
    unsafe {
        let raw = device.GetId().ok()?;
        let id = PCWSTR::from_raw(raw.as_ptr()).to_string().ok();
        CoTaskMemFree(Some(raw.as_ptr().cast()));
        id
    }
}

fn key_matches(key: &PROPERTYKEY, expected: &PROPERTYKEY) -> bool {
    key.fmtid == expected.fmtid && key.pid == expected.pid
}

#[implement(IMMNotificationClient)]
struct RateNotify {
    endpoint_id: String,
    notify: Notify,
}

impl IMMNotificationClient_Impl for RateNotify_Impl {
    fn OnDeviceStateChanged(
        &self,
        _pwstrdeviceid: &PCWSTR,
        _dwnewstate: windows::Win32::Media::Audio::DEVICE_STATE,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceAdded(&self, _pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        _flow: windows::Win32::Media::Audio::EDataFlow,
        _role: windows::Win32::Media::Audio::ERole,
        _pwstrdefaultdeviceid: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        pwstrdeviceid: &PCWSTR,
        key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        if key_matches(key, &PKEY_AudioEngine_DeviceFormat)
            && self.endpoint_id == unsafe { pwstrdeviceid.to_string() }.unwrap_or_default()
        {
            (self.notify)();
        }
        Ok(())
    }
}

/// Watches one render endpoint's device format. `notify` must be
/// non-blocking: it runs on a Windows notification thread and only queues the
/// engine command. Returns None when the endpoint cannot be resolved from the
/// device name; the speaker then behaves as before (no live rate watch).
pub fn watch_output_sample_rate(name: &str, notify: Notify) -> Option<EndpointRateListener> {
    ensure_com();
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| com_err(e))
                .ok()?;
        let collection: IMMDeviceCollection = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| com_err(e))
            .ok()?;
        let count = collection.GetCount().ok()?;
        let mut found: Option<String> = None;
        for i in 0..count {
            let Ok(device) = collection.Item(i) else {
                continue;
            };
            if endpoint_name(&device).as_deref() == Some(name) {
                found = endpoint_id(&device);
                break;
            }
        }
        let Some(endpoint_id) = found else {
            warn!(device = %name, "endpoint not found for rate watcher");
            return None;
        };

        let client: IMMNotificationClient = RateNotify {
            endpoint_id: endpoint_id.clone(),
            notify,
        }
        .into();
        enumerator
            .RegisterEndpointNotificationCallback(&client)
            .map_err(|e| com_err(e))
            .ok()?;
        info!(device = %name, "watching native speaker rate");
        Some(EndpointRateListener { enumerator, client })
    }
}
