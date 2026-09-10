use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::Arc;
use std::time::Instant;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::audio::effects::{
    instantiate_effect, EffectControl, EffectRegistry, GrHandle, LufsHandle, MeterHandle,
    WaveformHandle,
};
use crate::audio::graph::{EdgeKind, EffectSpec, InputSpec, NetCodec, OutputSpec, ValidGraph};
use crate::audio::netaudio::packet::Format;
use crate::audio::resample::MultiResampler;
use crate::audio::stream_recv::ChannelReceiver;
use crate::error::{AppError, AppResult};

use super::graph::{BuiltOutputGraph, DelayLine, OutputGraph};
use super::nodes::{
    edge_channels, parse_ch, parse_stereo, tap_handle_width, tap_key, target_route, ConsumerState,
    DagNode, EffectState, IncomingEdge, OutputMeta, ProducerState, SourceMeta, SourceState,
    SourceStats, TerminalEdge, SPLICE_FADE_FRAMES,
};
use super::staging::StagingRing;
use super::{ring_capacity_frames, DSP_BLOCK_FRAMES, MAX_NET_CH, RESAMPLE_CHUNK};

/// Build the per-output DAG: walk backward from `output_id`, topo-sort the
/// reachable sub-graph, instantiate sources (with their rings) and effects
/// (with their parameter atomics) in order.
///
/// `output_id = None` means monitor mode: every surviving input + effect is
/// reachable (validate already trimmed anything that doesn't drive an
/// analyzer), and the resulting graph has no output terminals.
/// `producer_pairs` carries Producer ends of the ring per Source node,
/// paired with their input node id. Caller tags each pair with the owning
/// output id and routes them into the matching input's broadcast.
pub fn build_output_graph(
    output_id: Option<&str>,
    output_sr: u32,
    realtime: bool,
    valid: &ValidGraph,
    input_native_sr: &HashMap<String, u32>,
    input_native_channels: &HashMap<String, u32>,
    producer_pairs: &mut Vec<(String, Producer<f32>)>,
    registry: &mut EffectRegistry,
    input_volumes: &HashMap<String, Arc<AtomicU32>>,
    input_paused: &HashMap<String, Arc<AtomicBool>>,
    input_drain: &HashMap<String, Arc<AtomicU64>>,
    input_meters: &HashMap<String, MeterHandle>,
    mut cut_leaves: HashMap<String, (Consumer<f32>, u32, usize)>,
) -> AppResult<BuiltOutputGraph> {
    let cut_leaf_ids: HashSet<String> = cut_leaves.keys().cloned().collect();
    let reachable: HashSet<String> = match output_id {
        Some(id) => reachable_backward_cut(id, valid, &cut_leaf_ids),
        None => {
            let roots: Vec<String> = valid
                .effects
                .iter()
                .filter(|e| is_analyzer(&e.spec))
                .map(|e| e.id.clone())
                .collect();
            reachable_backward_from(&roots, valid, &cut_leaf_ids)
        }
    };

    let mut indegree: HashMap<String, usize> = HashMap::new();
    for id in &reachable {
        indegree.entry(id.clone()).or_insert(0);
    }
    for edge in &valid.edges {
        if reachable.contains(&edge.from) && reachable.contains(&edge.to) {
            *indegree.entry(edge.to.clone()).or_insert(0) += 1;
        }
    }
    let mut queue: Vec<String> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(id, _)| id.clone())
        .collect();
    queue.sort();
    let mut topo: Vec<String> = Vec::with_capacity(reachable.len());
    while let Some(id) = queue.pop() {
        topo.push(id.clone());
        for edge in &valid.edges {
            if edge.from == id && reachable.contains(&edge.to) {
                let d = indegree.get_mut(&edge.to).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push(edge.to.clone());
                }
            }
        }
    }
    if topo.len() != reachable.len() {
        return Err(AppError::Validation(format!(
            "internal: topo sort failed for output {}",
            output_id.unwrap_or("<monitor>")
        )));
    }

    let mut nodes: Vec<DagNode> = Vec::with_capacity(topo.len());
    let mut id_to_index: HashMap<String, usize> = HashMap::new();
    let mut node_meta: HashMap<String, (usize, usize)> = HashMap::new();
    let mut controls: Vec<(String, EffectControl)> = Vec::new();
    let mut bypasses: Vec<(String, Arc<AtomicBool>)> = Vec::new();
    let mut meters: Vec<MeterHandle> = Vec::new();
    let mut lufs: Vec<LufsHandle> = Vec::new();
    let mut gr_handles: Vec<GrHandle> = Vec::new();
    let mut scopes: Vec<WaveformHandle> = Vec::new();
    let mut sources: Vec<SourceMeta> = Vec::new();
    let mut node_latencies: Vec<usize> = Vec::with_capacity(topo.len());
    let mut node_channels: Vec<usize> = Vec::with_capacity(topo.len());

    for id in &topo {
        if let Some((consumer, owner_sr, width)) = cut_leaves.remove(id) {
            let source = ring_source(id, consumer, owner_sr, output_sr, width, realtime, valid)?;
            sources.push(SourceMeta {
                label: format!("{} out={}", source.label, output_id.unwrap_or("monitor")),
                stats: source.stats.clone(),
                channels: width,
                native_sr: owner_sr,
                frames_per_block: source.input_samples_per_block / width.max(1),
                input_id: None,
                output_id: output_id.unwrap_or("monitor").to_string(),
                capture: None,
            });
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Source(source));
            node_latencies.push(0);
            node_channels.push(width);
            continue;
        }
        if let Some(input) = valid.inputs.iter().find(|i| &i.id == id) {
            let network = match &input.spec {
                InputSpec::NetReceiver { port } => {
                    let receiver = crate::audio::netaudio::receiver::get_or_create(id, *port);
                    Some(ChannelReceiver::new(
                        receiver.register_consumer(output_sr, realtime),
                    ))
                }
                InputSpec::WebRtcRecv {
                    node_id,
                    opus_bitrate,
                    opus_application,
                } => {
                    let session = crate::audio::webrtc::get_or_create(
                        node_id,
                        *opus_bitrate,
                        *opus_application,
                    );
                    Some(ChannelReceiver::new(
                        session.register_bridge(output_sr, realtime),
                    ))
                }
                _ => None,
            };
            if let Some(receiver) = network {
                let mut handles: Vec<String> = valid
                    .edges
                    .iter()
                    .filter(|e| &e.from == id)
                    .filter_map(|e| e.source_handle.clone())
                    .collect();
                handles.sort();
                handles.dedup();
                let pw = 2;
                let mut handle_bufs = Vec::with_capacity(handles.len());
                let mut wire_keys = Vec::with_capacity(handles.len());
                for h in handles {
                    let Some(key) = tap_key(&h) else { continue };
                    wire_keys.push(key);
                    handle_bufs.push((h, vec![0.0; DSP_BLOCK_FRAMES]));
                }
                id_to_index.insert(id.clone(), nodes.len());
                nodes.push(DagNode::Producer(ProducerState {
                    receiver,
                    out_buf: vec![0.0; DSP_BLOCK_FRAMES * pw],
                    handle_bufs,
                    wire_keys,
                }));
                node_latencies.push(0);
                node_channels.push(pw);
                continue;
            }
            let source_realtime = realtime && !matches!(input.spec, InputSpec::AudioFile { .. });
            let input_sr = *input_native_sr
                .get(id)
                .ok_or_else(|| AppError::Validation(format!("input {id} has no SR")))?;
            let source_channels = input_native_channels.get(id).copied().unwrap_or(2) as usize;
            let (producer, consumer) =
                RingBuffer::<f32>::new(ring_capacity_frames(input_sr) * source_channels);
            producer_pairs.push((id.clone(), producer));
            let mut ch_handles: Vec<String> = valid
                .edges
                .iter()
                .filter(|e| &e.from == id)
                .filter_map(|e| e.source_handle.clone())
                .filter(|h| tap_handle_width(h).is_some())
                .collect();
            ch_handles.sort();
            ch_handles.dedup();
            let source_handle_bufs: Vec<(String, Vec<f32>)> = ch_handles
                .into_iter()
                .map(|h| {
                    let w = tap_handle_width(&h).unwrap_or(1);
                    (h, vec![0.0; DSP_BLOCK_FRAMES * w])
                })
                .collect();
            let resampler = if input_sr == output_sr {
                None
            } else {
                Some(MultiResampler::new(
                    input_sr,
                    output_sr,
                    RESAMPLE_CHUNK,
                    source_channels,
                )?)
            };
            let out_max = resampler
                .as_ref()
                .map(|r| r.out_max())
                .unwrap_or(RESAMPLE_CHUNK);
            let staging_cap = (out_max * 4 + DSP_BLOCK_FRAMES) * source_channels;
            let input_frames_per_block =
                (DSP_BLOCK_FRAMES as u64 * input_sr as u64 + output_sr as u64 - 1)
                    / output_sr as u64;
            let input_samples_per_block = (input_frames_per_block as usize) * source_channels;

            let kind = match &input.spec {
                InputSpec::Microphone { device_id } => format!("mic:{device_id}"),
                InputSpec::SystemAudio { .. } => "system-audio".to_string(),
                InputSpec::AppAudio { bundle_id } => format!("app:{bundle_id}"),
                InputSpec::AudioFile { file_path } => format!("file:{file_path}"),
                InputSpec::NetReceiver { .. } | InputSpec::WebRtcRecv { .. } => {
                    unreachable!("network inputs are built as producers")
                }
            };
            let label = format!(
                "{kind}@{input_sr}->{output_sr} out={}",
                output_id.unwrap_or("monitor")
            );
            let stats = SourceStats::new();
            sources.push(SourceMeta {
                label: label.clone(),
                stats: stats.clone(),
                channels: source_channels,
                native_sr: input_sr,
                frames_per_block: input_frames_per_block as usize,
                input_id: Some(id.clone()),
                output_id: output_id.unwrap_or("monitor").to_string(),
                capture: None,
            });
            let source = SourceState {
                label,
                channels: source_channels,
                consumer,
                resampler,
                input_staging: Vec::with_capacity(
                    (RESAMPLE_CHUNK + SPLICE_FADE_FRAMES) * source_channels + 8,
                ),
                splice_tmp: Vec::with_capacity(SPLICE_FADE_FRAMES * source_channels),
                out_pending: StagingRing::with_capacity(staging_cap),
                chunk_tmp: Vec::with_capacity(out_max * source_channels),
                out_buf: vec![0.0; DSP_BLOCK_FRAMES * source_channels],
                input_samples_per_block,
                realtime: source_realtime,
                last_pop_at: Instant::now(),
                first_data_logged: false,
                volume: input_volumes
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits()))),
                paused: input_paused.get(id).cloned(),
                drain: input_drain.get(id).cloned(),
                last_drain_gen: 0,
                meter: input_meters.get(id).cloned(),
                handle_bufs: source_handle_bufs,
                stats,
            };
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Source(source));
            node_latencies.push(0);
            node_channels.push(source_channels);
        } else if let Some(effect) = valid.effects.iter().find(|e| &e.id == id) {
            type Upstream = (usize, Option<String>, Option<String>);
            let mut main_upstream: Vec<Upstream> = Vec::new();
            let mut side_upstream: Vec<Upstream> = Vec::new();
            for e in &valid.edges {
                if &e.to == id && reachable.contains(&e.from) {
                    let idx = id_to_index[&e.from];
                    let entry = (idx, e.source_handle.clone(), e.target_handle.clone());
                    match e.kind {
                        EdgeKind::Main => main_upstream.push(entry),
                        EdgeKind::Sidechain => side_upstream.push(entry),
                    }
                }
            }
            let max_upstream = main_upstream
                .iter()
                .chain(side_upstream.iter())
                .map(|(i, _, _)| node_latencies[*i])
                .max()
                .unwrap_or(0);
            let upstream_w = main_upstream
                .iter()
                .map(|(i, sh, _)| edge_channels(&nodes, &node_channels, *i, sh.as_deref()))
                .max()
                .unwrap_or(2);
            let target_w = main_upstream
                .iter()
                .filter_map(|(_, _, t)| t.as_deref().and_then(target_route))
                .map(|(off, w)| off + w)
                .max()
                .unwrap_or(0);
            let tap_w = valid
                .edges
                .iter()
                .filter(|e| &e.from == id)
                .filter_map(|e| e.source_handle.as_deref())
                .filter_map(|h| parse_stereo(h).map(|a| a + 1).or_else(|| parse_ch(h)))
                .max()
                .unwrap_or(0);
            let eff_channels = upstream_w.max(target_w).max(tap_w).max(1);
            let build = instantiate_effect(
                &effect.spec,
                id,
                output_sr,
                realtime,
                true,
                eff_channels,
                registry,
            );
            if let Some(c) = build.control {
                controls.push((id.clone(), c));
            }
            if build.bypass_is_new {
                bypasses.push((id.clone(), build.bypass.clone()));
            }
            if let Some(m) = build.meter {
                meters.push(m);
            }
            if let Some(l) = build.lufs {
                lufs.push(l);
            }
            if let Some(g) = build.gr {
                gr_handles.push(g);
            }
            if let Some(s) = build.scope {
                scopes.push(s);
            }
            let bypass = build.bypass;
            let make_edge =
                |src_idx: usize, source_handle: Option<String>, target_handle: Option<String>| {
                    let pad = max_upstream - node_latencies[src_idx];
                    let width =
                        edge_channels(&nodes, &node_channels, src_idx, source_handle.as_deref());
                    IncomingEdge {
                        src_idx,
                        source_handle,
                        target_handle,
                        delay: if pad > 0 {
                            Some(DelayLine::new(pad, width))
                        } else {
                            None
                        },
                    }
                };
            let incoming: Vec<IncomingEdge> = main_upstream
                .into_iter()
                .map(|(i, s, t)| make_edge(i, s, t))
                .collect();
            let sidechain: Vec<IncomingEdge> = side_upstream
                .into_iter()
                .map(|(i, s, t)| make_edge(i, s, t))
                .collect();
            let sidechain_buf = if sidechain.is_empty() {
                None
            } else {
                Some(vec![0.0; DSP_BLOCK_FRAMES * eff_channels])
            };
            let mut handle_ids: Vec<String> = valid
                .edges
                .iter()
                .filter(|e| &e.from == id)
                .filter_map(|e| e.source_handle.clone())
                .filter(|h| tap_handle_width(h).is_some())
                .collect();
            handle_ids.sort();
            handle_ids.dedup();
            let handle_bufs: Vec<(String, Vec<f32>)> = handle_ids
                .into_iter()
                .map(|h| {
                    let w = tap_handle_width(&h).unwrap_or(2);
                    (h, vec![0.0; DSP_BLOCK_FRAMES * w])
                })
                .collect();
            let full_width = build.full_width
                || matches!(
                    effect.spec,
                    EffectSpec::LevelMeter(_) | EffectSpec::Waveform(_) | EffectSpec::Spectrum(_)
                );
            let pairs = if full_width {
                1
            } else {
                eff_channels.div_ceil(2)
            };
            let mut effects = Vec::with_capacity(pairs);
            let own = build.effect.latency_frames();
            effects.push(build.effect);
            for _ in 1..pairs {
                let extra =
                    instantiate_effect(&effect.spec, id, output_sr, realtime, false, 2, registry);
                effects.push(extra.effect);
            }
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Effect(EffectState {
                effects,
                full_width,
                bypass,
                incoming,
                sidechain,
                out_buf: vec![0.0; DSP_BLOCK_FRAMES * eff_channels],
                sidechain_buf,
                pair_main: vec![0.0; DSP_BLOCK_FRAMES * 2],
                pair_side: vec![0.0; DSP_BLOCK_FRAMES * 2],
                handle_bufs,
                taps: Vec::new(),
            }));
            node_meta.insert(id.clone(), (nodes.len() - 1, eff_channels));
            node_latencies.push(max_upstream + own);
            node_channels.push(eff_channels);
        }
    }

    let out_label = output_id
        .map(|id| format!("out={id}"))
        .unwrap_or_else(|| "monitor".to_string());
    let blocks = Arc::new(AtomicU64::new(0));

    let wire_sender = output_id
        .and_then(|oid| valid.outputs.iter().find(|o| o.id == oid))
        .and_then(|o| match &o.spec {
            OutputSpec::NetSender { .. } | OutputSpec::WebRtcSend { .. } => Some(o.spec.clone()),
            _ => None,
        });
    if let Some(spec) = wire_sender {
        let oid = output_id.unwrap();
        let mut up: Vec<(usize, Option<String>, Option<String>)> = Vec::new();
        for e in &valid.edges {
            if e.to == oid && reachable.contains(&e.from) {
                let idx = id_to_index[&e.from];
                up.push((idx, e.source_handle.clone(), e.target_handle.clone()));
            }
        }
        let max_up = up
            .iter()
            .map(|(i, _, _)| node_latencies[*i])
            .max()
            .unwrap_or(0);
        let incoming: Vec<IncomingEdge> = up
            .into_iter()
            .map(|(idx, source_handle, target_handle)| {
                let pad = max_up - node_latencies[idx];
                let width = edge_channels(&nodes, &node_channels, idx, source_handle.as_deref());
                IncomingEdge {
                    src_idx: idx,
                    source_handle,
                    target_handle,
                    delay: if pad > 0 {
                        Some(DelayLine::new(pad, width))
                    } else {
                        None
                    },
                }
            })
            .collect();

        let channels = match &spec {
            OutputSpec::NetSender { channels, .. } | OutputSpec::WebRtcSend { channels, .. } => {
                *channels
            }
            _ => unreachable!("wire sender spec"),
        };
        let n = channels.clamp(1, MAX_NET_CH) as usize;
        let mut channel_bufs: Vec<(String, Vec<f32>)> = Vec::with_capacity(n);
        let mut send_producers: Vec<Producer<f32>> = Vec::with_capacity(n);
        let mut send_consumers: Vec<Consumer<f32>> = Vec::with_capacity(n);
        for c in 1..=n {
            channel_bufs.push((format!("ch{c}"), vec![0.0; DSP_BLOCK_FRAMES]));
            let (prod, cons) = RingBuffer::<f32>::new(crate::audio::netaudio::SEND_RING);
            send_producers.push(prod);
            send_consumers.push(cons);
        }
        match &spec {
            OutputSpec::NetSender {
                node_id,
                target,
                codec,
                opus_bitrate,
                opus_application,
                ..
            } => {
                let format = match codec {
                    NetCodec::PcmF32 => Format::PcmF32,
                    NetCodec::PcmI16 => Format::PcmI16,
                    NetCodec::Opus => Format::Opus,
                };
                let sender = crate::audio::netaudio::sender::get_or_create(
                    node_id,
                    *target,
                    format,
                    *opus_bitrate,
                    *opus_application,
                    output_sr,
                );
                sender.set_send_consumers(send_consumers);
            }
            OutputSpec::WebRtcSend {
                node_id,
                opus_bitrate,
                opus_application,
                ..
            } => {
                let session =
                    crate::audio::webrtc::get_or_create(node_id, *opus_bitrate, *opus_application);
                session.set_send_consumers(send_consumers, output_sr);
            }
            _ => unreachable!("wire sender spec"),
        }

        nodes.push(DagNode::Consumer(ConsumerState {
            incoming,
            channel_bufs,
            send_producers,
        }));

        return Ok(BuiltOutputGraph {
            graph: OutputGraph {
                sample_rate: output_sr,
                out_channels: 2,
                nodes,
                terminals: Vec::new(),
                latency_frames: max_up,
                blocks: blocks.clone(),
            },
            controls,
            bypasses,
            meters,
            lufs,
            gr_handles,
            scopes,
            sources,
            output: OutputMeta {
                label: out_label,
                blocks,
                sample_rate: output_sr,
                channels: 2,
                io: None,
            },
            node_meta,
        });
    }

    let terminals: Vec<TerminalEdge> = match output_id {
        Some(id) => {
            let upstream: Vec<(usize, Option<String>, Option<(usize, usize)>)> = valid
                .edges
                .iter()
                .filter(|e| e.to == id)
                .filter_map(|e| {
                    id_to_index.get(&e.from).copied().map(|idx| {
                        let route = e.target_handle.as_deref().and_then(target_route);
                        (idx, e.source_handle.clone(), route)
                    })
                })
                .collect();
            let max_upstream = upstream
                .iter()
                .map(|(i, _, _)| node_latencies[*i])
                .max()
                .unwrap_or(0);
            upstream
                .into_iter()
                .map(|(src_idx, source_handle, route)| {
                    let pad = max_upstream - node_latencies[src_idx];
                    let width =
                        edge_channels(&nodes, &node_channels, src_idx, source_handle.as_deref());
                    TerminalEdge {
                        src_idx,
                        source_handle,
                        route,
                        delay: if pad > 0 {
                            Some(DelayLine::new(pad, width))
                        } else {
                            None
                        },
                    }
                })
                .collect()
        }
        None => Vec::new(),
    };

    Ok(BuiltOutputGraph {
        graph: OutputGraph {
            sample_rate: output_sr,
            out_channels: 2,
            nodes,
            terminals,
            latency_frames: node_latencies.iter().copied().max().unwrap_or(0),
            blocks: blocks.clone(),
        },
        controls,
        bypasses,
        meters,
        lufs,
        gr_handles,
        scopes,
        sources,
        output: OutputMeta {
            label: out_label,
            blocks,
            sample_rate: output_sr,
            channels: 2,
            io: None,
        },
        node_meta,
    })
}

