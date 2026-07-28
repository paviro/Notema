#![forbid(unsafe_code)]

//! Opt-in process timing for released Notema binaries.
//!
//! Set `NOTEMA_TIMING=1` for phase timings or `NOTEMA_TIMING=2` for detailed
//! cache diagnostics. TUI output can be held with [`defer`] while the alternate
//! screen is active.

use std::{
    fmt::Write as _,
    io::Write as _,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU8, Ordering},
    },
    thread::ThreadId,
    time::{Duration, Instant},
};

static LEVEL: AtomicU8 = AtomicU8::new(0);
static ORIGIN: OnceLock<Instant> = OnceLock::new();
static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();
static STATE: Mutex<State> = Mutex::new(State::new());

struct State {
    last_main: Option<Instant>,
    defer_depth: usize,
    buffer: Vec<String>,
}

impl State {
    const fn new() -> Self {
        Self {
            last_main: None,
            defer_depth: 0,
            buffer: Vec::new(),
        }
    }

    fn is_deferred(&self) -> bool {
        self.defer_depth > 0
    }
}

/// Arm timing from `NOTEMA_TIMING`.
pub fn init() {
    let level = std::env::var("NOTEMA_TIMING")
        .ok()
        .map_or(0, |value| parse_level(&value));
    if level == 0 {
        return;
    }

    let origin = Instant::now();
    let _ = ORIGIN.set(origin);
    let _ = MAIN_THREAD.set(std::thread::current().id());
    LEVEL.store(level, Ordering::Relaxed);

    if let Some(age) = process_age() {
        emit_with(|line| {
            let _ = write!(
                line,
                "[timing] pre-main ~{:.0} ms  (exec to main, from procfs)",
                age.as_secs_f64() * 1000.0
            );
        });
    }
}

/// Whether timing is enabled.
#[inline]
fn enabled() -> bool {
    LEVEL.load(Ordering::Relaxed) != 0
}

/// Whether detailed diagnostics were requested.
#[inline]
pub fn detailed() -> bool {
    LEVEL.load(Ordering::Relaxed) >= 2
}

/// Record a phase boundary. Off the main thread this is a background event
/// instead — it cannot advance a phase delta it is not part of.
#[inline]
pub fn mark(label: &str) {
    if enabled() {
        record(label);
    }
}

/// Record a phase with a label built only when timing is enabled.
#[inline]
pub fn mark_with(label: impl FnOnce() -> String) {
    if enabled() {
        record(&label());
    }
}

/// Emit an annotation without advancing the phase delta.
#[inline]
pub fn note(text: &str) {
    if enabled() {
        emit_with(|line| {
            let _ = write!(line, "[timing] {text}");
        });
    }
}

/// Emit an annotation built only when timing is enabled.
#[inline]
pub fn note_with(text: impl FnOnce() -> String) {
    if enabled() {
        note(&text());
    }
}

/// Hold timing output until the returned guard is dropped.
///
/// Guards may be nested. The outermost guard flushes after terminal restoration
/// during both ordinary returns and unwinding.
#[must_use]
pub fn defer() -> DeferredOutput {
    if !enabled() {
        return DeferredOutput { active: false };
    }
    if let Ok(mut state) = STATE.lock() {
        state.defer_depth = state.defer_depth.saturating_add(1);
        DeferredOutput { active: true }
    } else {
        DeferredOutput { active: false }
    }
}

/// RAII guard returned by [`defer`].
pub struct DeferredOutput {
    active: bool,
}

impl Drop for DeferredOutput {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let Ok(mut state) = STATE.lock() else {
            return;
        };
        if !end_defer(&mut state) {
            return;
        }
        let mut stderr = std::io::stderr().lock();
        for line in state.buffer.drain(..) {
            let _ = writeln!(stderr, "{line}");
        }
    }
}

#[derive(Clone, Copy)]
enum RecordKind {
    Phase,
    Background,
}

fn record(label: &str) {
    let Some(origin) = ORIGIN.get() else {
        return;
    };
    // Only the main thread has a phase delta to advance; everything else is an
    // event beside the timeline.
    let main = MAIN_THREAD
        .get()
        .is_some_and(|thread| *thread == std::thread::current().id());
    let kind = if main {
        RecordKind::Phase
    } else {
        RecordKind::Background
    };

    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let now = Instant::now();
    let thread = std::thread::current();
    let line = record_line(
        &mut state,
        now,
        *origin,
        label,
        kind,
        thread.name().unwrap_or("unnamed"),
    );
    write_line(&mut state, line);
}

fn record_line(
    state: &mut State,
    now: Instant,
    origin: Instant,
    label: &str,
    kind: RecordKind,
    background_name: &str,
) -> String {
    let mut line = String::with_capacity(80);
    let _ = write!(
        line,
        "[timing] {:>8.1} ms",
        now.duration_since(origin).as_secs_f64() * 1000.0
    );
    match kind {
        RecordKind::Phase => {
            let delta = state
                .last_main
                .map_or(Duration::ZERO, |last| now.duration_since(last));
            state.last_main = Some(now);
            let _ = write!(line, " {:>+8.1} ms  {label}", delta.as_secs_f64() * 1000.0);
        }
        RecordKind::Background => {
            let _ = write!(
                line,
                "            {label}  [background:{}]",
                background_name
            );
        }
    }
    line
}

