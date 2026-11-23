use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::{Captures, Regex};
use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

const CAPACITY: usize = 2000;

const LATEST_LOG: &str = "latest.log";
const PREVIOUS_LOG: &str = "previous.log";
/// Past this the file is cut down to its newest `FILE_KEEP` bytes.
const FILE_CAP: u64 = 4 * 1024 * 1024;
const FILE_KEEP: u64 = 1024 * 1024;
/// Enough to cover what led up to a crash without bloating the report.
const TAIL_BYTES: u64 = 64 * 1024;
const TAIL_LINES: usize = 300;
/// User names shorter than this match too many ordinary words to redact.
const MIN_USER_NAME: usize = 3;
/// A spam that runs into a crash still leaves its count on disk.
const REPEAT_FLUSH: u32 = 100;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub at: u64,
    pub level: String,
    pub target: String,
    pub message: String,
    pub thread: String,
}

/// Ring and file share one lock, so a line lands in both or in neither and
/// the file starts with exactly what the ring held when it was opened.
struct Sinks {
    ring: VecDeque<LogLine>,
    file: Option<SessionFile>,
    /// How many copies of the last line were dropped since it was written.
    repeats: u32,
}

static SINKS: Mutex<Sinks> = Mutex::new(Sinks {
    ring: VecDeque::new(),
    file: None,
    repeats: 0,
});

/// A panic while the lock is held must not silence logging for the rest of
/// the session, so poisoning is ignored.
fn sinks() -> MutexGuard<'static, Sinks> {
    SINKS.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn snapshot() -> Vec<LogLine> {
    sinks().ring.iter().cloned().collect()
}

pub fn clear() {
    sinks().clear();
}

/// Mirrors every `tracing` event into a ring buffer so the in-app log viewer
/// works in release builds, where stdout goes nowhere, and into the session
/// file once it is open.
pub struct RingLayer;

impl<S: Subscriber> Layer<S> for RingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor {
            message: String::new(),
        };
        event.record(&mut visitor);

        let meta = event.metadata();
        let line = LogLine {
            at: now_ms(),
            level: meta.level().to_string(),
            target: meta.target().to_string(),
            message: visitor.message,
            thread: thread_label(),
        };

        sinks().record(line);
    }
}

impl Sinks {
    fn clear(&mut self) {
        self.ring.clear();
        self.repeats = 0;
    }

    /// A line equal to the previous one only bumps a counter, so a message
    /// spammed in a loop does not push the history out of the ring and the
    /// file. The count is written once a different line arrives, or every
    /// `REPEAT_FLUSH` copies.
    fn record(&mut self, line: LogLine) {
        if self.ring.back().is_some_and(|last| same_line(last, &line)) {
            self.repeats += 1;
            if self.repeats >= REPEAT_FLUSH {
                self.flush_repeats();
            }
            return;
        }
        self.flush_repeats();
        self.push(line);
    }

    fn flush_repeats(&mut self) {
        let Some(last) = self.ring.back().filter(|_| self.repeats > 0) else {
            return;
        };
        let summary = LogLine {
            message: format!("last message repeated {} more times", self.repeats),
            ..last.clone()
        };
        self.repeats = 0;
        self.push(summary);
    }

    fn push(&mut self, line: LogLine) {
        if let Some(file) = self.file.as_mut() {
            file.append(&line);
        }
        if self.ring.len() == CAPACITY {
            self.ring.pop_front();
        }
        self.ring.push_back(line);
    }
}

fn same_line(a: &LogLine, b: &LogLine) -> bool {
    a.message == b.message && a.target == b.target && a.level == b.level && a.thread == b.thread
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn thread_label() -> String {
    let current = std::thread::current();
    match current.name() {
        Some(name) => name.to_string(),
        None => format!("{:?}", current.id()),
    }
}

fn format_line(line: &LogLine) -> String {
    format!(
        "{} {:>5} [{}] {}: {}\n",
        format_timestamp(line.at),
        line.level,
        line.thread,
        line.target,
        line.message
    )
}

/// ISO 8601 in UTC, e.g. `2026-10-28T16:02:48.787Z`.
fn format_timestamp(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    let day_ms = ms % 86_400_000;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        day_ms / 3_600_000,
        day_ms / 60_000 % 60,
        day_ms / 1000 % 60,
        day_ms % 1000,
    )
}

