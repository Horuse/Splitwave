//! Audio Unit discovery. AU plugins are not found by walking directories: the
//! system component manager owns the registry, and a v3 AU may live inside an
//! app bundle with no `.component` on disk at all. So this backend enumerates
//! `AudioComponent`s instead of implementing the default directory scan.

use std::path::PathBuf;
use std::ptr::{self, NonNull};

use objc2_audio_toolbox::{
    AudioComponent, AudioComponentCopyName, AudioComponentDescription, AudioComponentFindNext,
    AudioComponentGetDescription, AudioComponentGetVersion,
};
use objc2_core_foundation::{CFRetained, CFString};

use super::{PluginBackend, PluginDescriptor, PluginFormat};

pub(super) const fn fourcc(s: &[u8; 4]) -> u32 {
    ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | (s[3] as u32)
}

fn fourcc_str(v: u32) -> String {
    let bytes = v.to_be_bytes();
    // A type/subtype/manufacturer code is four printable ASCII bytes by
    // convention, but nothing enforces it, so a stray byte becomes hex.
    if bytes.iter().all(|b| (0x20..0x7f).contains(b)) {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        format!("{v:08x}")
    }
}

fn fourcc_from_str(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.len() == 4 {
        return Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    }
    // The hex form `fourcc_str` falls back to for non-printable codes.
    (b.len() == 8)
        .then(|| u32::from_str_radix(s, 16).ok())
        .flatten()
}

/// Effects only. An instrument (`aumu`) is driven by MIDI and has no audio
/// input, so it cannot sit in a signal chain the way a graph node must.
const SCANNED_TYPES: [u32; 2] = [fourcc(b"aufx"), fourcc(b"aumf")];

/// Identifies an AU by its component triple rather than a file path, and marks
/// it as an AU so activation can pick the backend without a second field in the
/// stored node data.
fn component_url(desc: &AudioComponentDescription) -> String {
    format!(
        "au://{}/{}/{}",
        fourcc_str(desc.componentType),
        fourcc_str(desc.componentSubType),
        fourcc_str(desc.componentManufacturer)
    )
}

/// Inverse of [`component_url`]. `None` for anything that is not an AU
/// reference, which is how activation tells the two formats apart.
pub(super) fn parse_component_url(url: &str) -> Option<AudioComponentDescription> {
    let rest = url.strip_prefix("au://")?;
    let mut parts = rest.split('/');
    let ty = fourcc_from_str(parts.next()?)?;
    let sub = fourcc_from_str(parts.next()?)?;
    let manu = fourcc_from_str(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(AudioComponentDescription {
        componentType: ty,
        componentSubType: sub,
        componentManufacturer: manu,
        componentFlags: 0,
        componentFlagsMask: 0,
    })
}

/// Looks up the one registered component matching `desc` exactly.
pub(super) fn find_component(desc: &AudioComponentDescription) -> AudioComponent {
    // SAFETY: a null start argument means "first match"; `desc` is valid.
    unsafe { AudioComponentFindNext(ptr::null_mut(), NonNull::from(desc)) }
}

pub struct AuBackend;

impl PluginBackend for AuBackend {
    fn format(&self) -> PluginFormat {
        PluginFormat::Au
    }

    fn scan(&self) -> Vec<PluginDescriptor> {
        let mut out = Vec::new();
        for ty in SCANNED_TYPES {
            let search = AudioComponentDescription {
                componentType: ty,
                componentSubType: 0,
                componentManufacturer: 0,
                componentFlags: 0,
                componentFlagsMask: 0,
            };
            let mut comp: AudioComponent = ptr::null_mut();
            loop {
                // SAFETY: `comp` is either null (start) or a component handle
                // returned by this same call.
                comp = unsafe { AudioComponentFindNext(comp, NonNull::from(&search)) };
                if comp.is_null() {
                    break;
                }
                if let Some(d) = describe(comp) {
                    out.push(d);
                }
            }
        }
        out
    }

    fn search_dirs(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn extension(&self) -> &'static str {
        "component"
    }
}

fn describe(comp: AudioComponent) -> Option<PluginDescriptor> {
    let mut desc = AudioComponentDescription {
        componentType: 0,
        componentSubType: 0,
        componentManufacturer: 0,
        componentFlags: 0,
        componentFlagsMask: 0,
    };
    // SAFETY: `comp` came from AudioComponentFindNext and is non-null.
    if unsafe { AudioComponentGetDescription(comp, NonNull::from(&mut desc)) } != 0 {
        return None;
    }

    let mut name: *const CFString = ptr::null();
    // SAFETY: the call writes an owned CFStringRef on success.
    if unsafe { AudioComponentCopyName(comp, NonNull::from(&mut name)) } != 0 {
        return None;
    }
    // SAFETY: `AudioComponentCopyName` follows the CF copy rule, so the
    // reference is ours and `CFRetained` releases it.
    let full = unsafe { CFRetained::from_raw(NonNull::new(name.cast_mut())?) }.to_string();

    // AudioComponentCopyName yields "Manufacturer: Product".
    let (vendor, name) = match full.split_once(": ") {
        Some((v, n)) => (v.trim().to_string(), n.trim().to_string()),
        None => (String::new(), full),
    };

    let mut raw_version = 0u32;
    // SAFETY: `comp` is a live component; the call only writes the version.
    let version = if unsafe { AudioComponentGetVersion(comp, NonNull::from(&mut raw_version)) } == 0
    {
        format!(
            "{}.{}.{}",
            raw_version >> 16,
            (raw_version >> 8) & 0xff,
            raw_version & 0xff
        )
    } else {
        String::new()
    };

    Some(PluginDescriptor {
        uid: format!("au:{}", component_url(&desc)),
        format: PluginFormat::Au,
        plugin_id: fourcc_str(desc.componentSubType),
        path: component_url(&desc),
        name,
        vendor,
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS always ships Apple's own `aufx` units, so an empty scan means the
    /// component enumeration itself is broken, not that the machine is bare.
    #[test]
    fn lists_installed_audio_units() {
        let found = AuBackend.scan();
        assert!(!found.is_empty(), "no audio units found");
        assert!(found.iter().all(|d| d.path.starts_with("au://")));
    }

    #[test]
    fn component_url_round_trips() {
        let desc = parse_component_url("au://aufx/nbeq/appl").expect("parses");
        assert_eq!(desc.componentType, fourcc(b"aufx"));
        assert_eq!(component_url(&desc), "au://aufx/nbeq/appl");
        assert!(parse_component_url("/Library/Audio/Plug-Ins/CLAP/x.clap").is_none());
    }
}
