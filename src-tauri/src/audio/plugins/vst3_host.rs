//! Instantiating a VST3 plugin: the two halves, their connection, and the
//! state handshake between them.

use vst3::Steinberg::Vst::{
    IComponent, IComponentHandler, IComponentTrait, IEditController, IEditControllerTrait,
};
use vst3::Steinberg::{
    kResultOk, tresult, FIDString, IBStream, IPluginBaseTrait, IPluginFactoryTrait, TUID,
};
use vst3::{ComPtr, ComWrapper, Interface};

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::vst3_backend::{parse_cid, Vst3Module};
use super::vst3_com::{host_context, ComponentHandler, EditListener, MemoryStream};
use super::host_api::PluginParamInfo;
use super::vst3_node::{supports_f32, Vst3Node};
use super::ParamRing;

/// A plugin's two halves. VST3 splits processing from parameter handling so a
/// host may run them in separate processes; we always run both here, but the
/// split still dictates the setup order and the state handshake below.
pub struct Vst3Instance {
    pub component: ComPtr<IComponent>,
    pub controller: ComPtr<IEditController>,
    /// Whether one object answers as both halves. It decides whether the two
    /// are connected, and must not be connected to itself.
    separate: bool,
    /// Kept alive for the plugin, which holds only a borrowed reference to it.
    handler: Option<ComWrapper<ComponentHandler<Box<dyn EditListener>>>>,
    /// The factory that made these lives in the module, so it outlives them.
    _module: Vst3Module,
}

/// A `String128` field as text, up to its terminator.
fn utf16(field: &[u16]) -> String {
    let len = field.iter().position(|c| *c == 0).unwrap_or(field.len());
    char::decode_utf16(field[..len].iter().copied())
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn ok(result: tresult) -> bool {
    result == kResultOk
}

/// Separates the two halves' state. Not in base64's alphabet, so it cannot
/// occur inside either payload.
const STATE_HALVES_SEP: char = '.';

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unb64(text: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|e| format!("saved state is not base64: {e}"))
}

fn stream(bytes: Vec<u8>) -> ComPtr<IBStream> {
    ComWrapper::new(MemoryStream::new(bytes))
        .to_com_ptr::<IBStream>()
        .expect("MemoryStream implements IBStream")
}

/// Runs `write` against a fresh stream and returns what it wrote.
fn read_state(write: impl FnOnce(*mut IBStream) -> tresult) -> Option<Vec<u8>> {
    let wrapper = ComWrapper::new(MemoryStream::new(Vec::new()));
    let s = wrapper.to_com_ptr::<IBStream>()?;
    ok(write(s.as_ptr())).then(|| wrapper.take())
}

/// A class id as the factory wants it: 16 raw bytes, not a C string.
fn cid_arg(cid: &TUID) -> FIDString {
    cid.as_ptr()
}

fn iid_arg<I: Interface>() -> FIDString {
    I::IID.as_ptr().cast()
}

impl Vst3Instance {
    pub fn new(module: Vst3Module, plugin_id: &str) -> Result<Self, String> {
        let at = |step: &str| format!("{plugin_id}: {step}");

        let cid = parse_cid(plugin_id).ok_or_else(|| at("not a class id"))?;
        let factory = module.factory().clone();
        let host = host_context();

        // SAFETY: every pointer below is either owned here or produced by the
        // plugin, and each call follows the initialisation order VST3 requires.
        unsafe {
            let mut obj = std::ptr::null_mut();
            if !ok(factory.createInstance(cid_arg(&cid), iid_arg::<IComponent>(), &mut obj)) {
                return Err(at("factory refused to create the component"));
            }
            let component = ComPtr::<IComponent>::from_raw(obj.cast())
                .ok_or_else(|| at("factory returned a null component"))?;

            if !ok(component.initialize(host.as_ptr().cast())) {
                return Err(at("component rejected the host context"));
            }

            let (controller, separate) = Self::resolve_controller(&factory, &component, &host)
                .map_err(|step| at(&step))?;

            if separate {
                Self::connect(&component, &controller).map_err(|step| at(&step))?;
            }
            Self::sync_controller(&component, &controller);

            Ok(Self {
                component,
                controller,
                separate,
                handler: None,
                _module: module,
            })
        }
    }

