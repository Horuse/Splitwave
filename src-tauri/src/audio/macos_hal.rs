//! Minimal CoreAudio HAL device enumeration.
//!
//! `cpal::Host::output_devices()` filters by `supported_output_configs().is_ok()`,
//! which on macOS can drop perfectly-valid devices that aren't currently the
//! default route (e.g. the built-in speakers while AirPods are connected). Pro
//! workflows need the full device list, so we go straight to the HAL and use
//! cpal only for the stream once the user selects a device.
//!
//! The FFI surface is intentionally tiny — no `coreaudio-sys` dependency.

use std::ffi::c_void;
use std::mem;
use std::ptr;

type OSStatus = i32;
type AudioObjectID = u32;
type AudioObjectPropertySelector = u32;
type AudioObjectPropertyScope = u32;
type AudioObjectPropertyElement = u32;

#[repr(C)]
#[derive(Copy, Clone)]
struct AudioObjectPropertyAddress {
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
    element: AudioObjectPropertyElement,
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | (s[3] as u32)
}

const K_AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectID = 1;
const K_AUDIO_HARDWARE_PROPERTY_DEVICES: AudioObjectPropertySelector = fourcc(b"dev#");
const K_AUDIO_DEVICE_PROPERTY_STREAMS: AudioObjectPropertySelector = fourcc(b"stm#");
const K_AUDIO_DEVICE_PROPERTY_STREAM_CONFIGURATION: AudioObjectPropertySelector = fourcc(b"slay");
const K_AUDIO_DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE: AudioObjectPropertySelector = fourcc(b"nsrt");
const K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR: AudioObjectPropertySelector = fourcc(b"volm");
const K_AUDIO_OBJECT_PROPERTY_NAME: AudioObjectPropertySelector = fourcc(b"lnam");
const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: AudioObjectPropertyScope = fourcc(b"glob");
const K_AUDIO_OBJECT_PROPERTY_SCOPE_INPUT: AudioObjectPropertyScope = fourcc(b"inpt");
const K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT: AudioObjectPropertyScope = fourcc(b"outp");
const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: AudioObjectPropertyElement = 0;

#[repr(C)]
struct AudioBuffer {
    m_number_channels: u32,
    m_data_byte_size: u32,
    m_data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    m_number_buffers: u32,
    m_buffers: [AudioBuffer; 1],
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyDataSize(
        in_object: AudioObjectID,
        in_address: *const AudioObjectPropertyAddress,
        in_qualifier_size: u32,
        in_qualifier_data: *const c_void,
        out_data_size: *mut u32,
    ) -> OSStatus;

    fn AudioObjectGetPropertyData(
        in_object: AudioObjectID,
        in_address: *const AudioObjectPropertyAddress,
        in_qualifier_size: u32,
        in_qualifier_data: *const c_void,
        io_data_size: *mut u32,
        out_data: *mut c_void,
    ) -> OSStatus;

    fn AudioObjectSetPropertyData(
        in_object: AudioObjectID,
        in_address: *const AudioObjectPropertyAddress,
        in_qualifier_size: u32,
        in_qualifier_data: *const c_void,
        in_data_size: u32,
        in_data: *const c_void,
    ) -> OSStatus;

    fn AudioObjectHasProperty(
        in_object: AudioObjectID,
        in_address: *const AudioObjectPropertyAddress,
    ) -> bool;

    fn AudioObjectIsPropertySettable(
        in_object: AudioObjectID,
        in_address: *const AudioObjectPropertyAddress,
        out_is_settable: *mut bool,
    ) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetLength(s: *const c_void) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(
        s: *const c_void,
        buffer: *mut u8,
        buffer_size: isize,
        encoding: u32,
    ) -> i8;
    fn CFRelease(cf: *const c_void);
}

unsafe fn cfstring_to_string(cfstring: *const c_void) -> Option<String> {
    if cfstring.is_null() {
        return None;
    }
    let len = CFStringGetLength(cfstring);
    let max = CFStringGetMaximumSizeForEncoding(len, K_CF_STRING_ENCODING_UTF8) + 1;
    if max <= 0 {
        return None;
    }
    let mut buf = vec![0u8; max as usize];
    if CFStringGetCString(cfstring, buf.as_mut_ptr(), max, K_CF_STRING_ENCODING_UTF8) == 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    std::str::from_utf8(&buf[..end]).ok().map(|s| s.to_string())
}