/// Builds a `SourceState` that reads a fan-out node's published block from a
/// ring (written at `owner_sr`) and resamples it to this graph's `output_sr`.
/// Reuses the source machinery so per-channel taps and backlog-dropping behave
/// exactly like a captured input.
#[allow(clippy::too_many_arguments)]
fn ring_source(
    id: &str,
    consumer: Consumer<f32>,
    owner_sr: u32,
    output_sr: u32,
    channels: usize,
    realtime: bool,
    valid: &ValidGraph,
) -> AppResult<SourceState> {
    let resampler = if owner_sr == output_sr {
        None
    } else {
        Some(MultiResampler::new(
            owner_sr,
            output_sr,
            RESAMPLE_CHUNK,
            channels,
        )?)
    };
    let out_max = resampler
        .as_ref()
        .map(|r| r.out_max())
        .unwrap_or(RESAMPLE_CHUNK);
    let staging_cap = (out_max * 4 + DSP_BLOCK_FRAMES) * channels;
    let input_frames_per_block =
        (DSP_BLOCK_FRAMES as u64 * owner_sr as u64 + output_sr as u64 - 1) / output_sr as u64;
    let input_samples_per_block = input_frames_per_block as usize * channels;

    let mut ch_handles: Vec<String> = valid
        .edges
        .iter()
        .filter(|e| e.from == id)
        .filter_map(|e| e.source_handle.clone())
        .filter(|h| tap_handle_width(h).is_some())
        .collect();
    ch_handles.sort();
    ch_handles.dedup();
    let handle_bufs: Vec<(String, Vec<f32>)> = ch_handles
        .into_iter()
        .map(|h| {
            let w = tap_handle_width(&h).unwrap_or(1);
            (h, vec![0.0; DSP_BLOCK_FRAMES * w])
        })
        .collect();

    Ok(SourceState {
        label: format!("cut:{id}"),
        channels,
        consumer,
        resampler,
        input_staging: Vec::with_capacity((RESAMPLE_CHUNK + SPLICE_FADE_FRAMES) * channels + 8),
        splice_tmp: Vec::with_capacity(SPLICE_FADE_FRAMES * channels),
        out_pending: StagingRing::with_capacity(staging_cap),
        chunk_tmp: Vec::with_capacity(out_max * channels),
        out_buf: vec![0.0; DSP_BLOCK_FRAMES * channels],
        input_samples_per_block,
        realtime,
        last_pop_at: Instant::now(),
        first_data_logged: false,
        volume: Arc::new(AtomicU32::new(0x3F80_0000)),
        paused: None,
        drain: None,
        last_drain_gen: 0,
        meter: None,
        handle_bufs,
        stats: SourceStats::new(),
    })
}

