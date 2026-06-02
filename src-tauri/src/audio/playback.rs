use pipewire as pw;
use pw::spa;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::pod::Pod;

use crate::error::AppResult;

const CHANNELS: usize = 2;
const RATE: u32 = 48_000;
const F32_SIZE: usize = std::mem::size_of::<f32>();
const STRIDE: usize = F32_SIZE * CHANNELS;

struct Terminate;

struct UserData {
    fill: Box<dyn FnMut(&mut [f32]) -> usize>,
}

pub struct Playback {
    sender: pw::channel::Sender<Terminate>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Playback {
    fn drop(&mut self) {
        let _ = self.sender.send(Terminate);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Playback {
    pub fn start(
        sink_node_name: &str,
        fill: impl FnMut(&mut [f32]) -> usize + Send + 'static,
    ) -> AppResult<Self> {
        let (sender, receiver) = pw::channel::channel::<Terminate>();
        let target = sink_node_name.to_string();
        let thread = std::thread::spawn(move || {
            if let Err(e) = run(receiver, &target, Box::new(fill)) {
                tracing::error!("pipewire playback: {e:?}");
            }
        });
        Ok(Playback { sender, thread: Some(thread) })
    }
}

fn run(
    receiver: pw::channel::Receiver<Terminate>,
    sink_node_name: &str,
    fill: Box<dyn FnMut(&mut [f32]) -> usize + Send>,
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
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "Music",
    };
    props.insert(*pw::keys::TARGET_OBJECT, sink_node_name);

    let stream = pw::stream::StreamRc::new(core.clone(), "splitwave-playback", props)?;
    let user_data = UserData { fill };

    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .process(|stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else { return };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            let data = &mut datas[0];
            let Some(raw) = data.data() else { return };
            let capacity = raw.len() / F32_SIZE;
            if capacity == 0 {
                return;
            }
            let mut samples = vec![0.0f32; capacity];
            let written = (user_data.fill)(&mut samples).min(capacity);
            for (i, s) in samples[..written].iter().enumerate() {
                raw[i * F32_SIZE..(i + 1) * F32_SIZE].copy_from_slice(&s.to_le_bytes());
            }
            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = STRIDE as i32;
            *chunk.size_mut() = (written * F32_SIZE) as u32;
        })
        .register()?;

    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    audio_info.set_rate(RATE);
    audio_info.set_channels(CHANNELS as u32);

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
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    mainloop.run();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn playback_pulls_samples() {
        let calls = Arc::new(AtomicUsize::new(0));
        let total = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let t = total.clone();
        let mut phase = 0.0f32;
        let pb = Playback::start("alsa_output.pci-0000_00_0a.0.stereo-fallback", move |buf| {
            c.fetch_add(1, Ordering::Relaxed);
            t.fetch_add(buf.len(), Ordering::Relaxed);
            for f in buf.chunks_mut(2) {
                let s = (phase * 2.0 * std::f32::consts::PI * 440.0 / 48000.0).sin() * 0.2;
                phase += 1.0;
                if f.len() == 2 {
                    f[0] = s;
                    f[1] = s;
                }
            }
            buf.len()
        })
        .expect("start playback");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        drop(pb);
        let n = calls.load(Ordering::Relaxed);
        let samples = total.load(Ordering::Relaxed);
        println!("playback: {n} process calls, {samples} f32 requested");
        assert!(n > 0, "process never called");
        assert!(samples > 0, "no samples requested");
    }
}
