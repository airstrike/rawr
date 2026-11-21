//! `check`: `rawr check` in a child, which records its diagnostics as data
//! ([`crate::diagnostic::SINK`]). A first check of a program with Rust
//! dependencies builds them for minutes. A `check` the client cancels leaves
//! it going, and the next `check` of that root waits for it again, or takes
//! its result if nothing changed since it began.
//!
//! The server's loop owns the checks. A call sends it a [`Request`] and
//! waits on its own channel, which its cancel also wakes. Each check runs
//! on a thread that sends the loop a [`Report`] once it is done, and the
//! loop answers every call still waiting.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Instant, SystemTime};

use serde_json::{json, Value};

use super::call::{Call, Wake, Waker};
use super::content;
use super::options::Options;
use super::process::{self, Tail};
use super::tools::Place;
use super::{Heard, Server};

/// What one check checks: a program, its root and the call's options. Two
/// calls share a check only when they ask the same.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    root: PathBuf,
    options: Options,
}

/// A call's ask for a check of its program.
pub(super) struct Request {
    key: Key,
    /// Where the check's diagnostics are written.
    directory: PathBuf,
    /// The root's files when the call asked.
    files: Fingerprint,
    /// The call the result goes to.
    reply: Waker,
}

/// A check thread's result for its key.
pub(super) struct Report {
    key: Key,
    result: Value,
}

/// The checks, one per key. The server's loop owns them.
#[derive(Debug, Default)]
pub(super) struct Checks {
    jobs: HashMap<Key, Job>,
    /// The number of the next check's diagnostics file.
    next: u64,
}

/// One `rawr check` of a root.
#[derive(Debug)]
struct Job {
    /// The root's files when it began.
    files: Fingerprint,
    state: State,
}

#[derive(Debug)]
enum State {
    /// `rawr check` runs, leading `group`, and the calls that joined it
    /// wait for it.
    Running { group: u32, joined: Vec<Waker> },
    /// It finished with no call waiting. The next call takes the result
    /// while the root's files are unchanged.
    Done(Value),
}

/// Check `place` through the server's loop, until it is checked or `call`
/// is cancelled.
pub(super) fn check(server: &Server, place: &Place, options: &Options, call: &Call) -> Value {
    server.ask(Heard::Check(Request {
        key: Key {
            path: place.path.clone(),
            root: place.root.clone(),
            options: options.clone(),
        },
        directory: place.directory(),
        files: fingerprint(&place.root),
        reply: call.waker(),
    }));
    // The server's end cancels every call, so a loop that stopped reading
    // still wakes this one.
    call.wait(|wake| match wake {
        Wake::Checked(result) => Some(result),
        _ => None,
    })
    .unwrap_or_else(|| content::failure("cancelled"))
}

impl Checks {
    /// Answer `request`: join the running check of its key, take its
    /// result while nothing changed since it began, or start a check.
    pub(super) fn ask(&mut self, request: Request, heard: &mpsc::Sender<Heard>) {
        let Request {
            key,
            directory,
            files,
            reply,
        } = request;
        match self.jobs.get_mut(&key) {
            Some(Job {
                state: State::Running { joined, .. },
                ..
            }) => {
                joined.push(reply);
                return;
            }
            Some(job) if job.files == files => {
                // Taken once: the next check runs again.
                if let Some(Job {
                    state: State::Done(result),
                    ..
                }) = self.jobs.remove(&key)
                {
                    reply.wake(Wake::Checked(result));
                }
                return;
            }
            Some(_) | None => {}
        }
        self.next += 1;
        let sink = directory.join(format!("check-{}-{}.jsonl", std::process::id(), self.next));
        match start(&key, sink, heard) {
            Ok(group) => {
                let joined = vec![reply];
                let state = State::Running { group, joined };
                self.jobs.insert(key, Job { files, state });
            }
            Err(error) => {
                self.jobs.remove(&key);
                let failure = content::failure(format!("cannot run rawr check: {error}"));
                reply.wake(Wake::Checked(failure));
            }
        }
    }

    /// A check is done: the calls still waiting get its result, or the
    /// next call takes it.
    pub(super) fn finished(&mut self, report: Report) {
        let Report { key, result } = report;
        // A check `stop_all` ended has no job.
        let Some(job) = self.jobs.get_mut(&key) else {
            return;
        };
        let State::Running { joined, .. } = &mut job.state else {
            return;
        };
        // A cancelled call takes no wake. The loop sets a call's flag
        // itself, so none is cancelled after it took the result.
        let mut taken = false;
        for reply in joined.drain(..) {
            taken |= reply.wake(Wake::Checked(result.clone()));
        }
        if taken {
            self.jobs.remove(&key);
        } else {
            job.state = State::Done(result);
        }
    }

    /// End every check.
    pub(super) fn stop_all(&mut self) {
        for (_, job) in self.jobs.drain() {
            if let State::Running { group, .. } = job.state {
                process::end_group(group);
            }
        }
    }
}

