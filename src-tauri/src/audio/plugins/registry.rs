//! The only place a plugin format is chosen.
//!
//! Two maps and nothing else: format to its host, and node id to the host that
//! last activated it. Every other entry point is a lookup plus one call, so a
//! new format is a `PluginHost` impl and one line here.
//!
//! Changing a node's owner *is* forgetting the previous one, which is what
//! makes a stale registration -- a node that keeps answering as the format it
//! no longer runs -- impossible rather than merely unlikely.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[cfg(target_os = "macos")]
use super::au_host::AuHost;
use super::clap_registry::ClapHost;
use super::host_api::{ActivateRequest, HostedNode, PluginHost};
use super::vst3_registry::Vst3Host;
use super::PluginFormat;

fn host_for(format: PluginFormat) -> &'static dyn PluginHost {
    match format {
        PluginFormat::Clap => &ClapHost,
        #[cfg(target_os = "macos")]
        PluginFormat::Au => &AuHost,
        PluginFormat::Vst3 => &Vst3Host,
    }
}

fn owners() -> &'static Mutex<HashMap<String, PluginFormat>> {
    static OWNERS: OnceLock<Mutex<HashMap<String, PluginFormat>>> = OnceLock::new();
    OWNERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Every registered host, for work that is not tied to one node.
pub fn hosts() -> impl Iterator<Item = &'static dyn PluginHost> {
    [
        &ClapHost as &'static dyn PluginHost,
        #[cfg(target_os = "macos")]
        &AuHost,
        &Vst3Host,
    ]
    .into_iter()
}

/// The host currently running this node, or `None` if it runs no plugin.
pub fn for_node(node_id: &str) -> Option<&'static dyn PluginHost> {
    owners().lock().unwrap().get(node_id).copied().map(host_for)
}

/// Instantiates through the format's host, taking ownership of the node away
/// from whichever host held it before.
pub fn activate(format: PluginFormat, req: ActivateRequest<'_>) -> Result<HostedNode, String> {
    let node_id = req.node_id.to_string();
    let primary = req.primary;

    let previous = owners().lock().unwrap().get(&node_id).copied();
    if previous.is_some_and(|p| p != format) {
        // Only the outgoing host can release its own hold; the instance itself
        // survives until its RT node leaves the old graph.
        host_for(previous.expect("checked")).forget(&node_id);
        owners().lock().unwrap().remove(&node_id);
    }

    let node = host_for(format).activate(req)?;
    if primary {
        owners().lock().unwrap().insert(node_id, format);
    }
    Ok(node)
}

/// Releases a node entirely: used when its plugin is cleared, when a rebuild
/// fails to load one, and when the pipeline that owned it goes away.
pub fn forget(node_id: &str) {
    let owner = owners().lock().unwrap().remove(node_id);
    if let Some(format) = owner {
        host_for(format).forget(node_id);
    }
}
