//! Shared memory and event synchronization for real-time audio transfer.

#![cfg(target_os = "windows")]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
    MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_ALL_ACCESS,
};

pub const MAX_SHM_FRAMES: usize = 4096;
pub const MAX_SHM_CHANNELS: usize = 8;
pub const MAX_SHM_SAMPLES: usize = MAX_SHM_FRAMES * MAX_SHM_CHANNELS;

const SHM_MAGIC: u32 = 0x53504C57; // 'SPLW'

#[repr(C)]
pub struct ShmHeader {
    pub magic: AtomicU32,
    pub sample_rate: AtomicU32,
    pub channels: AtomicU32,
    pub frames_in: AtomicU32,
    pub frames_out: AtomicU32,
    pub latency_frames: AtomicU32,
    pub host_seq: AtomicU64,
    pub helper_seq: AtomicU64,
    pub alive: AtomicBool,
}

#[repr(C)]
pub struct ShmBuffer {
    pub header: ShmHeader,
    pub input_samples: [f32; MAX_SHM_SAMPLES],
    pub output_samples: [f32; MAX_SHM_SAMPLES],
}

pub struct ShmHost {
    mapping: HANDLE,
    view: *mut ShmBuffer,
    host_event: HANDLE,
    helper_event: HANDLE,
    seq: u64,
}

unsafe impl Send for ShmHost {}
unsafe impl Sync for ShmHost {}

impl ShmHost {
    pub fn create(session_id: &str) -> Result<Self, String> {
        unsafe {
            let map_name = HSTRING::from(format!("Local\\Splitwave_SHM_{session_id}"));
            let host_ev_name = HSTRING::from(format!("Local\\Splitwave_HostEv_{session_id}"));
            let helper_ev_name = HSTRING::from(format!("Local\\Splitwave_HelperEv_{session_id}"));

            let size = std::mem::size_of::<ShmBuffer>() as u32;
            let mapping = CreateFileMappingW(
                HANDLE::default(),
                None,
                PAGE_READWRITE,
                0,
                size,
                PCWSTR(map_name.as_ptr()),
            )
            .map_err(|e| format!("CreateFileMappingW failed: {e}"))?;

            let view_ptr = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, size as usize);
            if view_ptr.Value.is_null() {
                let _ = CloseHandle(mapping);
                return Err("MapViewOfFile returned null".into());
            }
            let view = view_ptr.Value as *mut ShmBuffer;

            // Initialize header
            let header = &mut (*view).header;
            header.magic.store(SHM_MAGIC, Ordering::Release);
            header.channels.store(2, Ordering::Release);
            header.frames_in.store(0, Ordering::Release);
            header.frames_out.store(0, Ordering::Release);
            header.latency_frames.store(0, Ordering::Release);
            header.host_seq.store(0, Ordering::Release);
            header.helper_seq.store(0, Ordering::Release);
            header.alive.store(true, Ordering::Release);

            // Auto-reset events
            let host_event = CreateEventW(None, false, false, PCWSTR(host_ev_name.as_ptr()))
                .map_err(|e| format!("CreateEventW host_event failed: {e}"))?;

            let helper_event = CreateEventW(None, false, false, PCWSTR(helper_ev_name.as_ptr()))
                .map_err(|e| {
                    let _ = CloseHandle(host_event);
                    format!("CreateEventW helper_event failed: {e}")
                })?;

            Ok(Self {
                mapping,
                view,
                host_event,
                helper_event,
                seq: 0,
            })
        }
    }

    /// Process a block of audio through the shared memory bridge.
    /// `samples` must contain interleaved audio (frames * channels).
    pub fn process(
        &mut self,
        samples: &mut [f32],
        frames: usize,
        channels: usize,
    ) -> Result<usize, String> {
        let total_samples = frames * channels;
        if total_samples == 0 {
            return Ok(0);
        }
        if total_samples > MAX_SHM_SAMPLES {
            return Err("block size exceeds maximum shared memory buffer capacity".into());
        }

        unsafe {
            let buf = &mut *self.view;
            if !buf.header.alive.load(Ordering::Acquire) {
                samples.fill(0.0);
                return Ok(0);
            }

            self.seq += 1;
            buf.header
                .channels
                .store(channels as u32, Ordering::Relaxed);
            buf.header.frames_in.store(frames as u32, Ordering::Relaxed);
            buf.header.host_seq.store(self.seq, Ordering::Release);

            // Copy input samples
            let input_slice = &mut buf.input_samples[..total_samples];
            input_slice.copy_from_slice(&samples[..total_samples]);

            // Signal helper that input is ready
            let _ = SetEvent(self.host_event);

            // Wait for helper to finish processing (timeout 25ms to prevent hanging RT audio thread)
            let wait_res = WaitForSingleObject(self.helper_event, 25);
            if wait_res == WAIT_OBJECT_0 {
                let out_slice = &buf.output_samples[..total_samples];
                samples[..total_samples].copy_from_slice(out_slice);
                let latency = buf.header.latency_frames.load(Ordering::Acquire) as usize;
                Ok(latency)
            } else {
                // Timeout or error: output silence
                samples.fill(0.0);
                if wait_res == WAIT_TIMEOUT {
                    tracing::warn!("plugin bridge helper process timed out on audio block");
                }
                Ok(0)
            }
        }
    }

    pub fn mark_dead(&self) {
        unsafe {
            if !self.view.is_null() {
                (*self.view).header.alive.store(false, Ordering::Release);
            }
            let _ = SetEvent(self.host_event);
        }
    }
}