/// Cross-output fan-out plan: which effect nodes are computed once and shared
/// via rings. `owner[n]` builds node `n` and publishes it; every output in
/// `consumers[n]` reads it back as a ring-source.
pub struct CutPlan {
    pub owner: HashMap<String, String>,
    pub consumers: HashMap<String, Vec<String>>,
}

impl CutPlan {
    /// Outputs that participate in any cut (owners + consumers). When one of
    /// them is rebuilt they must all rebuild together, so producer and consumer
    /// ends of every ring are created in the same pass.
    pub fn participants(&self) -> HashSet<String> {
        let mut set = HashSet::new();
        for (node, cons) in &self.consumers {
            if cons.is_empty() {
                continue;
            }
            if let Some(o) = self.owner.get(node) {
                set.insert(o.clone());
            }
            set.extend(cons.iter().cloned());
        }
        set
    }
}

/// Assigns each effect node to the first output (in graph order) that can
/// compute it, and records where later graphs must read it back via a ring.
/// Traversal stops at nodes already owned by an earlier graph -- those become
/// ring-source leaves -- so a shared node is computed exactly once. The monitor
/// (identified by `monitor_key`) is treated as a final consumer, so a plugin
/// feeding both a speaker and an analyzer is computed once, not duplicated.
pub fn plan_cuts(valid: &ValidGraph, monitor_key: Option<&str>) -> CutPlan {
    let effect_ids: HashSet<&str> = valid.effects.iter().map(|e| e.id.as_str()).collect();
    let mut owner: HashMap<String, String> = HashMap::new();
    let mut consumers: HashMap<String, Vec<String>> = HashMap::new();

    let mut assign = |oid: &str, starts: Vec<String>| {
        let mut visited: HashSet<String> = HashSet::new();
        let mut stack = starts;
        while let Some(m) = stack.pop() {
            if !effect_ids.contains(m.as_str()) {
                continue;
            }
            if owner.contains_key(&m) {
                consumers.entry(m).or_default().push(oid.to_string());
                continue;
            }
            if !visited.insert(m.clone()) {
                continue;
            }
            for e in &valid.edges {
                if e.to == m {
                    stack.push(e.from.clone());
                }
            }
        }
        for m in visited {
            owner.insert(m, oid.to_string());
        }
    };

    for out in &valid.outputs {
        let starts = valid
            .edges
            .iter()
            .filter(|e| e.to == out.id)
            .map(|e| e.from.clone())
            .collect();
        assign(&out.id, starts);
    }
    if let Some(mk) = monitor_key {
        let starts = valid
            .effects
            .iter()
            .filter(|e| is_analyzer(&e.spec))
            .map(|e| e.id.clone())
            .collect();
        assign(mk, starts);
    }

    for v in consumers.values_mut() {
        v.dedup();
    }
    CutPlan { owner, consumers }
}

