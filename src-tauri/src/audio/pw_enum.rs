use std::cell::RefCell;
use std::rc::Rc;

use pipewire as pw;
use pw::types::ObjectType;

use crate::error::{AppError, AppResult};

pub struct PwNode {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub sample_rate: u32,
    pub channels: u16,
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

    let nodes: Rc<RefCell<Vec<PwNode>>> = Rc::new(RefCell::new(Vec::new()));
    let nodes_cb = nodes.clone();
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
                .and_then(parse_sample_rate)
                .unwrap_or(48_000);
            let channels = props
                .get("audio.channels")
                .and_then(|value| value.parse().ok())
                .or_else(|| props.get("audio.position").and_then(channel_count))
                .unwrap_or(2);
            nodes_cb.borrow_mut().push(PwNode {
                id: global.id,
                name: name.to_string(),
                description,
                sample_rate,
                channels,
            });
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
    let out = std::mem::take(&mut *nodes.borrow_mut());
    Ok(out)
}

fn parse_sample_rate(value: &str) -> Option<u32> {
    value.split('/').next()?.trim().parse().ok()
}

fn channel_count(value: &str) -> Option<u16> {
    let count = value
        .trim_matches(|character| matches!(character, '[' | ']'))
        .split_whitespace()
        .filter(|position| !position.is_empty())
        .count();
    u16::try_from(count).ok().filter(|count| *count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pipewire_format_properties() {
        assert_eq!(parse_sample_rate("48000"), Some(48_000));
        assert_eq!(parse_sample_rate("48000/1"), Some(48_000));
        assert_eq!(channel_count("[ FL FR AUX0 AUX1 ]"), Some(4));
    }
}

fn pw_err(e: impl std::fmt::Display) -> AppError {
    AppError::Host(format!("pipewire: {e}"))
}
