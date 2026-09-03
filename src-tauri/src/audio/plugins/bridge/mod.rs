//! Out-of-Process Plugin Bridge for Windows.

#![cfg(target_os = "windows")]

pub mod bridge_host;
pub mod helper_main;
pub mod protocol;
pub mod shm_audio;

pub use bridge_host::{BridgeHost, BridgeNode};