fn is_analyzer(spec: &EffectSpec) -> bool {
    matches!(
        spec,
        EffectSpec::LevelMeter(_)
            | EffectSpec::LufsMeter(_)
            | EffectSpec::Waveform(_)
            | EffectSpec::Spectrum(_)
    )
}

fn reachable_backward_cut(
    output_id: &str,
    valid: &ValidGraph,
    stop: &HashSet<String>,
) -> HashSet<String> {
    let starts: Vec<String> = valid
        .edges
        .iter()
        .filter(|e| e.to == output_id)
        .map(|e| e.from.clone())
        .collect();
    reachable_backward_from(&starts, valid, stop)
}

fn reachable_backward_from(
    starts: &[String],
    valid: &ValidGraph,
    stop: &HashSet<String>,
) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack: Vec<String> = starts.to_vec();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if stop.contains(&id) {
            continue;
        }
        for edge in &valid.edges {
            if edge.to == id {
                stack.push(edge.from.clone());
            }
        }
    }
    seen
}

pub fn reachable_backward(output_id: &str, valid: &ValidGraph) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack: Vec<String> = valid
        .edges
        .iter()
        .filter(|e| e.to == output_id)
        .map(|e| e.from.clone())
        .collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for edge in &valid.edges {
            if edge.to == id {
                stack.push(edge.from.clone());
            }
        }
    }
    seen
}

#[allow(dead_code)]
pub fn inputs_feeding_output<'a>(output_id: &str, valid: &'a ValidGraph) -> Vec<&'a str> {
    let reachable = reachable_backward(output_id, valid);
    valid
        .inputs
        .iter()
        .filter(|i| reachable.contains(&i.id))
        .map(|i| i.id.as_str())
        .collect()
}