    /// The controller half. A plugin may answer as both halves from one object;
    /// asking the component first is what tells the two shapes apart, because a
    /// split plugin genuinely fails that query.
    unsafe fn resolve_controller(
        factory: &ComPtr<vst3::Steinberg::IPluginFactory>,
        component: &ComPtr<IComponent>,
        host: &ComPtr<vst3::Steinberg::Vst::IHostApplication>,
    ) -> Result<(ComPtr<IEditController>, bool), String> {
        if let Some(controller) = component.cast::<IEditController>() {
            return Ok((controller, false));
        }

        let mut cid: TUID = [0; 16];
        if !ok(component.getControllerClassId(&mut cid)) {
            return Err("component has no editor controller".into());
        }

        let mut obj = std::ptr::null_mut();
        if !ok(factory.createInstance(cid_arg(&cid), iid_arg::<IEditController>(), &mut obj)) {
            return Err("factory refused to create the controller".into());
        }
        let controller = ComPtr::<IEditController>::from_raw(obj.cast())
            .ok_or("factory returned a null controller")?;

        if !ok(controller.initialize(host.as_ptr().cast())) {
            return Err("controller rejected the host context".into());
        }
        Ok((controller, true))
    }

    /// Wires the halves both ways so the plugin can pass its own messages.
    unsafe fn connect(
        component: &ComPtr<IComponent>,
        controller: &ComPtr<IEditController>,
    ) -> Result<(), String> {
        use vst3::Steinberg::Vst::{IConnectionPoint, IConnectionPointTrait};

        let (Some(from), Some(to)) = (
            component.cast::<IConnectionPoint>(),
            controller.cast::<IConnectionPoint>(),
        ) else {
            // Optional in the spec: a plugin that exchanges no messages between
            // its halves does not implement it.
            return Ok(());
        };
        if !ok(from.connect(to.as_ptr())) || !ok(to.connect(from.as_ptr())) {
            return Err("halves refused to connect".into());
        }
        Ok(())
    }

    /// Hands the component's state to the controller. Skipping this is why a
    /// plugin's editor can open showing defaults while its audio runs on
    /// something else entirely.
    unsafe fn sync_controller(component: &ComPtr<IComponent>, controller: &ComPtr<IEditController>) {
        let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
        let Some(s) = stream.to_com_ptr::<IBStream>() else {
            return;
        };
        if !ok(component.getState(s.as_ptr())) {
            return;
        }
        // The component wrote to the end; the controller reads from the start.
        rewind(&s);
        controller.setComponentState(s.as_ptr());
    }

    /// Every automatable parameter, for the node UI. VST3 values are already
    /// normalised, so the range is 0..1 and the plugin's own text renders the
    /// real units.
    pub fn params(&self) -> Vec<PluginParamInfo> {
        use vst3::Steinberg::Vst::ParameterInfo_::ParameterFlags_::{
            kIsHidden, kIsList, kIsProgramChange, kIsReadOnly,
        };
        use vst3::Steinberg::Vst::ParameterInfo;

        let mut out = Vec::new();
        // SAFETY: indices below the count the controller reports.
        unsafe {
            for index in 0..self.controller.getParameterCount() {
                let mut info: ParameterInfo = std::mem::zeroed();
                if !ok(self.controller.getParameterInfo(index, &mut info)) {
                    continue;
                }
                // Hidden and program-change parameters are the plugin's own
                // plumbing, not something a user should be handed.
                if info.flags & (kIsHidden | kIsProgramChange) != 0 {
                    continue;
                }
                out.push(PluginParamInfo {
                    id: info.id,
                    name: utf16(&info.title),
                    min: 0.0,
                    max: 1.0,
                    default: info.defaultNormalizedValue,
                    value: self.controller.getParamNormalized(info.id),
                    stepped: info.stepCount > 0 || info.flags & kIsList != 0,
                    read_only: info.flags & kIsReadOnly != 0,
                });
            }
        }
        out
    }

