use super::{DeviceVolume, Notify};
use crate::audio::device::DeviceKind;

pub fn device_volume(kind: DeviceKind, name: &str) -> Option<DeviceVolume> {
    crate::audio::macos_hal::device_volume(kind, name)
}

pub fn set_device_volume(kind: DeviceKind, name: &str, scalar: f32) -> bool {
    crate::audio::macos_hal::set_device_volume(kind, name, scalar)
}

pub type Watch = crate::audio::macos_hal::VolumeListener;

pub fn watch_device(kind: DeviceKind, name: &str, notify: Notify) -> Option<Watch> {
    crate::audio::macos_hal::watch_volume(kind, name, notify)
}
