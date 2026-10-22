//! Two live children under one parent-owned attempt and unchanged wire protocol.

use std::collections::BTreeSet;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use rawr_import::metrics::{self, cell, report};

use super::super::{corruption, provenance, Assets, Error, ProcessObserver, Project};
use super::{Execution, Reached};

struct Live {
    child: Child,
    started: Instant,
    observer: Option<ProcessObserver>,
    receiver: Option<std::thread::JoinHandle<Result<cell::Capsule, cell::ReceptionFailure>>>,
    closure: Option<cell::Closure>,
    status: Option<ExitStatus>,
}

/// Both requests bind the same prestate; scheduling drift is an ordinary failure.
pub(super) fn run(
    executable: &Path,
    assets: &Assets,
    project: &Project,
    home: &corruption::Home,
    corpus: [u8; 32],
    owner: report::Command,
    reached: &mut Reached,
) -> Result<(), Error> {
    let before = cell::project::Identity::read(&project.root)?;
    let fixture = assets.fixture()?;
    let fixture_digest =
        metrics::evidence::parse_digest(std::ffi::OsStr::new(fixture.digest_sha256.as_ref()))?;
    let build = provenance::load(executable, provenance::Kind::Evidence)?.cell(executable)?;
    let mut live = Vec::with_capacity(2);
    let mut listeners = Vec::with_capacity(2);
    let attempt = Instant::now();
    let result = (|| {
        for _ in 0..2 {
            let listening = cell::Listening::transform()?;
            let request = cell::Request::transform(
                corpus,
                listening.launch_id(),
                metrics::monotonic_ns().ok_or_else(|| failure("shared clock unavailable"))?,
                build.clone(),
                before.clone(),
                fixture_digest,
                owner,
                report::Scenario::ConcurrentCold,
                cell::Role::RetainedCell,
                cell::Profile::Inactive,
                1,
            )?;
            let mut command = std::process::Command::new(executable);
            command
                .current_dir(&project.root)
                .arg("--baseline-cell-v1")
                .arg(listening.socket())
                .arg(super::super::hex(&listening.launch_id()).as_ref())
                .arg(super::super::hex(&request.request_digest).as_ref())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0);
            home.configure(&mut command)
                .map_err(|error| failure(error.to_string()))?;
            let started = Instant::now();
            let child = command
                .spawn()
                .map_err(|error| failure(error.to_string()))?;
            let pid = child.id();
            reached.executions.push(Execution {
                request: request.clone(),
                pid,
                capsule: None,
                rejected_capsule: None,
                wall_ns: None,
                resources: None,
                exit: None,
            });
            // Custody enters `live` before any fallible observer/authentication work.
            live.push(Live {
                child,
                started,
                observer: None,
                receiver: None,
                closure: None,
                status: None,
            });
            let current = live
                .last_mut()
                .ok_or_else(|| failure("spawn custody lost"))?;
            current.observer = Some(ProcessObserver::transform(pid)?);
            listeners.push((listening, request));
        }
        // Authenticate BOTH exact peers before either unchanged Request+EOF is sent.
        let mut authenticated = Vec::with_capacity(2);
        for ((listening, request), child) in listeners.drain(..).zip(live.iter_mut()) {
            authenticated.push(listening.authenticate(
                &mut child.child,
                request,
                attempt + Duration::from_secs(10),
            )?);
        }
        for (pending, child) in authenticated.into_iter().zip(live.iter_mut()) {
            let (exchange, closure) = pending.exchange(attempt + Duration::from_secs(30 * 60))?;
            child.closure = Some(closure);
            child.receiver = Some(std::thread::spawn(move || exchange.finish()));
        }
        wait(&mut live, reached, attempt + Duration::from_secs(30 * 60))?;
        let after = cell::project::Identity::read(&project.root)?;
        if before.path != after.path
            || before.device != after.device
            || before.inode != after.inode
            || before.source_digest != after.source_digest
        {
            return Err(failure(
                "paired children changed parent-owned fixture inputs",
            ));
        }
        Ok(())
    })();
    if result.is_err() {
        retire(&mut live, reached);
    }
    result
}