unsafe fn device_name(device_id: AudioObjectID) -> Option<String> {
    let addr = AudioObjectPropertyAddress {
        selector: K_AUDIO_OBJECT_PROPERTY_NAME,
        scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
        element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    };
    let mut cfstring: *const c_void = ptr::null();
    let mut size: u32 = mem::size_of::<*const c_void>() as u32;
    if AudioObjectGetPropertyData(
        device_id,
        &addr,
        0,
        ptr::null(),
        &mut size,
        &mut cfstring as *mut _ as *mut c_void,
    ) != 0
    {
        return None;
    }
    let name = cfstring_to_string(cfstring);
    if !cfstring.is_null() {
        CFRelease(cfstring);
    }
    name
}

/// Whether the device exposes at least one stream object in `scope`.
/// AUHAL requires this to bind the device for I/O — devices with a static
/// channel layout but no stream objects (typical of non-routable aliases)
/// can't be opened and must not appear in the user-facing list.
unsafe fn has_streams_in_scope(
    device_id: AudioObjectID,
    scope: AudioObjectPropertyScope,
) -> bool {
    let addr = AudioObjectPropertyAddress {
        selector: K_AUDIO_DEVICE_PROPERTY_STREAMS,
        scope,
        element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    };
    let mut size: u32 = 0;
    if AudioObjectGetPropertyDataSize(device_id, &addr, 0, ptr::null(), &mut size) != 0 {
        return false;
    }
    (size as usize / mem::size_of::<AudioObjectID>()) > 0
}

unsafe fn channel_count_in_scope(
    device_id: AudioObjectID,
    scope: AudioObjectPropertyScope,
) -> u32 {
    let addr = AudioObjectPropertyAddress {
        selector: K_AUDIO_DEVICE_PROPERTY_STREAM_CONFIGURATION,
        scope,
        element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    };
    let mut size: u32 = 0;
    if AudioObjectGetPropertyDataSize(device_id, &addr, 0, ptr::null(), &mut size) != 0
        || size == 0
    {
        return 0;
    }
    let mut buf = vec![0u8; size as usize];
    let mut io_size = size;
    if AudioObjectGetPropertyData(
        device_id,
        &addr,
        0,
        ptr::null(),
        &mut io_size,
        buf.as_mut_ptr() as *mut c_void,
    ) != 0
    {
        return 0;
    }
    let list = buf.as_ptr() as *const AudioBufferList;
    let n_buffers = (*list).m_number_buffers as usize;
    let buffers = std::ptr::addr_of!((*list).m_buffers[0]);
    let mut total = 0u32;
    for i in 0..n_buffers {
        total = total.saturating_add((*buffers.add(i)).m_number_channels);
    }
    total
}

unsafe fn all_device_ids() -> Vec<AudioObjectID> {
    let addr = AudioObjectPropertyAddress {
        selector: K_AUDIO_HARDWARE_PROPERTY_DEVICES,
        scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
        element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    };
    let mut size: u32 = 0;
    if AudioObjectGetPropertyDataSize(
        K_AUDIO_OBJECT_SYSTEM_OBJECT,
        &addr,
        0,
        ptr::null(),
        &mut size,
    ) != 0
    {
        return Vec::new();
    }
    let count = size as usize / mem::size_of::<AudioObjectID>();
    let mut ids = vec![0u32; count];
    let mut io_size = size;
    if AudioObjectGetPropertyData(
        K_AUDIO_OBJECT_SYSTEM_OBJECT,
        &addr,
        0,
        ptr::null(),
        &mut io_size,
        ids.as_mut_ptr() as *mut c_void,
    ) != 0
    {
        return Vec::new();
    }
    ids
}

/// Full HAL view of one device: enough to open it without ever asking cpal
/// about supported configs (which CoreAudio reports as empty for non-active
/// routes — the root cause of "no Output device found" for inactive built-in
/// speakers).
#[derive(Debug, Clone)]
pub struct HalDevice {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u32,
}

unsafe fn nominal_sample_rate(device_id: AudioObjectID) -> Option<u32> {
    let addr = AudioObjectPropertyAddress {
        selector: K_AUDIO_DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE,
        scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
        element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    };
    let mut sr: f64 = 0.0;
    let mut size: u32 = mem::size_of::<f64>() as u32;
    if AudioObjectGetPropertyData(
        device_id,
        &addr,
        0,
        ptr::null(),
        &mut size,
        &mut sr as *mut _ as *mut c_void,
    ) != 0
    {
        return None;
    }
    if sr.is_finite() && sr > 0.0 {
        Some(sr.round() as u32)
    } else {
        None
    }
}

