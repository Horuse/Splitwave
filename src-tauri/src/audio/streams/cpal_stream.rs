//! Build cpal input/output streams (CoreAudio on macOS, WASAPI on Windows)
//! with runtime sample-format dispatch.
//!
//! Internally the pipeline carries `f32` interleaved stereo. Input streams convert
//! the device-native sample format (`i8/i16/i32/u8/u16/u32/f32/f64`) to f32 stereo
//! losslessly. Output streams accept f32 stereo and convert back to the device-
//! native format.
//!
//! Each cpal input is broadcast to N subscriber producer rings (one per output
//! that uses this input). On any ring full, that ring drops the current frame
//! rather than blocking -- non-RT safe operations are disallowed in the callback.

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Sample, SampleFormat, StreamConfig};
use tracing::error;

use crate::audio::effects::{update_meter, MeterHandle};
use crate::audio::health;
use crate::audio::input_bridge::BroadcastRx;
use crate::audio::streams::bulk_push_frames_counted;
use crate::error::{AppError, AppResult};

const RAW_CAPTURE_CALLBACK_FRAMES: usize = 4_096;

/// Build and start an input stream. `bridge` carries broadcast subscribers
/// at runtime; the callback drains pending add/remove commands at the top
/// of each block before broadcasting the converted-to-stereo f32 frames.
pub fn build_input_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    src_channels: usize,
    bridge: BroadcastRx,
    meter: Option<MeterHandle>,
    err_cb: impl FnMut(cpal::StreamError) + Send + 'static,
) -> AppResult<cpal::Stream> {
    match sample_format {
        SampleFormat::F32 => {
            build_input_typed::<f32>(device, config, src_channels, bridge, meter, err_cb)
        }
        SampleFormat::I16 => {
            build_input_typed::<i16>(device, config, src_channels, bridge, meter, err_cb)
        }
        SampleFormat::I32 => {
            build_input_typed::<i32>(device, config, src_channels, bridge, meter, err_cb)
        }
        SampleFormat::I8 => {
            build_input_typed::<i8>(device, config, src_channels, bridge, meter, err_cb)
        }
        SampleFormat::U8 => {
            build_input_typed::<u8>(device, config, src_channels, bridge, meter, err_cb)
        }
        SampleFormat::U16 => {
            build_input_typed::<u16>(device, config, src_channels, bridge, meter, err_cb)
        }
        SampleFormat::U32 => {
            build_input_typed::<u32>(device, config, src_channels, bridge, meter, err_cb)
        }
        SampleFormat::F64 => {
            build_input_typed::<f64>(device, config, src_channels, bridge, meter, err_cb)
        }
        fmt => Err(AppError::Validation(format!(
            "unsupported input sample format: {fmt:?}"
        ))),
    }
}

/// Build one physical device stream that forwards its complete native frames
/// into a private SPSC ring. This is used by Microphone Array before channels
/// are selected, so every member of one device keeps the exact same clock and
/// sample boundary.
pub fn build_raw_input_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    src_channels: usize,
    producer: rtrb::Producer<f32>,
    err_cb: impl FnMut(cpal::StreamError) + Send + 'static,
) -> AppResult<cpal::Stream> {
    match sample_format {
        SampleFormat::F32 => {
            build_raw_input_typed::<f32>(device, config, src_channels, producer, err_cb)
        }
        SampleFormat::I16 => {
            build_raw_input_typed::<i16>(device, config, src_channels, producer, err_cb)
        }
        SampleFormat::I32 => {
            build_raw_input_typed::<i32>(device, config, src_channels, producer, err_cb)
        }
        SampleFormat::I8 => {
            build_raw_input_typed::<i8>(device, config, src_channels, producer, err_cb)
        }
        SampleFormat::U8 => {
            build_raw_input_typed::<u8>(device, config, src_channels, producer, err_cb)
        }
        SampleFormat::U16 => {
            build_raw_input_typed::<u16>(device, config, src_channels, producer, err_cb)
        }
        SampleFormat::U32 => {
            build_raw_input_typed::<u32>(device, config, src_channels, producer, err_cb)
        }
        SampleFormat::F64 => {
            build_raw_input_typed::<f64>(device, config, src_channels, producer, err_cb)
        }
        fmt => Err(AppError::Validation(format!(
            "unsupported input sample format: {fmt:?}"
        ))),
    }
}

