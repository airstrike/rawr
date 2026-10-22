//! The existing qualification ps measurements, contained in the observer group.

use std::io::Read as _;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, ExitStatus, Stdio};

use super::protocol::{self, Costs, Error, Resources};

pub(super) struct Sampling {
    product: Resources,
    costs: Costs,
    pending: Option<Pending>,
}

struct Pending {
    child: Child,
    stdout: UnixStream,
    stderr: UnixStream,
    output: Vec<u8>,
    diagnostic: Vec<u8>,
    out_eof: bool,
    err_eof: bool,
    status: Option<ExitStatus>,
    product: Option<[u32; 2]>,
    deadline: u64,
}

impl Sampling {
    pub(super) fn new() -> Self {
        Self {
            product: Resources::default(),
            costs: Costs {
                sampled: Resources::default(),
                sampler_children: 0,
                first_sample_ns: 0,
                last_sample_ns: 0,
            },
            pending: None,
        }
    }

    pub(super) fn product(&self) -> Resources {
        self.product.clone()
    }
    pub(super) fn costs(&self) -> Costs {
        self.costs.clone()
    }

    /// Start exactly one contained ps child; visibility scanning owns the cadence.
    pub(super) fn start(&mut self, product: Option<[u32; 2]>, deadline: u64) -> Result<(), Error> {
        if self.pending.is_some() {
            return Err(Error::Invalid("overlapping ps samples"));
        }
        protocol::before(deadline)?;
        let observer = std::process::id();
        let selection = match product {
            Some([cli, runner]) => format!("{cli},{runner},{observer}"),
            None => observer.to_string(),
        };
        let (stdout, out) = UnixStream::pair()?;
        let (stderr, err) = UnixStream::pair()?;
        stdout.set_nonblocking(true)?;
        stderr.set_nonblocking(true)?;
        let out: OwnedFd = out.into();
        let err: OwnedFd = err.into();
        let next_sampler = self
            .costs
            .sampler_children
            .checked_add(1)
            .filter(|count| *count <= 1_000_000)
            .ok_or(Error::Invalid("existing observation counter bound"))?;
        let child = Command::new("/bin/ps")
            .args(["-p", &selection, "-o", "pid=,rss=,utime=,stime="])
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()?;
        self.costs.sampler_children = next_sampler;
        self.pending = Some(Pending {
            child,
            stdout,
            stderr,
            output: Vec::new(),
            diagnostic: Vec::new(),
            out_eof: false,
            err_eof: false,
            status: None,
            product,
            deadline,
        });
        Ok(())
    }

    pub(super) fn idle(&self) -> bool {
        self.pending.is_none()
    }

    /// Drain at most one bounded chunk from each pipe and inspect exit once.
    pub(super) fn poll(&mut self) -> Result<(), Error> {
        let Some(pending) = self.pending.as_mut() else {
            return Ok(());
        };
        if !pending.poll()? {
            return Ok(());
        }
        let pending = self
            .pending
            .take()
            .ok_or(Error::Invalid("completed ps sample disappeared"))?;
        self.record(pending.product, &pending.output)
            .map_err(|source| {
                Error::Observation(
                    format!(
                        "{source}; ps pid={}; reap={:?}; stdout_eof={}; stderr_eof={}",
                        pending.child.id(),
                        pending.status,
                        pending.out_eof,
                        pending.err_eof
                    )
                    .into_boxed_str(),
                )
            })
    }

    /// Used only outside first-visibility scanning or after its timestamp is sealed.
    pub(super) fn finish(&mut self) -> Result<(), Error> {
        while !self.idle() {
            self.poll()?;
            if !self.idle() {
                std::thread::sleep(protocol::POLL);
            }
        }
        Ok(())
    }

    pub(super) fn sample(&mut self, product: Option<[u32; 2]>, deadline: u64) -> Result<(), Error> {
        self.start(product, deadline)?;
        self.finish()
    }

    /// Every observer failure passes through this owner before terminal reporting.
    pub(super) fn retire_failed(&mut self, source: Error) -> Error {
        let Some(mut pending) = self.pending.take() else {
            return source;
        };
        let pid = pending.child.id();
        let signal = match pending.child.try_wait() {
            Ok(Some(status)) => {
                pending.status = Some(status);
                "already reaped; not signaled".to_owned()
            }
            Ok(None) => format!("{:?}", pending.child.kill()),
            Err(error) => format!("ownership uncertain: {error}; not signaled"),
        };
        let cleanup_deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut wait_error = None;
        while pending.status.is_none() && std::time::Instant::now() < cleanup_deadline {
            match pending.child.try_wait() {
                Ok(status) => pending.status = status,
                Err(error) => {
                    wait_error = Some(error);
                    break;
                }
            }
            if pending.status.is_none() {
                std::thread::sleep(protocol::POLL);
            }
        }
        Error::Observation(format!("{source}; ps pid={pid}; signal={signal}; reap={:?}; wait_error={wait_error:?}; stdout_eof={}; stderr_eof={}; parent group cancellation may interrupt cleanup and leaves its outcome unproved", pending.status, pending.out_eof, pending.err_eof).into_boxed_str())
    }