/// Writes a panic straight into the log, so the session file says why it
/// ends. `try_lock`: the panic may come from inside a log call on this very
/// thread, and waiting on that lock would deadlock the hook.
pub fn record_panic(message: &str) {
    let line = LogLine {
        at: now_ms(),
        level: "ERROR".into(),
        target: "panic".into(),
        message: message.to_string(),
        thread: thread_label(),
    };
    let sinks = match SINKS.try_lock() {
        Ok(sinks) => Some(sinks),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    };
    if let Some(mut sinks) = sinks {
        sinks.record(line);
    }
}

/// Moves the last session's log aside and starts mirroring this one to disk,
/// so whatever led up to a crash or a kill is still there on the next launch.
pub fn start_file_sink(dir: &Path, header: &str) {
    let path = dir.join(LATEST_LOG);
    let _ = std::fs::rename(&path, dir.join(PREVIOUS_LOG));
    let Ok(file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
    else {
        return;
    };
    let mut session = SessionFile { file, path, len: 0 };
    let started = format!("{} session start: {header}\n", format_timestamp(now_ms()));
    if session.file.write_all(started.as_bytes()).is_ok() {
        session.len += started.len() as u64;
    }
    let mut sinks = sinks();
    // Lines logged before the log dir was known only made it to the ring.
    for line in &sinks.ring {
        session.append(line);
    }
    sinks.file = Some(session);
}

/// Unbuffered on purpose: each line is in the kernel once `write` returns, so
/// it survives a panic abort, a native crash or a kill of the process.
struct SessionFile {
    file: File,
    path: PathBuf,
    len: u64,
}

impl SessionFile {
    fn append(&mut self, line: &LogLine) {
        let text = format_line(line);
        if self.file.write_all(text.as_bytes()).is_ok() {
            self.len += text.len() as u64;
        }
        if self.len > FILE_CAP {
            self.keep_tail();
        }
    }

    fn keep_tail(&mut self) {
        let Some(tail) = read_tail(&self.path, FILE_KEEP) else {
            return;
        };
        // Through the handle already open: on Windows a second writable open
        // of the same file is not guaranteed to be allowed.
        let rewritten = self.file.set_len(0).is_ok()
            && self.file.seek(SeekFrom::Start(0)).is_ok()
            && self.file.write_all(tail.as_bytes()).is_ok();
        self.len = if rewritten { tail.len() as u64 } else { 0 };
    }
}

/// The newest lines of the previous session's log, if it left one.
/// Redacted: the report may be pasted into a public issue.
pub fn previous_session_tail(dir: &Path, home: Option<&Path>) -> Option<String> {
    let tail = read_tail(&dir.join(PREVIOUS_LOG), TAIL_BYTES)?;
    let lines: Vec<&str> = tail.lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    let text = lines[start..].join("\n");
    (!text.is_empty()).then(|| redact(&text, home))
}

/// Hides the home directory, the user name it carries (device names such as
/// "Jane's AirPods" repeat it) and IP addresses.
pub fn redact(text: &str, home: Option<&Path>) -> String {
    let mut text = text.to_string();
    if let Some(home) = home.and_then(Path::to_str).filter(|h| h.len() > 1) {
        // `?path` fields log the Debug form, with backslashes doubled on Windows.
        let escaped = home.replace('\\', "\\\\");
        for form in [escaped.as_str(), home] {
            // `\b` keeps `/Users/jane` from matching inside `/Users/janet`.
            if let Ok(re) = Regex::new(&format!(r"{}\b", regex::escape(form))) {
                text = re.replace_all(&text, "~").into_owned();
            }
        }
        let user = Path::new(home).file_name().and_then(|n| n.to_str());
        if let Some(user) = user.filter(|u| u.chars().count() >= MIN_USER_NAME) {
            if let Ok(re) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(user))) {
                text = re.replace_all(&text, "<user>").into_owned();
            }
        }
    }
    redact_ips(&text)
}