fn emit_with(build: impl FnOnce(&mut String)) {
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let mut line = String::with_capacity(96);
    build(&mut line);
    write_line(&mut state, line);
}

fn write_line(state: &mut State, line: String) {
    if let Some(line) = stage_line(state, line) {
        let _ = writeln!(std::io::stderr().lock(), "{line}");
    }
}

fn stage_line(state: &mut State, line: String) -> Option<String> {
    if state.is_deferred() {
        state.buffer.push(line);
        None
    } else {
        Some(line)
    }
}

fn end_defer(state: &mut State) -> bool {
    state.defer_depth = state.defer_depth.saturating_sub(1);
    !state.is_deferred()
}

fn parse_level(value: &str) -> u8 {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "0" | "no" | "off" | "false" => 0,
        "1" | "yes" | "on" | "true" => 1,
        // An unrecognized word means on; a number means itself, zero included.
        other => other.parse::<u8>().unwrap_or(1),
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_age() -> Option<Duration> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let uptime = std::fs::read_to_string("/proc/uptime").ok()?;
    process_age_from(&stat, &uptime, rustix::param::clock_ticks_per_second())
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn process_age() -> Option<Duration> {
    None
}

#[cfg(any(target_os = "linux", target_os = "android", test))]
fn process_age_from(stat: &str, uptime: &str, ticks_per_second: u64) -> Option<Duration> {
    if ticks_per_second == 0 {
        return None;
    }
    let after_comm = stat.rsplit_once(')')?.1;
    let start_ticks: f64 = after_comm.split_whitespace().nth(19)?.parse().ok()?;
    let uptime_seconds: f64 = uptime.split_whitespace().next()?.parse().ok()?;
    Duration::try_from_secs_f64(uptime_seconds - start_ticks / ticks_per_second as f64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_level_parsing_accepts_names_and_numbers() {
        for value in ["", "0", "no", "OFF", "false"] {
            assert_eq!(parse_level(value), 0);
        }
        for value in ["1", "yes", "ON", "true", "invalid"] {
            assert_eq!(parse_level(value), 1);
        }
        assert_eq!(parse_level("2"), 2);
        // A number means itself. Only a word nobody recognizes falls back to on,
        // so a zero written any other way still reads as off.
        assert_eq!(parse_level("00"), 0);
    }

    #[test]
    fn lazy_labels_are_not_built_when_disabled() {
        assert!(!enabled());
        mark_with(|| panic!("phase label evaluated"));
        note_with(|| panic!("note text evaluated"));
    }

    #[test]
    fn deferred_lines_preserve_order() {
        let mut state = State::new();
        state.defer_depth = 2;
        assert_eq!(stage_line(&mut state, "first".to_string()), None);
        assert_eq!(stage_line(&mut state, "second".to_string()), None);
        assert_eq!(state.buffer, ["first", "second"]);
        assert!(!end_defer(&mut state));
        assert!(end_defer(&mut state));
        assert_eq!(
            state.buffer.drain(..).collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[test]
    fn background_events_do_not_advance_main_delta() {
        let mut state = State::new();
        let origin = Instant::now();
        let first = origin + Duration::from_millis(10);
        let second = origin + Duration::from_millis(30);
        let worker = origin + Duration::from_millis(20);
        let _ = record_line(
            &mut state,
            first,
            origin,
            "first",
            RecordKind::Phase,
            "main",
        );
        let _ = record_line(
            &mut state,
            worker,
            origin,
            "worker",
            RecordKind::Background,
            "worker",
        );
        let line = record_line(
            &mut state,
            second,
            origin,
            "second",
            RecordKind::Phase,
            "main",
        );

        assert!(line.contains("+20.0 ms"));
    }

    #[test]
    fn concurrent_deferred_output_is_serialized_without_loss() {
        let state = std::sync::Arc::new(Mutex::new(State::new()));
        state.lock().unwrap().defer_depth = 1;
        let threads: Vec<_> = (0..4)
            .map(|thread| {
                let state = std::sync::Arc::clone(&state);
                std::thread::spawn(move || {
                    for item in 0..16 {
                        let mut state = state.lock().unwrap();
                        assert_eq!(stage_line(&mut state, format!("{thread}:{item}")), None);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }

        let state = state.lock().unwrap();
        assert_eq!(state.buffer.len(), 64);
        let unique: std::collections::HashSet<_> = state.buffer.iter().collect();
        assert_eq!(unique.len(), 64);
    }

    #[test]
    fn proc_stat_parser_handles_spaces_and_parentheses() {
        let mut fields = vec!["0"; 20];
        fields[0] = "S";
        fields[19] = "250";
        let stat = format!("123 (notema worker (1)) {}", fields.join(" "));
        let age = process_age_from(&stat, "10.00 2.00", 100).unwrap();
        assert_eq!(age, Duration::from_millis(7_500));
    }

    #[test]
    fn proc_stat_parser_rejects_invalid_inputs() {
        assert!(process_age_from("invalid", "10", 100).is_none());
        assert!(process_age_from("1 (x) S", "10", 100).is_none());
        assert!(process_age_from("1 (x) S", "10", 0).is_none());
    }
}
