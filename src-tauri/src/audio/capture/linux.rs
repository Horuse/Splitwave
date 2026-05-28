use std::cell::RefCell;
use std::rc::Rc;

use pipewire as pw;
use pw::spa;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::pod::Pod;

use crate::error::{AppError, AppResult};

struct Terminate;

struct UserData {
    callback: Box<dyn FnMut(&[f32])>,
}

pub struct Capture {
    sender: pw::channel::Sender<Terminate>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.sender.send(Terminate);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Capture {
    pub fn start_system(callback: impl FnMut(&[f32]) + Send + 'static) -> AppResult<Self> {
        spawn(None, true, Box::new(callback))
    }

    pub fn start_app(
        binary: &str,
        callback: impl FnMut(&[f32]) + Send + 'static,
    ) -> AppResult<Self> {
        let serial = resolve_serial(binary)?.ok_or_else(|| {
            AppError::Stream(format!("no audio stream found for {binary:?}"))
        })?;
        spawn(Some(serial), false, Box::new(callback))
    }
}

fn spawn(
    target_serial: Option<u32>,
    capture_sink: bool,
    callback: Box<dyn FnMut(&[f32]) + Send>,
) -> AppResult<Capture> {
    let (sender, receiver) = pw::channel::channel::<Terminate>();
    let thread = std::thread::spawn(move || {
        if let Err(e) = run(receiver, target_serial, capture_sink, callback) {
            tracing::error!("pipewire capture: {e:?}");
        }
    });
    Ok(Capture { sender, thread: Some(thread) })
}

fn run(
    receiver: pw::channel::Receiver<Terminate>,
    target_serial: Option<u32>,
    capture_sink: bool,
    callback: Box<dyn FnMut(&[f32]) + Send>,
) -> Result<(), pw::Error> {
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let _stopper = {
        let ml = mainloop.clone();
        receiver.attach(mainloop.loop_(), move |_| ml.quit())
    };

    let mut props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
    };
    if capture_sink {
        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }
    if let Some(serial) = target_serial {
        props.insert(*pw::keys::TARGET_OBJECT, serial.to_string());
    }

    let stream = pw::stream::StreamRc::new(core.clone(), "splitwave-capture", props)?;
    let user_data = UserData { callback };

    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .process(|stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else { return };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            let data = &mut datas[0];
            let offset = data.chunk().offset() as usize;
            let size = data.chunk().size() as usize;
            if size == 0 {
                return;
            }
            let Some(raw) = data.data() else { return };
            let bytes = &raw[offset..offset + size];
            let n = size / std::mem::size_of::<f32>();
            // F32LE is negotiated below and PipeWire aligns its buffers.
            let samples = unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const f32, n) };
            (user_data.callback)(samples);
        })
        .register()?;

    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    audio_info.set_rate(48_000);
    audio_info.set_channels(2);

    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&values).unwrap()];

    stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    mainloop.run();
    Ok(())
}

// TARGET_OBJECT takes the node's object.serial, so resolve it from the binary
// reported on the app's output stream.
fn resolve_serial(binary: &str) -> AppResult<Option<u32>> {
    let binary = binary.to_string();
    std::thread::spawn(move || serial_snapshot(&binary))
        .join()
        .map_err(|_| AppError::Host("pipewire serial lookup thread panicked".into()))?
}

fn serial_snapshot(binary: &str) -> AppResult<Option<u32>> {
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pw_err)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pw_err)?;
    let core = context.connect_rc(None).map_err(pw_err)?;
    let registry = core.get_registry_rc().map_err(pw_err)?;

    let found: Rc<RefCell<Option<u32>>> = Rc::new(RefCell::new(None));
    let found_cb = found.clone();
    let want = binary.to_string();

    let _reg = registry
        .add_listener_local()
        .global(move |global| {
            if global.type_ != pw::types::ObjectType::Node {
                return;
            }
            let Some(props) = &global.props else { return };
            if props.get("media.class") != Some("Stream/Output/Audio") {
                return;
            }
            // Match the same fields the app list builds bundle_id from: binary
            // first, then application.name, then node.name.
            let want = want.as_str();
            let matches = props.get("application.process.binary") == Some(want)
                || props.get("application.name") == Some(want)
                || props.get("node.name") == Some(want);
            if !matches {
                return;
            }
            if let Some(serial) = props.get("object.serial").and_then(|s| s.parse().ok()) {
                found_cb.borrow_mut().get_or_insert(serial);
            }
        })
        .register();

    let pending = core.sync(0).map_err(pw_err)?;
    let ml = mainloop.clone();
    let _core = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == 0 && seq == pending {
                ml.quit();
            }
        })
        .register();

    mainloop.run();
    let serial = *found.borrow();
    Ok(serial)
}

fn pw_err(e: impl std::fmt::Display) -> AppError {
    AppError::Host(format!("pipewire: {e}"))
}