    fn record(&mut self, product: Option<[u32; 2]>, output: &[u8]) -> Result<(), Error> {
        let observer = std::process::id();
        let document =
            std::str::from_utf8(output).map_err(|_| Error::Invalid("non-UTF8 ps output"))?;
        let mut observed_product = Resources::default();
        let mut observed_observer = None;
        let mut seen = Vec::with_capacity(3);
        for line in document.lines() {
            let mut fields = line.split_ascii_whitespace();
            let pid = fields
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .ok_or(Error::Invalid("ps PID"))?;
            if seen.contains(&pid) || seen.len() == 3 {
                return Err(Error::Invalid("duplicate or excessive ps row"));
            }
            seen.push(pid);
            let rss = fields
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .and_then(|value| value.checked_mul(1024))
                .ok_or(Error::Invalid("ps RSS"))?;
            let user = time(fields.next().ok_or(Error::Invalid("ps user time"))?)?;
            let system = time(fields.next().ok_or(Error::Invalid("ps system time"))?)?;
            if fields.next().is_some() {
                return Err(Error::Invalid("extra ps fields"));
            }
            if pid == observer {
                observed_observer = Some(Resources {
                    user_ns: user,
                    system_ns: system,
                    peak_rss_bytes: rss,
                    samples: 1,
                });
            } else if product.is_some_and(|pids| pids.contains(&pid)) {
                observed_product.user_ns = observed_product
                    .user_ns
                    .checked_add(user)
                    .ok_or(Error::Invalid("ps CPU sum"))?;
                observed_product.system_ns = observed_product
                    .system_ns
                    .checked_add(system)
                    .ok_or(Error::Invalid("ps CPU sum"))?;
                observed_product.peak_rss_bytes = observed_product
                    .peak_rss_bytes
                    .checked_add(rss)
                    .ok_or(Error::Invalid("ps RSS sum"))?;
                observed_product.samples = 1;
            } else {
                return Err(Error::Invalid("unselected ps row"));
            }
        }
        let observer = observed_observer.ok_or(Error::Invalid("ps omitted the observer"))?;
        merge(&mut self.costs.sampled, observer)?;
        if observed_product.samples != 0 {
            merge(&mut self.product, observed_product)?;
        }
        let now = protocol::now()?;
        if self.costs.first_sample_ns == 0 {
            self.costs.first_sample_ns = now;
        }
        self.costs.last_sample_ns = now;
        Ok(())
    }
}

impl Pending {
    fn poll(&mut self) -> Result<bool, Error> {
        protocol::before(self.deadline)?;
        if !self.out_eof {
            self.out_eof = drain(&mut self.stdout, &mut self.output)?;
        }
        if !self.err_eof {
            self.err_eof = drain(&mut self.stderr, &mut self.diagnostic)?;
        }
        if self.status.is_none() {
            self.status = self.child.try_wait()?;
        }
        if self.out_eof
            && self.err_eof
            && let Some(status) = self.status
        {
            if !status.success() {
                return Err(Error::Observation(
                    String::from_utf8_lossy(&self.diagnostic)
                        .into_owned()
                        .into_boxed_str(),
                ));
            }
            return Ok(true);
        }
        Ok(false)
    }
}

fn drain(stream: &mut UnixStream, output: &mut Vec<u8>) -> Result<bool, Error> {
    let mut buffer = [0; 4096];
    match stream.read(&mut buffer) {
        Ok(0) => Ok(true),
        Ok(count) => {
            if output.len().saturating_add(count) > protocol::OUTPUT_BYTES {
                return Err(Error::Invalid("ps output bound"));
            }
            output
                .try_reserve_exact(count)
                .map_err(|_| Error::Invalid("ps output allocation"))?;
            output.extend_from_slice(&buffer[..count]);
            Ok(false)
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

fn merge(retained: &mut Resources, sample: Resources) -> Result<(), Error> {
    retained.user_ns = retained.user_ns.max(sample.user_ns);
    retained.system_ns = retained.system_ns.max(sample.system_ns);
    retained.peak_rss_bytes = retained.peak_rss_bytes.max(sample.peak_rss_bytes);
    retained.samples = retained
        .samples
        .checked_add(1)
        .filter(|count| *count <= 1_000_000)
        .ok_or(Error::Invalid("existing sample counter bound"))?;
    Ok(())
}

fn time(value: &str) -> Result<u64, Error> {
    let mut fields = value.rsplit(':');
    let seconds = fields.next().ok_or(Error::Invalid("ps seconds"))?;
    let minutes = fields.next().unwrap_or("0");
    let hours = fields.next().unwrap_or("0");
    if fields.next().is_some() {
        return Err(Error::Invalid("ps time fields"));
    }
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    if fraction.len() > 9 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Invalid("ps fraction"));
    }
    let integer = |text: &str| {
        text.parse::<u64>()
            .map_err(|_| Error::Invalid("ps time integer"))
    };
    let base = integer(hours)?
        .checked_mul(3600)
        .and_then(|hours| integer(minutes).ok()?.checked_mul(60)?.checked_add(hours))
        .and_then(|minutes| minutes.checked_add(integer(whole).ok()?))
        .and_then(|seconds| seconds.checked_mul(1_000_000_000))
        .ok_or(Error::Invalid("ps time overflow"))?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        integer(fraction)?
            .checked_mul(10_u64.pow((9 - fraction.len()) as u32))
            .ok_or(Error::Invalid("ps time overflow"))?
    };
    base.checked_add(fraction)
        .ok_or(Error::Invalid("ps time overflow"))
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;

    #[test]
    fn a_pending_ps_pipe_returns_control_before_data_or_eof() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let mut output = Vec::new();
        assert!(!super::drain(&mut reader, &mut output).unwrap());
        writer.write_all(b"partial").unwrap();
        assert!(!super::drain(&mut reader, &mut output).unwrap());
        assert!(!super::drain(&mut reader, &mut output).unwrap());
        assert_eq!(output, b"partial");
        drop(writer);
        assert!(super::drain(&mut reader, &mut output).unwrap());
    }
}
