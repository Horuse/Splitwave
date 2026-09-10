use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;

use crate::audio::graph::EffectSpec;
use crate::audio::plugins::host_api::HostedEffect;

use super::channel_balance::ChannelBalanceEffect;
use super::compressor::CompressorEffect;
use super::controls::EffectControl;
use super::de_esser::DeEsserEffect;
use super::declick::DeclickEffect;
use super::delay::DelayEffect;
use super::eq::EqEffect;
use super::gain::GainEffect;
use super::level_meter::{LevelMeterEffect, MeterHandle};
use super::limiter::LimiterEffect;
use super::lufs_meter::{LufsHandle, LufsMeterEffect};
use super::mute::MuteEffect;
use super::noise_gate::NoiseGateEffect;
use super::noise_suppressor::NoiseSuppressorEffect;
use super::registry::{EffectBuild, EffectRegistry, GrHandle};
use super::reverb::ReverbEffect;
use super::saturator::SaturatorEffect;
use super::waveform::{WaveformEffect, WaveformHandle};
use super::{RuntimeEffect, PLUGIN_MAX_BLOCK};

pub fn instantiate_effect(
    spec: &EffectSpec,
    node_id: &str,
    sample_rate: u32,
    // False for file-recording outputs: an offline render outruns real time,
    // so an expensive effect must process in place instead of on a worker.
    realtime: bool,
    // False when building the monitor graph: a plugin instantiated there is a
    // metering-only duplicate, not the one its editor window attaches to.
    primary: bool,
    // Channels this node carries. An effect that can take them all says so
    // through `EffectBuild::full_width`; the rest are driven one pair at a time.
    channels: usize,
    registry: &mut EffectRegistry,
) -> EffectBuild {
    let (bypass, bypass_is_new) = match registry.bypasses.get(node_id) {
        Some(b) => (b.clone(), false),
        None => {
            let b = Arc::new(AtomicBool::new(spec.bypassed()));
            registry.bypasses.insert(node_id.to_string(), b.clone());
            (b, true)
        }
    };
    let mk = |effect: RuntimeEffect,
              control: Option<EffectControl>,
              meter: Option<MeterHandle>,
              lufs: Option<LufsHandle>,
              gr: Option<GrHandle>,
              scope: Option<WaveformHandle>| EffectBuild {
        effect,
        control,
        meter,
        lufs,
        gr,
        scope,
        bypass: bypass.clone(),
        bypass_is_new,
        full_width: false,
    };
    match *spec {
        EffectSpec::Gain(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Gain { linear }) => mk(
                RuntimeEffect::Gain(GainEffect::from_state(linear.clone())),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = GainEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Gain(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Mute(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Mute { muted }) => mk(
                RuntimeEffect::Mute(MuteEffect::from_state(muted.clone())),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = MuteEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Mute(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::ChannelBalance(d) => match registry.controls.get(node_id) {
            Some(EffectControl::ChannelBalance { left, right }) => mk(
                RuntimeEffect::ChannelBalance(ChannelBalanceEffect::from_state(
                    left.clone(),
                    right.clone(),
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = ChannelBalanceEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(
                    RuntimeEffect::ChannelBalance(e),
                    Some(c),
                    None,
                    None,
                    None,
                    None,
                )
            }
        },
        EffectSpec::Saturator(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Saturator { ceiling, drive }) => mk(
                RuntimeEffect::Saturator(SaturatorEffect::from_state(
                    ceiling.clone(),
                    drive.clone(),
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = SaturatorEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Saturator(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Eq(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Eq { gains }) => mk(
                RuntimeEffect::Eq(EqEffect::from_state(gains.clone(), sample_rate)),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = EqEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Eq(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::LevelMeter(d) => match registry.meters.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::LevelMeter(LevelMeterEffect::from_handle(handle.clone())),
                None,
                None,
                None,
                None,
                None,
            ),
            None => {
                let (e, handle) = LevelMeterEffect::new(d, node_id.to_string());
                registry.meters.insert(node_id.to_string(), handle.clone());
                mk(
                    RuntimeEffect::LevelMeter(e),
                    None,
                    Some(handle),
                    None,
                    None,
                    None,
                )
            }
        },
        EffectSpec::LufsMeter(d) => match registry.lufs.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::LufsMeter(LufsMeterEffect::from_handle(handle.clone(), sample_rate)),
                None,
                None,
                None,
                None,
                None,
            ),
            None => {
                let (e, handle) = LufsMeterEffect::new(d, node_id.to_string(), sample_rate);
                registry.lufs.insert(node_id.to_string(), handle.clone());
                mk(
                    RuntimeEffect::LufsMeter(e),
                    None,
                    None,
                    Some(handle),
                    None,
                    None,
                )
            }
        },
        EffectSpec::Waveform(d) => match registry.scopes.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::Waveform(WaveformEffect::from_handle(handle.clone())),
                None,
                None,
                None,
                None,
                None,
            ),
            None => {
                let (e, handle) = WaveformEffect::new(d, node_id.to_string(), sample_rate);
                registry.scopes.insert(node_id.to_string(), handle.clone());
                mk(
                    RuntimeEffect::Waveform(e),
                    None,
                    None,
                    None,
                    None,
                    Some(handle),
                )
            }
        },
        // Spectrum reuses the scope's time-domain capture and SCOPE_EVENT
        // transport; the UI runs the FFT. No distinct runtime effect is needed.
        EffectSpec::Spectrum(_) => match registry.scopes.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::Waveform(WaveformEffect::from_handle(handle.clone())),
                None,
                None,
                None,
                None,
                None,
            ),
            None => {
                let (e, handle) = WaveformEffect::new_for(node_id.to_string(), sample_rate);
                registry.scopes.insert(node_id.to_string(), handle.clone());
                mk(
                    RuntimeEffect::Waveform(e),
                    None,
                    None,
                    None,
                    None,
                    Some(handle),
                )
            }
        },
        EffectSpec::Limiter(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Limiter {
                ceiling,
                release_ms,
            }) => {
                let lookahead_frames =
                    ((d.lookahead_ms.max(0.1) * sample_rate as f32 / 1000.0) as usize).max(1);
                let gr_arc = registry
                    .gr_atomics
                    .get(node_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc.clone(),
                };
                mk(
                    RuntimeEffect::Limiter(LimiterEffect::from_state(
                        ceiling.clone(),
                        release_ms.clone(),
                        lookahead_frames,
                        sample_rate,
                        gr_arc,
                    )),
                    None,
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
            _ => {
                let (e, c, gr_arc) = LimiterEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc,
                };
                mk(
                    RuntimeEffect::Limiter(e),
                    Some(c),
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
        },
        EffectSpec::Compressor(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Compressor {
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                knee_db,
                makeup_db,
            }) => {
                let gr_arc = registry
                    .gr_atomics
                    .get(node_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc.clone(),
                };
                mk(
                    RuntimeEffect::Compressor(CompressorEffect::from_state(
                        threshold_db.clone(),
                        ratio.clone(),
                        attack_ms.clone(),
                        release_ms.clone(),
                        knee_db.clone(),
                        makeup_db.clone(),
                        sample_rate,
                        gr_arc,
                    )),
                    None,
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
            _ => {
                let (e, c, gr_arc) = CompressorEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc,
                };
                mk(
                    RuntimeEffect::Compressor(e),
                    Some(c),
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
        },
        EffectSpec::NoiseGate(d) => match registry.controls.get(node_id) {
            Some(EffectControl::NoiseGate {
                threshold_db,
                range_db,
                attack_ms,
                hold_ms,
                release_ms,
            }) => {
                let gr_arc = registry
                    .gr_atomics
                    .get(node_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc.clone(),
                };
                mk(
                    RuntimeEffect::NoiseGate(NoiseGateEffect::from_state(
                        threshold_db.clone(),
                        range_db.clone(),
                        attack_ms.clone(),
                        hold_ms.clone(),
                        release_ms.clone(),
                        sample_rate,
                        gr_arc,
                    )),
                    None,
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
            _ => {
                let (e, c, gr_arc) = NoiseGateEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc,
                };
                mk(
                    RuntimeEffect::NoiseGate(e),
                    Some(c),
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
        },
        EffectSpec::Delay(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Delay {
                time_ms,
                feedback,
                mix,
            }) => mk(
                RuntimeEffect::Delay(DelayEffect::from_state(
                    time_ms.clone(),
                    feedback.clone(),
                    mix.clone(),
                    sample_rate,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = DelayEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Delay(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Reverb(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Reverb {
                room_size,
                damping,
                width,
                mix,
            }) => mk(
                RuntimeEffect::Reverb(ReverbEffect::from_state(
                    room_size.clone(),
                    damping.clone(),
                    width.clone(),
                    mix.clone(),
                    sample_rate,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = ReverbEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Reverb(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::NoiseSuppressor(d) => match registry.controls.get(node_id) {
            Some(EffectControl::NoiseSuppressor { controls }) => mk(
                RuntimeEffect::NoiseSuppressor(NoiseSuppressorEffect::from_state(
                    controls.clone(),
                    sample_rate,
                    realtime,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = NoiseSuppressorEffect::new(d, sample_rate, realtime);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(
                    RuntimeEffect::NoiseSuppressor(e),
                    Some(c),
                    None,
                    None,
                    None,
                    None,
                )
            }
        },
        EffectSpec::Declick(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Declick {
                sensitivity,
                max_width_ms,
            }) => mk(
                RuntimeEffect::Declick(DeclickEffect::from_state(
                    sensitivity.clone(),
                    max_width_ms.clone(),
                    sample_rate,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = DeclickEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Declick(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::DeEsser(d) => match registry.controls.get(node_id) {
            Some(EffectControl::DeEsser {
                frequency,
                threshold_db,
                ratio,
            }) => mk(
                RuntimeEffect::DeEsser(DeEsserEffect::from_state(
                    frequency.clone(),
                    threshold_db.clone(),
                    ratio.clone(),
                    sample_rate,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = DeEsserEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::DeEsser(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Plugin {
            format,
            ref path,
            ref plugin_id,
            ref state,
            ..
        } => {
            if path.is_empty() {
                crate::audio::plugins::registry::forget(node_id);
                let muted = Arc::new(AtomicBool::new(false));
                return mk(
                    RuntimeEffect::Mute(MuteEffect::from_state(muted)),
                    None,
                    None,
                    None,
                    None,
                    None,
                );
            }
            let Some(format) = format else {
                tracing::error!(node_id, path, "plugin node has no format");
                let muted = Arc::new(AtomicBool::new(true));
                return mk(
                    RuntimeEffect::Mute(MuteEffect::from_state(muted)),
                    None,
                    None,
                    None,
                    None,
                    None,
                );
            };
            let primary = primary && !registry.plugin_primary_claimed.contains(node_id);
            let ring = registry
                .plugin_param_rings
                .entry(node_id.to_string())
                .or_insert_with(|| Arc::new(crate::audio::plugins::ParamRing::new()))
                .clone();
            let request = crate::audio::plugins::host_api::ActivateRequest {
                node_id,
                path,
                plugin_id,
                sample_rate,
                max_frames: PLUGIN_MAX_BLOCK,
                channels,
                state: state.as_deref(),
                primary,
                params: ring.clone(),
            };
            match crate::audio::plugins::registry::activate(format, request) {
                Ok(node) => {
                    if primary {
                        registry.plugin_primary_claimed.insert(node_id.to_string());
                    }
                    let control = primary.then_some(EffectControl::Plugin { events: ring });
                    let full_width = node.channels() == channels;
                    let mut build = mk(
                        RuntimeEffect::HostedPlugin(HostedEffect::new(node, realtime)),
                        control,
                        None,
                        None,
                        None,
                        None,
                    );
                    build.full_width = full_width;
                    build
                }
                Err(e) => {
                    tracing::error!(node_id, path, plugin_id, error = %e, "plugin failed to load");
                    crate::audio::plugins::registry::forget(node_id);
                    let muted = Arc::new(AtomicBool::new(true));
                    mk(
                        RuntimeEffect::Mute(MuteEffect::from_state(muted)),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                }
            }
        }
    }
}