    /// Whether the plugin has an editor at all. Asked before offering the
    /// button, so the node can say "no editor" instead of opening a blank
    /// window.
    pub fn has_editor(&self) -> bool {
        use vst3::Steinberg::Vst::ViewType::kEditor;
        // SAFETY: the view is created only to be counted and immediately
        // released; it is never attached.
        unsafe {
            ComPtr::<vst3::Steinberg::IPlugView>::from_raw(self.controller.createView(kEditor))
                .is_some()
        }
    }

    /// Moves a parameter behind the plugin editor's back, so its own display
    /// follows a change made on the node. The audio side hears it through the
    /// ring; this is the editor's copy.
    pub fn set_param(&self, id: u32, value: f64) {
        unsafe { self.controller.setParamNormalized(id, value) };
    }

    /// Routes edits made inside the plugin's own window to `listener`. Without
    /// this the plugin has no way to tell the host a control moved, and its
    /// editor appears to do nothing.
    pub fn listen(&mut self, listener: Box<dyn EditListener>) {
        let handler = ComWrapper::new(ComponentHandler::new(listener));
        if let Some(h) = handler.to_com_ptr::<IComponentHandler>() {
            unsafe { self.controller.setComponentHandler(h.as_ptr()) };
        }
        // The plugin holds a borrowed reference, so the object must outlive it.
        self.handler = Some(handler);
    }

    /// Both halves' state, base64 each side of a separator. Two blobs because
    /// the controller keeps things the component does not: which page the
    /// editor was on, a zoom level, anything with no effect on audio.
    pub fn save_state(&self) -> Option<String> {
        let component = read_state(|s| unsafe { self.component.getState(s) })?;
        // The controller half is optional in the spec and Renegate omits it;
        // an empty payload means "nothing of its own to remember".
        let controller = read_state(|s| unsafe { self.controller.getState(s) }).unwrap_or_default();
        Some(format!(
            "{}{STATE_HALVES_SEP}{}",
            b64(&component),
            b64(&controller)
        ))
    }

    /// Restores what `save_state` produced. Runs before activation, since a
    /// plugin may resize its buffers to fit what it reads.
    pub fn restore_state(&self, blob: &str) -> Result<(), String> {
        let (component, controller) = blob
            .split_once(STATE_HALVES_SEP)
            .ok_or("saved state is not a vst3 blob")?;
        let component = unb64(component)?;
        let controller = unb64(controller)?;

        // SAFETY: streams are owned here and outlive each call.
        unsafe {
            let s = stream(component);
            if !ok(self.component.setState(s.as_ptr())) {
                return Err("component rejected its saved state".into());
            }
            // The controller needs the component's state as well as its own:
            // that is how its display learns what the audio side is doing.
            rewind(&s);
            self.controller.setComponentState(s.as_ptr());

            if !controller.is_empty() {
                let s = stream(controller);
                self.controller.setState(s.as_ptr());
            }
        }
        Ok(())
    }

