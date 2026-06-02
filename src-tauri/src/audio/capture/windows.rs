use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};

use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Capture {
    pub fn start_system(
        _exclude_current_app: bool,
        _sample_rate: u32,
        _channels: u32,
        bridge: BroadcastRx,
    ) -> AppResult<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let thread = std::thread::spawn(move || {
            if let Err(e) = run_loopback(stop_thread, bridge) {
                tracing::error!("wasapi loopback capture: {e:?}");
            }
        });
        Ok(Capture {
            stop,
            thread: Some(thread),
        })
    }

    pub fn start_app(
        _bundle_id: &str,
        _sample_rate: u32,
        _channels: u32,
        _bridge: BroadcastRx,
    ) -> AppResult<Self> {
        Err(AppError::Stream(
            "app-audio capture is not implemented on Windows yet".into(),
        ))
    }
}

// The pipeline resamples from this to the output rate, so report the loopback
// device's actual mix rate rather than assuming 48 kHz.
pub fn loopback_mix_rate() -> AppResult<u32> {
    unsafe {
        ensure_com();
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(com_err)?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(com_err)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(com_err)?;
        let pwfx = client.GetMixFormat().map_err(com_err)?;
        let rate = (*pwfx).nSamplesPerSec;
        CoTaskMemFree(Some(pwfx as *const _));
        Ok(rate)
    }
}

fn ensure_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn com_err(e: windows::core::Error) -> AppError {
    AppError::Host(format!("wasapi: {e}"))
}

fn run_loopback(stop: Arc<AtomicBool>, mut bridge: BroadcastRx) -> AppResult<()> {
    unsafe {
        ensure_com();
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(com_err)?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(com_err)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(com_err)?;

        let pwfx = client.GetMixFormat().map_err(com_err)?;
        let wfx: WAVEFORMATEX = *pwfx;
        // Copy packed fields into aligned locals before use.
        let channels = wfx.nChannels as usize;
        let rate = wfx.nSamplesPerSec;
        let bits = wfx.wBitsPerSample;
        if bits != 32 {
            CoTaskMemFree(Some(pwfx as *const _));
            return Err(AppError::Stream(format!(
                "loopback mix format is {bits}-bit, expected 32-bit float"
            )));
        }

        // 200 ms shared-mode buffer; loopback is drained by polling below.
        const BUFFER_HNS: i64 = 2_000_000;
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                BUFFER_HNS,
                0,
                pwfx,
                None,
            )
            .map_err(com_err)?;
        CoTaskMemFree(Some(pwfx as *const _));

        let capture: IAudioCaptureClient = client.GetService().map_err(com_err)?;
        client.Start().map_err(com_err)?;

        // Loopback delivers nothing while the render endpoint is idle. Pace the
        // thread to the wall clock and emit real-time silence on idle ticks so
        // the source never looks stalled (which would make the OnAvailability
        // recorder free-run and balloon the file).
        let frames_per_tick = (rate / 100).max(1) as u64;
        let tick = Duration::from_millis(10);
        let start = Instant::now();
        let mut delivered: u64 = 0;
        let mut next = start;
        let mut stereo: Vec<f32> = Vec::with_capacity(4096);
        while !stop.load(Ordering::Relaxed) {
            next += tick;
            loop {
                let packet = capture.GetNextPacketSize().map_err(com_err)?;
                if packet == 0 {
                    break;
                }
                let mut pdata: *mut u8 = std::ptr::null_mut();
                let mut nframes: u32 = 0;
                let mut flags: u32 = 0;
                capture
                    .GetBuffer(&mut pdata, &mut nframes, &mut flags, None, None)
                    .map_err(com_err)?;
                let frames = nframes as usize;

                stereo.clear();
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || pdata.is_null() {
                    stereo.resize(frames * 2, 0.0);
                } else {
                    let src = std::slice::from_raw_parts(pdata as *const f32, frames * channels);
                    for f in 0..frames {
                        let base = f * channels;
                        let l = src[base];
                        let r = if channels > 1 { src[base + 1] } else { l };
                        stereo.push(l);
                        stereo.push(r);
                    }
                }
                capture.ReleaseBuffer(nframes).map_err(com_err)?;

                delivered += frames as u64;
                bridge.apply_commands();
                bridge.broadcast(&stereo);
            }
            // Pad real-time silence only when genuinely behind (idle endpoint),
            // not on per-tick jitter, so active audio gets no silence holes.
            let expected = (start.elapsed().as_secs_f64() * rate as f64) as u64;
            if expected > delivered + frames_per_tick {
                let deficit = (expected - delivered) as usize;
                stereo.clear();
                stereo.resize(deficit * 2, 0.0);
                bridge.apply_commands();
                bridge.broadcast(&stereo);
                delivered += deficit as u64;
            }
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }

        let _ = client.Stop();
        Ok(())
    }
}
