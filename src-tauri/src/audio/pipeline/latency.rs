//! End-to-end latency of the running pipeline, the way a DAW reports round
//! trip: capture device (buffer plus what the hardware adds), the queue ahead
//! of the graph, the graph's own delay compensation, the output adapter, and
//! the playback device (buffer plus hardware).

use serde::Serialize;
use ts_rs::TS;

/// One direction of one device, in its own frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DeviceIo {
    pub rate: u32,
    pub buffer_frames: u32,
    /// Past the buffer: safety offset, converters, transport. `None` where
    /// the OS does not report it.
    pub hardware_frames: Option<u32>,
}

/// A source feeding a speaker.
#[derive(Debug, Clone, Copy)]
pub(super) struct PathInput {
    pub device: Option<DeviceIo>,
    /// Smoothed frames queued ahead of the graph, at `rate`.
    pub queue_frames: u64,
    /// Frames spent in the capture normalizer's resampler, at `rate`.
    pub normalizer_frames: u64,
    pub rate: u32,
}

/// A speaker and the graph it renders.
#[derive(Debug, Clone, Copy)]
pub(super) struct PathOutput {
    pub pipeline_rate: u32,
    pub graph_latency_frames: u64,
    pub device_rate: u32,
    /// Measured from the device's own callbacks.
    pub callback_frames: u64,
    pub carried_frames: u64,
    pub resampler_frames: u64,
    pub hardware_frames: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LatencyBreakdown {
    pub total_ms: f32,
    /// Capture device buffer and hardware.
    pub input_device_ms: f32,
    /// Audio queued ahead of the graph, including capture resampling.
    pub input_queue_ms: f32,
    /// Delay compensation for the slowest path through the graph.
    pub processing_ms: f32,
    /// Output resampling and block adapter.
    pub output_adapter_ms: f32,
    /// Playback device buffer and hardware.
    pub output_device_ms: f32,
    /// False when some device did not report what it adds past its buffer,
    /// so the total is a lower bound.
    pub hardware_included: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NodeTiming {
    pub node_id: String,
    /// Delay this node adds, in pipeline frames.
    pub latency_frames: u32,
    /// Block the node actually works in, when larger than the engine buffer.
    pub working_block: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LatencyReport {
    /// The slowest input-to-speaker path; `None` without a speaker.
    pub path: Option<LatencyBreakdown>,
    /// Engine buffer the settings asked for.
    pub buffer_frames: u32,
    /// Buffer the slowest speaker actually runs at, in its own frames.
    pub device_buffer_frames: Option<u32>,
    /// Peak share of the audio period spent rendering since the last report
    /// (1.0 = the device outran the graph).
    pub dsp_load: f32,
    /// Callbacks that ran past their period since the last report.
    pub overloads: u32,
    pub nodes: Vec<NodeTiming>,
}

fn ms(frames: u64, rate: u32) -> f32 {
    (frames as f64 * 1000.0 / rate.max(1) as f64) as f32
}

/// The slowest of the speaker's inputs sets its latency; parallel inputs do
/// not add up. A speaker with no live input still has its output side.
pub(super) fn breakdown(inputs: &[PathInput], out: &PathOutput) -> LatencyBreakdown {
    let mut hardware_included = out.hardware_frames.is_some();
    let worst = inputs
        .iter()
        .map(|i| {
            let device_ms = i.device.map_or(0.0, |d| {
                ms(
                    d.buffer_frames as u64 + d.hardware_frames.unwrap_or(0) as u64,
                    d.rate,
                )
            });
            let queue_ms = ms(i.queue_frames + i.normalizer_frames, i.rate);
            // A source with no capture device (a process tap, a file, the
            // network) has no hardware of its own to report.
            (
                device_ms,
                queue_ms,
                i.device.is_none_or(|d| d.hardware_frames.is_some()),
            )
        })
        .max_by(|a, b| (a.0 + a.1).total_cmp(&(b.0 + b.1)));
    let (input_device_ms, input_queue_ms) = match worst {
        Some((device, queue, known)) => {
            hardware_included &= known;
            (device, queue)
        }
        None => (0.0, 0.0),
    };
    let processing_ms = ms(out.graph_latency_frames, out.pipeline_rate);
    let output_adapter_ms = ms(out.carried_frames + out.resampler_frames, out.device_rate);
    let output_device_ms = ms(
        out.callback_frames + out.hardware_frames.unwrap_or(0) as u64,
        out.device_rate,
    );
    LatencyBreakdown {
        total_ms: input_device_ms
            + input_queue_ms
            + processing_ms
            + output_adapter_ms
            + output_device_ms,
        input_device_ms,
        input_queue_ms,
        processing_ms,
        output_adapter_ms,
        output_device_ms,
        hardware_included,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(callback: u64, hardware: Option<u32>) -> PathOutput {
        PathOutput {
            pipeline_rate: 48_000,
            graph_latency_frames: 0,
            device_rate: 48_000,
            callback_frames: callback,
            carried_frames: 0,
            resampler_frames: 0,
            hardware_frames: hardware,
        }
    }

    fn mic(buffer: u32, hardware: Option<u32>, queue: u64) -> PathInput {
        PathInput {
            device: Some(DeviceIo {
                rate: 48_000,
                buffer_frames: buffer,
                hardware_frames: hardware,
            }),
            queue_frames: queue,
            normalizer_frames: 0,
            rate: 48_000,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn round_trip_adds_every_stage() {
        // 64-frame buffers both ways, 1 ms of hardware each way, a 2 ms queue
        // and a 2 ms limiter lookahead: 1.33+1+2+2+1.33+1 ms.
        let mut o = out(64, Some(48));
        o.graph_latency_frames = 96;
        let b = breakdown(&[mic(64, Some(48), 96)], &o);
        assert!(close(b.input_device_ms, 2.333), "{b:?}");
        assert!(close(b.input_queue_ms, 2.0));
        assert!(close(b.processing_ms, 2.0));
        assert!(close(b.output_device_ms, 2.333));
        assert!(close(b.total_ms, 8.667));
        assert!(b.hardware_included);
    }

    #[test]
    fn slowest_input_sets_the_path() {
        let b = breakdown(
            &[mic(64, Some(0), 0), mic(512, Some(0), 480)],
            &out(64, Some(0)),
        );
        assert!(close(b.input_device_ms, 10.667));
        assert!(close(b.input_queue_ms, 10.0));
    }

    #[test]
    fn unreported_hardware_marks_the_total_as_a_lower_bound() {
        assert!(!breakdown(&[mic(64, None, 0)], &out(64, Some(0))).hardware_included);
        assert!(!breakdown(&[mic(64, Some(0), 0)], &out(64, None)).hardware_included);
        assert!(breakdown(&[], &out(64, Some(0))).hardware_included);
        let tap = PathInput {
            device: None,
            queue_frames: 300,
            normalizer_frames: 0,
            rate: 48_000,
        };
        assert!(
            breakdown(&[tap], &out(64, Some(0))).hardware_included,
            "a tap has no device to leave unreported"
        );
    }

    #[test]
    fn output_side_uses_the_device_rate() {
        let mut o = out(441, Some(0));
        o.device_rate = 44_100;
        o.carried_frames = 100;
        o.resampler_frames = 73;
        let b = breakdown(&[], &o);
        assert!(close(b.output_device_ms, 10.0));
        assert!(close(b.output_adapter_ms, 173.0 / 44.1));
    }
}
