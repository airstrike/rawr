//! One finite qualification-only process; no ordinary command dispatch reaches it.

use std::ffi::OsStr;
use std::os::fd::AsFd as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::net::UnixStream;
use std::path::Path;

use rawr_import::metrics::evidence;
use sha2::{Digest as _, Sha256};

mod darwin;
mod native;
pub(crate) mod protocol;
mod resources;

use protocol::{Command, Error, Event};

pub(crate) fn run() -> Result<(), Error> {
    let mut arguments = std::env::args_os().skip(1);
    let launch = evidence::parse_digest(
        &arguments
            .next()
            .ok_or(Error::Invalid("missing private launch id"))?,
    )?;
    let build = evidence::parse_digest(
        &arguments
            .next()
            .ok_or(Error::Invalid("missing private build id"))?,
    )?;
    if arguments.next().is_some() {
        return Err(Error::Invalid("unexpected private observer argument"));
    }
    let stream = UnixStream::from(std::io::stdin().as_fd().try_clone_to_owned()?);
    let mut channel = protocol::Channel::transform(stream, launch, build)?;
    let mut sampling = resources::Sampling::new();
    let result = observe(&mut channel, launch, build, &mut sampling)
        .map_err(|source| sampling.retire_failed(source));
    if let Err(error) = &result {
        // Only one bounded failure record, never another action. If transport was
        // lost, nonzero process status is independently retained by the parent.
        if channel.idle() {
            let reason = error.to_string();
            if reason.len() <= protocol::OUTPUT_BYTES / 2 {
                let _ = channel.send(Event::Failed {
                    reason: reason.into_boxed_str(),
                });
            }
        }
    }
    result
}

fn observe(
    channel: &mut protocol::Channel,
    launch: [u8; 32],
    build: [u8; 32],
    sampling: &mut resources::Sampling,
) -> Result<(), Error> {
    let preflight_deadline = protocol::now()?
        .checked_add(protocol::GRACE_NS)
        .ok_or(Error::Invalid("preflight clock"))?;
    let Command::Initialize {
        observer,
        executable,
        title,
    } = receive(channel, preflight_deadline)?
    else {
        return Err(Error::Invalid("initialize required"));
    };
    if observer.digest != build
        || evidence::process::file::Identity::read(&std::env::current_exe()?)? != observer
    {
        return Err(Error::Invalid(
            "observer artifact does not match selected build",
        ));
    }
    unchanged(&executable, &title)?;
    darwin::preflight().map_err(window_error)?;
    sampling.sample(None, preflight_deadline)?;
    emit(channel, Event::Ready, preflight_deadline)?;
    let Command::Start {
        origin_ns,
        deadline_ns,
    } = receive(channel, preflight_deadline)?
    else {
        return Err(Error::Invalid("start required"));
    };
    if deadline_ns <= origin_ns
        || deadline_ns > evidence::Envelope::transform(origin_ns)?.deadline_ns()
        || origin_ns > protocol::now()?
    {
        return Err(Error::Invalid("invalid original observation deadline"));
    }
    emit(channel, Event::Started, deadline_ns)?;
    let exact_title =
        std::str::from_utf8(title.bytes()).map_err(|_| Error::Invalid("title encoding"))?;
    let mut receipt = None;
    let mut first = None;
    let mut cli = None;
    let visible = loop {
        protocol::before(deadline_ns)?;
        if let Some(command) = channel.poll::<Command>()? {
            match command {
                Command::Bind(bound) if receipt.is_none() => {
                    if bound.launch_id != launch
                        || bound.title != title
                        || bound.spawned_ns < origin_ns
                    {
                        return Err(Error::Invalid("runner receipt association"));
                    }
                    let identity = native::observe(bound.cli.pid)?;
                    if identity.path != executable.path || identity.uid != bound.cli.uid {
                        return Err(Error::Invalid("CLI current identity"));
                    }
                    cli = Some(identity);
                    receipt = Some(bound);
                }
                _ => return Err(Error::Invalid("unexpected visibility command")),
            }
        }
        if channel.eof() {
            return Err(Error::Invalid("parent disappeared before visibility"));
        }
        let windows = darwin::scan(exact_title).map_err(window_error)?;
        let candidate =
            darwin::provisional_candidate(&windows, exact_title).map_err(window_error)?;
        let observed_ns = protocol::now()?;
        if first.is_none()
            && observed_ns <= deadline_ns
            && let Some(window) = candidate
        {
            let identity = native::observe(
                u32::try_from(window.pid).map_err(|_| Error::Invalid("CG PID range"))?,
            )?;
            first = Some((window, observed_ns, identity));
        }
        if let Some(bound) = &receipt {
            sampling.poll()?;
            if sampling.idle() {
                sampling.start(Some([bound.cli.pid, bound.runner.pid]), deadline_ns)?;
            }
            if let Some((window, observed_ns, runner)) = &first {
                // Only after preserving the first CG timestamp may resource
                // sealing wait for the already-owned sample's actual retirement.
                sampling.finish()?;
                let cli = cli.as_ref().ok_or(Error::Invalid("CLI binding missing"))?;
                bind(bound, cli, runner)?;
                let file = evidence::process::file::Identity::read(path(&runner.path))?;
                if file.digest != bound.executable_digest || file.size != bound.executable_bytes {
                    return Err(Error::Invalid(
                        "running executable bytes differ from the authenticated seal",
                    ));
                }
                // The first timestamp is retained through all later native,
                // artifact, CG and resource observations.
                check(cli, runner, &file, &executable, &title)?;
                if !darwin::remains(window).map_err(window_error)? {
                    return Err(Error::Invalid(
                        "first visible row disappeared before sealing",
                    ));
                }
                sampling.sample(Some([bound.cli.pid, bound.runner.pid]), deadline_ns)?;
                if sampling.product().samples == 0 {
                    return Err(Error::Invalid("product resource sample absent"));
                }
                break protocol::Visible {
                    window: window.clone(),
                    observed_ns: *observed_ns,
                    cli: cli.clone(),
                    runner: runner.clone(),
                    executable: file,
                    resources: sampling.product(),
                };
            }
        }
        std::thread::sleep(protocol::POLL);
    };
    emit(channel, Event::Visible(visible.clone()), deadline_ns)?;
    let action_deadline = protocol::now()?
        .checked_add(protocol::GRACE_NS)
        .ok_or(Error::Invalid("close clock"))?;
    if !matches!(receive(channel, action_deadline)?, Command::Close) {
        return Err(Error::Invalid("close required after visibility seal"));
    }
    darwin::press_close(&visible.window, || {
        protocol::before(action_deadline)
            .and_then(|()| {
                check(
                    &visible.cli,
                    &visible.runner,
                    &visible.executable,
                    &executable,
                    &title,
                )
            })
            .map_err(|error| error.to_string().into_boxed_str())
    })
    .map_err(window_error)?;
    let pressed_ns = protocol::now()?;
    let retirement_deadline = pressed_ns
        .checked_add(protocol::GRACE_NS)
        .ok_or(Error::Invalid("retirement clock"))?;
    emit(
        channel,
        Event::Pressed {
            observed_ns: pressed_ns,
        },
        retirement_deadline,
    )?;
    loop {
        protocol::before(retirement_deadline)?;
        if channel.poll::<Command>()?.is_some() || channel.eof() {
            return Err(Error::Invalid("premature retirement command or EOF"));
        }
        if !darwin::remains(&visible.window).map_err(window_error)? {
            emit(
                channel,
                Event::Disappeared {
                    observed_ns: protocol::now()?,
                },
                retirement_deadline,
            )?;
            break;
        }
        std::thread::sleep(protocol::POLL);
    }
    if !matches!(receive(channel, retirement_deadline)?, Command::Finish) {
        return Err(Error::Invalid("finish required after product retirement"));
    }
    unchanged(&executable, &title)?;
    sampling.sample(None, retirement_deadline)?;
    emit(
        channel,
        Event::Finished {
            costs: sampling.costs(),
        },
        retirement_deadline,
    )
}

