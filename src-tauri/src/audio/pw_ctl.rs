use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;

use pipewire as pw;
use pw::spa::param::ParamType;
use pw::spa::pod::deserialize::PodDeserializer;
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Object, Pod, Property, Value, ValueArray};
use pw::spa::utils::SpaTypes;
use pw::types::ObjectType;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy)]
pub enum DefaultRoute {
    Sink,
    Source,
}

impl DefaultRoute {
    fn metadata_key(self) -> &'static str {
        match self {
            Self::Sink => "default.audio.sink",
            Self::Source => "default.audio.source",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodeVolume {
    pub channel_volumes: Vec<f32>,
    pub mute: bool,
}

// Every entry point runs its own short-lived main loop on a dedicated thread:
// the pipewire proxies are !Send and the loop owns them for its lifetime.
fn on_pw_thread<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    std::thread::spawn(f)
        .join()
        .map_err(|_| AppError::Host("pipewire control thread panicked".into()))?
}

struct Session {
    mainloop: pw::main_loop::MainLoopRc,
    core: pw::core::CoreRc,
}

impl Session {
    fn new() -> AppResult<Self> {
        let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pw_err)?;
        let context = pw::context::ContextRc::new(&mainloop, None).map_err(pw_err)?;
        let core = context.connect_rc(None).map_err(pw_err)?;
        Ok(Self { mainloop, core })
    }

    // Blocks until the server has processed everything queued so far. The error
    // slot carries a server-side rejection back out of the callback.
    fn round_trip(&self) -> AppResult<()> {
        let pending = self.core.sync(0).map_err(pw_err)?;
        let error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        let ml_done = self.mainloop.clone();
        let ml_err = self.mainloop.clone();
        let error_cb = error.clone();
        let _listener = self
            .core
            .add_listener_local()
            .done(move |id, seq| {
                if id == 0 && seq == pending {
                    ml_done.quit();
                }
            })
            .error(move |_, _, res, message| {
                *error_cb.borrow_mut() = Some(format!("{message} ({res})"));
                ml_err.quit();
            })
            .register();

        self.mainloop.run();

        match error.take() {
            Some(e) => Err(AppError::Host(format!("pipewire: {e}"))),
            None => Ok(()),
        }
    }
}

pub fn create_null_sink(node_name: String, description: String, positions: String) -> AppResult<()> {
    on_pw_thread(move || {
        let session = Session::new()?;
        let props = pw::properties::properties! {
            "factory.name" => "support.null-audio-sink",
            "node.name" => node_name.as_str(),
            "node.description" => description.as_str(),
            "media.class" => "Audio/Sink",
            "audio.position" => positions.as_str(),
            // keeps the node alive once this client disconnects
            "object.linger" => "true",
        };
        let _node: pw::node::Node = session.core.create_object("adapter", &props).map_err(pw_err)?;
        session.round_trip()
    })
}

pub fn destroy_node(id: u32) -> AppResult<()> {
    on_pw_thread(move || {
        let session = Session::new()?;
        let registry = session.core.get_registry_rc().map_err(pw_err)?;
        registry.destroy_global(id).into_result().map_err(pw_err)?;
        session.round_trip()
    })
}

pub fn node_volume(id: u32) -> AppResult<NodeVolume> {
    on_pw_thread(move || {
        let session = Session::new()?;
        let registry = session.core.get_registry_rc().map_err(pw_err)?;

        let node: Rc<RefCell<Option<pw::node::Node>>> = Rc::new(RefCell::new(None));
        let node_cb = node.clone();
        let volume: Rc<RefCell<Option<NodeVolume>>> = Rc::new(RefCell::new(None));
        let volume_cb = volume.clone();
        let listeners: Rc<RefCell<Vec<pw::node::NodeListener>>> = Rc::new(RefCell::new(Vec::new()));
        let registry_bind = registry.clone();

        let _reg = registry
            .add_listener_local()
            .global(move |global| {
                if global.type_ != ObjectType::Node || global.id != id {
                    return;
                }
                let Ok(proxy) = registry_bind.bind::<pw::node::Node, _>(global) else {
                    return;
                };
                let volume_param = volume_cb.clone();
                let listener = proxy
                    .add_listener_local()
                    .param(move |_, param_id, _, _, param| {
                        if param_id != ParamType::Props {
                            return;
                        }
                        if let Some(v) = param.and_then(parse_volume) {
                            *volume_param.borrow_mut() = Some(v);
                        }
                    })
                    .register();
                listeners.borrow_mut().push(listener);
                *node_cb.borrow_mut() = Some(proxy);
            })
            .register();

        session.round_trip()?;

        let node = node.borrow();
        let node = node
            .as_ref()
            .ok_or_else(|| AppError::Host(format!("pipewire: node {id} not found")))?;
        node.enum_params(0, Some(ParamType::Props), 0, u32::MAX);
        session.round_trip()?;

        volume
            .take()
            .ok_or_else(|| AppError::Host(format!("pipewire: node {id} exposes no Props")))
    })
}