fn build_raw_input_typed<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    src_channels: usize,
    mut producer: rtrb::Producer<f32>,
    err_cb: impl FnMut(cpal::StreamError) + Send + 'static,
) -> AppResult<cpal::Stream>
where
    T: Sample + cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    if src_channels == 0 {
        return Err(AppError::Validation(
            "Microphone Array source has no input channels".into(),
        ));
    }
    let mut staging = vec![0.0; RAW_CAPTURE_CALLBACK_FRAMES * src_channels];
    let stream = device
        .build_input_stream::<T, _, _>(
            config,
            move |data, _| {
                // `staging` is allocated before the stream starts. Splitting
                // a large callback preserves whole frames and never grows it.
                for raw in data.chunks(staging.len()) {
                    for (out, &sample) in staging[..raw.len()].iter_mut().zip(raw) {
                        *out = sample.to_sample::<f32>();
                    }
                    bulk_push_frames_counted(
                        &mut producer,
                        &staging[..raw.len()],
                        src_channels,
                        &health::CAPTURE_RING_OVERRUN_SAMPLES,
                    );
                }
            },
            err_cb,
            None,
        )
        .map_err(|e| AppError::Stream(format!("input build: {e}")))?;
    stream
        .play()
        .map_err(|e| AppError::Stream(format!("input play: {e}")))?;
    Ok(stream)
}

fn build_input_typed<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    src_channels: usize,
    mut bridge: BroadcastRx,
    meter: Option<MeterHandle>,
    err_cb: impl FnMut(cpal::StreamError) + Send + 'static,
) -> AppResult<cpal::Stream>
where
    T: Sample + cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let mut staging: Vec<f32> = vec![0.0; 16384];
    let stream = device
        .build_input_stream::<T, _, _>(
            config,
            move |data, _| {
                bridge.apply_commands();
                if src_channels == 0 || data.is_empty() {
                    return;
                }
                let needed = data.len();
                if staging.len() < needed {
                    staging.resize(needed, 0.0);
                }
                for (o, &s) in staging[..needed].iter_mut().zip(data) {
                    *o = s.to_sample::<f32>();
                }
                if let Some(m) = &meter {
                    update_meter(m, &staging[..needed], src_channels);
                }
                bridge.broadcast(&staging[..needed]);
            },
            err_cb,
            None,
        )
        .map_err(|e| AppError::Stream(format!("input build: {e}")))?;
    stream
        .play()
        .map_err(|e| AppError::Stream(format!("input play: {e}")))?;
    Ok(stream)
}

/// Build and start an output stream that pulls f32 stereo from `fill`.
pub fn build_output_stream<F>(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    out_channels: usize,
    fill: F,
    err_cb: impl FnMut(cpal::StreamError) + Send + 'static,
) -> AppResult<cpal::Stream>
where
    F: FnMut(&mut [f32], usize) + Send + 'static,
{
    match sample_format {
        SampleFormat::F32 => {
            build_output_typed::<f32, _>(device, config, out_channels, fill, err_cb)
        }
        SampleFormat::I16 => {
            build_output_typed::<i16, _>(device, config, out_channels, fill, err_cb)
        }
        SampleFormat::I32 => {
            build_output_typed::<i32, _>(device, config, out_channels, fill, err_cb)
        }
        SampleFormat::I8 => build_output_typed::<i8, _>(device, config, out_channels, fill, err_cb),
        SampleFormat::U8 => build_output_typed::<u8, _>(device, config, out_channels, fill, err_cb),
        SampleFormat::U16 => {
            build_output_typed::<u16, _>(device, config, out_channels, fill, err_cb)
        }
        SampleFormat::U32 => {
            build_output_typed::<u32, _>(device, config, out_channels, fill, err_cb)
        }
        SampleFormat::F64 => {
            build_output_typed::<f64, _>(device, config, out_channels, fill, err_cb)
        }
        fmt => Err(AppError::Validation(format!(
            "unsupported output sample format: {fmt:?}"
        ))),
    }
}

fn build_output_typed<T, F>(
    device: &cpal::Device,
    config: &StreamConfig,
    out_channels: usize,
    mut fill: F,
    err_cb: impl FnMut(cpal::StreamError) + Send + 'static,
) -> AppResult<cpal::Stream>
where
    T: Sample + cpal::SizedSample + cpal::FromSample<f32> + Send + 'static,
    F: FnMut(&mut [f32], usize) + Send + 'static,
{
    let mut buf: Vec<f32> = vec![0.0; 16384];
    let stream = device
        .build_output_stream::<T, _, _>(
            config,
            move |data, _| {
                if out_channels == 0 || data.is_empty() {
                    return;
                }
                let total = data.len();
                let frames = total / out_channels;
                if buf.len() < total {
                    buf.resize(total, 0.0);
                }
                // `fill` supplies interleaved audio already at the device's
                // channel width (the DSP worker produces `out_channels`-wide).
                fill(&mut buf[..total], frames);
                for (out, s) in data.iter_mut().zip(&buf[..total]) {
                    *out = T::from_sample(*s);
                }
            },
            err_cb,
            None,
        )
        .map_err(|e| {
            let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());
            error!(
                device = %device_name,
                requested_sample_rate = config.sample_rate.0,
                requested_channels = config.channels,
                buffer_size = ?config.buffer_size,
                cpal_error_variant = ?e,
                cpal_error_display = %e,
                "build_output_stream failed"
            );
            AppError::Stream(format!("output build: {e}"))
        })?;
    stream
        .play()
        .map_err(|e| AppError::Stream(format!("output play: {e}")))?;
    Ok(stream)
}
