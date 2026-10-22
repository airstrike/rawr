//! Same-thread deterministic scripts at the ordinary production launch boundary.
//!
//! This module exists only in unit-test builds. A script intercepts every
//! production launch on its thread, including discovery and replay handles.
//! It does not install a process owner, grant a Native capability, or simulate
//! process metrics. Product tests must separately establish those authorities.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::{Command, Limits, Output};
use crate::{Error, Result};

mod files;
mod paths;

pub(crate) use files::{File, FileLimits};
pub(crate) use paths::{Binding, Paths};

/// One exact sealed command and its executable and stream limits.
pub(crate) struct Expected {
    executable: PathBuf,
    command: Command,
    limits: Limits,
}

/// Owned fixture bytes returned only after the complete command matches.
pub(crate) struct Reply {
    code: u8,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    files: Vec<File>,
}

/// Builders receive only independently checked path bindings, never the call.
pub(crate) struct Step {
    binding: Option<Binding>,
    expected: Box<dyn FnOnce(&Paths) -> Expected>,
    reply: Box<dyn FnOnce(&Paths) -> Reply>,
}

/// Explicit fixture storage and an ordered, exhaustively consumed child script.
pub(crate) struct Script {
    steps: VecDeque<Step>,
    paths: Paths,
    files: files::Writes,
}

/// Script failures remain distinct from expected child exit failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum Failure {
    #[error("a test launch script is already active")]
    Nested,
    #[error("a test launch callback attempted another child")]
    Reentrant,
    #[error("a test launch callback panicked")]
    Panicked,
    #[error("the test launch script has no expected child")]
    Unexpected,
    #[error("a test child differs in {0}")]
    Mismatch(&'static str),
    #[error("the test launch script has unconsumed children")]
    Unconsumed,
    #[error("invalid test Attempt binding: {0}")]
    Binding(&'static str),
    #[error("invalid test file effect: {0}")]
    File(&'static str),
    #[error("test file effects exceed their cumulative limit")]
    FileLimit,
    #[error("test fixture filesystem operation failed")]
    Io,
}

struct Active {
    script: RefCell<Script>,
    busy: Cell<bool>,
    failure: Cell<Option<Failure>>,
}

struct Scope(Rc<Active>);
struct Calling(Rc<Active>);

thread_local! {
    static ACTIVE: RefCell<Option<Rc<Active>>> = const { RefCell::new(None) };
}

impl Expected {
    /// Keep a complete independently constructed command for central comparison.
    pub(crate) fn new(executable: PathBuf, command: Command, limits: Limits) -> Self {
        Self {
            executable,
            command,
            limits,
        }
    }

    fn compare(
        &self,
        executable: &Path,
        command: &Command,
        limits: Limits,
    ) -> std::result::Result<(), Failure> {
        let comparisons = [
            (
                self.executable.as_os_str() == executable.as_os_str(),
                "executable",
            ),
            (self.command.program == command.program, "program"),
            (self.command.kind == command.kind, "kind"),
            (self.command.arguments == command.arguments, "arguments"),
            (
                self.command.current_dir.as_os_str() == command.current_dir.as_os_str(),
                "current directory",
            ),
            (
                self.command.environment == command.environment,
                "environment",
            ),
            (self.limits.stdout == limits.stdout, "stdout limit"),
            (self.limits.stderr == limits.stderr, "stderr limit"),
        ];
        for (equal, field) in comparisons {
            if !equal {
                return Err(Failure::Mismatch(field));
            }
        }
        Ok(())
    }
}

impl Reply {
    /// Return the given bounded streams and listed file effects, with no child.
    pub(crate) fn exit(code: u8, stdout: Vec<u8>, stderr: Vec<u8>, files: Vec<File>) -> Self {
        Self {
            code,
            stdout,
            stderr,
            files,
        }
    }

    /// A successful command with exact stdout and no stderr or file effects.
    pub(crate) fn stdout(bytes: Vec<u8>) -> Self {
        Self::exit(0, bytes, Vec::new(), Vec::new())
    }
}

impl Step {
    /// Expect one exact command before constructing its response.
    pub(crate) fn new(
        expected: impl FnOnce(&Paths) -> Expected + 'static,
        reply: impl FnOnce(&Paths) -> Reply + 'static,
    ) -> Self {
        Self {
            binding: None,
            expected: Box::new(expected),
            reply: Box::new(reply),
        }
    }

    /// Bind only this declared path position; other fields stay literal.
    pub(crate) fn bind(mut self, binding: Binding) -> Self {
        self.binding = Some(binding);
        self
    }
}

impl Script {
    /// The root must be an existing canonical fixture directory.
    pub(crate) fn new(
        root: PathBuf,
        limits: FileLimits,
        steps: Vec<Step>,
    ) -> std::result::Result<Self, Failure> {
        let paths = Paths::new(root)?;
        Ok(Self {
            steps: steps.into(),
            paths,
            files: files::Writes::new(limits),
        })
    }

    /// Exhaust every expected launch, retaining failures even if the body ignores them.
    pub(crate) fn run<T>(self, body: impl FnOnce() -> T) -> std::result::Result<T, Failure> {
        let active = Rc::new(Active {
            script: RefCell::new(self),
            busy: Cell::new(false),
            failure: Cell::new(None),
        });
        ACTIVE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if let Some(existing) = slot.as_ref() {
                return Err(existing.fail(Failure::Nested));
            }
            *slot = Some(Rc::clone(&active));
            Ok(())
        })?;
        let scope = Scope(Rc::clone(&active));
        let value = body();
        drop(scope);
        if let Some(failure) = active.failure.get() {
            return Err(failure);
        }
        if !active.script.borrow().steps.is_empty() {
            return Err(Failure::Unconsumed);
        }
        Ok(value)
    }

    /// Preserve a reached operation error before checking unused later steps.
    /// A successful body must still consume the entire script; command mismatch
    /// and unexpected-child failures retain precedence even over a body error.
    pub(crate) fn run_result<T, E>(
        self,
        body: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<std::result::Result<T, E>, Failure> {
        let mut result = None;
        let checked = self.run(|| result = Some(body()));
        match (checked, result) {
            (Ok(()), Some(result)) => Ok(result),
            (Err(Failure::Unconsumed), Some(Err(error))) => Ok(Err(error)),
            (Err(error), _) => Err(error),
            (Ok(()), None) => Err(Failure::Panicked),
        }
    }
}

impl Active {
    fn fail(&self, failure: Failure) -> Failure {
        if let Some(first) = self.failure.get() {
            return first;
        }
        self.failure.set(Some(failure));
        failure
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|active| Rc::ptr_eq(active, &self.0))
            {
                *slot = None;
            }
        });
    }
}

