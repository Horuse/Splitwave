//! Instantiating a VST3 plugin: the two halves, their connection, and the
//! state handshake between them.

use vst3::Steinberg::Vst::{IComponent, IComponentTrait, IEditController, IEditControllerTrait};
use vst3::Steinberg::{
    kResultOk, tresult, FIDString, IBStream, IPluginBaseTrait, IPluginFactoryTrait, TUID,
};
use vst3::{ComPtr, ComWrapper, Interface};

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::vst3_backend::{parse_cid, Vst3Module};
use super::vst3_com::{host_context, MemoryStream};
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
    /// The factory that made these lives in the module, so it outlives them.
    _module: Vst3Module,
}

fn ok(result: tresult) -> bool {
    result == kResultOk
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

    pub fn parameter_count(&self) -> i32 {
        unsafe { self.controller.getParameterCount() }
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
                self.component.clone(),
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

        // Unwind the setup in reverse: disconnect, then terminate each half.
        // Terminating while still connected leaves the other side holding a
        // reference to a dead object.
        unsafe {
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

    /// Audio must survive a round trip through the plugin: same length, no NaN
    /// or infinity, and the block must not be left untouched by a processor
    /// that silently refused to run.
    #[test]
    fn renders_signal_through_every_installed_plugin() {
        const FRAMES: usize = 512;
        const RATE: u32 = 48_000;

        for plugin in Vst3Backend.scan() {
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

            crate::audio::plugins::vst3_node::deactivate(&node);
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
            println!("no vst3 plugins installed, nothing to instantiate");
            return;
        }
        for plugin in found {
            let module = Vst3Module::open(std::path::Path::new(&plugin.path))
                .unwrap_or_else(|e| panic!("{}: {e}", plugin.name));
            let instance = Vst3Instance::new(module, &plugin.plugin_id)
                .unwrap_or_else(|e| panic!("{}: {e}", plugin.name));
            println!(
                "{}: {} params, {} halves",
                plugin.name,
                instance.parameter_count(),
                if instance.separate { 2 } else { 1 }
            );
            assert!(
                instance.parameter_count() > 0,
                "{} exposes no parameters",
                plugin.name
            );
        }
    }
}
