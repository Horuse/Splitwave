use crate::audio::device::DeviceKind;

pub fn device_volume(_kind: DeviceKind, _name: &str) -> Option<f32> {
    None
}

pub fn set_device_volume(_kind: DeviceKind, _name: &str, _scalar: f32) -> bool {
    false
}
