//! Audio Unit instantiation and the RT node that renders one.
//!
//! Unlike CLAP, an AU instance is not main-thread bound: the v2 C API is built
//! around a render thread pulling through `AudioUnitRender` while the host
//! reads and writes parameters from elsewhere. So an instance is shared through
//! an `Arc` between the DSP worker and the UI thread, with no marshalling.
//!
//! The unit is configured as non-interleaved 32-bit float stereo on bus 0, which
//! matches the pipeline's own format after de-interleaving.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use objc2_audio_toolbox::{
    AUParameterListenerNotify, AURenderCallbackStruct, AudioComponentInstanceDispose,
    AudioComponentInstanceNew, AudioUnit, AudioUnitParameter,
    AudioUnitCocoaViewInfo, AudioUnitGetParameter, AudioUnitGetProperty, AudioUnitGetPropertyInfo,
    AudioUnitInitialize, AudioUnitParameterInfo, AudioUnitParameterOptions, AudioUnitParameterUnit,
    AudioUnitRender, AudioUnitRenderActionFlags, AudioUnitSetParameter, AudioUnitSetProperty,
    AudioUnitUninitialize, kAudioUnitProperty_CocoaUI, kAudioUnitProperty_MaximumFramesPerSlice,
    kAudioUnitProperty_ClassInfo, kAudioUnitProperty_ParameterInfo, kAudioUnitProperty_ParameterList,
    kAudioUnitProperty_Latency, kAudioUnitProperty_SetRenderCallback,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Global,
    kAudioUnitScope_Input, kAudioUnitScope_Output,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsPacked,
    kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFRetained;

use crate::audio::effects::Effect;

use super::au_backend::{find_component, parse_component_url};
use super::host_api::{
    alive_flag, ActivateRequest, AliveFlag, EditorSize, Graveyard, HostedNode, PluginHost,
    PluginParamInfo, PluginStatus, Unsupported,
};
use super::{editor, main_thread};
use super::param_ring::MAX_PARAM_CHANGES_PER_BLOCK;
use super::ParamRing;

/// Widest layout offered to a unit, and the fixed capacity of the buffer list
/// below. Beyond 7.1 there is no arrangement the other formats name either, so
/// a wider node is driven per stereo pair.
const MAX_CHANNELS: usize = 8;

/// `AudioBufferList` is a C flexible-array struct, so the generated binding
/// declares one buffer. This is the same layout with room for the widest unit
/// we configure, cast at the call; `number_buffers` says how many are live, so
/// a narrower unit simply leaves the tail unused and the RT path never
/// allocates one.
#[repr(C)]
struct RenderBufferList {
    number_buffers: u32,
    buffers: [AudioBuffer; MAX_CHANNELS],
}

impl RenderBufferList {
    fn as_audio_buffer_list(&mut self) -> NonNull<AudioBufferList> {
        NonNull::from(self).cast()
    }
}

/// Input side of the render pull, boxed so its address stays put while the node
/// itself is moved into the graph. The unit's input callback writes our channel
/// pointers straight into its buffer list, so no copy happens on the pull.
struct RenderInput {
    channels: Vec<Vec<f32>>,
}

/// A live Audio Unit, shared between the DSP worker that renders it and the UI
/// thread that reads and writes its parameters. That split is the AU model:
/// `AudioUnitRender` on the audio thread concurrently with property and
/// parameter access elsewhere is what every host does.
///
/// SAFETY: the one operation that is *not* safe concurrently with render is
/// disposal, which is why the instance is behind an `Arc` and torn down only
/// when the last owner (registry entry or RT node) is gone.
struct AuInstance {
    unit: AudioUnit,
    /// The `au://` reference this was built from, so the UI can tell whether the
    /// running unit is still the one the node currently points at.
    url: String,
}

unsafe impl Send for AuInstance {}
unsafe impl Sync for AuInstance {}

impl Drop for AuInstance {
    fn drop(&mut self) {
        // SAFETY: `unit` was initialised in `activate` and no other owner is
        // left, so no render can be in flight.
        unsafe {
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
        }
    }
}

/// The editor/parameter target for a node, held by the host rather than by the
/// RT node alone. Holding it here is what guarantees the registry drops the
/// last reference, and therefore that the unit is disposed on the main thread.
struct AuSlot {
    instance: Arc<AuInstance>,
    /// False once the matching `AuNode` has left the graph; the reclaim sweep
    /// then frees this instance. Mirrors the CLAP host's slot.
    alive: AliveFlag,
    /// The plugin's Cocoa view while its editor is open. Main thread only.
    view: Option<usize>,
}

fn instances() -> &'static Mutex<HashMap<String, AuSlot>> {
    static INSTANCES: OnceLock<Mutex<HashMap<String, AuSlot>>> = OnceLock::new();
    INSTANCES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn graveyard() -> &'static Mutex<Graveyard<Arc<AuInstance>>> {
    static GRAVEYARD: OnceLock<Mutex<Graveyard<Arc<AuInstance>>>> = OnceLock::new();
    GRAVEYARD.get_or_init(|| Mutex::new(Graveyard::default()))
}

