use std::cell::RefCell;
use std::rc::Rc;

use pipewire as pw;
use pw::spa::pod::deserialize::PodDeserializer;
use pw::spa::pod::{ChoiceValue, Pod, Value};
use pw::spa::utils::{Choice, ChoiceEnum};
use pw::types::ObjectType;

use crate::error::{AppError, AppResult};

pub struct PwNode {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    format_rank: u8,
}

fn parse_rate(value: &str) -> Option<u32> {
    let value = value.trim();
    if let Some((numerator, denominator)) = value.split_once('/') {
        let numerator: u32 = numerator.trim().parse().ok()?;
        let denominator: u32 = denominator.trim().parse().ok()?;
        return (numerator == 1 && denominator > 0).then_some(denominator);
    }
    value.parse().ok().filter(|rate| *rate > 0)
}

fn parse_positions(value: &str) -> Option<u32> {
    let brackets: &[char] = &['[', ']'];
    let count = value
        .trim_matches(brackets)
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|position| !position.is_empty())
        .count();
    u32::try_from(count).ok().filter(|channels| *channels > 0)
}

fn choice_default(choice: &Choice<i32>) -> i32 {
    match &choice.1 {
        ChoiceEnum::None(value) => *value,
        ChoiceEnum::Range { default, .. }
        | ChoiceEnum::Step { default, .. }
        | ChoiceEnum::Enum { default, .. }
        | ChoiceEnum::Flags { default, .. } => *default,
    }
}

fn pod_int(value: &Value) -> Option<u32> {
    let value = match value {
        Value::Int(value) => *value,
        Value::Choice(ChoiceValue::Int(choice)) => choice_default(choice),
        _ => return None,
    };
    u32::try_from(value).ok().filter(|value| *value > 0)
}

fn parse_audio_format(param: &Pod) -> (Option<u32>, Option<u32>) {
    let Ok((_, Value::Object(object))) = PodDeserializer::deserialize_any_from(param.as_bytes())
    else {
        return (None, None);
    };
    let mut rate = None;
    let mut channels = None;
    for property in &object.properties {
        if property.key == pw::spa::param::format::FormatProperties::AudioRate.as_raw() {
            rate = pod_int(&property.value);
        } else if property.key == pw::spa::param::format::FormatProperties::AudioChannels.as_raw() {
            channels = pod_int(&property.value);
        }
    }
    (rate, channels)
}

pub fn nodes_by_class(media_class: &'static str) -> AppResult<Vec<PwNode>> {
    std::thread::spawn(move || snapshot(media_class))
        .join()
        .map_err(|_| AppError::Host("pipewire enum thread panicked".into()))?
}

fn snapshot(media_class: &str) -> AppResult<Vec<PwNode>> {
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pw_err)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pw_err)?;
    let core = context.connect_rc(None).map_err(pw_err)?;
    let registry = core.get_registry_rc().map_err(pw_err)?;
    let registry_weak = registry.downgrade();

    let nodes: Rc<RefCell<Vec<PwNode>>> = Rc::new(RefCell::new(Vec::new()));
    let nodes_cb = nodes.clone();
    // Listener must be dropped before its proxy.
    let proxies: Rc<RefCell<Vec<(pw::node::NodeListener, pw::node::Node)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let proxies_cb = proxies.clone();
    let want = media_class.to_string();

    let _reg = registry
        .add_listener_local()
        .global(move |global| {
            if global.type_ != ObjectType::Node {
                return;
            }
            let Some(props) = &global.props else { return };
            if props.get("media.class") != Some(want.as_str()) {
                return;
            }
            let Some(name) = props.get("node.name") else {
                return;
            };
            let description = props
                .get("node.description")
                .filter(|d| !d.is_empty())
                .unwrap_or(name)
                .to_string();
            let sample_rate = props
                .get("audio.rate")
                .and_then(parse_rate)
                .or_else(|| props.get("node.rate").and_then(parse_rate));
            let channels = props
                .get("audio.channels")
                .and_then(|value| value.parse().ok())
                .filter(|channels| *channels > 0)
                .or_else(|| props.get("audio.position").and_then(parse_positions));
            nodes_cb.borrow_mut().push(PwNode {
                id: global.id,
                name: name.to_string(),
                description,
                sample_rate,
                channels,
                format_rank: 0,
            });

            let Some(registry) = registry_weak.upgrade() else {
                return;
            };
            let node: pw::node::Node = match registry.bind(global) {
                Ok(node) => node,
                Err(_) => return,
            };
            let node_id = global.id;
            let formats = nodes_cb.clone();
            let listener = node
                .add_listener_local()
                .param(move |_, id, _, _, param| {
                    let rank = match id {
                        spa_id if spa_id == pw::spa::param::ParamType::Format => 2,
                        spa_id if spa_id == pw::spa::param::ParamType::EnumFormat => 1,
                        _ => return,
                    };
                    let Some(param) = param else { return };
                    let (rate, channels) = parse_audio_format(param);
                    if rate.is_none() && channels.is_none() {
                        return;
                    }
                    let mut formats = formats.borrow_mut();
                    let Some(entry) = formats.iter_mut().find(|entry| entry.id == node_id) else {
                        return;
                    };
                    if rank <= entry.format_rank {
                        return;
                    }
                    if let Some(rate) = rate {
                        entry.sample_rate = Some(rate);
                    }
                    if let Some(channels) = channels {
                        entry.channels = Some(channels);
                    }
                    entry.format_rank = rank;
                })
                .register();
            node.enum_params(1, Some(pw::spa::param::ParamType::Format), 0, u32::MAX);
            node.enum_params(2, Some(pw::spa::param::ParamType::EnumFormat), 0, u32::MAX);
            proxies_cb.borrow_mut().push((listener, node));
        })
        .register();

    // The first round-trip delivers registry globals. enum_params() is issued
    // from those callbacks, after this first sync request is already in flight,
    // so a second round-trip is required to wait for the parameter replies.
    roundtrip(&core, &mainloop)?;
    roundtrip(&core, &mainloop)?;
    let out = std::mem::take(&mut *nodes.borrow_mut());
    drop(proxies);
    Ok(out)
}

fn roundtrip(core: &pw::core::CoreRc, mainloop: &pw::main_loop::MainLoopRc) -> AppResult<()> {
    let pending = core.sync(0).map_err(pw_err)?;
    let ml = mainloop.clone();
    let listener = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == 0 && seq == pending {
                ml.quit();
            }
        })
        .register();
    mainloop.run();
    drop(listener);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_positions, parse_rate};

    #[test]
    fn parses_node_rates() {
        assert_eq!(parse_rate("44100"), Some(44_100));
        assert_eq!(parse_rate("1/96000"), Some(96_000));
    }

    #[test]
    fn counts_channel_positions() {
        assert_eq!(parse_positions("[ FL FR ]"), Some(2));
        assert_eq!(parse_positions("[ AUX0, AUX1, AUX2 ]"), Some(3));
    }
}

fn pw_err(e: impl std::fmt::Display) -> AppError {
    AppError::Host(format!("pipewire: {e}"))
}
