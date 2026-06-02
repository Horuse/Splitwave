use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{implement, Interface, Ref, IUnknown, HRESULT, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::{
    eConsole, eRender, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
use windows::Win32::System::Variant::VT_BLOB;

use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

const TARGET_RATE: u32 = 48_000;
const TARGET_CHANNELS: u16 = 2;
// 200 ms shared-mode buffer; we drain it by polling.
const BUFFER_HNS: i64 = 2_000_000;

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
        Ok(spawn(bridge, run_loopback))
    }

    pub fn start_app(
        bundle_id: &str,
        _sample_rate: u32,
        _channels: u32,
        bridge: BroadcastRx,
    ) -> AppResult<Self> {
        let pid = crate::audio::system_audio::pid_for_exe(bundle_id).ok_or_else(|| {
            AppError::Stream(format!("no active audio session found for {bundle_id:?}"))
        })?;
        Ok(spawn(bridge, move |stop, bridge| {
            run_process_loopback(pid, stop, bridge)
        }))
    }
}

fn spawn(
    bridge: BroadcastRx,
    run: impl FnOnce(Arc<AtomicBool>, BroadcastRx) -> AppResult<()> + Send + 'static,
) -> Capture {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let thread = std::thread::spawn(move || {
        if let Err(e) = run(stop_thread, bridge) {
            tracing::error!("wasapi capture: {e:?}");
        }
    });
    Capture {
        stop,
        thread: Some(thread),
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

fn run_loopback(stop: Arc<AtomicBool>, bridge: BroadcastRx) -> AppResult<()> {
    unsafe {
        ensure_com();
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(com_err)?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(com_err)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(com_err)?;

        let pwfx = client.GetMixFormat().map_err(com_err)?;
        let channels = (*pwfx).nChannels as usize;
        let rate = (*pwfx).nSamplesPerSec;
        let bits = (*pwfx).wBitsPerSample;
        if bits != 32 {
            CoTaskMemFree(Some(pwfx as *const _));
            return Err(AppError::Stream(format!(
                "loopback mix format is {bits}-bit, expected 32-bit float"
            )));
        }
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
        let r = pump(&capture, channels, rate, &stop, bridge);
        let _ = client.Stop();
        r
    }
}

// Per-app capture via the Win10 2004+ process-loopback activation. The virtual
// device has no mix format, so we ask for 48 kHz stereo f32 explicitly.
fn run_process_loopback(pid: u32, stop: Arc<AtomicBool>, bridge: BroadcastRx) -> AppResult<()> {
    unsafe {
        ensure_com();
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        let mut prop: PROPVARIANT = std::mem::zeroed();
        let pv = &mut *prop.Anonymous.Anonymous;
        pv.vt = VT_BLOB;
        pv.Anonymous.blob.cbSize = std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32;
        pv.Anonymous.blob.pBlobData = &mut params as *mut _ as *mut u8;

        let event = CreateEventW(None, false, false, PCWSTR::null()).map_err(com_err)?;
        let handler: IActivateAudioInterfaceCompletionHandler =
            ActivateHandler { event }.into();

        let op: IActivateAudioInterfaceAsyncOperation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&prop),
            &handler,
        )
        .map_err(com_err)?;

        WaitForSingleObject(event, INFINITE);
        let _ = CloseHandle(event);
        // The BLOB points at the stack `params`; skip PROPVARIANT's Drop so it
        // doesn't try to CoTaskMemFree a non-heap pointer (crash on teardown).
        std::mem::forget(prop);

        let mut hr = HRESULT(0);
        let mut unknown: Option<IUnknown> = None;
        op.GetActivateResult(&mut hr, &mut unknown).map_err(com_err)?;
        hr.ok().map_err(com_err)?;
        let client: IAudioClient = unknown
            .ok_or_else(|| AppError::Stream("process loopback returned no client".into()))?
            .cast()
            .map_err(com_err)?;

        let wfx = WAVEFORMATEX {
            wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
            nChannels: TARGET_CHANNELS,
            nSamplesPerSec: TARGET_RATE,
            nAvgBytesPerSec: TARGET_RATE * TARGET_CHANNELS as u32 * 4,
            nBlockAlign: TARGET_CHANNELS * 4,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                BUFFER_HNS,
                0,
                &wfx,
                None,
            )
            .map_err(com_err)?;

        let capture: IAudioCaptureClient = client.GetService().map_err(com_err)?;
        client.Start().map_err(com_err)?;
        let r = pump(&capture, TARGET_CHANNELS as usize, TARGET_RATE, &stop, bridge);
        let _ = client.Stop();
        r
    }
}

// Drain capture packets, downmix to stereo f32, and broadcast. Loopback delivers
// nothing while the endpoint is idle, so pace to the wall clock and emit
// real-time silence on idle ticks (only when genuinely behind, not on jitter)
// so the source never looks stalled.
unsafe fn pump(
    capture: &IAudioCaptureClient,
    channels: usize,
    rate: u32,
    stop: &AtomicBool,
    mut bridge: BroadcastRx,
) -> AppResult<()> {
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
    Ok(())
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivateHandler {
    event: HANDLE,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivateHandler_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        unsafe {
            let _ = windows::Win32::System::Threading::SetEvent(self.event);
        }
        Ok(())
    }
}
