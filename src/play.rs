//! A universe runner that serves `?play` (#515). What every snippet needs
//! (the runner's bindings decoded and installed in an engine, its runtime
//! bound) is the same for every snippet, so the runner does it once and
//! then [`serve`]s. Each snippet runs in a fork of that prepared process:
//! its own copy of the engine, its globals and its native values, so it
//! pays only for its own compile and run, and one snippet never sees
//! another's state.
//!
//! A request is answered by two forks. The first reads the request, takes
//! the snippet's memory limit and forks the second, which runs the snippet
//! with its output on pipes. The first collects that output, stops the
//! second at the time limit, and answers with how it ended. The server
//! itself only accepts and forks, and stays single-threaded, so every fork
//! is a whole copy of it.

use std::ffi::{c_int, OsString};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::{Duration, Instant};

/// The variable naming the socket a universe runner serves on.
pub const SOCKET: &str = "RAWR_PLAY_SOCKET";
/// The line a server writes on stdout once it accepts snippets.
pub const READY: &str = "ready";

/// The status of a snippet stopped at its time limit, as a Rust program
/// that ended in an error exits.
const LIMIT_STATUS: i32 = 1;
/// The status of a snippet that ended by a signal (an abort, as an
/// allocation past the memory limit aborts), as a Rust panic exits.
const SIGNAL_STATUS: i32 = 101;
/// The bound on one field of a request or an answer.
const FIELD_BYTES: u64 = 256 * 1024 * 1024;

/// One snippet to run.
#[derive(Clone, Debug)]
pub struct Request {
    /// The crate root's source.
    pub source: String,
    /// Variables set for this run only.
    pub env: Vec<(OsString, OsString)>,
    /// The wall-clock budget, from the fork to the end of the run.
    pub time: Duration,
    /// The address space the run may have (`RLIMIT_AS`, Linux only).
    pub memory: Option<u64>,
}

/// How a run ended, and what it wrote.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Ran {
    /// The process's exit status.
    pub status: i32,
    /// Whether the time limit stopped it.
    pub limit: bool,
    /// Everything it wrote to stdout.
    pub stdout: Vec<u8>,
    /// Everything it wrote to stderr.
    pub stderr: Vec<u8>,
}

/// Run `request` on the server listening at `socket`.
pub fn run(socket: &Path, request: &Request) -> io::Result<Ran> {
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(&request.encode())?;
    stream.shutdown(std::net::Shutdown::Write)?;
    // The answer comes once the run ends, at its time limit at the latest.
    stream.set_read_timeout(Some(request.time + Duration::from_secs(10)))?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer)?;
    Ran::decode(&answer)
}