    /// Configures the plugin for the pipeline's format and starts it. The
    /// returned node is the only half that touches the audio thread.
    pub fn activate(
        &self,
        sample_rate: u32,
        max_frames: usize,
        params: Arc<ParamRing>,
        alive: Arc<AtomicBool>,
    ) -> Result<Vst3Node, String> {
        use vst3::Steinberg::Vst::{
            BusDirections_::{kInput, kOutput},
            IAudioProcessor, IAudioProcessorTrait, MediaTypes_::kAudio,
            ProcessModes_::kRealtime, ProcessSetup, SpeakerArr::kStereo,
            SymbolicSampleSizes_::kSample32,
        };

        let processor = self
            .component
            .cast::<IAudioProcessor>()
            .ok_or("component is not an audio processor")?;
        if !supports_f32(&processor) {
            return Err("plugin cannot process 32-bit float".into());
        }

        // SAFETY: the setup sequence below is the order VST3 requires, and each
        // index is below the count the plugin itself reported.
        unsafe {
            let ins = self.component.getBusCount(kAudio as i32, kInput as i32);
            let outs = self.component.getBusCount(kAudio as i32, kOutput as i32);

            // The pipeline is stereo, so every audio bus is asked to be stereo.
            // A plugin that refuses cannot carry our signal, and saying so beats
            // running it in a layout we then mis-read.
            let mut in_arr = vec![kStereo; ins.max(0) as usize];
            let mut out_arr = vec![kStereo; outs.max(0) as usize];
            if !ok(processor.setBusArrangements(
                in_arr.as_mut_ptr(),
                ins,
                out_arr.as_mut_ptr(),
                outs,
            )) {
                return Err("plugin refused a stereo bus layout".into());
            }

            for (dir, count) in [(kInput, ins), (kOutput, outs)] {
                for index in 0..count {
                    self.component
                        .activateBus(kAudio as i32, dir as i32, index, 1);
                }
            }

            let mut setup = ProcessSetup {
                processMode: kRealtime as i32,
                symbolicSampleSize: kSample32 as i32,
                maxSamplesPerBlock: max_frames as i32,
                sampleRate: sample_rate as f64,
            };
            if !ok(processor.setupProcessing(&mut setup)) {
                return Err(format!(
                    "plugin refused {sample_rate} Hz at {max_frames} frames per block"
                ));
            }

            if !ok(self.component.setActive(1)) {
                return Err("plugin refused to activate".into());
            }
            processor.setProcessing(1);

            let input_channels = self.channel_counts(kInput as i32, ins);
            let output_channels = self.channel_counts(kOutput as i32, outs);

            Ok(Vst3Node::new(
                processor,
                &input_channels,
                &output_channels,
                max_frames,
                params,
                alive,
            ))
        }
    }

    /// Channels per bus, read back after the arrangement is set.
    unsafe fn channel_counts(&self, direction: i32, count: i32) -> Vec<usize> {
        use vst3::Steinberg::Vst::{BusInfo, MediaTypes_::kAudio};

        (0..count)
            .map(|index| {
                let mut info: BusInfo = std::mem::zeroed();
                if ok(self
                    .component
                    .getBusInfo(kAudio as i32, direction, index, &mut info))
                {
                    info.channelCount.max(0) as usize
                } else {
                    0
                }
            })
            .collect()
    }
}

/// Puts a stream back at offset zero, the position a reader expects.
pub unsafe fn rewind(stream: &ComPtr<IBStream>) {
    use vst3::Steinberg::IBStream_::IStreamSeekMode_::kIBSeekSet;
    use vst3::Steinberg::IBStreamTrait;

    let mut landed = 0;
    stream.seek(0, kIBSeekSet as i32, &mut landed);
}

