use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::{Captures, Regex};
use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::layer::{Context, Layer};

const CAPACITY: usize = 2000;

const SESSION_LOG: &str = "session.log";
const PREVIOUS_SESSION_LOG: &str = "previous-session.log";
/// Past this the file is cut down to its newest `FILE_KEEP` bytes.
const FILE_CAP: u64 = 4 * 1024 * 1024;
const FILE_KEEP: u64 = 1024 * 1024;
/// Enough to cover what led up to a crash without bloating the report.
const TAIL_BYTES: u64 = 64 * 1024;
const TAIL_LINES: usize = 300;

static FILE_WRITER: OnceLock<NonBlocking> = OnceLock::new();
static FILE_GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub at: u64,
    pub level: String,
    pub target: String,
    pub message: String,
}

static BUFFER: Mutex<VecDeque<LogLine>> = Mutex::new(VecDeque::new());

pub fn snapshot() -> Vec<LogLine> {
    BUFFER
        .lock()
        .map(|b| b.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn clear() {
    if let Ok(mut b) = BUFFER.lock() {
        b.clear();
    }
}

/// Mirrors every `tracing` event into a ring buffer so the in-app log viewer
/// works in release builds, where stdout goes nowhere.
pub struct RingLayer;

impl<S: Subscriber> Layer<S> for RingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor {
            message: String::new(),
        };
        event.record(&mut visitor);

        let meta = event.metadata();
        let line = LogLine {
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            level: meta.level().to_string(),
            target: meta.target().to_string(),
            message: visitor.message,
        };

        if let Some(writer) = FILE_WRITER.get() {
            let _ = writer.clone().write_all(format_line(&line).as_bytes());
        }
        if let Ok(mut buffer) = BUFFER.lock() {
            if buffer.len() == CAPACITY {
                buffer.pop_front();
            }
            buffer.push_back(line);
        }
    }
}

fn format_line(line: &LogLine) -> String {
    let day_ms = line.at % 86_400_000;
    format!(
        "{:02}:{:02}:{:02}.{:03}Z {:>5} {}: {}\n",
        day_ms / 3_600_000,
        day_ms / 60_000 % 60,
        day_ms / 1000 % 60,
        day_ms % 1000,
        line.level,
        line.target,
        line.message
    )
}

/// Moves the last session's log aside and starts mirroring this one to disk,
/// so whatever led up to a crash or a kill is still there on the next launch.
pub fn start_file_sink(dir: &Path) {
    let path = dir.join(SESSION_LOG);
    let _ = std::fs::rename(&path, dir.join(PREVIOUS_SESSION_LOG));
    let Ok(file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
    else {
        return;
    };
    let (mut writer, guard) = NonBlockingBuilder::default()
        .lossy(false)
        .thread_name("log-file")
        .finish(SessionFile { file, path });
    // Lines logged before the log dir was known only made it to the ring.
    for line in snapshot() {
        let _ = writer.write_all(format_line(&line).as_bytes());
    }
    if FILE_WRITER.set(writer).is_ok() {
        if let Ok(mut slot) = FILE_GUARD.lock() {
            *slot = Some(guard);
        }
    }
}

/// Flushes what is still queued for the file; call on a clean exit.
pub fn flush_file_sink() {
    if let Ok(mut slot) = FILE_GUARD.lock() {
        slot.take();
    }
}

struct SessionFile {
    file: File,
    path: std::path::PathBuf,
}

impl Write for SessionFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()?;
        if self.file.metadata().map_or(0, |m| m.len()) > FILE_CAP {
            if let Some(trimmed) = keep_tail(&self.path) {
                self.file = trimmed;
            }
        }
        Ok(())
    }
}

fn keep_tail(path: &Path) -> Option<File> {
    let tail = read_tail(path, FILE_KEEP)?;
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .ok()?;
    file.write_all(tail.as_bytes()).ok()?;
    Some(file)
}

/// The newest lines of the previous session's log, if it left one.
/// Redacted: the report may be pasted into a public issue.
pub fn previous_session_tail(dir: &Path, home: Option<&Path>) -> Option<String> {
    let tail = read_tail(&dir.join(PREVIOUS_SESSION_LOG), TAIL_BYTES)?;
    let lines: Vec<&str> = tail.lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    let text = lines[start..].join("\n");
    (!text.is_empty()).then(|| redact(&text, home))
}

/// Hides the home directory (it carries the user name) and IP addresses.
fn redact(text: &str, home: Option<&Path>) -> String {
    let text = match home.and_then(Path::to_str) {
        Some(home) if home.len() > 1 => text.replace(home, "~"),
        _ => text.to_string(),
    };
    redact_ips(&text)
}

fn redact_ips(text: &str) -> String {
    // Candidates only; std's parsers decide what is an address, so clock
    // times and `crate::path` targets stay untouched.
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w:.]+").unwrap());
    TOKEN
        .replace_all(text, |caps: &Captures| {
            let token = &caps[0];
            let addr = token.trim_end_matches('.');
            let rest = &token[addr.len()..];
            if addr.parse::<IpAddr>().is_ok() {
                format!("<ip>{rest}")
            } else if let Ok(socket) = addr.parse::<SocketAddr>() {
                format!("<ip>:{}{rest}", socket.port())
            } else {
                token.to_string()
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_home_and_addresses() {
        let home = Path::new("/Users/jane");
        let log = "16:02:48.787Z  INFO splitwave_lib: crash log path=/Users/jane/Library/Logs/x\n\
                   peer 192.168.1.20:5004 joined, candidate fe80::1c2b:3aff:fe4d:2 and 2001:db8::7\n\
                   WARN splitwave_lib::audio::engine: v1.4.1 at 12:34:56";
        let out = redact(log, Some(home));
        assert!(out.contains("path=~/Library/Logs/x"));
        assert!(out.contains("peer <ip>:5004 joined"));
        assert!(out.contains("candidate <ip> and <ip>"));
        assert!(out.contains("splitwave_lib::audio::engine: v1.4.1 at 12:34:56"));
        assert!(out.starts_with("16:02:48.787Z"));
    }

    #[test]
    fn leaves_non_addresses_alone() {
        let line = "blocked_ms=20081 device \"MacBook Pro Speakers\" 48000 Hz 10.0.26100";
        assert_eq!(redact_ips(line), line);
    }
}

/// Last `max` bytes, starting at a line boundary.
fn read_tail(path: &Path, max: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let from = len.saturating_sub(max);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if from > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Some(text.to_string())
}

struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.message, " {}={value:?}", field.name());
        }
    }
}
