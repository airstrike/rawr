//! The session's events and output as the protocol's events: a pause is
//! `stopped`, its end `continued`, the program's output and each reload
//! `output`, and the end of the run `exited` and `terminated`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{Adapter, THREAD};
use crate::mcp::process::Cancel;
use crate::mcp::session::{Phase, Session};

impl Adapter {
    /// Tell the client what `session` tells until it is over: from its
    /// start for a run the adapter started, and for one started elsewhere,
    /// its output so far and where it is paused, if it is, then what comes.
    pub(super) fn follow(&self, session: &Arc<Session>) {
        let mut event = 0;
        let mut output = 0;
        let mut past = !session.is_started();
        loop {
            let answer = session.answer(Some(event), Some(output)).structured;
            event = answer["cursor"].as_u64().unwrap_or(event);
            output = answer["output_cursor"].as_u64().unwrap_or(output);
            if std::mem::take(&mut past) {
                if let Some(mut paused) = session.paused() {
                    paused["event"] = json!("paused");
                    self.tell(&paused);
                }
            } else {
                for told in answer["events"].as_array().into_iter().flatten() {
                    self.tell(told);
                }
            }
            for stream in ["stdout", "stderr"] {
                if let Some(text) = answer[stream].as_str().filter(|text| !text.is_empty()) {
                    self.output(stream, text);
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
            session.hear(event, output, &Cancel::default());
        }
    }

    /// One event of the session's.
    fn tell(&self, event: &Value) {
        let reason = event["reason"].as_str().unwrap_or_default();
        match event["event"].as_str().unwrap_or_default() {
            "paused" => {
                self.handles().drop_all();
                self.stopped.store(true, Ordering::Release);
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
    fn went_on(&self) {
        self.handles().drop_all();
        if self.stopped.swap(false, Ordering::AcqRel) {
            self.out.event(
                "continued",
                json!({ "threadId": THREAD, "allThreadsContinued": true }),
            );
        }
    }
}