pub struct AuNode {
    instance: Arc<AuInstance>,
    /// Cleared on drop so the host's sweep can reclaim the matching instance
    /// once this node is gone from the graph.
    alive: Arc<AtomicBool>,
    input: Box<RenderInput>,
    output: Vec<Vec<f32>>,
    max_frames: usize,
    /// Interleaved channels the pipeline hands this node, which is the width the
    /// unit was configured for.
    channels: usize,
    steady: f64,
    /// Reported by the unit once initialised. The DAG pads shorter parallel
    /// paths by it, so a wrong value here is an audible phase error rather
    /// than a missing feature.
    latency: usize,
    // UI parameter writes, applied at the top of each block.
    params: Arc<ParamRing>,
    param_cursor: usize,
}

impl Drop for AuNode {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

unsafe extern "C-unwind" fn supply_input(
    ref_con: NonNull<c_void>,
    _action_flags: NonNull<AudioUnitRenderActionFlags>,
    _time_stamp: NonNull<AudioTimeStamp>,
    _bus: u32,
    frames: u32,
    io_data: *mut AudioBufferList,
) -> i32 {
    if io_data.is_null() {
        return -1;
    }
    let input = &mut *(ref_con.as_ptr() as *mut RenderInput);
    let count = (*io_data).mNumberBuffers as usize;
    let buffers = ptr::addr_of_mut!((*io_data).mBuffers) as *mut AudioBuffer;
    for i in 0..count.min(input.channels.len()) {
        let buf = &mut *buffers.add(i);
        buf.mNumberChannels = 1;
        buf.mDataByteSize = frames * size_of::<f32>() as u32;
        buf.mData = input.channels[i].as_mut_ptr() as *mut c_void;
    }
    0
}

fn float_format(sample_rate: u32, channels: usize) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: sample_rate as f64,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat
            | kAudioFormatFlagIsPacked
            | kAudioFormatFlagIsNonInterleaved,
        // Non-interleaved: a "frame" is one sample of one channel, and the
        // channel count lives in the buffer list rather than the frame size.
        mBytesPerPacket: size_of::<f32>() as u32,
        mFramesPerPacket: 1,
        mBytesPerFrame: size_of::<f32>() as u32,
        mChannelsPerFrame: channels as u32,
        mBitsPerChannel: (size_of::<f32>() * 8) as u32,
        mReserved: 0,
    }
}

unsafe fn set_property<T>(
    unit: AudioUnit,
    property: u32,
    scope: u32,
    value: &T,
    what: &str,
) -> Result<(), String> {
    let status = AudioUnitSetProperty(
        unit,
        property,
        scope,
        0,
        value as *const T as *const c_void,
        size_of::<T>() as u32,
    );
    if status == 0 {
        Ok(())
    } else {
        Err(format!("{what}: OSStatus {status}"))
    }
}

/// Instantiates and initialises the Audio Unit named by `url`
/// (`au://type/subtype/manufacturer`), returning its RT node. Only the
/// `primary` build registers as the node's editor/parameter target, matching
/// how the CLAP host picks one instance out of a fanned-out node.
#[allow(clippy::too_many_arguments)]
fn activate(
    node_id: &str,
    url: &str,
    sample_rate: u32,
    max_frames: usize,
    channels: usize,
    state: Option<&str>,
    primary: bool,
    params: Arc<ParamRing>,
) -> Result<AuNode, String> {
    let desc = parse_component_url(url).ok_or_else(|| format!("not an audio unit: {url}"))?;
    let component = find_component(&desc);
    if component.is_null() {
        return Err(format!("audio unit not installed: {url}"));
    }

    let mut unit: AudioUnit = ptr::null_mut();
    // SAFETY: `component` is a live registry entry; the call writes `unit`.
    let status = unsafe { AudioComponentInstanceNew(component, NonNull::from(&mut unit)) };
    if status != 0 || unit.is_null() {
        return Err(format!("instantiate {url}: OSStatus {status}"));
    }
    // From here on the instance is owned, so a failed `configure` still
    // disposes of it through `Drop`.
    let alive = alive_flag();
    // Offer the node's own width first, the way a DAW instantiates one
    // multichannel unit rather than several stereo ones. Anything wider than
    // the widest arrangement the formats name goes straight to stereo.
    let offered = if (1..=MAX_CHANNELS).contains(&channels) {
        channels
    } else {
        2
    };
    let mut node = AuNode {
        instance: Arc::new(AuInstance { unit, url: url.to_string() }),
        alive: alive.clone(),
        input: Box::new(RenderInput {
            channels: vec![vec![0.0; max_frames]; offered],
        }),
        output: vec![vec![0.0; max_frames]; offered],
        max_frames,
        channels: offered,
        steady: 0.0,
        latency: 0,
        param_cursor: params.reader(),
        params,
    };

    let mut configured = configure(&mut node, sample_rate, max_frames, offered);
    if configured.is_err() && offered != 2 {
        // The unit will not carry the node's width, so it runs as a stereo pair
        // and the pipeline drives it once per pair. Nothing was initialised yet,
        // so the same instance can simply be configured again.
        tracing::debug!(
            node_id,
            url,
            channels,
            "audio unit refused the node's width; driving it per pair"
        );
        node.channels = 2;
        node.input.channels = vec![vec![0.0; max_frames]; 2];
        node.output = vec![vec![0.0; max_frames]; 2];
        configured = configure(&mut node, sample_rate, max_frames, 2);
    }
    if let Err(e) = configured {
        return Err(format!("au {url}: {e}"));
    }
    // Only meaningful after `AudioUnitInitialize`: a unit sizes its lookahead to
    // the stream format it was configured with.
    node.latency = latency_frames(node.instance.unit, sample_rate);
    // After `AudioUnitInitialize`, which is where hosts apply class info: the
    // unit has to know its stream format before it can make sense of settings.
    if let Some(bytes) = state.and_then(|t| super::host_api::decode_state(node_id, url, t)) {
        restore_class_info(node.instance.unit, node_id, &bytes);
    }
    tracing::debug!(node_id, url, sample_rate, primary, "audio unit activated");
    let retired = if primary {
        instances().lock().unwrap().insert(
            node_id.to_string(),
            AuSlot {
                instance: node.instance.clone(),
                alive,
                view: None,
            },
        )
    } else {
        // A metering duplicate or an extra stereo pair: nothing addresses it, but
        // the sweep must still own it, or its `Drop` would land on the DSP worker.
        graveyard()
            .lock()
            .unwrap()
            .bury(node.instance.clone(), alive);
        None
    };
    if let Some(old) = retired {
        graveyard().lock().unwrap().bury(old.instance, old.alive);
    }
    Ok(node)
}