impl Drop for ShmHost {
    fn drop(&mut self) {
        self.mark_dead();
        unsafe {
            if !self.view.is_null() {
                let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.view as *mut c_void,
                });
            }
            if !self.host_event.is_invalid() {
                let _ = CloseHandle(self.host_event);
            }
            if !self.helper_event.is_invalid() {
                let _ = CloseHandle(self.helper_event);
            }
            if !self.mapping.is_invalid() {
                let _ = CloseHandle(self.mapping);
            }
        }
    }
}

pub struct ShmHelper {
    mapping: HANDLE,
    view: *mut ShmBuffer,
    host_event: HANDLE,
    helper_event: HANDLE,
}

unsafe impl Send for ShmHelper {}
unsafe impl Sync for ShmHelper {}

impl ShmHelper {
    pub fn open(session_id: &str) -> Result<Self, String> {
        unsafe {
            let map_name = HSTRING::from(format!("Local\\Splitwave_SHM_{session_id}"));
            let host_ev_name = HSTRING::from(format!("Local\\Splitwave_HostEv_{session_id}"));
            let helper_ev_name = HSTRING::from(format!("Local\\Splitwave_HelperEv_{session_id}"));

            let mapping = OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, PCWSTR(map_name.as_ptr()))
                .map_err(|e| format!("OpenFileMappingW failed: {e}"))?;

            let size = std::mem::size_of::<ShmBuffer>();
            let view_ptr = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, size);
            if view_ptr.Value.is_null() {
                let _ = CloseHandle(mapping);
                return Err("MapViewOfFile returned null in helper".into());
            }
            let view = view_ptr.Value as *mut ShmBuffer;

            let host_event = OpenEventW(EVENT_ALL_ACCESS, false, PCWSTR(host_ev_name.as_ptr()))
                .map_err(|e| format!("OpenEventW host_event failed: {e}"))?;

            let helper_event = OpenEventW(EVENT_ALL_ACCESS, false, PCWSTR(helper_ev_name.as_ptr()))
                .map_err(|e| {
                    let _ = CloseHandle(host_event);
                    format!("OpenEventW helper_event failed: {e}")
                })?;