fn list_by_scope(scope: AudioObjectPropertyScope) -> Vec<HalDevice> {
    let mut out = Vec::new();
    unsafe {
        for id in all_device_ids() {
            if !has_streams_in_scope(id, scope) {
                continue;
            }
            let channels = channel_count_in_scope(id, scope);
            if channels == 0 {
                continue;
            }
            let Some(name) = device_name(id) else {
                continue;
            };
            let Some(sample_rate) = nominal_sample_rate(id) else {
                continue;
            };
            out.push(HalDevice {
                name,
                sample_rate,
                channels,
            });
        }
    }
    out
}

pub fn list_input_devices() -> Vec<HalDevice> {
    list_by_scope(K_AUDIO_OBJECT_PROPERTY_SCOPE_INPUT)
}

pub fn list_output_devices() -> Vec<HalDevice> {
    list_by_scope(K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT)
}

pub fn find_input_device(name: &str) -> Option<HalDevice> {
    list_input_devices().into_iter().find(|d| d.name == name)
}

pub fn find_output_device(name: &str) -> Option<HalDevice> {
    list_output_devices().into_iter().find(|d| d.name == name)
}

fn scope_for(kind: crate::audio::device::DeviceKind) -> AudioObjectPropertyScope {
    match kind {
        crate::audio::device::DeviceKind::Input => K_AUDIO_OBJECT_PROPERTY_SCOPE_INPUT,
        crate::audio::device::DeviceKind::Output => K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
    }
}

fn find_device_id(name: &str, scope: AudioObjectPropertyScope) -> Option<AudioObjectID> {
    unsafe {
        for id in all_device_ids() {
            if !has_streams_in_scope(id, scope) {
                continue;
            }
            if device_name(id).as_deref() == Some(name) {
                return Some(id);
            }
        }
    }
    None
}

unsafe fn read_volume_for_element(
    device_id: AudioObjectID,
    scope: AudioObjectPropertyScope,
    element: u32,
) -> Option<f32> {
    let addr = AudioObjectPropertyAddress {
        selector: K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR,
        scope,
        element,
    };
    if !AudioObjectHasProperty(device_id, &addr) {
        return None;
    }
    let mut v: f32 = 0.0;
    let mut size: u32 = mem::size_of::<f32>() as u32;
    if AudioObjectGetPropertyData(
        device_id,
        &addr,
        0,
        ptr::null(),
        &mut size,
        &mut v as *mut _ as *mut c_void,
    ) != 0
    {
        return None;
    }
    Some(v.clamp(0.0, 1.0))
}

unsafe fn write_volume_for_element(
    device_id: AudioObjectID,
    scope: AudioObjectPropertyScope,
    element: u32,
    scalar: f32,
) -> bool {
    let addr = AudioObjectPropertyAddress {
        selector: K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR,
        scope,
        element,
    };
    if !AudioObjectHasProperty(device_id, &addr) {
        return false;
    }
    let mut settable = false;
    if AudioObjectIsPropertySettable(device_id, &addr, &mut settable) != 0 || !settable {
        return false;
    }
    let v = scalar.clamp(0.0, 1.0);
    let size = mem::size_of::<f32>() as u32;
    AudioObjectSetPropertyData(
        device_id,
        &addr,
        0,
        ptr::null(),
        size,
        &v as *const _ as *const c_void,
    ) == 0
}

/// Returns None when CoreAudio exposes no settable volume — common for
/// hardware-knob interfaces and some USB mics.
pub fn device_volume(kind: crate::audio::device::DeviceKind, name: &str) -> Option<f32> {
    let scope = scope_for(kind);
    let device_id = find_device_id(name, scope)?;
    unsafe {
        // Master element first; per-channel fallback for multi-channel devices.
        if let Some(v) =
            read_volume_for_element(device_id, scope, K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN)
        {
            return Some(v);
        }
        let l = read_volume_for_element(device_id, scope, 1);
        let r = read_volume_for_element(device_id, scope, 2);
        match (l, r) {
            (Some(l), Some(r)) => Some((l + r) * 0.5),
            (Some(v), None) | (None, Some(v)) => Some(v),
            (None, None) => None,
        }
    }
}

/// Returns false when the property isn't settable.
pub fn set_device_volume(
    kind: crate::audio::device::DeviceKind,
    name: &str,
    scalar: f32,
) -> bool {
    let scope = scope_for(kind);
    let Some(device_id) = find_device_id(name, scope) else {
        return false;
    };
    unsafe {
        if write_volume_for_element(
            device_id,
            scope,
            K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
            scalar,
        ) {
            return true;
        }
        let l = write_volume_for_element(device_id, scope, 1, scalar);
        let r = write_volume_for_element(device_id, scope, 2, scalar);
        l || r
    }
}