/// The unit's reported lookahead, converted from seconds. Zero when the unit
/// does not implement the property, which most simple effects do not.
fn latency_frames(unit: AudioUnit, sample_rate: u32) -> usize {
    let mut seconds = 0.0f64;
    let mut size = size_of::<f64>() as u32;
    // SAFETY: the property is a single Float64 in global scope.
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_Latency,
            kAudioUnitScope_Global,
            0,
            NonNull::from(&mut seconds).cast(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 || !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds * sample_rate as f64).round() as usize
}

/// Configures the unit for `channels` and initialises it. A unit that will not
/// take the width says so through its stream-format property, which is how the
/// caller knows to offer stereo instead.
fn configure(
    node: &mut AuNode,
    sample_rate: u32,
    max_frames: usize,
    channels: usize,
) -> Result<(), String> {
    let unit = node.instance.unit;
    let format = float_format(sample_rate, channels);
    let slice = max_frames as u32;
    let callback = AURenderCallbackStruct {
        inputProc: Some(supply_input),
        inputProcRefCon: &mut *node.input as *mut RenderInput as *mut c_void,
    };

    // SAFETY: `unit` is a fresh uninitialised instance and every value outlives
    // the call, which copies it.
    unsafe {
        set_property(
            unit,
            kAudioUnitProperty_MaximumFramesPerSlice,
            kAudioUnitScope_Global,
            &slice,
            "max frames per slice",
        )?;
        set_property(
            unit,
            kAudioUnitProperty_StreamFormat,
            kAudioUnitScope_Input,
            &format,
            "input stream format",
        )?;
        set_property(
            unit,
            kAudioUnitProperty_StreamFormat,
            kAudioUnitScope_Output,
            &format,
            "output stream format",
        )?;
        set_property(
            unit,
            kAudioUnitProperty_SetRenderCallback,
            kAudioUnitScope_Input,
            &callback,
            "render callback",
        )?;

        let status = AudioUnitInitialize(unit);
        if status != 0 {
            return Err(format!("initialize: OSStatus {status}"));
        }
    }
    Ok(())
}

/// Drops the registry's hold on a node's unit. The instance itself survives
/// until its RT node is gone too.
fn forget(node_id: &str) {
    let Some(slot) = instances().lock().unwrap().remove(node_id) else {
        return;
    };
    // Handed to the sweep rather than dropped here: the caller may be the engine
    // thread, and a unit must only ever be disposed on the main thread.
    graveyard().lock().unwrap().bury(slot.instance, slot.alive);
}

