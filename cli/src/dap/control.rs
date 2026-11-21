//! What moves the program: breakpoints, `continue`, `pause` and the steps.
//! Each goes to the program's inspector through the session, which waits
//! for a program that starts or restarts. Breakpoint requests take turns:
//! one changes the program's breakpoints at a time, one ask after another,
//! and the next starts once it is answered.

use std::collections::VecDeque;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::breakpoints::{Change, Point};
use super::Adapter;
use crate::mcp::tools;

/// A breakpoint request with the turn: the breakpoints it sets, the
/// program's answers so far, and the list the last answer gave.
struct Setting<T> {
    request: Value,
    setting: Vec<T>,
    answers: Vec<(T, Result<Value, String>)>,
    last: Option<Value>,
}

impl<T> Setting<T> {
    const fn new(request: Value, setting: Vec<T>) -> Self {
        Self {
            request,
            setting,
            answers: Vec::new(),
            last: None,
        }
    }
}

/// A file's line breakpoints as the client set them.
struct Lines {
    file: PathBuf,
    setting: Setting<u64>,
}

impl Adapter {
    /// `setBreakpoints` or `setFunctionBreakpoints`: started once the
    /// requests before it are answered.
    pub(super) fn take_turn(&mut self, request: Value) {
        self.turn.push_back(request);
        if !self.breaking {
            self.next_turn();
        }
    }

    /// Start the next breakpoint request waiting, if any.
    fn next_turn(&mut self) {
        let Some(request) = self.turn.pop_front() else {
            self.breaking = false;
            return;
        };
        self.breaking = true;
        if request["command"] == "setBreakpoints" {
            self.set_lines(request);
        } else {
            self.set_functions(request);
        }
    }

    /// The breakpoint request with the turn is answered: the next goes.
    fn turn_answered(&mut self, request: &Value, answer: Result<Value, String>) {
        self.out.respond(request, answer);
        self.next_turn();
    }