fn wait(live: &mut [Live], reached: &mut Reached, deadline: Instant) -> Result<(), Error> {
    loop {
        let mut complete = true;
        for (child, execution) in live.iter_mut().zip(reached.executions.iter_mut()) {
            let Some(observer) = &child.observer else {
                // Observer is consumed only after full successful retirement.
                continue;
            };
            let (alive, observer_failure, _) = observer.state();
            if let Some(reason) = observer_failure {
                return Err(failure(reason));
            }
            if child.status.is_none() {
                child.status = child
                    .child
                    .try_wait()
                    .map_err(|error| failure(error.to_string()))?;
            }
            if let Some(status) = child.status {
                execution.exit = Some(status.to_string().into_boxed_str());
            }
            receive(child, execution)?;
            if let Some(capsule) = &execution.capsule {
                match capsule {
                    super::Capsule::Retained(value) => {
                        if let cell::Outcome::Failure { reason } = &value.outcome {
                            return Err(Error::Owner {
                                command: value.request.core.owner,
                                reason: reason.clone(),
                            });
                        }
                    }
                    super::Capsule::Setup(_) => {
                        return Err(failure("paired retained request returned setup evidence"))
                    }
                }
            }
            if let Some(status) = child.status
                && !status.success()
            {
                return Err(failure(format!(
                    "paired child {} exited with {status}",
                    child.child.id()
                )));
            }
            if child.status.is_some() && !alive && execution.capsule.is_some() {
                let observer = child
                    .observer
                    .take()
                    .ok_or_else(|| failure("observer custody lost"))?;
                execution.resources = Some(observer.finish()?);
                execution.wall_ns = Some(super::super::duration_ns(child.started.elapsed())?);
            } else {
                complete = false;
                if Instant::now() >= deadline {
                    return Err(failure(
                        "paired owner exceeded its original thirty-minute launch deadline",
                    ));
                }
            }
        }
        if complete {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn receive(child: &mut Live, execution: &mut Execution) -> Result<(), Error> {
    if child
        .receiver
        .as_ref()
        .is_some_and(|receiver| receiver.is_finished())
    {
        let receiver = child
            .receiver
            .take()
            .ok_or_else(|| failure("receiver custody lost"))?;
        match receiver
            .join()
            .map_err(|_| failure("paired capsule receiver panicked"))?
        {
            Ok(capsule) => execution.capsule = Some(capsule.into()),
            Err(error) => {
                execution.rejected_capsule = error.canonical_capsule.map(|bytes| {
                    String::from_utf8_lossy(&bytes)
                        .into_owned()
                        .into_boxed_str()
                });
                return Err(Error::CellProtocol(error.source));
            }
        }
    }
    Ok(())
}

fn retire(live: &mut [Live], reached: &mut Reached) {
    // Invalidate every capability before waiting on either process.
    for child in live.iter_mut() {
        if let Some(closure) = child.closure.take() {
            cleanup(reached, closure.invalidate());
        }
    }
    settle(live, reached, Instant::now() + Duration::from_secs(10));
    for child in live.iter_mut() {
        if let Err(error) = kill(child) {
            reached.cleanup.push(error.to_string().into_boxed_str());
        }
    }
    settle(live, reached, Instant::now() + Duration::from_secs(10));
    for (index, child) in live.iter_mut().enumerate() {
        if let Err(error) = receive(child, &mut reached.executions[index]) {
            reached.cleanup.push(error.to_string().into_boxed_str());
        }
        if child.status.is_none()
            || child.receiver.is_some()
            || child
                .observer
                .as_ref()
                .is_some_and(|observer| observer.state().0)
        {
            reached.cleanup.push(
                format!(
                    "child {} did not completely retire within bounded cleanup",
                    child.child.id()
                )
                .into_boxed_str(),
            );
        }
        if let Some(observer) = child.observer.take() {
            match observer.finish() {
                Ok(resources) => reached.executions[index].resources = Some(resources),
                Err(error) => reached.cleanup.push(error.to_string().into_boxed_str()),
            }
        }
        match super::super::duration_ns(child.started.elapsed()) {
            Ok(wall_ns) => reached.executions[index].wall_ns = Some(wall_ns),
            Err(error) => reached.cleanup.push(error.to_string().into_boxed_str()),
        }
    }
}

fn settle(live: &mut [Live], reached: &mut Reached, deadline: Instant) {
    loop {
        let mut complete = true;
        for (index, child) in live.iter_mut().enumerate() {
            if child.status.is_none() {
                match child.child.try_wait() {
                    Ok(status) => child.status = status,
                    Err(error) => {
                        reached.cleanup.push(error.to_string().into_boxed_str());
                        return;
                    }
                }
            }
            if let Some(status) = child.status {
                reached.executions[index].exit = Some(status.to_string().into_boxed_str());
            }
            if let Err(error) = receive(child, &mut reached.executions[index]) {
                reached.cleanup.push(error.to_string().into_boxed_str());
            }
            if child.status.is_none()
                || child.receiver.is_some()
                || child
                    .observer
                    .as_ref()
                    .is_some_and(|observer| observer.state().0)
            {
                complete = false;
            }
        }
        if complete || Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn kill(child: &mut Live) -> Result<(), Error> {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::{getpgid, Pid};
    let mut groups = child
        .observer
        .as_ref()
        .map_or_else(BTreeSet::new, |observer| observer.state().2);
    if child.status.is_none() {
        let pid =
            Pid::from_raw(i32::try_from(child.child.id()).map_err(|_| failure("child PID range"))?);
        if getpgid(Some(pid)).map_err(|error| failure(error.to_string()))? != pid {
            return Err(failure("paired child lost its private process group"));
        }
        groups.insert(child.child.id());
    }
    for group in groups {
        let pid = Pid::from_raw(i32::try_from(group).map_err(|_| failure("group PID range"))?);
        match killpg(pid, Signal::SIGKILL) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(error) => return Err(failure(error.to_string())),
        }
    }
    Ok(())
}

fn cleanup(reached: &mut Reached, result: Result<(), impl std::fmt::Display>) {
    if let Err(error) = result {
        reached.cleanup.push(error.to_string().into_boxed_str());
    }
}

fn failure(reason: impl Into<Box<str>>) -> Error {
    Error::Process {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rawr_import::metrics::report;

    use super::super::super::{corruption, Assets, Error};
    use super::{run, Reached};

    #[test]
    fn paired_run_refuses_unproven_executable_before_child_custody() {
        let assets = Assets::transform().expect("immutable fixture assets validate");
        let project = assets
            .materialize()
            .expect("exclusive fixture materializes");
        let home = corruption::Home::create(&project.root).expect("exclusive Cargo home creates");
        let executable = project.root.join("unproven-owner");
        fs::write(&executable, b"not a retained release artifact")
            .expect("unproven candidate writes");
        let mut reached = Reached::default();

        // Exercise the actual loader and run path without editing producer state.
        // A missing/invalid receipt or one for another executable must all stop
        // at provenance; current-file corroboration alone cannot admit a child.
        let error = run(
            &executable,
            &assets,
            &project,
            &home,
            [1; 32],
            report::Command::Check,
            &mut reached,
        )
        .expect_err("unproven candidate cannot enter the pair");
        assert!(matches!(error, Error::Provenance(_)), "{error}");
        assert!(reached.executions.is_empty());
        assert!(reached.cleanup.is_empty());
    }
}