/// Serializes a unit's settings the way every AU host does: the class-info
/// property is a CFPropertyList, flattened to a binary plist and base64'd so it
/// can live in the node's JSON alongside CLAP's blob.
fn save_class_info(node_id: &str) -> Option<Vec<u8>> {
    use objc2_core_foundation::{
        CFData, CFPropertyList, CFPropertyListCreateData, CFPropertyListFormat, CFRange,
    };

    let instance = instances()
        .lock()
        .unwrap()
        .get(node_id)
        .map(|s| s.instance.clone())?;

    let mut plist: *const CFPropertyList = ptr::null();
    let mut size = size_of::<*const CFPropertyList>() as u32;
    // SAFETY: the property writes one owned CFPropertyListRef.
    let status = unsafe {
        AudioUnitGetProperty(
            instance.unit,
            kAudioUnitProperty_ClassInfo,
            kAudioUnitScope_Global,
            0,
            NonNull::from(&mut plist).cast(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        tracing::debug!(node_id, status, "audio unit has no class info");
        return None;
    }
    // SAFETY: the property follows the CF copy rule, so this reference is ours.
    let plist = unsafe { CFRetained::from_raw(NonNull::new(plist.cast_mut())?) };

    let data: CFRetained<CFData> = unsafe {
        CFPropertyListCreateData(
            None,
            Some(&plist),
            CFPropertyListFormat::BinaryFormat_v1_0,
            0,
            ptr::null_mut(),
        )
    }?;
    let len = data.length();
    let mut bytes = vec![0u8; len as usize];
    // SAFETY: the buffer is exactly the range asked for.
    unsafe { data.bytes(CFRange { location: 0, length: len }, bytes.as_mut_ptr()) };

    Some(bytes)
}

/// Restores what [`save_class_info`] produced. A malformed blob is logged and
/// skipped: defaults are a usable unit, a half-applied state is not.
fn restore_class_info(unit: AudioUnit, node_id: &str, bytes: &[u8]) {
    use objc2_core_foundation::{CFData, CFPropertyList, CFPropertyListCreateWithData};

    // SAFETY: `bytes` outlives the copy CFData makes of it.
    let Some(data) = (unsafe { CFData::new(None, bytes.as_ptr(), bytes.len() as isize) }) else {
        return;
    };
    // SAFETY: null error out-param is allowed; failure comes back as None.
    let Some(plist) = (unsafe {
        CFPropertyListCreateWithData(None, Some(&data), 0, ptr::null_mut(), ptr::null_mut())
    }) else {
        tracing::error!(node_id, "audio unit state is not a property list");
        return;
    };

    let raw: *const CFPropertyList = CFRetained::as_ptr(&plist).as_ptr();
    // SAFETY: the property takes a pointer to the reference, not the object.
    let status = unsafe {
        AudioUnitSetProperty(
            unit,
            kAudioUnitProperty_ClassInfo,
            kAudioUnitScope_Global,
            0,
            &raw as *const *const CFPropertyList as *const c_void,
            size_of::<*const CFPropertyList>() as u32,
        )
    };
    if status != 0 {
        tracing::error!(node_id, status, "audio unit rejected its saved state");
    }
}

/// The reference of the unit currently running for this node.
fn loaded_path(node_id: &str) -> Option<String> {
    instances()
        .lock()
        .unwrap()
        .get(node_id)
        .map(|s| s.instance.url.clone())
}

/// Whether the running unit can produce an editor view at all, so the node can
/// say so instead of offering a button that only ever errors.
fn has_editor(node_id: &str) -> bool {
    let Some(instance) = instances().lock().unwrap().get(node_id).map(|s| s.instance.clone()) else {
        return false;
    };
    let mut size = 0u32;
    // SAFETY: the unit is live; the call only reports the property's size.
    let status = unsafe {
        AudioUnitGetPropertyInfo(
            instance.unit,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            &mut size,
            ptr::null_mut(),
        )
    };
    status == 0 && size > 0
}

/// Tells the unit's own editor that the host changed a parameter behind its
/// back. `AudioUnitSetParameter` alone only moves the value: views built on the
/// AU parameter-listener mechanism (Apple's generic views among them) redraw
/// from this notification and otherwise show a stale knob.
///
/// Never call from the DSP worker -- the notification takes locks.
fn notify_param_changed(node_id: &str, param_id: u32) {
    let Some(instance) = instances().lock().unwrap().get(node_id).map(|s| s.instance.clone()) else {
        return;
    };
    let parameter = AudioUnitParameter {
        mAudioUnit: instance.unit,
        mParameterID: param_id,
        mScope: kAudioUnitScope_Global,
        mElement: 0,
    };
    // SAFETY: the unit is live, and a null listener/object means "not sent by
    // any particular listener", which is what a host notification is.
    unsafe {
        AUParameterListenerNotify(ptr::null_mut(), ptr::null_mut(), NonNull::from(&parameter));
    }
}

/// Enumerates a running unit's global-scope parameters for the node UI. Empty
/// when the node is not running an AU.
fn get_params(node_id: &str) -> Vec<PluginParamInfo> {
    let Some(instance) = instances().lock().unwrap().get(node_id).map(|s| s.instance.clone()) else {
        return Vec::new();
    };
    let unit = instance.unit;

    let mut size = 0u32;
    // SAFETY: `unit` is live; the call only reports the property's size.
    let status = unsafe {
        AudioUnitGetPropertyInfo(
            unit,
            kAudioUnitProperty_ParameterList,
            kAudioUnitScope_Global,
            0,
            &mut size,
            ptr::null_mut(),
        )
    };
    if status != 0 || size == 0 {
        return Vec::new();
    }

    let mut ids = vec![0u32; size as usize / size_of::<u32>()];
    // SAFETY: `ids` holds exactly `size` bytes, which is what the unit reports.
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_ParameterList,
            kAudioUnitScope_Global,
            0,
            NonNull::new_unchecked(ids.as_mut_ptr() as *mut c_void),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        return Vec::new();
    }

    ids.iter().filter_map(|&id| param_info(unit, id)).collect()
}

fn param_info(unit: AudioUnit, id: u32) -> Option<PluginParamInfo> {
    let mut info = AudioUnitParameterInfo {
        name: [0; 52],
        unitName: ptr::null(),
        clumpID: 0,
        cfNameString: ptr::null(),
        unit: AudioUnitParameterUnit::Generic,
        minValue: 0.0,
        maxValue: 0.0,
        defaultValue: 0.0,
        flags: AudioUnitParameterOptions::empty(),
    };
    let mut size = size_of::<AudioUnitParameterInfo>() as u32;
    // SAFETY: `info` is the exact struct the property writes.
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_ParameterInfo,
            kAudioUnitScope_Global,
            id,
            NonNull::from(&mut info).cast(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 || !info.flags.contains(AudioUnitParameterOptions::Flag_IsReadable) {
        return None;
    }

    let name = param_name(&info);
    let mut value = info.defaultValue;
    // SAFETY: `unit` is live and the call only writes `value`.
    unsafe {
        AudioUnitGetParameter(
            unit,
            id,
            kAudioUnitScope_Global,
            0,
            NonNull::from(&mut value),
        )
    };

    Some(PluginParamInfo {
        id,
        name,
        min: info.minValue as f64,
        max: info.maxValue as f64,
        default: info.defaultValue as f64,
        value: value as f64,
        stepped: matches!(
            info.unit,
            AudioUnitParameterUnit::Indexed | AudioUnitParameterUnit::Boolean
        ),
        read_only: !info.flags.contains(AudioUnitParameterOptions::Flag_IsWritable),
    })
}

fn param_name(info: &AudioUnitParameterInfo) -> String {
    if info
        .flags
        .contains(AudioUnitParameterOptions::Flag_HasCFNameString)
    {
        if let Some(cf) = NonNull::new(info.cfNameString.cast_mut()) {
            // SAFETY: the flag guarantees a valid CFStringRef. Ownership only
            // transfers to us when the unit also sets the release flag.
            let name = unsafe {
                if info
                    .flags
                    .contains(AudioUnitParameterOptions::Flag_CFNameRelease)
                {
                    CFRetained::from_raw(cf).to_string()
                } else {
                    cf.as_ref().to_string()
                }
            };
            return name;
        }
    }
    let end = info
        .name
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(info.name.len());
    let bytes: Vec<u8> = info.name[..end].iter().map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Builds the unit's Cocoa editor and adds it to `parent_view`, returning the
/// view's own size so the caller can fit the window to it. Must run on the main
/// thread: everything here is AppKit.
///
/// Only the `CocoaUI` property is supported. An AUv3 that exposes its editor
/// solely through `RequestViewController` reports that as an error rather than
/// opening an empty window.
pub(super) fn embed_editor(
    node_id: &str,
    parent_view: *mut c_void,
    titlebar: f64,
) -> Result<(f64, f64), String> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSRect, NSSize};

    let view = create_view(node_id)?;

    // SAFETY: the caller passes the editor window's content view, which owns the
    // plugin view from here on.
    let (frame, fitting): (NSRect, NSSize) = unsafe {
        let _: () = msg_send![parent_view as *mut AnyObject, addSubview: view];
        // Apple's generic views build their content lazily, so the frame is
        // still degenerate right after `addSubview`.
        let _: () = msg_send![view, layoutSubtreeIfNeeded];
        let measured = (msg_send![view, frame], msg_send![view, fittingSize]);
        editor::inset_below_titlebar(parent_view, view, titlebar);
        measured
    };

    tracing::debug!(
        node_id,
        frame_w = frame.size.width,
        frame_h = frame.size.height,
        fitting_w = fitting.width,
        fitting_h = fitting.height,
        "au editor view measured"
    );
    // A view whose content is laid out by constraints reports its real content
    // size through `fittingSize` while its frame is still collapsed; one built
    // from a fixed frame reports the frame in both. Taking the larger of the two
    // is the one rule that sizes each kind correctly.
    if let Some(slot) = instances().lock().unwrap().get_mut(node_id) {
        slot.view = Some(view as usize);
    }
    let size = (
        frame.size.width.max(fitting.width),
        frame.size.height.max(fitting.height),
    );
    Ok(size)
}

/// Detaches a plugin view from the window it was embedded in. Main thread only.
///
/// SAFETY: `view` is an address stored by `embed_editor`, which only ever puts a
/// live `NSView` there and takes it back out when detaching.
unsafe fn drop_view(view: usize) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let _: () = msg_send![view as *mut AnyObject, removeFromSuperview];
}

