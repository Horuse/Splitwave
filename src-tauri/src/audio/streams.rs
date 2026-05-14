//! Build cpal input/output streams with runtime sample-format dispatch.
//!
//! Internally the pipeline carries `f32` interleaved stereo. Input streams convert
//! the device-native sample format (`i8/i16/i32/u8/u16/u32/f32/f64`) to f32 stereo
//! losslessly. Output streams accept f32 stereo and convert back to the device-
//! native format.
//!
//! Each cpal input is broadcast to N subscriber producer rings (one per output
//! that uses this input). On any ring full, that ring drops the current frame
//! rather than blocking — non-RT safe operations are disallowed in the callback.

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Sample, SampleFormat, StreamConfig};
use rtrb::Producer;
use tracing::error;

use crate::audio::effects::{update_meter, MeterHandle};
use crate::audio::format::{convert_to_stereo, write_stereo_to_output};
use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

/// Bulk drain `dst.len()` samples from an SPSC ring. Anything we couldn't
/// read (consumer faster than producer) is zero-filled — that's the device
/// playing silence, not glitching.
pub fn bulk_pop(cons: &mut rtrb::Consumer<f32>, dst: &mut [f32]) {
    let want = dst.len();
    if want == 0 {
        return;
    }
    let avail = cons.slots();
    let to_read = want.min(avail);
    if to_read > 0 {
        if let Ok(chunk) = cons.read_chunk(to_read) {
            let (first, second) = chunk.as_slices();
            let n1 = first.len();
            dst[..n1].copy_from_slice(first);
            let n2 = second.len();
            if n2 > 0 {
                dst[n1..n1 + n2].copy_from_slice(second);
            }
            chunk.commit_all();
        }
    }
    for s in &mut dst[to_read..] {
        *s = 0.0;
    }
}

/// Bulk push via one `write_chunk` reservation — one atomic-CAS per block
/// instead of one per sample. On overflow only the head fits and the rest is
/// dropped (consumer is behind anyway; staying RT-safe beats blocking).
pub fn bulk_push(prod: &mut Producer<f32>, samples: &[f32]) {
    let want = samples.len();
    if want == 0 {
        return;
    }
    let avail = prod.slots();
    let to_write = want.min(avail);
    if to_write == 0 {
        return;
    }
    if let Ok(mut chunk) = prod.write_chunk(to_write) {
        let (first, second) = chunk.as_mut_slices();
        let n1 = first.len();
        first.copy_from_slice(&samples[..n1]);
        let n2 = second.len();
        if n2 > 0 {
            second.copy_from_slice(&samples[n1..n1 + n2]);
        }
        chunk.commit_all();
    }
}

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
        SampleFormat::F32 => build_input_typed::<f32>(device, config, src_channels, bridge, meter, err_cb),
        SampleFormat::I16 => build_input_typed::<i16>(device, config, src_channels, bridge, meter, err_cb),
        SampleFormat::I32 => build_input_typed::<i32>(device, config, src_channels, bridge, meter, err_cb),
        SampleFormat::I8 => build_input_typed::<i8>(device, config, src_channels, bridge, meter, err_cb),
        SampleFormat::U8 => build_input_typed::<u8>(device, config, src_channels, bridge, meter, err_cb),
        SampleFormat::U16 => build_input_typed::<u16>(device, config, src_channels, bridge, meter, err_cb),
        SampleFormat::U32 => build_input_typed::<u32>(device, config, src_channels, bridge, meter, err_cb),
        SampleFormat::F64 => build_input_typed::<f64>(device, config, src_channels, bridge, meter, err_cb),
        fmt => Err(AppError::Validation(format!(
            "unsupported input sample format: {fmt:?}"
        ))),
    }
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
                let frames = data.len() / src_channels;
                let needed = frames * 2;
                if staging.len() < needed {
                    staging.resize(needed, 0.0);
                }
                convert_to_stereo::<T>(data, &mut staging[..needed], src_channels);
                if let Some(m) = &meter {
                    update_meter(m, &staging[..needed]);
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
        SampleFormat::F32 => build_output_typed::<f32, _>(device, config, out_channels, fill, err_cb),
        SampleFormat::I16 => build_output_typed::<i16, _>(device, config, out_channels, fill, err_cb),
        SampleFormat::I32 => build_output_typed::<i32, _>(device, config, out_channels, fill, err_cb),
        SampleFormat::I8 => build_output_typed::<i8, _>(device, config, out_channels, fill, err_cb),
        SampleFormat::U8 => build_output_typed::<u8, _>(device, config, out_channels, fill, err_cb),
        SampleFormat::U16 => build_output_typed::<u16, _>(device, config, out_channels, fill, err_cb),
        SampleFormat::U32 => build_output_typed::<u32, _>(device, config, out_channels, fill, err_cb),
        SampleFormat::F64 => build_output_typed::<f64, _>(device, config, out_channels, fill, err_cb),
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
    let mut stereo_buf: Vec<f32> = vec![0.0; 16384];
    let mut planar_out: Vec<f32> = vec![0.0; 16384];
    let stream = device
        .build_output_stream::<T, _, _>(
            config,
            move |data, _| {
                if out_channels == 0 || data.is_empty() {
                    return;
                }
                let total = data.len();
                let frames = total / out_channels;
                let stereo_needed = frames * 2;
                if stereo_buf.len() < stereo_needed {
                    stereo_buf.resize(stereo_needed, 0.0);
                }
                if planar_out.len() < total {
                    planar_out.resize(total, 0.0);
                }

                fill(&mut stereo_buf[..stereo_needed], frames);
                write_stereo_to_output(
                    &stereo_buf[..stereo_needed],
                    &mut planar_out[..total],
                    out_channels,
                );
                for (out, s) in data.iter_mut().zip(&planar_out[..total]) {
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
