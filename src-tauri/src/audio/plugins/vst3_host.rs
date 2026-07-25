//! Instantiating a VST3 plugin: the two halves, their connection, and the
//! state handshake between them.

use vst3::Steinberg::Vst::{IComponent, IComponentTrait, IEditController, IEditControllerTrait};
use vst3::Steinberg::{
    kResultOk, tresult, FIDString, IBStream, IPluginBaseTrait, IPluginFactoryTrait, TUID,
};
use vst3::{ComPtr, ComWrapper, Interface};

use super::vst3_backend::{parse_cid, Vst3Module};
use super::vst3_com::{host_context, MemoryStream};

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