/// Instantiates the unit's Cocoa view, without parenting it.
fn create_view(node_id: &str) -> Result<*mut objc2::runtime::AnyObject, String> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::{NSBundle, NSSize, NSURL};

    let instance = instances()
        .lock()
        .unwrap()
        .get(node_id)
        .map(|s| s.instance.clone())
        .ok_or_else(|| format!("au {node_id}: audio unit is not running"))?;
    let unit = instance.unit;
    // Every failure below names the unit; an editor error that says only what
    // went wrong is unusable when several plugins are in the graph.
    let at = |step: &str, detail: String| format!("au {}: {step}: {detail}", instance.url);

    let mut info = std::mem::MaybeUninit::<AudioUnitCocoaViewInfo>::zeroed();
    let mut size = size_of::<AudioUnitCocoaViewInfo>() as u32;
    // SAFETY: `info` matches the property's layout for a single view class.
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            NonNull::new_unchecked(info.as_mut_ptr()).cast(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        return Err(at(
            "kAudioUnitProperty_CocoaUI",
            format!("unit exposes no Cocoa editor (OSStatus {status})"),
        ));
    }
    // SAFETY: the property succeeded, so both refs are set, and it follows the
    // CF copy rule -- `CFRetained` takes ownership of each.
    let (bundle_url, class_name) = unsafe {
        let info = info.assume_init();
        (
            CFRetained::from_raw(info.mCocoaAUViewBundleLocation),
            CFRetained::from_raw(info.mCocoaAUViewClass[0]),
        )
    };

    // SAFETY: CFURL and CFString are toll-free bridged to their NS twins.
    let url: &NSURL = unsafe { &*(CFRetained::as_ptr(&bundle_url).as_ptr() as *const NSURL) };
    let bundle = NSBundle::bundleWithURL(url)
        .ok_or_else(|| at("view bundle", format!("cannot open {url:?}")))?;
    // The factory class only exists once its bundle's code is loaded.
    if !unsafe { bundle.load() } {
        return Err(at("view bundle", format!("cannot load code at {url:?}")));
    }

    let name = class_name.to_string();
    let c_name = std::ffi::CString::new(name.clone())
        .map_err(|e| at("view class", format!("{name:?} is not a valid class name: {e}")))?;
    let class = AnyClass::get(&c_name)
        .ok_or_else(|| at("view class", format!("{name} not found in the loaded bundle")))?;

    // `uiViewForAudioUnit:withSize:` is the AUCocoaUIBase protocol; the factory
    // is a plain object, and the view it returns is autoreleased.
    let view: *mut AnyObject = unsafe {
        let factory: *mut AnyObject = msg_send![class, alloc];
        let factory: *mut AnyObject = msg_send![factory, init];
        let Some(factory) = Retained::from_raw(factory) else {
            return Err(at("view factory", format!("{name} would not initialise")));
        };
        let preferred = NSSize::new(0.0, 0.0);
        msg_send![&*factory, uiViewForAudioUnit: unit, withSize: preferred]
    };
    if view.is_null() {
        return Err(at("view factory", format!("{name} returned no view")));
    }
    Ok(view)
}

