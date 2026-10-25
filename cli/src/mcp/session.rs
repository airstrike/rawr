//! The programs `start` runs (D537.3): one `rawr run` per project root, in
//! a process group of its own, with a stdin pipe, its output kept with a
//! cursor and its owner's events ([`crate::events`]) numbered as they
//! come. The owner keeps watching and reloading, as at a terminal; the
//! session only records what it says.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::process;

/// How often the events file is read.
const TAIL: Duration = Duration::from_millis(40);
/// Output kept per session, oldest first out.
const OUTPUT: usize = 256 * 1024;

/// Where a session's program is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Spawned; nothing reported yet.
    Starting,
    /// A cold start builds: its stage and percent.
    Building,
    /// The program runs.
    Running,
    /// A save restarts it.
    Restarting,
    /// It failed to start; a save that fixes it starts it.
    Failed,
    /// It ended with this status; a save runs it again.
    Ended(i32),
    /// `rawr run` itself exited with this status.
    Exited(i32),
    /// `stop` ended it.
    Stopped,
}

impl Phase {
    /// The phase's name, as tools report it.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Building => "building",
            Self::Running => "running",
            Self::Restarting => "restarting",
            Self::Failed => "failed",
            Self::Ended(_) => "ended",
            Self::Exited(_) => "exited",
            Self::Stopped => "stopped",
        }
    }

    /// Still on its way to running or failing.
    #[must_use]
    pub const fn is_pending(&self) -> bool {
        matches!(self, Self::Starting | Self::Building | Self::Restarting)
    }

    /// Over: nothing more will happen.
    #[must_use]
    pub const fn is_over(&self) -> bool {
        matches!(self, Self::Exited(_) | Self::Stopped)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
struct State {
    phase: Phase,
    events: Vec<(u64, Value)>,
    /// The latest building stage and percent.
    building: Option<(String, u64)>,
    /// The latest failure while a program kept running, cleared by the
    /// next good reload.
    failure: Option<Value>,
    output: VecDeque<(u64, Stream, String)>,
    output_bytes: usize,
    next_output: u64,
    /// What the last answer returned: a call without a cursor goes on
    /// from here.
    given_event: u64,
    given_output: u64,
}

/// One started program.
#[derive(Debug)]
pub struct Session {
    /// The project root, the session's key.
    pub root: PathBuf,
    /// The path given to `rawr run`.
    pub path: PathBuf,
    /// The program's arguments.
    pub arguments: Vec<String>,
    /// The pid of `rawr run`, which leads the session's group.
    pub pid: u32,
    events: PathBuf,
    state: Mutex<State>,
    changed: Condvar,
}

/// The sessions, one per project root.
#[derive(Debug, Default)]
pub struct Sessions(Mutex<HashMap<PathBuf, Arc<Session>>>);

impl Sessions {
    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Arc<Session>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The session for `root`.
    #[must_use]
    pub fn get(&self, root: &Path) -> Option<Arc<Session>> {
        self.lock().get(root).cloned()
    }

    /// Every session.
    #[must_use]
    pub fn all(&self) -> Vec<Arc<Session>> {
        let mut all: Vec<_> = self.lock().values().cloned().collect();
        all.sort_by(|a, b| a.root.cmp(&b.root));
        all
    }

    /// Start `rawr run <path> -- arguments` for `root`, replacing the
    /// session there. Events go to a file under `directory`.
    pub fn start(
        &self,
        root: &Path,
        path: &Path,
        arguments: &[String],
        stdin: Option<&str>,
        directory: &Path,
    ) -> std::io::Result<Arc<Session>> {
        if let Some(old) = self.lock().remove(root) {
            old.stop();
        }
        std::fs::create_dir_all(directory)?;
        let events = directory.join(format!(
            "session-{}-{}.events",
            std::process::id(),
            next_number()
        ));
        File::create(&events)?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("run")
            .arg(path)
            .arg("--")
            .args(arguments)
            .current_dir(if root.is_dir() {
                root
            } else {
                root.parent().unwrap_or(root)
            })
            .env(crate::events::EVENTS, &events)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = process::spawn(&mut command)?;
        let session = Arc::new(Session {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
            arguments: arguments.to_vec(),
            pid: child.id(),
            events,
            state: Mutex::new(State {
                phase: Phase::Starting,
                events: Vec::new(),
                building: None,
                failure: None,
                output: VecDeque::new(),
                output_bytes: 0,
                next_output: 0,
                given_event: 0,
                given_output: 0,
            }),
            changed: Condvar::new(),
        });
        // The program reads what the agent gave, then end of input.
        if let Some(mut pipe) = child.stdin.take() {
            let text = stdin.unwrap_or_default().to_owned();
            let _ = std::thread::Builder::new()
                .name("rawr-mcp-stdin".into())
                .spawn(move || {
                    let _ = pipe.write_all(text.as_bytes());
                });
        }
        for (pipe, stream) in [
            (
                child
                    .stdout
                    .take()
                    .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
                Stream::Stdout,
            ),
            (
                child
                    .stderr
                    .take()
                    .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
                Stream::Stderr,
            ),
        ] {
            if let Some(pipe) = pipe {
                let reading = Arc::clone(&session);
                let _ = std::thread::Builder::new()
                    .name("rawr-mcp-session-output".into())
                    .spawn(move || reading.read_output(pipe, stream));
            }
        }
        let waiting = Arc::clone(&session);
        let _ = std::thread::Builder::new()
            .name("rawr-mcp-session".into())
            .spawn(move || waiting.follow(child));
        self.lock().insert(root.to_path_buf(), Arc::clone(&session));
        Ok(session)
    }

    /// End the session for `root`, if any.
    pub fn stop(&self, root: &Path) -> Option<Arc<Session>> {
        let session = self.lock().remove(root)?;
        session.stop();
        Some(session)
    }

    /// End every session.
    pub fn stop_all(&self) {
        let sessions: Vec<_> = self.lock().drain().map(|(_, session)| session).collect();
        for session in sessions {
            session.stop();
        }
    }
}

fn next_number() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// What one answer about a session holds.
#[derive(Debug)]
pub struct Answer {
    /// The data.
    pub structured: Value,
    /// The text form.
    pub summary: String,
}

impl Session {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The current phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.state().phase.clone()
    }

    fn read_output(&self, mut pipe: Box<dyn std::io::Read + Send>, stream: Stream) {
        let mut buffer = [0_u8; 8192];
        while let Ok(read) = pipe.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let mut state = self.state();
            state.next_output += 1;
            let number = state.next_output;
            state.output_bytes += text.len();
            state.output.push_back((number, stream, text));
            while state.output_bytes > OUTPUT {
                let Some((_, _, dropped)) = state.output.pop_front() else {
                    break;
                };
                state.output_bytes -= dropped.len();
            }
        }
    }

    /// Read the events file as it grows until `rawr run` exits, then record
    /// the exit.
    fn follow(&self, mut child: std::process::Child) {
        let mut offset = 0;
        let mut partial = String::new();
        loop {
            let exited = child.try_wait().ok().flatten();
            self.read_events(&mut offset, &mut partial);
            if let Some(status) = exited {
                let mut state = self.state();
                if state.phase != Phase::Stopped {
                    state.phase = Phase::Exited(process::code(status));
                }
                drop(state);
                self.changed.notify_all();
                return;
            }
            std::thread::sleep(TAIL);
        }
    }

    fn read_events(&self, offset: &mut u64, partial: &mut String) {
        let Ok(mut file) = File::open(&self.events) else {
            return;
        };
        if file.seek(SeekFrom::Start(*offset)).is_err() {
            return;
        }
        let mut text = String::new();
        let Ok(read) = file.read_to_string(&mut text) else {
            return;
        };
        if read == 0 {
            return;
        }
        *offset += read as u64;
        partial.push_str(&text);
        let Some(end) = partial.rfind('\n') else {
            return;
        };
        let complete: String = partial.drain(..=end).collect();
        let mut state = self.state();
        for line in complete.lines() {
            if let Ok(event) = serde_json::from_str::<Value>(line) {
                state.record(event);
            }
        }
        drop(state);
        self.changed.notify_all();
    }

    /// End the session's group: SIGTERM, then SIGKILL after a grace.
    pub fn stop(&self) {
        {
            let mut state = self.state();
            if state.phase.is_over() {
                return;
            }
            state.phase = Phase::Stopped;
        }
        process::end_group(self.pid);
        self.changed.notify_all();
    }

    /// Block until something past `event` happens that `wakes`, the session
    /// is over, or `timeout` passes; `building` hears each build update.
    fn block(
        &self,
        event: u64,
        timeout: Duration,
        wakes: impl Fn(&State, u64) -> bool,
        mut building: impl FnMut(&str, u64),
    ) {
        let deadline = Instant::now() + timeout;
        let mut heard = None;
        let mut state = self.state();
        loop {
            if let Some((stage, percent)) = state.building.clone()
                && heard.as_ref() != Some(&(stage.clone(), percent))
            {
                building(&stage, percent);
                heard = Some((stage, percent));
            }
            if wakes(&state, event) || state.phase.is_over() {
                return;
            }
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            state = self
                .changed
                .wait_timeout(state, (deadline - now).min(Duration::from_millis(250)))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Wait until the program runs or fails, or `timeout` passes.
    pub fn settle(&self, timeout: Duration, building: impl FnMut(&str, u64)) {
        self.block(0, timeout, |state, _| !state.phase.is_pending(), building);
    }

    /// Wait for the next event past `cursor` other than build progress, or
    /// `timeout`.
    pub fn next(&self, cursor: Option<u64>, timeout: Duration, building: impl FnMut(&str, u64)) {
        let after = cursor.unwrap_or_else(|| self.state().given_event);
        self.block(
            after,
            timeout,
            |state, after| {
                state
                    .events
                    .iter()
                    .any(|(seq, event)| *seq > after && event["event"] != "building")
            },
            building,
        );
    }

    /// The session now: its phase, the events past `cursor` and the output
    /// past `output_cursor` (each defaulting to what was last given), which
    /// then count as given.
    pub fn answer(&self, cursor: Option<u64>, output_cursor: Option<u64>) -> Answer {
        let mut state = self.state();
        let after = cursor.unwrap_or(state.given_event);
        let output_after = output_cursor.unwrap_or(state.given_output);
        let events: Vec<Value> = state
            .events
            .iter()
            .filter(|(seq, event)| *seq > after && event["event"] != "building")
            .map(|(seq, event)| {
                let mut event = event.clone();
                event["seq"] = json!(seq);
                event
            })
            .collect();
        let cursor = state
            .events
            .last()
            .map_or(after, |(seq, _)| *seq.max(&after));
        let dropped = state
            .output
            .front()
            .is_some_and(|(number, _, _)| *number > output_after + 1);
        let mut stdout = String::new();
        let mut stderr = String::new();
        for (number, stream, text) in &state.output {
            if *number > output_after {
                match stream {
                    Stream::Stdout => stdout.push_str(text),
                    Stream::Stderr => stderr.push_str(text),
                }
            }
        }
        let output_cursor = state.next_output.max(output_after);
        state.given_event = cursor;
        state.given_output = output_cursor;
        // The newest failure among these events, else the one the running
        // program carries.
        let diagnostics = events
            .iter()
            .rev()
            .find(|event| event["event"] == "failed")
            .or(state.failure.as_ref())
            .map(|event| event["diagnostics"].clone());
        let phase = state.phase.clone();
        let status = match phase {
            Phase::Ended(status) | Phase::Exited(status) => Some(status),
            _ => None,
        };
        let building = state
            .building
            .clone()
            .filter(|_| phase == Phase::Building)
            .map(|(stage, percent)| json!({ "stage": stage, "percent": percent }));
        drop(state);
        let structured = json!({
            "root": self.root,
            "pid": self.pid,
            "state": phase.name(),
            "status": status,
            "building": building,
            "events": events,
            "diagnostics": diagnostics,
            "stdout": stdout,
            "stderr": stderr,
            "output_dropped": dropped,
            "cursor": cursor,
            "output_cursor": output_cursor,
        });
        let summary = summary(&structured);
        Answer {
            structured,
            summary,
        }
    }
}

impl State {
    fn record(&mut self, event: Value) {
        let seq = self.events.last().map_or(1, |(seq, _)| seq + 1);
        match event["event"].as_str().unwrap_or_default() {
            "building" => {
                self.phase = Phase::Building;
                self.building = Some((
                    event["stage"].as_str().unwrap_or_default().to_owned(),
                    event["percent"].as_u64().unwrap_or(0),
                ));
            }
            "running" | "swapped" => {
                self.phase = Phase::Running;
                self.failure = None;
            }
            "restarting" => self.phase = Phase::Restarting,
            "failed" => {
                if self.phase == Phase::Running {
                    self.failure = Some(event.clone());
                } else {
                    self.phase = Phase::Failed;
                }
            }
            "ended" => self.phase = Phase::Ended(event["status"].as_i64().unwrap_or(0) as i32),
            _ => {}
        }
        if self.phase.is_over() {
            return;
        }
        self.events.push((seq, event));
    }
}

/// A session's answer as an agent reads it.
fn summary(structured: &Value) -> String {
    let mut text = format!(
        "{}: {}",
        structured["root"].as_str().unwrap_or_default(),
        structured["state"].as_str().unwrap_or_default()
    );
    if let Some(status) = structured["status"].as_i64() {
        text.push_str(&format!(" (status {status})"));
    }
    if let Some(building) = structured["building"].as_object() {
        text.push_str(&format!(
            ", {} {}%",
            building["stage"].as_str().unwrap_or_default(),
            building["percent"]
        ));
    }
    text.push_str(&format!(
        "\ncursor {}, output cursor {}\n",
        structured["cursor"], structured["output_cursor"]
    ));
    for event in structured["events"].as_array().into_iter().flatten() {
        let name = event["event"].as_str().unwrap_or_default();
        match name {
            "restarting" => text.push_str(&format!(
                "#{} restarting: {}\n",
                event["seq"],
                event["reason"].as_str().unwrap_or_default()
            )),
            "ended" => text.push_str(&format!(
                "#{} ended with {}\n",
                event["seq"], event["status"]
            )),
            _ => text.push_str(&format!("#{} {name}\n", event["seq"])),
        }
    }
    if let Some(diagnostics) = structured["diagnostics"].as_array() {
        text.push_str(&crate::diagnostic::lines(diagnostics));
    }
    for (stream, key) in [("stdout", "stdout"), ("stderr", "stderr")] {
        let output = structured[key].as_str().unwrap_or_default();
        if !output.is_empty() {
            text.push_str(&format!("--- {stream} ---\n{output}"));
            if !output.ends_with('\n') {
                text.push('\n');
            }
        }
    }
    if structured["output_dropped"] == true {
        text.push_str("(older output was dropped)\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{Phase, State};
    use serde_json::json;
    use std::collections::VecDeque;

    fn state() -> State {
        State {
            phase: Phase::Starting,
            events: Vec::new(),
            building: None,
            failure: None,
            output: VecDeque::new(),
            output_bytes: 0,
            next_output: 0,
            given_event: 0,
            given_output: 0,
        }
    }

    #[test]
    fn events_move_the_phase_and_a_failure_keeps_the_running_program() {
        let mut state = state();
        state.record(json!({ "event": "building", "stage": "building API", "percent": 40 }));
        assert_eq!(state.phase, Phase::Building);
        state.record(json!({ "event": "running" }));
        assert_eq!(state.phase, Phase::Running);
        state.record(json!({ "event": "failed", "diagnostics": [] }));
        assert_eq!(state.phase, Phase::Running);
        assert!(state.failure.is_some());
        state.record(json!({ "event": "swapped" }));
        assert!(state.failure.is_none());
        state.record(json!({ "event": "ended", "status": 3 }));
        assert_eq!(state.phase, Phase::Ended(3));
        assert_eq!(
            state.events.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
        let mut failing = super::tests::state();
        failing.record(json!({ "event": "failed", "diagnostics": [] }));
        assert_eq!(failing.phase, Phase::Failed);
    }
}