/// Start `rawr check` of `key`'s path, with its feature flags and in its
/// environment, in a group of its own, writing its diagnostics to `sink`,
/// and a thread that reports its result to the loop. Returns the group.
fn start(key: &Key, sink: PathBuf, heard: &mpsc::Sender<Heard>) -> std::io::Result<u32> {
    if let Some(parent) = sink.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let started = Instant::now();
    let root = key.root.as_path();
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("check").arg(&key.path);
    key.options.apply(&mut command);
    command
        .current_dir(if root.is_dir() {
            root
        } else {
            root.parent().unwrap_or(root)
        })
        .env(crate::diagnostic::SINK, &sink)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process::spawn(&mut command)?;
    let group = child.id();
    let stdout = child.stdout.take().map(Tail::read).unwrap_or_default();
    let stderr = child.stderr.take().map(Tail::read).unwrap_or_default();
    let key = key.clone();
    let reporting = heard.clone();
    let spawned = std::thread::Builder::new()
        .name("rawr-mcp-check".into())
        .spawn(move || {
            // The group ends while its leader is a zombie, so no other
            // process holds that group yet.
            rawr_watch::exit::wait(group);
            process::kill_group(group);
            let status = child.wait().ok();
            let finished = process::Finished {
                status,
                stdout,
                stderr,
                elapsed: started.elapsed(),
            };
            let result = answer(finished, &sink);
            let _ = std::fs::remove_file(&sink);
            let _ = reporting.send(Heard::Checked(Report { key, result }));
        });
    if let Err(error) = spawned {
        // The child went with the thread's closure: its group ends, and
        // its leader is reaped by pid.
        process::kill_group(group);
        if let Ok(pid) = i32::try_from(group) {
            let _ = nix::sys::wait::waitpid(nix::unistd::Pid::from_raw(pid), None);
        }
        return Err(error);
    }
    Ok(group)
}

/// The answer to a check that `finished`, with the diagnostics it wrote
/// to `sink`.
fn answer(finished: process::Finished, sink: &Path) -> Value {
    let diagnostics: Vec<Value> = std::fs::read_to_string(sink)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let passed = finished.status.is_some_and(|status| status.success());
    let (stdout, _) = finished.stdout.finish();
    let (stderr, _) = finished.stderr.finish();
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["severity"] == "error")
        .count();
    let warnings = diagnostics.len() - errors;
    let structured = json!({
        "ok": passed,
        "diagnostics": diagnostics,
        "errors": errors,
        "warnings": warnings,
        "seconds": finished.elapsed.as_secs_f64(),
        "stdout": stdout,
        "stderr": if diagnostics.is_empty() { stderr.clone() } else { String::new() },
    });
    let mut summary = crate::diagnostic::lines(&diagnostics);
    if passed {
        summary.push_str(&format!(
            "check passed{}\n",
            match warnings {
                0 => String::new(),
                1 => " with 1 warning".to_owned(),
                count => format!(" with {count} warnings"),
            }
        ));
        summary.push_str(stdout.trim_end());
        content::success(structured, summary.trim_end(), Vec::new())
    } else {
        if diagnostics.is_empty() {
            // Nothing recorded as data (cargo itself failed, say): what it
            // printed.
            summary.push_str(stderr.trim_end());
            summary.push('\n');
        }
        summary.push_str(&format!(
            "check failed: {errors} error{}",
            if errors == 1 { "" } else { "s" }
        ));
        content::failure_with(structured, summary, Vec::new())
    }
}

/// The files under a root that a check reads: their count, and the newest
/// modification. `target` and hidden directories are skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Fingerprint(usize, Option<SystemTime>);

fn fingerprint(root: &Path) -> Fingerprint {
    let mut print = Fingerprint::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == "target" || name.starts_with('.') {
                    continue;
                }
                stack.push(entry.path());
            }
        } else {
            print.0 += 1;
            print.1 = print.1.max(metadata.modified().ok());
        }
    }
    print
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;

    use serde_json::{json, Value};

    use super::{Call, Checks, Fingerprint, Job, Key, Report, Request, State, Wake, Waker};
    use crate::mcp::options::Options;

    fn key() -> Key {
        Key {
            path: PathBuf::from("/p/src/main.rs"),
            root: PathBuf::from("/p"),
            options: Options::default(),
        }
    }

    fn request(files: &Fingerprint) -> (Request, Call) {
        let call = Call::default();
        let request = Request {
            key: key(),
            directory: PathBuf::from("/p/target/mcp"),
            files: files.clone(),
            reply: call.waker(),
        };
        (request, call)
    }

    fn checked(call: &Call) -> Option<Value> {
        call.wait(|wake| match wake {
            Wake::Checked(result) => Some(result),
            _ => None,
        })
    }

    /// The waker of a call that was cancelled.
    fn cancelled() -> Waker {
        let waker = Call::default().waker();
        waker.cancel();
        waker
    }

    fn running(files: &Fingerprint, joined: Vec<Waker>) -> Job {
        Job {
            files: files.clone(),
            state: State::Running { group: 0, joined },
        }
    }

    /// A check that finishes answers the calls still waiting. With none
    /// waiting, the next call of unchanged files takes its result once.
    #[test]
    fn a_finished_check_answers_its_waiting_calls_or_keeps_its_result() {
        let files = Fingerprint(1, None);
        let (heard, _hearing) = mpsc::channel();
        let mut checks = Checks::default();
        let waiting = Call::default();
        checks
            .jobs
            .insert(key(), running(&files, vec![cancelled(), waiting.waker()]));
        let (joining, joined) = request(&files);
        checks.ask(joining, &heard);
        checks.finished(Report {
            key: key(),
            result: json!(1),
        });
        assert_eq!(checked(&waiting), Some(json!(1)));
        assert_eq!(checked(&joined), Some(json!(1)));
        assert!(
            checks.jobs.is_empty(),
            "a result some call took is not kept"
        );

        checks
            .jobs
            .insert(key(), running(&files, vec![cancelled()]));
        checks.finished(Report {
            key: key(),
            result: json!(2),
        });
        let (taking, taken) = request(&files);
        checks.ask(taking, &heard);
        assert_eq!(checked(&taken), Some(json!(2)));
        assert!(checks.jobs.is_empty(), "a kept result is taken once");
    }
}