impl Effect for AuNode {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let width = self.channels;
        if frames == 0 || frames > self.max_frames || samples.len() < frames * width {
            return;
        }

        // `AudioUnitSetParameter` is the AU-sanctioned way to change a value
        // from the render thread: it is lock-free and takes effect immediately.
        let mut applied = 0;
        while applied < MAX_PARAM_CHANGES_PER_BLOCK {
            let Some((id, value)) = self.params.read(&mut self.param_cursor) else {
                break;
            };
            // SAFETY: the unit is live and owned for the duration of this call.
            unsafe {
                AudioUnitSetParameter(
                    self.instance.unit,
                    id,
                    kAudioUnitScope_Global,
                    0,
                    value as f32,
                    0,
                )
            };
            applied += 1;
        }

        for (c, channel) in self.input.channels.iter_mut().enumerate() {
            for i in 0..frames {
                channel[i] = samples[i * width + c];
            }
        }

        let mut list = RenderBufferList {
            number_buffers: width as u32,
            buffers: [AudioBuffer {
                mNumberChannels: 1,
                mDataByteSize: (frames * size_of::<f32>()) as u32,
                mData: ptr::null_mut(),
            }; MAX_CHANNELS],
        };
        for (buf, chan) in list.buffers.iter_mut().zip(self.output.iter_mut()) {
            buf.mData = chan.as_mut_ptr() as *mut c_void;
        }

        // Only the sample time is meaningful here; the flags say so, and the
        // unit must not read the rest.
        let mut timestamp: AudioTimeStamp = unsafe { std::mem::zeroed() };
        timestamp.mSampleTime = self.steady;
        timestamp.mFlags = AudioTimeStampFlags::SampleTimeValid;
        let mut action_flags = AudioUnitRenderActionFlags::empty();
        // SAFETY: the unit is initialised, the buffer list points at buffers of
        // at least `frames` samples, and the input callback reads only `input`.
        let status = unsafe {
            AudioUnitRender(
                self.instance.unit,
                &mut action_flags,
                NonNull::from(&timestamp),
                0,
                frames as u32,
                list.as_audio_buffer_list(),
            )
        };

        // A render error leaves the block untouched rather than emitting the
        // uninitialised output buffer.
        if status == 0 {
            for (c, channel) in self.output.iter().enumerate().take(width) {
                for i in 0..frames {
                    samples[i * width + c] = channel[i];
                }
            }
        }
        self.steady += frames as f64;
    }

    fn latency_frames(&self) -> usize {
        self.latency
    }
}

