//! The session's events and output as the protocol's events: a pause is
//! `stopped`, its end `continued`, the program's output, rawr's own lines
//! and each reload `output`, a change to the breakpoints `breakpoint`, and
//! the end of the run `exited` and `terminated`.

use serde_json::{json, Value};

use super::{Adapter, THREAD};
use crate::mcp::session::{Phase, Session, Wait};

/// How far the adapter told the session: its events, its output and its
/// breakpoints' revision.
#[derive(Debug, Default)]
pub(super) struct Following {
    event: u64,
    output: u64,
    breakpoints: u64,
    /// The session's past is still to tell, as where it is paused: for a
    /// run started elsewhere, whose events before now the client never
    /// heard.
    past: bool,
}

impl Following {
    /// From the start of `session`, for a run the adapter started, and for
    /// one started elsewhere from its output so far and where it is
    /// paused.
    pub(super) fn of(session: &Session) -> Self {
        Self {
            past: !session.is_started(),
            ..Self::default()
        }
    }
}

impl Adapter {
    /// Tell the client what the session told since it last did, then wait
    /// for more, until the session is over.
    pub(super) fn follow(&mut self) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let told = session.breakpoints();
        self.following.breakpoints = told["revision"]
            .as_u64()
            .unwrap_or(self.following.breakpoints);
        self.follow_breakpoints(&told);
        let answer = session
            .answer(Some(self.following.event), Some(self.following.output))
            .structured;
        self.following.event = answer["cursor"].as_u64().unwrap_or(self.following.event);
        self.following.output = answer["output_cursor"]
            .as_u64()
            .unwrap_or(self.following.output);
        if std::mem::take(&mut self.following.past) {
            if let Some(mut paused) = session.paused() {
                paused["event"] = json!("paused");
                self.tell(&paused);
            }
        } else {
            for told in answer["events"].as_array().into_iter().flatten() {
                self.tell(told);
            }
        }
        // Rawr's own lines, such as a cold start's progress, are the
        // adapter's console; the program's streams keep their names.
        for (stream, category) in [
            ("rawr", "console"),
            ("stdout", "stdout"),
            ("stderr", "stderr"),
        ] {
            if let Some(text) = answer[stream].as_str().filter(|text| !text.is_empty()) {
                self.output(category, text);
            }
        }
        let phase = session.phase();
        if phase.is_over() {
            if let Phase::Exited(Some(status)) = phase {
                self.out.event("exited", json!({ "exitCode": status }));
            }
            self.out.event("terminated", json!({}));
            return;
        }
        let change = Wait::Change {
            event: self.following.event,
            output: self.following.output,
            breakpoints: self.following.breakpoints,
        };
        self.park(&session, change, |adapter, _| adapter.follow());
    }

    /// One event of the session's.
    fn tell(&mut self, event: &Value) {
        let reason = event["reason"].as_str().unwrap_or_default();
        match event["event"].as_str().unwrap_or_default() {
            "paused" => {
                self.handles.drop_all();
                self.stopped = true;
                let mut description = format!(
                    "Paused at {}:{}",
                    event["file"].as_str().unwrap_or_default(),
                    event["line"]
                );
                if let Some(function) = event["function"].as_str() {
                    description.push_str(&format!(" in {function}"));
                }
                self.out.event(
                    "stopped",
                    json!({
                        "reason": match reason {
                            "step" | "pause" => reason,
                            _ => "breakpoint",
                        },
                        "description": description,
                        "threadId": THREAD,
                        "allThreadsStopped": true,
                    }),
                );
            }
            "resumed" => self.went_on(),
            "running" => self.output("console", "rawr: the program runs\n"),
            "swapped" => self.output(
                "console",
                "rawr: reloaded: the running program took the save\n",
            ),
            "queued" => self.output(
                "console",
                "rawr: a save waits for the program's next call into its own code\n",
            ),
            "preparing" => self.output("console", &format!("rawr: preparing a save: {reason}\n")),
            "restarting" => {
                self.went_on();
                self.output("console", &format!("rawr: restarting: {reason}\n"));
            }
            "failed" => {
                let diagnostics = event["diagnostics"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice);
                self.output("stderr", &crate::diagnostic::lines(diagnostics));
            }
            "ended" => self.output(
                "console",
                &format!("rawr: the program ended with status {}\n", event["status"]),
            ),
            _ => {}
        }
    }

    /// The program goes on, or a restart ends where it was paused: the
    /// client's frames and references no longer hold.
    fn went_on(&mut self) {
        self.handles.drop_all();
        if std::mem::take(&mut self.stopped) {
            self.out.event(
                "continued",
                json!({ "threadId": THREAD, "allThreadsContinued": true }),
            );
        }
    }
}