fn path(bytes: &[u8]) -> &Path {
    Path::new(OsStr::from_bytes(bytes))
}

fn unchanged(
    executable: &evidence::process::file::Identity,
    title: &evidence::title::Identity,
) -> Result<(), Error> {
    if evidence::process::file::Identity::read(path(&executable.path))? != *executable
        || evidence::title::Identity::read(path(title.path()))? != *title
    {
        return Err(Error::Invalid("executable or title custody changed"));
    }
    Ok(())
}

fn check(
    cli: &protocol::Identity,
    runner: &protocol::Identity,
    file: &evidence::process::file::Identity,
    executable: &evidence::process::file::Identity,
    title: &evidence::title::Identity,
) -> Result<(), Error> {
    if native::observe(cli.pid)? != *cli || native::observe(runner.pid)? != *runner {
        return Err(Error::Invalid("current native process identity changed"));
    }
    unchanged(executable, title)?;
    if evidence::process::file::Identity::read(path(&runner.path))? != *file {
        return Err(Error::Invalid("runner file custody changed"));
    }
    if native::observe(cli.pid)? != *cli || native::observe(runner.pid)? != *runner {
        return Err(Error::Invalid("identity changed across file custody check"));
    }
    Ok(())
}

fn bind(
    receipt: &evidence::runner::Receipt,
    cli: &protocol::Identity,
    runner: &protocol::Identity,
) -> Result<(), Error> {
    let mut digest = Sha256::new();
    digest.update(b"rawr.phase9.runner-executable-path/v1");
    digest.update((runner.path.len() as u64).to_le_bytes());
    digest.update(&runner.path);
    let digest: [u8; 32] = digest.finalize().into();
    if cli.pid != receipt.cli.pid
        || runner.pid != receipt.runner.pid
        || runner.parent != cli.pid
        || runner.group != runner.pid
        || runner.uid != receipt.runner.uid
        || digest != receipt.executable_path_digest
    {
        return Err(Error::Invalid(
            "CLI/runner/native/executable receipt binding",
        ));
    }
    Ok(())
}

fn receive(channel: &mut protocol::Channel, deadline: u64) -> Result<Command, Error> {
    loop {
        protocol::before(deadline)?;
        if let Some(command) = channel.poll()? {
            return Ok(command);
        }
        if channel.eof() {
            return Err(Error::Invalid("parent protocol EOF before terminal state"));
        }
        std::thread::sleep(protocol::POLL);
    }
}
fn emit(channel: &mut protocol::Channel, event: Event, deadline: u64) -> Result<(), Error> {
    channel.send(event)?;
    while !channel.idle() {
        protocol::before(deadline)?;
        channel.flush()?;
        std::thread::sleep(protocol::POLL);
    }
    Ok(())
}
fn window_error(error: darwin::Error) -> Error {
    Error::Observation(error.to_string().into_boxed_str())
}