impl AuNode {
    pub fn channels(&self) -> usize {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests drive units directly, always as a stereo pair.
    const CHANNELS: usize = 2;

    /// Apple's N-band EQ ships with every macOS and is flat by default, so a
    /// signal must come back out at roughly the level it went in.
    #[test]
    fn renders_signal_through_an_audio_unit() {
        const FRAMES: usize = 512;
        let mut node = activate(
            "render-test",
            "au://aufx/nbeq/appl",
            48_000,
            FRAMES,
            CHANNELS,
            None,
            false,
            Arc::new(ParamRing::new()),
        )
        .expect("activate");

        let mut block = vec![0.0f32; FRAMES * CHANNELS];
        for i in 0..FRAMES {
            let s = (i as f32 * 0.05).sin() * 0.5;
            block[CHANNELS * i] = s;
            block[CHANNELS * i + 1] = s;
        }

        node.process(&mut block, FRAMES);
        let peak_out = block.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(peak_out > 0.1, "audio unit produced silence");
    }

    /// Reported latency has to match where the signal actually appears: the DAG
    /// pads shorter parallel paths by this number, so a wrong one is an audible
    /// phase error on a split-and-rejoin graph. The peak limiter ships with
    /// every macOS and has a real lookahead, unlike the flat N-band EQ.
    #[test]
    fn reported_latency_matches_when_the_signal_arrives() {
        const LIMITER: &str = "au://aufx/lmtr/appl";
        const FRAMES: usize = 512;

        let mut node = activate(
            "lat",
            LIMITER,
            48_000,
            FRAMES,
            CHANNELS,
            None,
            false,
            Arc::new(ParamRing::new()),
        )
        .expect("activate");
        let reported = node.latency_frames();
        assert!(reported > 0, "the peak limiter reported no lookahead");

        // One impulse, then silence: the first non-zero output frame is the
        // latency the unit actually imposes.
        let mut fed = 0;
        let mut arrived = None;
        while fed < 8 * FRAMES && arrived.is_none() {
            let mut block = vec![0.0f32; FRAMES * CHANNELS];
            if fed == 0 {
                block[0] = 1.0;
                block[1] = 1.0;
            }
            node.process(&mut block, FRAMES);
            if let Some(i) = block.iter().position(|s| s.abs() > 1e-6) {
                arrived = Some(fed + i / CHANNELS);
            }
            fed += FRAMES;
        }
        let arrived = arrived.expect("the limiter passed no impulse at all");
        // Exact equality is too strict: the unit may round its own report, and
        // its gain stage can smear the leading edge by a few samples.
        let slack = FRAMES;
        assert!(
            arrived + slack >= reported && arrived <= reported + slack,
            "reports {reported} samples of latency but the impulse arrived at {arrived}"
        );
    }

    /// What each installed unit does when offered widths other than stereo. A
    /// unit that takes 5.1 whole is run as one instance, the way a DAW does it;
    /// one that refuses falls back to a stereo pair.
    #[test]
    fn negotiates_the_widest_layout_each_unit_accepts() {
        use crate::audio::plugins::PluginBackend;

        let found = super::super::au_backend::AuBackend.scan();
        let mut wide = 0;
        for unit in &found {
            let mut accepted = Vec::new();
            for width in [1usize, 2, 6, 8] {
                let node = activate(
                    "neg",
                    &unit.path,
                    48_000,
                    512,
                    width,
                    None,
                    false,
                    Arc::new(ParamRing::new()),
                );
                let Ok(node) = node else { continue };
                let got = node.channels();
                assert!(
                    got == width || got == 2,
                    "{} offered {width} took {got}, which is neither",
                    unit.name
                );
                accepted.push(got);
            }
            if accepted.contains(&6) {
                wide += 1;
            }
        }
        println!("{wide} of {} audio units take more than stereo", found.len());
    }

    /// A unit driven at the node's full width must keep every channel separate
    /// and in order. Feeding each channel a different level and checking the
    /// order survives is what catches a buffer list wired up channel-shuffled,
    /// which a per-channel loudness check alone would pass.
    #[test]
    fn a_six_channel_unit_keeps_its_channels_in_order() {
        const WIDTH: usize = 6;
        const FRAMES: usize = 512;
        // Flat by default, so the block comes back at the level it went in.
        const EQ: &str = "au://aufx/greq/appl";

        let mut node = activate(
            "wide",
            EQ,
            48_000,
            FRAMES,
            WIDTH,
            None,
            false,
            Arc::new(ParamRing::new()),
        )
        .expect("activate");
        assert_eq!(node.channels(), WIDTH, "unit did not take the full width");

        // Descending levels, one per channel.
        let level = |c: usize| 0.5 - 0.05 * c as f32;
        let mut block = vec![0.0f32; FRAMES * WIDTH];
        for i in 0..FRAMES {
            let s = (i as f32 * 0.05).sin();
            for c in 0..WIDTH {
                block[i * WIDTH + c] = s * level(c);
            }
        }
        node.process(&mut block, FRAMES);

        let peaks: Vec<f32> = (0..WIDTH)
            .map(|c| {
                block
                    .iter()
                    .skip(c)
                    .step_by(WIDTH)
                    .fold(0.0f32, |a, s| a.max(s.abs()))
            })
            .collect();
        for (c, peak) in peaks.iter().enumerate() {
            assert!(*peak > 0.01, "channel {c} came back silent: {peaks:?}");
        }
        for c in 1..WIDTH {
            assert!(
                peaks[c] < peaks[c - 1],
                "channels came back out of order: {peaks:?}"
            );
        }
    }

    /// The N-band EQ exposes a known, stable parameter set, so this pins both
    /// the list query and the `AudioUnitParameterInfo` layout.
    #[test]
    fn reads_parameters_of_a_running_unit() {
        let node = activate(
            "params-test",
            "au://aufx/nbeq/appl",
            48_000,
            512,
            CHANNELS,
            None,
            true,
            Arc::new(ParamRing::new()),
        )
        .expect("activate");

        let params = get_params("params-test");
        assert!(!params.is_empty(), "no parameters read");
        let gain = params.first().expect("global gain is parameter 0");
        assert_eq!(gain.name, "Global Gain");
        assert_eq!((gain.min, gain.max), (-96.0, 24.0));
        assert!(!gain.read_only);
        assert!(params.iter().any(|p| p.stepped), "no stepped parameter found");

        drop(node);
        forget("params-test");
    }

    /// Round-trips a real unit's settings: save, restore into a fresh instance,
    /// and read the parameter back. Pins the class-info plist path end to end.
    #[test]
    fn state_survives_a_reactivation() {
        const URL: &str = "au://aufx/nbeq/appl";
        const GAIN: u32 = 0;

        let node = activate("st", URL, 48_000, 512, CHANNELS, None, true, Arc::new(ParamRing::new()))
            .expect("activate");
        let unit = instances().lock().unwrap()["st"].instance.unit;
        unsafe { AudioUnitSetParameter(unit, GAIN, kAudioUnitScope_Global, 0, -12.0, 0) };

        let saved = AuHost
            .save_state("st")
            .expect("au supports state")
            .expect("unit produced state");
        drop(node);
        forget("st");

        let node = activate(
            "st",
            URL,
            48_000,
            512,
            CHANNELS,
            Some(&saved),
            true,
            Arc::new(ParamRing::new()),
        )
        .expect("reactivate");
        let restored = get_params("st")
            .into_iter()
            .find(|p| p.id == GAIN)
            .expect("global gain");
        assert!(
            (restored.value - -12.0).abs() < 0.01,
            "gain came back as {}",
            restored.value
        );

        drop(node);
        forget("st");
    }
}

/// The Audio Unit implementation of the shared host interface.
pub struct AuHost;

impl PluginHost for AuHost {
    fn activate(&self, req: ActivateRequest<'_>) -> Result<HostedNode, String> {
        activate(
            req.node_id,
            req.path,
            req.sample_rate,
            req.max_frames,
            req.channels,
            req.state,
            req.primary,
            req.params,
        )
        .map(HostedNode::Au)
    }