            Ok(Self {
                mapping,
                view,
                host_event,
                helper_event,
            })
        }
    }

    /// Wait for input from the host.
    /// Returns `Ok(Some((frames, channels)))` when input is available, `Ok(None)` on timeout.
    pub fn wait_for_input(&self, timeout_ms: u32) -> Result<Option<(usize, usize)>, String> {
        unsafe {
            let wait_res = WaitForSingleObject(self.host_event, timeout_ms);
            if wait_res == WAIT_OBJECT_0 {
                let buf = &*self.view;
                if !buf.header.alive.load(Ordering::Acquire) {
                    return Ok(None);
                }
                let frames = buf.header.frames_in.load(Ordering::Acquire) as usize;
                let channels = buf.header.channels.load(Ordering::Acquire) as usize;
                Ok(Some((frames, channels)))
            } else if wait_res == WAIT_TIMEOUT {
                let buf = &*self.view;
                if !buf.header.alive.load(Ordering::Acquire) {
                    return Ok(None);
                }
                Ok(None)
            } else {
                Err("WaitForSingleObject failed on host_event".into())
            }
        }
    }

    /// Access input samples buffer.
    pub fn read_input(&self, total_samples: usize) -> &[f32] {
        unsafe {
            let buf = &*self.view;
            &buf.input_samples[..total_samples.min(MAX_SHM_SAMPLES)]
        }
    }

    /// Write output samples and notify host.
    pub fn write_output_and_signal(&mut self, output: &[f32], frames: usize, latency: usize) {
        unsafe {
            let buf = &mut *self.view;
            let len = output.len().min(MAX_SHM_SAMPLES);
            buf.output_samples[..len].copy_from_slice(&output[..len]);
            buf.header
                .frames_out
                .store(frames as u32, Ordering::Release);
            buf.header
                .latency_frames
                .store(latency as u32, Ordering::Release);
            let _ = SetEvent(self.helper_event);
        }
    }

    pub fn is_alive(&self) -> bool {
        unsafe {
            if self.view.is_null() {
                false
            } else {
                (*self.view).header.alive.load(Ordering::Acquire)
            }
        }
    }
}

impl Drop for ShmHelper {
    fn drop(&mut self) {
        unsafe {
            if !self.view.is_null() {
                let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.view as *mut c_void,
                });
            }
            if !self.host_event.is_invalid() {
                let _ = CloseHandle(self.host_event);
            }
            if !self.helper_event.is_invalid() {
                let _ = CloseHandle(self.helper_event);
            }
            if !self.mapping.is_invalid() {
                let _ = CloseHandle(self.mapping);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_shm_audio_roundtrip() {
        let session_id = format!("test_shm_{}", cuid2::create_id());
        let mut host = ShmHost::create(&session_id).expect("create host shm");
        let helper_session_id = session_id.clone();

        let helper_handle = thread::spawn(move || {
            let mut helper = ShmHelper::open(&helper_session_id).expect("open helper shm");
            for _ in 0..4 {
                if let Ok(Some((frames, channels))) = helper.wait_for_input(100) {
                    let total = frames * channels;
                    let input = helper.read_input(total);
                    let mut processed = input.to_vec();
                    // Multiply gain by 2.0
                    for s in &mut processed {
                        *s *= 2.0;
                    }
                    helper.write_output_and_signal(&processed, frames, 16);
                }
            }
        });

        const FRAMES: usize = 512;
        const CHANNELS: usize = 2;
        for i in 0..4 {
            let mut buffer = vec![0.25f32; FRAMES * CHANNELS];
            let latency = host
                .process(&mut buffer, FRAMES, CHANNELS)
                .expect("host process");
            assert_eq!(latency, 16, "iteration {i}: latency matches");
            for (idx, &sample) in buffer.iter().enumerate() {
                assert!(
                    (sample - 0.5).abs() < 1e-6,
                    "sample at {idx} is {sample}, expected 0.5"
                );
            }
        }

        helper_handle.join().expect("helper thread finished");
    }
}