impl Drop for Calling {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.fail(Failure::Panicked);
        }
        self.0.busy.set(false);
    }
}

/// `None` means no scope was installed; an installed scope never falls through.
pub(super) fn execute(
    executable: &Path,
    command: &Command,
    limits: Limits,
) -> Option<Result<Output>> {
    let active = ACTIVE.with(|slot| slot.borrow().as_ref().map(Rc::clone))?;
    if let Some(failure) = active.failure.get() {
        return Some(Err(error(failure)));
    }
    if active.busy.replace(true) {
        return Some(Err(error(active.fail(Failure::Reentrant))));
    }
    let _calling = Calling(Rc::clone(&active));
    let result = (|| {
        let mut script = active.script.borrow_mut();
        let step = script.steps.pop_front().ok_or(Failure::Unexpected)?;
        if let Some(binding) = step.binding {
            script.paths.bind(binding, command)?;
        }
        let expected = (step.expected)(&script.paths);
        if let Some(failure) = active.failure.get() {
            return Err(failure);
        }
        expected.compare(executable, command, limits)?;
        let reply = (step.reply)(&script.paths);
        if let Some(failure) = active.failure.get() {
            return Err(failure);
        }
        // Stream overflow is an ordinary child result, using the real boundary
        // checks. Invalid fixture file effects are script failures.
        let stdout_length = reply.stdout.len();
        let stderr_length = reply.stderr.len();
        let stdout =
            super::bounded_bytes("child stdout", limits.stdout, (reply.stdout, stdout_length));
        let stderr =
            super::bounded_bytes("child stderr", limits.stderr, (reply.stderr, stderr_length));
        let (stdout, stderr) = match (stdout, stderr) {
            (Ok(stdout), Ok(stderr)) => (stdout, stderr),
            (Err(error), _) | (_, Err(error)) => return Ok(Err(error)),
        };
        let Script { paths, files, .. } = &mut *script;
        files.apply(paths.root(), &reply.files)?;
        Ok(Ok(Output {
            success: reply.code == 0,
            status: format!("exit status: {}", reply.code).into_boxed_str(),
            stdout,
            stderr,
        }))
    })();
    Some(match result {
        Ok(output) => output,
        Err(failure) => Err(error(active.fail(failure))),
    })
}

fn error(failure: Failure) -> Error {
    Error::Request {
        field: "test launch script",
        reason: failure.to_string().into_boxed_str(),
    }
}

#[cfg(test)]
mod tests;