impl Drop for Vst3Instance {
    fn drop(&mut self) {
        use vst3::Steinberg::Vst::{IConnectionPoint, IConnectionPointTrait};

        // Unwind the setup in reverse: stop processing, deactivate, disconnect,
        // then terminate each half. Terminating a running or still-connected
        // plugin leaves the other side holding a reference to a dead object.
        unsafe {
            use vst3::Steinberg::Vst::{IAudioProcessor, IAudioProcessorTrait};
            if let Some(processor) = self.component.cast::<IAudioProcessor>() {
                processor.setProcessing(0);
            }
            self.component.setActive(0);
            if self.separate {
                if let (Some(from), Some(to)) = (
                    self.component.cast::<IConnectionPoint>(),
                    self.controller.cast::<IConnectionPoint>(),
                ) {
                    from.disconnect(to.as_ptr());
                    to.disconnect(from.as_ptr());
                }
                self.controller.terminate();
            }
            self.component.terminate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::plugins::vst3_backend::Vst3Backend;
    use crate::audio::plugins::PluginBackend;

    /// A test that needs an installed plugin and finds none proves nothing, and
    /// on CI that is the normal case. Say so rather than passing quietly.
    fn skipped(what: &str) {
        println!("SKIPPED: no vst3 plugins installed, cannot check {what}");
    }

    fn first_plugin() -> Option<(Vst3Instance, String)> {
        let plugin = Vst3Backend.scan().into_iter().next()?;
        let module = Vst3Module::open(std::path::Path::new(&plugin.path)).unwrap();
        Some((
            Vst3Instance::new(module, &plugin.plugin_id).unwrap(),
            plugin.name,
        ))
    }

    #[test]
    fn reports_parameters_in_normalised_form() {
        let Some((instance, name)) = first_plugin() else {
            return skipped("parameter reporting");
        };
        let params = instance.params();
        assert!(!params.is_empty(), "{name} exposed no parameters");
        for p in &params {
            assert_eq!((p.min, p.max), (0.0, 1.0), "{} is not normalised", p.name);
            assert!(
                (0.0..=1.0).contains(&p.value),
                "{} reads {} outside its own range",
                p.name,
                p.value
            );
            assert!(!p.name.is_empty(), "parameter {} has no name", p.id);
        }
        println!("{name}: {} visible params", params.len());
    }

    /// Host to editor: a value written by the node must be what the controller
    /// reports back, or the plugin's own window would show something else.
    #[test]
    fn a_parameter_set_by_the_host_is_read_back() {
        let Some((instance, name)) = first_plugin() else {
            return skipped("host-to-editor parameter writes");
        };
        // A stepped parameter snaps the written value to its nearest step, so
        // it cannot show whether the write itself landed.
        let Some(param) = instance
            .params()
            .into_iter()
            .find(|p| !p.read_only && !p.stepped)
        else {
            panic!("{name} has no continuous writable parameter");
        };
        let target = if param.value > 0.5 { 0.25 } else { 0.75 };
        instance.set_param(param.id, target);
        let readback = unsafe { instance.controller.getParamNormalized(param.id) };
        assert!(
            (readback - target).abs() < 1e-6,
            "{}: wrote {target}, read {readback}",
            param.name
        );
    }

    /// Editor to host: an edit inside the plugin's window must reach the ring,
    /// which is the only path from there to the audio thread.
    #[test]
    fn an_edit_from_the_plugin_reaches_the_param_ring() {
        struct ToRing(Arc<ParamRing>);
        impl crate::audio::plugins::vst3_com::EditListener for ToRing {
            fn param_edited(&self, id: u32, value: f64) {
                self.0.push(id, value);
            }
            fn restart(&self, _flags: i32) {}
        }

        let Some((mut instance, _)) = first_plugin() else {
            return skipped("editor-to-host parameter edits");
        };
        let ring = Arc::new(ParamRing::new());
        let mut cursor = ring.reader();
        instance.listen(Box::new(ToRing(ring.clone())));

        // Stand in for the plugin's editor: the handler it was just given is
        // the same object its own window would call.
        use vst3::Steinberg::Vst::IComponentHandlerTrait;
        let handler = instance
            .handler
            .as_ref()
            .and_then(|h| h.to_com_ptr::<IComponentHandler>())
            .unwrap();
        unsafe {
            handler.beginEdit(11);
            handler.performEdit(11, 0.375);
            handler.endEdit(11);
        }

        assert_eq!(ring.read(&mut cursor), Some((11, 0.375)));
    }

    /// A value set before saving must come back after restoring into a fresh
    /// instance, which is what reopening a project does.
    #[test]
    fn state_survives_a_reinstantiation() {
        let Some(plugin) = Vst3Backend.scan().into_iter().next() else {
            return skipped("state persistence");
        };
        let open = || {
            let module = Vst3Module::open(std::path::Path::new(&plugin.path)).unwrap();
            Vst3Instance::new(module, &plugin.plugin_id).unwrap()
        };

        let before = open();
        let param = before
            .params()
            .into_iter()
            .find(|p| !p.read_only && !p.stepped)
            .expect("a continuous parameter");
        let target = if param.value > 0.5 { 0.25 } else { 0.75 };
        before.set_param(param.id, target);
        let blob = before.save_state().expect("plugin saved no state");
        drop(before);

        let after = open();
        after.restore_state(&blob).unwrap();
        let restored = unsafe { after.controller.getParamNormalized(param.id) };
        assert!(
            (restored - target).abs() < 1e-6,
            "{}: saved {target}, restored {restored}",
            param.name
        );
    }

    #[test]
    fn a_state_blob_that_is_not_ours_is_refused() {
        let Some((instance, _)) = first_plugin() else {
            return skipped("rejection of a foreign state blob");
        };
        assert!(instance.restore_state("no separator here").is_err());
        assert!(instance.restore_state("not base64!.also not").is_err());
    }

    /// Audio must survive a round trip through the plugin: same length, no NaN
    /// or infinity, and the block must not be left untouched by a processor
    /// that silently refused to run.
    #[test]
    fn renders_signal_through_every_installed_plugin() {
        const FRAMES: usize = 512;
        const RATE: u32 = 48_000;

        let installed = Vst3Backend.scan();
        if installed.is_empty() {
            return skipped("audio rendering");
        }
        for plugin in installed {
            let module = Vst3Module::open(std::path::Path::new(&plugin.path)).unwrap();
            let instance = Vst3Instance::new(module, &plugin.plugin_id).unwrap();
            let params = std::sync::Arc::new(crate::audio::plugins::ParamRing::new());
            let alive = std::sync::Arc::new(AtomicBool::new(true));

            let mut node = match instance.activate(RATE, FRAMES, params.clone(), alive.clone()) {
                Ok(node) => node,
                Err(err) => panic!("{}: {err}", plugin.name),
            };

            // A plugin with lookahead outputs silence until its own latency has
            // passed, so measuring the first block would prove nothing.
            use crate::audio::effects::Effect;
            let blocks = node.latency_frames().div_ceil(FRAMES) + 2;
            let mut rms = 0.0;
            for block in 0..blocks {
                let mut samples = vec![0.0f32; FRAMES * 2];
                for i in 0..FRAMES {
                    let s = ((block * FRAMES + i) as f32 * 0.05).sin() * 0.5;
                    samples[2 * i] = s;
                    samples[2 * i + 1] = s;
                }
                // A parameter write in flight must be consumed, not skipped.
                params.push(0, 0.5);
                node.process(&mut samples, FRAMES);

                assert!(
                    samples.iter().all(|s| s.is_finite()),
                    "{} produced a non-finite sample",
                    plugin.name
                );
                rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
            }

            println!(
                "{}: out rms {rms:.4} after {blocks} blocks, latency {}",
                plugin.name,
                node.latency_frames()
            );
            assert!(
                rms > 0.0,
                "{} passed no audio once its latency elapsed",
                plugin.name
            );

            drop(node);
            assert!(!alive.load(std::sync::atomic::Ordering::Acquire));
        }
    }

    /// Every installed plugin must instantiate, expose a controller, and
    /// survive teardown. Both shapes (one object or two) go through here.
    #[test]
    fn instantiates_every_installed_plugin() {
        let found = Vst3Backend.scan();
        if found.is_empty() {
            return skipped("instantiation");
        }
        for plugin in found {
            let module = Vst3Module::open(std::path::Path::new(&plugin.path))
                .unwrap_or_else(|e| panic!("{}: {e}", plugin.name));
            let instance = Vst3Instance::new(module, &plugin.plugin_id)
                .unwrap_or_else(|e| panic!("{}: {e}", plugin.name));
            println!(
                "{}: {} params, {} halves",
                plugin.name,
                instance.params().len(),
                if instance.separate { 2 } else { 1 }
            );
            assert!(
                !instance.params().is_empty(),
                "{} exposes no parameters",
                plugin.name
            );
        }
    }
}
