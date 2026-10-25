//! `check`: `rawr check` in a child, which records its diagnostics as data
//! ([`crate::diagnostic::SINK`]). A first check of a program with Rust
//! dependencies builds them for minutes, past one call's bound. Then the
//! check goes on, and the next `check` of that root waits for it again, or
//! takes its result if nothing changed since it began.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

use super::content;
use super::process::{self, Tail};

/// One `rawr check` of a root.
#[derive(Debug)]
struct Job {
    started: Instant,
    /// The root's files when it began.
    files: Fingerprint,
    done: Mutex<Option<Value>>,
    finished: Condvar,
    /// The `rawr check` running, by the group it leads.
    group: Mutex<Option<u32>>,
}

/// The checks, one per root.
#[derive(Debug, Default)]
pub struct Checks(Mutex<HashMap<PathBuf, Arc<Job>>>);

impl Checks {
    /// Check `path` (whose root is `root`), returning within `bound`.
    pub fn check(&self, path: &Path, root: &Path, directory: &Path, bound: Duration) -> Value {
        let job = {
            let mut jobs = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            let current = fingerprint(root);
            let reusable = jobs.get(root).is_some_and(|job| {
                let done = job.done.lock().unwrap_or_else(PoisonError::into_inner);
                // A running check is joined; a finished one is reused only
                // when nothing changed since it began.
                done.is_none() || job.files == current
            });
            if !reusable {
                let job = Arc::new(Job {
                    started: Instant::now(),
                    files: current,
                    done: Mutex::new(None),
                    finished: Condvar::new(),
                    group: Mutex::new(None),
                });
                start(Arc::clone(&job), path, root, directory);
                jobs.insert(root.to_path_buf(), job);
            }
            Arc::clone(jobs.get(root).expect("inserted above"))
        };
        let deadline = Instant::now() + bound;
        let mut done = job.done.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(result) = done.as_ref() {
                let result = result.clone();
                drop(done);
                // Taken once: the next check runs again.
                let mut jobs = self.0.lock().unwrap_or_else(PoisonError::into_inner);
                if jobs.get(root).is_some_and(|kept| Arc::ptr_eq(kept, &job)) {
                    jobs.remove(root);
                }
                return result;
            }
            let now = Instant::now();
            if now >= deadline {
                let seconds = job.started.elapsed().as_secs();
                return content::success(
                    json!({ "state": "checking", "seconds": seconds }),
                    format!(
                        "still checking after {seconds} s: a first check builds the program's Rust dependencies. Call `check` again to keep waiting."
                    ),
                    Vec::new(),
                );
            }
            done = job
                .finished
                .wait_timeout(done, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// End every check.
    pub fn stop_all(&self) {
        for (_, job) in self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
        {
            if let Some(group) = *job.group.lock().unwrap_or_else(PoisonError::into_inner) {
                process::end_group(group);
            }
        }
    }
}

fn start(job: Arc<Job>, path: &Path, root: &Path, directory: &Path) {
    let path = path.to_path_buf();
    let root = root.to_path_buf();
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let sink = directory.join(format!("check-{}-{number}.jsonl", std::process::id()));
    let _ = std::thread::Builder::new()
        .name("rawr-mcp-check".into())
        .spawn(move || {
            let result = run(&path, &root, &sink, &job.group);
            let _ = std::fs::remove_file(&sink);
            *job.done.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
            job.finished.notify_all();
        });
}

fn run(path: &Path, root: &Path, sink: &Path, group: &Mutex<Option<u32>>) -> Value {
    if let Some(parent) = sink.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let started = Instant::now();
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return content::failure(format!("cannot find rawr: {error}")),
    };
    let mut command = Command::new(executable);
    command
        .arg("check")
        .arg(path)
        .current_dir(if root.is_dir() {
            root
        } else {
            root.parent().unwrap_or(root)
        })
        .env(crate::diagnostic::SINK, sink)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match process::spawn(&mut command) {
        Ok(child) => child,
        Err(error) => return content::failure(format!("cannot run rawr check: {error}")),
    };
    *group.lock().unwrap_or_else(PoisonError::into_inner) = Some(child.id());
    let stdout = child.stdout.take().map(Tail::read).unwrap_or_default();
    let stderr = child.stderr.take().map(Tail::read).unwrap_or_default();
    let status = child.wait();
    *group.lock().unwrap_or_else(PoisonError::into_inner) = None;
    process::kill_group(child.id());
    std::thread::sleep(Duration::from_millis(20));
    let diagnostics: Vec<Value> = std::fs::read_to_string(sink)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let passed = status.as_ref().is_ok_and(std::process::ExitStatus::success);
    let (stdout, _) = stdout.text();
    let (stderr, _) = stderr.text();
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
        "seconds": started.elapsed().as_secs_f64(),
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
