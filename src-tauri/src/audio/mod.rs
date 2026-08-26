pub mod capture;
pub mod clock;
pub mod device;
pub mod effects;
pub mod encoders;
pub mod engine;
pub mod graph;
pub mod health;
pub mod input_bridge;
#[cfg(target_os = "macos")]
pub mod macos_hal;
pub mod netaudio;
pub mod permission;
pub mod pipeline;
#[cfg(target_os = "linux")]
pub mod playback;
pub mod plugins;
#[cfg(target_os = "linux")]
pub mod pw_enum;
pub mod resample;
pub mod signaling;
pub mod stream_recv;
pub mod streams;
pub mod system_audio;
pub mod virtual_device;
pub mod volume;
pub mod webrtc;
pub mod webrtc_codec;

/// The only sample rate used by the DSP graph, effects, monitors, and
/// fixed-rate outputs. Device and capture adapters convert at its boundary.
pub const ENGINE_SAMPLE_RATE: u32 = 48_000;