    /// `setBreakpoints`: the source's breakpoints become the lines given,
    /// each verified where the program has a statement on it.
    fn set_lines(&mut self, request: Value) {
        let arguments = &request["arguments"];
        let Some(file) = arguments["source"]["path"].as_str() else {
            let refusal = Err("setBreakpoints needs the source's path".to_owned());
            return self.turn_answered(&request, refusal);
        };
        let file = tools::absolute(&self.root, Some(file));
        let lines: Vec<u64> = arguments["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|breakpoint| breakpoint["line"].as_u64())
            .map(|line| self.line_in(line))
            .collect();
        self.known
            .ask(lines.iter().map(|line| line_point(&file, *line)).collect());
        let clear = json!({ "op": "clear", "file": file });
        let mut job = Lines {
            file,
            setting: Setting::new(request, lines),
        };
        self.ask(clear, move |adapter, cleared| match cleared {
            Ok(answer) => {
                job.setting.last = Some(listed(&answer));
                adapter.break_lines(job);
            }
            Err(message) => adapter.lines_set(job, Err(message)),
        });
    }

    /// Set the next of `job`'s lines, or answer it once all are.
    fn break_lines(&mut self, mut job: Lines) {
        let Some(&line) = job.setting.setting.get(job.setting.answers.len()) else {
            return self.lines_set(job, Ok(()));
        };
        let set = json!({ "op": "break", "file": job.file, "line": line });
        self.ask(set, move |adapter, answer| {
            if let Ok(answer) = &answer {
                job.setting.last = Some(listed(answer));
            }
            job.setting.answers.push((line, answer));
            adapter.break_lines(job);
        });
    }

    /// Answer `job`, whose clear went as `cleared`.
    fn lines_set(&mut self, job: Lines, cleared: Result<(), String>) {
        let Lines { file, setting } = job;
        self.told(setting.last.as_ref());
        let source = json!({ "path": file });
        let answer = cleared.map(|()| {
            let set: Vec<Value> = setting
                .answers
                .into_iter()
                .map(|(line, answer)| match answer {
                    Ok(answer) => json!({
                        "id": self.known.id(&line_point(&file, line)),
                        "verified": true,
                        "line": self.line_out(answer["lines"][0]["line"].as_u64().unwrap_or(line)),
                        "source": source,
                    }),
                    Err(message) => json!({
                        "verified": false,
                        "line": self.line_out(line),
                        "message": message,
                        "source": source,
                    }),
                })
                .collect();
            json!({ "breakpoints": set })
        });
        self.turn_answered(&setting.request, answer);
    }

    /// `setFunctionBreakpoints`: the function breakpoints become those
    /// named, each pausing at its function's first statement.
    fn set_functions(&mut self, request: Value) {
        let names: Vec<String> = request["arguments"]["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|breakpoint| breakpoint["name"].as_str())
            .map(str::to_owned)
            .collect();
        let old: VecDeque<String> = std::mem::take(&mut self.functions).into();
        self.known.ask(
            old.iter()
                .chain(&names)
                .map(|name| Point::Function(name.clone()))
                .collect(),
        );
        self.clear_functions(old, Setting::new(request, names));
    }

    /// Clear the next of the `old` function breakpoints, then set `job`'s.
    /// Those left after a clear that failed are forgotten.
    fn clear_functions(&mut self, mut old: VecDeque<String>, mut job: Setting<String>) {
        let Some(function) = old.pop_front() else {
            return self.break_functions(job);
        };
        let clear = json!({ "op": "clear", "function": function });
        self.ask(clear, move |adapter, answer| match answer {
            Ok(answer) => {
                job.last = Some(listed(&answer));
                adapter.clear_functions(old, job);
            }
            Err(message) => adapter.functions_set(job, Err(message)),
        });
    }

    /// Set the next of `job`'s functions, or answer it once all are.
    fn break_functions(&mut self, mut job: Setting<String>) {
        let Some(name) = job.setting.get(job.answers.len()).cloned() else {
            return self.functions_set(job, Ok(()));
        };
        let set = json!({ "op": "break", "function": name });
        self.ask(set, move |adapter, answer| {
            if let Ok(answer) = &answer {
                adapter.functions.push(name.clone());
                job.last = Some(listed(answer));
            }
            job.answers.push((name, answer));
            adapter.break_functions(job);
        });
    }

    /// Answer `job`, whose clears went as `cleared`.
    fn functions_set(&mut self, job: Setting<String>, cleared: Result<(), String>) {
        self.told(job.last.as_ref());
        let answer = cleared.map(|()| {
            let set: Vec<Value> = job
                .answers
                .into_iter()
                .map(|(name, answer)| match answer {
                    Ok(answer) => {
                        let at = &answer["lines"][0];
                        let mut breakpoint = json!({
                            "id": self.known.id(&Point::Function(name)),
                            "verified": true,
                        });
                        if let (Some(file), Some(line)) = (at["file"].as_str(), at["line"].as_u64())
                        {
                            breakpoint["source"] = json!({ "path": file });
                            breakpoint["line"] = json!(self.line_out(line));
                        }
                        breakpoint
                    }
                    Err(message) => json!({ "verified": false, "message": message }),
                })
                .collect();
            json!({ "breakpoints": set })
        });
        self.turn_answered(&job.request, answer);
    }

    /// The client's breakpoint request ended, its last answer the list
    /// `last`: tell the client what other shells changed meanwhile. The
    /// breakpoints it knows then hold its own among them.
    fn told(&mut self, last: Option<&Value>) {
        let changes = self.known.answered(last);
        self.tell_breakpoints(&changes);
    }

    /// Tell the client what the session's newest breakpoints, `told`,
    /// changed of those it knows: what another shell set or cleared.
    pub(super) fn follow_breakpoints(&mut self, told: &Value) {
        let changes = self.known.follow(told);
        self.tell_breakpoints(&changes);
    }

    fn tell_breakpoints(&self, changes: &[Change]) {
        for change in changes {
            self.out
                .event("breakpoint", change.body(|line| self.line_out(line)));
        }
    }

    /// `continue`: the paused program runs on.
    pub(super) fn resume(&mut self, request: Value) {
        self.ask(json!({ "op": "continue" }), move |adapter, answer| {
            let answer = answer.map(|_| {
                if let Some(session) = &adapter.session {
                    session.observe_pause(None);
                }
                json!({ "allThreadsContinued": true })
            });
            adapter.out.respond(&request, answer);
        });
    }

    /// `pause`: the running program pauses at the next statement it runs,
    /// told as `stopped`.
    pub(super) fn pause(&mut self, request: Value) {
        self.ask(json!({ "op": "pause" }), move |adapter, answer| {
            adapter.out.respond(&request, answer.map(|_| json!({})));
        });
    }

    /// `next`, `stepIn` and `stepOut`, a step of `kind`: answered once the
    /// program goes, before the pause it ends in is told as `stopped`.
    pub(super) fn step(&mut self, request: &Value, kind: &str) {
        let paused = match &self.session {
            None => Err(super::NO_PROGRAM.to_owned()),
            Some(session) => session
                .paused()
                .map(|_| ())
                .ok_or_else(|| "the program isn't paused".to_owned()),
        };
        if let Err(message) = paused {
            return self.out.respond(request, Err(message));
        }
        self.out.respond(request, Ok(json!({})));
        self.ask(
            json!({ "op": "step", "kind": kind }),
            |adapter, answer| match answer {
                Ok(answer) => {
                    if let Some(session) = &adapter.session {
                        session.observe_pause(
                            answer["paused"]
                                .as_object()
                                .map(|_| answer["paused"].clone()),
                        );
                    }
                }
                Err(message) => {
                    adapter.output("stderr", &format!("rawr: the step failed: {message}\n"));
                }
            },
        );
    }
}

/// The breakpoint on `line` of `file`.
fn line_point(file: &std::path::Path, line: u64) -> Point {
    Point::Line {
        file: file.to_string_lossy().into_owned(),
        line,
    }
}

/// The breakpoints a `break` or `clear` answer says there are now, as the
/// session tells them: `{"revision", "breakpoints"}`.
fn listed(answer: &Value) -> Value {
    let breakpoints = answer.get("breakpoints").unwrap_or(&answer["left"]);
    json!({ "revision": answer["revision"], "breakpoints": breakpoints })
}