/// Listen at `socket`, write [`READY`] on stdout, and answer each request
/// with a fork that calls `run` with the snippet's source, as a fresh
/// process would. `run` ends its process (one that returns ends it as a
/// panic does). Returns `Ok` when stdin closes:
/// whoever started the server is gone.
pub fn serve(socket: &Path, mut run: impl FnMut(String)) -> io::Result<()> {
    match std::fs::remove_file(socket) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let listener = UnixListener::bind(socket)?;
    {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{READY}")?;
        stdout.flush()?;
    }
    loop {
        let mut fds = [
            sys::PollFd {
                fd: listener.as_raw_fd(),
                events: sys::POLLIN,
                revents: 0,
            },
            sys::PollFd {
                fd: 0,
                events: sys::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `fds` is a valid array of two pollfd records for the call.
        if unsafe { sys::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if fds[1].revents != 0 {
            return Ok(());
        }
        if fds[0].revents != 0 {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            match fork()? {
                Fork::Child => {
                    drop(listener);
                    answer(stream, &mut run);
                }
                Fork::Parent(_) => drop(stream),
            }
        }
        // Answers that ended since the last request.
        // SAFETY: waitpid writes the status of a child of this process.
        while unsafe { sys::waitpid(-1, &mut 0, sys::WNOHANG) } > 0 {}
    }
}

/// The first fork: read the request, run it in a second fork, answer.
fn answer(mut stream: UnixStream, run: &mut impl FnMut(String)) -> ! {
    let answered = respond(&mut stream, run).and_then(|ran| stream.write_all(&ran.encode()));
    exit(i32::from(answered.is_err()))
}

fn respond(stream: &mut UnixStream, run: &mut impl FnMut(String)) -> io::Result<Ran> {
    let request = Request::read(stream)?;
    // Both forks hold the limit: this one holds the run's output.
    if let Some(bytes) = request.memory {
        limit_memory(bytes)?;
    }
    let (stdout, stdout_writer) = io::pipe()?;
    let (stderr, stderr_writer) = io::pipe()?;
    let null = std::fs::File::open("/dev/null")?;
    let child = match fork()? {
        Fork::Child => {
            for (from, to) in [
                (null.as_raw_fd(), 0),
                (stdout_writer.as_raw_fd(), 1),
                (stderr_writer.as_raw_fd(), 2),
            ] {
                // SAFETY: both are open descriptors of this process.
                if unsafe { sys::dup2(from, to) } < 0 {
                    exit(SIGNAL_STATUS);
                }
            }
            drop((stdout, stderr, stdout_writer, stderr_writer, null));
            for (name, value) in request.env {
                // SAFETY: this fork has one thread, and nothing reads the
                // environment while it is set.
                unsafe { std::env::set_var(name, value) };
            }
            run(request.source);
            exit(SIGNAL_STATUS)
        }
        Fork::Parent(child) => child,
    };
    drop((stdout_writer, stderr_writer, null));
    let stdout = std::thread::spawn(move || read_all(stdout));
    let stderr = std::thread::spawn(move || read_all(stderr));
    let (status, limit) = wait(child, Instant::now() + request.time)?;
    Ok(Ran {
        status,
        limit,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Wait for `child` until `deadline`, then stop it: its status, and
/// whether the deadline stopped it.
fn wait(child: c_int, deadline: Instant) -> io::Result<(i32, bool)> {
    let mut status = 0;
    loop {
        // SAFETY: waitpid writes the status of this process's own child.
        match unsafe { sys::waitpid(child, &mut status, sys::WNOHANG) } {
            0 if Instant::now() >= deadline => {
                // SAFETY: `child` is this process's own unreaped child, so
                // its pid names no other process.
                unsafe { sys::kill(child, sys::SIGKILL) };
                // SAFETY: as above.
                while unsafe { sys::waitpid(child, &mut status, 0) } < 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                }
                return Ok((LIMIT_STATUS, true));
            }
            0 => std::thread::sleep(Duration::from_millis(5)),
            pid if pid > 0 => {
                // As `WIFEXITED` and `WEXITSTATUS` read it.
                let exited = status & 0x7f == 0;
                return Ok((
                    if exited {
                        (status >> 8) & 0xff
                    } else {
                        SIGNAL_STATUS
                    },
                    false,
                ));
            }
            _ => {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }
}

fn read_all(mut pipe: io::PipeReader) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = pipe.read_to_end(&mut bytes);
    bytes
}

/// End this fork at once: the server's exit handlers are not its own.
fn exit(status: i32) -> ! {
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    // SAFETY: _exit ends the process and returns nothing.
    unsafe { sys::_exit(status) }
}

enum Fork {
    Child,
    Parent(c_int),
}

fn fork() -> io::Result<Fork> {
    // SAFETY: the caller has one thread, so the child is a whole copy of it.
    match unsafe { sys::fork() } {
        0 => Ok(Fork::Child),
        pid if pid > 0 => Ok(Fork::Parent(pid)),
        _ => Err(io::Error::last_os_error()),
    }
}

#[cfg(target_os = "linux")]
fn limit_memory(bytes: u64) -> io::Result<()> {
    let limit = sys::Rlimit {
        cur: bytes,
        max: bytes,
    };
    // SAFETY: setrlimit reads a valid rlimit for this process.
    if unsafe { sys::setrlimit(sys::RLIMIT_AS, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// macOS does not enforce `RLIMIT_AS`; the limit holds on Linux.
#[cfg(not(target_os = "linux"))]
fn limit_memory(_bytes: u64) -> io::Result<()> {
    Ok(())
}

impl Request {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let millis = u64::try_from(self.time.as_millis()).unwrap_or(u64::MAX);
        bytes.extend_from_slice(&millis.to_le_bytes());
        bytes.extend_from_slice(&self.memory.unwrap_or(0).to_le_bytes());
        bytes.extend_from_slice(&(self.env.len() as u64).to_le_bytes());
        for (name, value) in &self.env {
            field(&mut bytes, name.as_bytes());
            field(&mut bytes, value.as_bytes());
        }
        field(&mut bytes, self.source.as_bytes());
        bytes
    }

    fn read(stream: &mut UnixStream) -> io::Result<Self> {
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes)?;
        let mut input = bytes.as_slice();
        let time = Duration::from_millis(number(&mut input)?);
        let memory = Some(number(&mut input)?).filter(|&bytes| bytes != 0);
        let count = number(&mut input)?;
        let mut env = Vec::new();
        for _ in 0..count {
            let name = OsString::from_vec(take(&mut input)?.to_vec());
            let value = OsString::from_vec(take(&mut input)?.to_vec());
            env.push((name, value));
        }
        let source = String::from_utf8(take(&mut input)?.to_vec())
            .map_err(|_| io::Error::other("a snippet is UTF-8"))?;
        Ok(Self {
            source,
            env,
            time,
            memory,
        })
    }
}

impl Ran {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.status.to_le_bytes());
        bytes.push(u8::from(self.limit));
        field(&mut bytes, &self.stdout);
        field(&mut bytes, &self.stderr);
        bytes
    }

    fn decode(mut input: &[u8]) -> io::Result<Self> {
        let (status, rest) = input
            .split_first_chunk::<4>()
            .ok_or_else(|| io::Error::other("the run ended before it answered"))?;
        let status = i32::from_le_bytes(*status);
        let (&limit, rest) = rest
            .split_first()
            .ok_or_else(|| io::Error::other("the run ended before it answered"))?;
        input = rest;
        let stdout = take(&mut input)?.to_vec();
        let stderr = take(&mut input)?.to_vec();
        Ok(Self {
            status,
            limit: limit != 0,
            stdout,
            stderr,
        })
    }
}

fn field(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value);
}

fn number(input: &mut &[u8]) -> io::Result<u64> {
    let (value, rest) = input
        .split_first_chunk::<8>()
        .ok_or_else(|| io::Error::other("a request ended early"))?;
    *input = rest;
    Ok(u64::from_le_bytes(*value))
}

fn take<'a>(input: &mut &'a [u8]) -> io::Result<&'a [u8]> {
    let length = number(input)?;
    if length > FIELD_BYTES || length > input.len() as u64 {
        return Err(io::Error::other("a request ended early"));
    }
    let (value, rest) = input.split_at(length as usize);
    *input = rest;
    Ok(value)
}

/// The few POSIX calls a fork server makes, with the values Linux and
/// macOS share.
mod sys {
    use std::ffi::{c_int, c_short};

    #[repr(C)]
    pub struct PollFd {
        pub fd: c_int,
        pub events: c_short,
        pub revents: c_short,
    }

    #[cfg(target_os = "linux")]
    pub type Nfds = std::ffi::c_ulong;
    #[cfg(not(target_os = "linux"))]
    pub type Nfds = std::ffi::c_uint;

    pub const POLLIN: c_short = 0x1;
    pub const WNOHANG: c_int = 1;
    pub const SIGKILL: c_int = 9;

    #[cfg(target_os = "linux")]
    #[repr(C)]
    pub struct Rlimit {
        pub cur: u64,
        pub max: u64,
    }

    #[cfg(target_os = "linux")]
    pub const RLIMIT_AS: c_int = 9;

    unsafe extern "C" {
        pub fn fork() -> c_int;
        pub fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
        pub fn kill(pid: c_int, signal: c_int) -> c_int;
        pub fn dup2(from: c_int, to: c_int) -> c_int;
        pub fn poll(fds: *mut PollFd, count: Nfds, timeout: c_int) -> c_int;
        pub fn _exit(status: c_int) -> !;
        #[cfg(target_os = "linux")]
        pub fn setrlimit(resource: c_int, limit: *const Rlimit) -> c_int;
    }
}

#[cfg(test)]
mod tests {
    use super::{Ran, Request};
    use std::time::Duration;

    #[test]
    fn a_request_and_an_answer_round_trip() {
        let request = Request {
            source: "fn main() {}\n".into(),
            env: vec![("PLAY_FRAME".into(), "/tmp/frame.png".into())],
            time: Duration::from_millis(1500),
            memory: Some(1 << 30),
        };
        let mut stream = std::os::unix::net::UnixStream::pair().unwrap();
        std::io::Write::write_all(&mut stream.0, &request.encode()).unwrap();
        drop(stream.0);
        let read = Request::read(&mut stream.1).unwrap();
        assert_eq!(
            (read.source, read.env, read.time, read.memory),
            (request.source, request.env, request.time, request.memory)
        );
        let ran = Ran {
            status: 101,
            limit: false,
            stdout: b"out".to_vec(),
            stderr: b"err".to_vec(),
        };
        assert_eq!(Ran::decode(&ran.encode()).unwrap(), ran);
        assert!(Ran::decode(&[]).is_err());
    }
}