fn redact_ips(text: &str) -> String {
    static V4: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap());
    // Candidates only; std's parser decides what is an address, so clock
    // times and `crate::path` targets stay untouched.
    static V6: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w:.]+").unwrap());
    let text = V4.replace_all(text, |caps: &Captures| match caps[0].parse::<Ipv4Addr>() {
        Ok(_) => "<ip>".to_string(),
        Err(_) => caps[0].to_string(),
    });
    V6.replace_all(&text, |caps: &Captures| {
        let token = &caps[0];
        // `::1` opens with colons of its own; a stray `:` around an address is
        // punctuation.
        for addr in [token.trim_end_matches('.'), token.trim_matches([':', '.'])] {
            // A bare `::` parses as the unspecified address but is punctuation.
            let has_digits = addr.chars().any(|c| c.is_ascii_hexdigit());
            if has_digits && addr.parse::<Ipv6Addr>().is_ok() {
                return token.replacen(addr, "<ip>", 1);
            }
        }
        token.to_string()
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
    fn redacts_addresses_next_to_colons() {
        assert_eq!(redact_ips("ip:1.2.3.4"), "ip:<ip>");
        assert_eq!(redact_ips("to 1.2.3.4: refused"), "to <ip>: refused");
        assert_eq!(redact_ips("host:[::1]:80"), "host:[<ip>]:80");
        assert_eq!(redact_ips("addr fe80::1:"), "addr <ip>:");
    }

    #[test]
    fn redacts_debug_escaped_home() {
        let home = Path::new(r"C:\Users\jane");
        let out = redact(
            r#"path="C:\\Users\\jane\\AppData" dir=C:\Users\jane\x"#,
            Some(home),
        );
        assert_eq!(out, r#"path="~\\AppData" dir=~\x"#);
    }

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            format_timestamp(951_782_400_000),
            "2000-02-29T00:00:00.000Z"
        );
        assert_eq!(
            format_timestamp(1_793_203_368_787),
            "2026-10-28T16:02:48.787Z"
        );
    }

    #[test]
    fn collapses_repeated_lines() {
        let line = |message: &str| LogLine {
            at: 0,
            level: "WARN".into(),
            target: "t".into(),
            message: message.into(),
            thread: "main".into(),
        };
        let mut sinks = Sinks {
            ring: VecDeque::new(),
            file: None,
            repeats: 0,
        };
        for message in ["a", "a", "a", "b"] {
            sinks.record(line(message));
        }
        let messages: Vec<&str> = sinks.ring.iter().map(|l| l.message.as_str()).collect();
        assert_eq!(messages, ["a", "last message repeated 2 more times", "b"]);

        sinks.record(line("b"));
        sinks.clear();
        sinks.record(line("c"));
        let messages: Vec<&str> = sinks.ring.iter().map(|l| l.message.as_str()).collect();
        assert_eq!(messages, ["c"]);
    }

    #[test]
    fn flushes_long_repeat_runs() {
        let line = LogLine {
            at: 0,
            level: "WARN".into(),
            target: "t".into(),
            message: "a".into(),
            thread: "main".into(),
        };
        let mut sinks = Sinks {
            ring: VecDeque::new(),
            file: None,
            repeats: 0,
        };
        for _ in 0..=REPEAT_FLUSH {
            sinks.record(line.clone());
        }
        let messages: Vec<&str> = sinks.ring.iter().map(|l| l.message.as_str()).collect();
        assert_eq!(messages, ["a", "last message repeated 100 more times"]);
    }

    #[test]
    fn leaves_non_addresses_alone() {
        let line = "blocked_ms=20081 device \"MacBook Pro Speakers\" 48000 Hz 10.0.26100 a :: b";
        assert_eq!(redact_ips(line), line);
    }

    #[test]
    fn redacts_user_name_and_respects_path_boundaries() {
        let home = Path::new("/Users/jane");
        let out = redact(
            "device \"Jane's AirPods\" dir=/Users/janet/x home=/Users/jane",
            Some(home),
        );
        assert_eq!(out, "device \"<user>'s AirPods\" dir=/Users/janet/x home=~");
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
