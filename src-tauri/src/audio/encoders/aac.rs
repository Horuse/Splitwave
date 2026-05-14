//! AAC-LC encoder writing M4A via AVAudioFile (macOS). Streaming — file is
//! finalised on Drop. Sample rate is whatever the pipeline delivers (AAC
//! supports the full 8–96 kHz range).

use std::ffi::{c_void, CString};
use std::os::raw::c_char;
use std::path::Path;

use super::AudioEncoder;
use crate::error::{AppError, AppResult};

extern "C" {
	fn ba_aac_create(
		path: *const c_char,
		sample_rate: i32,
		channels: i32,
		bitrate: i32,
	) -> *mut c_void;
	fn ba_aac_write(handle: *mut c_void, samples: *const f32, frames: i32) -> i32;
	fn ba_aac_destroy(handle: *mut c_void);
}

pub struct AacRecorder {
	handle: *mut c_void,
}

unsafe impl Send for AacRecorder {}

impl AacRecorder {
	pub fn create(path: &Path, sample_rate: u32, bitrate_bps: u32) -> AppResult<Self> {
		let path_str = path
			.to_str()
			.ok_or_else(|| AppError::Stream(format!("invalid path: {}", path.display())))?;
		let cpath = CString::new(path_str)
			.map_err(|_| AppError::Stream("nul in path".into()))?;
		let handle = unsafe {
			ba_aac_create(
				cpath.as_ptr(),
				sample_rate as i32,
				2,
				bitrate_bps as i32,
			)
		};
		if handle.is_null() {
			return Err(AppError::Stream(format!(
				"AAC encoder init failed for {}",
				path.display()
			)));
		}
		Ok(Self { handle })
	}
}

impl AudioEncoder for AacRecorder {
	fn write_stereo(&mut self, samples: &[f32]) -> AppResult<()> {
		debug_assert!(samples.len() % 2 == 0, "stereo buffer must be even length");
		let frames = (samples.len() / 2) as i32;
		if frames == 0 {
			return Ok(());
		}
		let rc = unsafe { ba_aac_write(self.handle, samples.as_ptr(), frames) };
		if rc != 0 {
			return Err(AppError::Stream(format!("AAC write failed: code {rc}")));
		}
		Ok(())
	}

	/// AVAudioFile buffers internally — no streaming flush hook is exposed.
	/// Bytes hit disk when the file finalises on drop.
	fn flush(&mut self) -> AppResult<()> {
		Ok(())
	}

	fn finalize(self: Box<Self>) -> AppResult<()> {
		// Drop runs after this scope returns; AVAudioFile closes there.
		Ok(())
	}
}

impl Drop for AacRecorder {
	fn drop(&mut self) {
		if !self.handle.is_null() {
			unsafe { ba_aac_destroy(self.handle) };
			self.handle = std::ptr::null_mut();
		}
	}
}
