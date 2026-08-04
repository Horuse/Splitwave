//! Moves a captured app off the speakers instead of copying its audio.
//!
//! A PipeWire capture stream is a tap: the app stays linked to its sink, so its
//! output and the graph's copy both reach the speakers. There is no per-stream
//! mute to reach for, but the graph itself is the answer -- retarget the app at
//! a null sink of ours and read that sink's monitor. Nothing is muted; the
//! original simply has nowhere else to go.
//!
//! Retargeting is a `target.object` entry in the default metadata store, which
//! is what WirePlumber reads to place a stream. Removing the entry hands the
//! stream back to normal routing.

use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use tracing::warn;

use crate::audio::pw_enum::nodes_by_class;
use crate::error::{AppError, AppResult};

const SINK_PREFIX: &str = "splitwave.capture";

/// Two App Audio nodes can capture with mute at once, and a name collision
/// would fail the second sink.
static SINK_SEQ: AtomicU32 = AtomicU32::new(0);

/// Restores the app's routing and removes the sink on drop.
pub struct MuteRedirect {
    node_id: u32,
    sink_name: String,
}

impl MuteRedirect {
    /// `node_id` is the app's stream node. Fails rather than falling back to a
    /// plain tap: a silent downgrade here is the doubling this exists to fix.
    pub fn apply(node_id: u32) -> AppResult<Self> {
        let seq = SINK_SEQ.fetch_add(1, Ordering::Relaxed);
        let sink_name = format!("{SINK_PREFIX}.{}.{seq}", std::process::id());
        create_sink(&sink_name)?;
        let redirect = MuteRedirect {
            node_id,
            sink_name: sink_name.clone(),
        };
        set_target(node_id, Some(&sink_name))?;
        Ok(redirect)
    }

    pub fn sink_name(&self) -> &str {
        &self.sink_name
    }
}

impl Drop for MuteRedirect {
    fn drop(&mut self) {
        if let Err(e) = set_target(self.node_id, None) {
            warn!(error = %e, "failed to restore app routing; it stays on the capture sink");
        }
        destroy_sink(&self.sink_name);
    }
}

fn create_sink(name: &str) -> AppResult<()> {
    // object.linger=false ties the node to this pw-cli connection... which exits
    // immediately, so linger has to stay on and `destroy_sink` do the cleanup.
    let props = format!(
        "{{ factory.name=support.null-audio-sink node.name={name} \
         node.description=\"Splitwave Capture\" media.class=Audio/Sink \
         audio.position=[ FL FR ] object.linger=true }}"
    );
    run("pw-cli", &["create-node", "adapter", props.as_str()])
}

fn destroy_sink(name: &str) {
    let Ok(nodes) = nodes_by_class("Audio/Sink") else {
        warn!("failed to enumerate sinks; the capture sink stays until PipeWire restarts");
        return;
    };
    for node in nodes.iter().filter(|n| n.name == name) {
        let id = node.id.to_string();
        if let Err(e) = run("pw-cli", &["destroy", id.as_str()]) {
            warn!(error = %e, "failed to destroy the capture sink");
        }
    }
}

fn set_target(node_id: u32, sink_name: Option<&str>) -> AppResult<()> {
    let id = node_id.to_string();
    match sink_name {
        Some(sink) => run("pw-metadata", &["-n", "default", &id, "target.object", sink]),
        None => run("pw-metadata", &["-n", "default", "-d", &id, "target.object"]),
    }
}

fn run(program: &str, args: &[&str]) -> AppResult<()> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| AppError::Host(format!("{program}: {e}")))?;
    if !out.status.success() {
        return Err(AppError::Host(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}
