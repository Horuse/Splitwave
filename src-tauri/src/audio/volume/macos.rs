use crate::audio::device::DeviceKind;

pub fn device_volume(kind: DeviceKind, name: &str) -> Option<f32> {
    crate::audio::macos_hal::device_volume(kind, name)
}

pub fn set_device_volume(kind: DeviceKind, name: &str, scalar: f32) -> bool {
    crate::audio::macos_hal::set_device_volume(kind, name, scalar)
}