pub fn set_node_volume(id: u32, channels: usize, scalar: f32, mute: bool) -> AppResult<()> {
    on_pw_thread(move || {
        let session = Session::new()?;
        let registry = session.core.get_registry_rc().map_err(pw_err)?;

        let node: Rc<RefCell<Option<pw::node::Node>>> = Rc::new(RefCell::new(None));
        let node_cb = node.clone();
        let registry_bind = registry.clone();
        let _reg = registry
            .add_listener_local()
            .global(move |global| {
                if global.type_ != ObjectType::Node || global.id != id {
                    return;
                }
                if let Ok(proxy) = registry_bind.bind::<pw::node::Node, _>(global) {
                    *node_cb.borrow_mut() = Some(proxy);
                }
            })
            .register();

        session.round_trip()?;

        let node = node.borrow();
        let node = node
            .as_ref()
            .ok_or_else(|| AppError::Host(format!("pipewire: node {id} not found")))?;

        let object = Object {
            type_: SpaTypes::ObjectParamProps.as_raw(),
            id: ParamType::Props.as_raw(),
            properties: vec![
                Property::new(pw::spa::sys::SPA_PROP_mute, Value::Bool(mute)),
                Property::new(
                    pw::spa::sys::SPA_PROP_channelVolumes,
                    Value::ValueArray(ValueArray::Float(vec![scalar; channels])),
                ),
            ],
        };
        let (bytes, _) = PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))
            .map_err(pw_err)?;
        let bytes = bytes.into_inner();
        let pod = Pod::from_bytes(&bytes).ok_or_else(|| AppError::Host("pipewire: malformed Props pod".into()))?;
        node.set_param(ParamType::Props, 0, pod);

        session.round_trip()
    })
}

// WirePlumber publishes the current defaults on the "default" metadata object as
// JSON values keyed by route.
pub fn default_node_name(route: DefaultRoute) -> AppResult<Option<String>> {
    on_pw_thread(move || {
        let session = Session::new()?;
        let registry = session.core.get_registry_rc().map_err(pw_err)?;

        let name: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let name_cb = name.clone();
        let metadata: Rc<RefCell<Option<pw::metadata::Metadata>>> = Rc::new(RefCell::new(None));
        let metadata_cb = metadata.clone();
        let listeners: Rc<RefCell<Vec<pw::metadata::MetadataListener>>> = Rc::new(RefCell::new(Vec::new()));
        let want = route.metadata_key();
        let registry_bind = registry.clone();

        let _reg = registry
            .add_listener_local()
            .global(move |global| {
                if global.type_ != ObjectType::Metadata {
                    return;
                }
                let Some(props) = &global.props else { return };
                if props.get("metadata.name") != Some("default") {
                    return;
                }
                let Ok(proxy) = registry_bind.bind::<pw::metadata::Metadata, _>(global) else {
                    return;
                };
                let name_prop = name_cb.clone();
                let listener = proxy
                    .add_listener_local()
                    .property(move |_, key, _, value| {
                        if key == Some(want) {
                            if let Some(v) = value.and_then(parse_metadata_name) {
                                *name_prop.borrow_mut() = Some(v);
                            }
                        }
                        0
                    })
                    .register();
                listeners.borrow_mut().push(listener);
                *metadata_cb.borrow_mut() = Some(proxy);
            })
            .register();

        session.round_trip()?;
        Ok(name.take())
    })
}

fn parse_metadata_name(value: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(value)
        .ok()?
        .get("name")?
        .as_str()
        .map(str::to_string)
}

fn parse_volume(param: &Pod) -> Option<NodeVolume> {
    let (_, value) = PodDeserializer::deserialize_any_from(param.as_bytes()).ok()?;
    let Value::Object(object) = value else { return None };

    let mut channel_volumes = None;
    let mut mute = None;
    for property in object.properties {
        match (property.key, property.value) {
            (pw::spa::sys::SPA_PROP_channelVolumes, Value::ValueArray(ValueArray::Float(v))) => {
                channel_volumes = Some(v)
            }
            (pw::spa::sys::SPA_PROP_mute, Value::Bool(m)) => mute = Some(m),
            _ => {}
        }
    }

    Some(NodeVolume { channel_volumes: channel_volumes?, mute: mute? })
}

fn pw_err(e: impl std::fmt::Display) -> AppError {
    AppError::Host(format!("pipewire: {e}"))
}