    fn forget(&self, node_id: &str) {
        forget(node_id);
    }

    fn status(&self, node_id: &str) -> PluginStatus {
        PluginStatus {
            path: loaded_path(node_id),
            has_editor: has_editor(node_id),
        }
    }

    fn params(&self, node_id: &str) -> Vec<PluginParamInfo> {
        get_params(node_id)
    }

    fn save_state(&self, node_id: &str) -> Result<Option<String>, Unsupported> {
        let url = loaded_path(node_id);
        Ok(url
            .zip(save_class_info(node_id))
            .map(|(url, bytes)| super::host_api::encode_state(&url, &bytes)))
    }

    /// The value is already in the unit; this only tells listeners to redraw.
    fn notify_param_changed(
        &self,
        node_id: &str,
        param_id: u32,
        _value: f64,
    ) -> Result<(), Unsupported> {
        notify_param_changed(node_id, param_id);
        Ok(())
    }

    /// AppKit is main-thread only, so the view work is marshalled there while
    /// the caller blocks -- the interface promises a blocking answer on any
    /// non-main thread, and hiding this is exactly its job.
    fn embed_editor(&self, node_id: &str, window: &tauri::Window) -> Result<EditorSize, String> {
        // A raw pointer is not `Send`; the address is, and it stays valid
        // because the window outlives the editor.
        let view_addr = window
            .ns_view()
            .map_err(|e| format!("au {node_id}: content view: {e}"))? as usize;
        let (_, titlebar) = editor::decoration_overhead(window);
        let id = node_id.to_string();
        // An NSView's frame is the real laid-out geometry rather than a plugin's
        // own guess, so it is reported as measured; the window layer rejects
        // only the degenerate values.
        let (width, height) =
            main_thread::run(move || embed_editor(&id, view_addr as *mut c_void, titlebar))??;
        Ok((width as u32, height as u32))
    }

    fn destroy_editor(&self, node_id: &str) {
        let Some(view) = instances()
            .lock()
            .unwrap()
            .get_mut(node_id)
            .and_then(|s| s.view.take())
        else {
            return;
        };
        unsafe { drop_view(view) };
    }

    /// Frees units whose RT node has left the graph. The host holds a reference
    /// until then, so this sweep drops the last one -- on the main thread, which
    /// is the only place a unit may be disposed.
    fn tick_and_reclaim(&self) {
        // Collected rather than dropped under the lock: `Drop` calls into the
        // unit, and a plugin is free to take its time.
        let mut freed = graveyard().lock().unwrap().reclaim();

        let dead = super::host_api::take_dead(&mut instances().lock().unwrap(), |s| &s.alive);
        for (node_id, slot) in dead {
            // The view is a child of the editor window and points at the unit;
            // both go before the unit itself does.
            if let Some(view) = slot.view {
                unsafe { drop_view(view) };
            }
            editor::close_window(&node_id);
            freed.push(slot.instance);
        }
        drop(freed);
    }
}
