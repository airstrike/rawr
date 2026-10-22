//! Closed, launch-bound transport shared only by the private harness and observer.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;

use rawr_import::metrics::evidence;
use serde::{Deserialize, Serialize};

// Reuse the existing qualification output bound for each owned wire buffer.
pub(crate) const OUTPUT_BYTES: usize = 64 * 1024;
pub(crate) const POLL: std::time::Duration = std::time::Duration::from_millis(2);
pub(crate) const GRACE_NS: u64 = 10_000_000_000;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct Window {
    pub(crate) number: i64,
    pub(crate) pid: i64,
    pub(crate) title: Box<str>,
    pub(crate) layer: i64,
    pub(crate) alpha: f64,
    pub(crate) on_screen: bool,
    pub(crate) bounds: Bounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct Bounds {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// Complete non-snapshot native observation; never a vnode/exec-generation claim.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct Identity {
    pub(crate) pid: u32,
    pub(crate) parent: u32,
    pub(crate) group: u32,
    pub(crate) uid: u32,
    pub(crate) birth_seconds: u64,
    pub(crate) birth_microseconds: u64,
    pub(crate) path: Box<[u8]>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Resources {
    pub(crate) user_ns: u64,
    pub(crate) system_ns: u64,
    pub(crate) peak_rss_bytes: u64,
    pub(crate) samples: u64,
}

/// Observer sampling is reported separately and is never deducted from a workload.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Costs {
    pub(crate) sampled: Resources,
    pub(crate) sampler_children: u64,
    pub(crate) first_sample_ns: u64,
    pub(crate) last_sample_ns: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Visible {
    pub(crate) window: Window,
    pub(crate) observed_ns: u64,
    pub(crate) cli: Identity,
    pub(crate) runner: Identity,
    pub(crate) executable: evidence::process::file::Identity,
    pub(crate) resources: Resources,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum Command {
    Initialize {
        observer: evidence::process::file::Identity,
        executable: evidence::process::file::Identity,
        title: evidence::title::Identity,
    },
    Start {
        origin_ns: u64,
        deadline_ns: u64,
    },
    Bind(evidence::runner::Receipt),
    Close,
    Finish,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum Event {
    Ready,
    Started,
    Visible(Visible),
    Pressed { observed_ns: u64 },
    Disappeared { observed_ns: u64 },
    Finished { costs: Costs },
    Failed { reason: Box<str> },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Frame<T> {
    version: u8,
    launch: [u8; 32],
    build: [u8; 32],
    sequence: u8,
    body: T,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("qualification observer I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("qualification observer wire JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("qualification observer refused: {0}")]
    Invalid(&'static str),
    #[error(
        "qualification observer native call {operation} failed: errno {errno}, result {result}"
    )]
    Native {
        operation: &'static str,
        errno: i32,
        result: i32,
    },
    #[error("qualification observer failed: {0}")]
    Observation(Box<str>),
    #[error(transparent)]
    Evidence(#[from] evidence::Error),
}

/// One bounded pending write and one bounded partial frame; no reader threads.
pub(crate) struct Channel {
    stream: UnixStream,
    launch: [u8; 32],
    build: [u8; 32],
    sent: u8,
    received: u8,
    pending: Vec<u8>,
    written: usize,
    header: [u8; 4],
    header_read: usize,
    payload: Vec<u8>,
    payload_read: usize,
    eof: bool,
}

impl Channel {
    pub(crate) fn transform(
        stream: UnixStream,
        launch: [u8; 32],
        build: [u8; 32],
    ) -> Result<Self, Error> {
        if launch == [0; 32] || build == [0; 32] {
            return Err(Error::Invalid("missing launch/build association"));
        }
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            launch,
            build,
            sent: 0,
            received: 0,
            pending: Vec::new(),
            written: 0,
            header: [0; 4],
            header_read: 0,
            payload: Vec::new(),
            payload_read: 0,
            eof: false,
        })
    }

    pub(crate) fn send(&mut self, body: impl Serialize) -> Result<(), Error> {
        if !self.pending.is_empty() || self.eof {
            return Err(Error::Invalid("overlapping or post-EOF observer command"));
        }
        let sequence = self
            .sent
            .checked_add(1)
            .ok_or(Error::Invalid("observer sequence overflow"))?;
        let frame = Frame {
            version: 1,
            launch: self.launch,
            build: self.build,
            sequence,
            body,
        };
        let mut buffer = Buffer(Vec::new());
        serde_json::to_writer(&mut buffer, &frame)?;
        let length =
            u32::try_from(buffer.0.len()).map_err(|_| Error::Invalid("observer frame length"))?;
        self.pending
            .try_reserve_exact(4 + buffer.0.len())
            .map_err(|_| Error::Invalid("observer pending allocation"))?;
        self.pending.extend_from_slice(&length.to_be_bytes());
        self.pending.extend_from_slice(&buffer.0);
        self.sent = sequence;
        self.flush()
    }

    pub(crate) fn flush(&mut self) -> Result<(), Error> {
        if self.pending.is_empty() {
            return Ok(());
        }
        match self.stream.write(&self.pending[self.written..]) {
            Ok(0) => return Err(Error::Io(std::io::ErrorKind::WriteZero.into())),
            Ok(count) => self.written += count,
            Err(error) if retry(&error) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        if self.written == self.pending.len() {
            self.pending.clear();
            self.written = 0;
        }
        Ok(())
    }

    pub(crate) fn idle(&self) -> bool {
        self.pending.is_empty()
    }
    pub(crate) fn eof(&self) -> bool {
        self.eof
    }

    /// Failure-only drain: abandon invalid framing and pending writes, retain
    /// actual EOF without allowing the failed channel to regain acceptance.
    pub(crate) fn abandon(&mut self) -> Result<bool, Error> {
        self.pending.clear();
        self.payload.clear();
        if self.eof {
            return Ok(true);
        }
        let mut bytes = [0; 4096];
        match self.stream.read(&mut bytes) {
            Ok(0) => {
                self.eof = true;
                Ok(true)
            }
            Ok(_) => Ok(false),
            Err(error) if retry(&error) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn poll<T: serde::de::DeserializeOwned + Serialize>(
        &mut self,
    ) -> Result<Option<T>, Error> {
        self.flush()?;
        if self.eof {
            return Ok(None);
        }
        if self.header_read < 4 {
            match self.stream.read(&mut self.header[self.header_read..]) {
                Ok(0) if self.header_read == 0 => {
                    self.eof = true;
                    return Ok(None);
                }
                Ok(0) => return Err(Error::Invalid("truncated observer header")),
                Ok(count) => self.header_read += count,
                Err(error) if retry(&error) => return Ok(None),
                Err(error) => return Err(error.into()),
            }
            if self.header_read < 4 {
                return Ok(None);
            }
            let length = u32::from_be_bytes(self.header) as usize;
            if length == 0 || length > OUTPUT_BYTES {
                return Err(Error::Invalid("observer frame bound"));
            }
            self.payload
                .try_reserve_exact(length)
                .map_err(|_| Error::Invalid("observer buffer allocation"))?;
            self.payload.resize(length, 0);
        }
        match self.stream.read(&mut self.payload[self.payload_read..]) {
            Ok(0) => return Err(Error::Invalid("truncated observer payload")),
            Ok(count) => self.payload_read += count,
            Err(error) if retry(&error) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        if self.payload_read != self.payload.len() {
            return Ok(None);
        }
        let frame: Frame<T> = serde_json::from_slice(&self.payload)?;
        // The typed sender emits this exact encoding. Compare without another
        // buffer so ignored nested fields and alternate encodings cannot enter.
        let mut comparison = Comparison {
            bytes: &self.payload,
            offset: 0,
        };
        serde_json::to_writer(&mut comparison, &frame)?;
        if comparison.offset != self.payload.len() {
            return Err(Error::Invalid("noncanonical observer frame"));
        }
        let sequence = self
            .received
            .checked_add(1)
            .ok_or(Error::Invalid("observer sequence overflow"))?;
        if frame.version != 1
            || frame.launch != self.launch
            || frame.build != self.build
            || frame.sequence != sequence
        {
            return Err(Error::Invalid("observer launch/build/sequence mismatch"));
        }
        self.received = sequence;
        self.header_read = 0;
        self.payload.clear();
        self.payload_read = 0;
        Ok(Some(frame.body))
    }
}

struct Comparison<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl std::io::Write for Comparison<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let end = self
            .offset
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("observer comparison overflow"))?;
        if self.bytes.get(self.offset..end) != Some(bytes) {
            return Err(std::io::Error::other("noncanonical observer frame"));
        }
        self.offset = end;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Buffer(Vec<u8>);
impl std::io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > OUTPUT_BYTES {
            return Err(std::io::Error::other("observer frame bound"));
        }
        self.0
            .try_reserve_exact(bytes.len())
            .map_err(std::io::Error::other)?;
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn now() -> Result<u64, Error> {
    rawr_import::metrics::monotonic_ns().ok_or(Error::Invalid("monotonic clock unavailable"))
}
pub(crate) fn before(deadline: u64) -> Result<(), Error> {
    if now()? >= deadline {
        Err(Error::Invalid("observer phase deadline exceeded"))
    } else {
        Ok(())
    }
}
fn retry(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;

    #[test]
    fn partial_frames_return_without_waiting_and_preserve_association() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let mut channel = super::Channel::transform(reader, [1; 32], [2; 32]).unwrap();
        let bytes = serde_json::to_vec(&super::Frame {
            version: 1,
            launch: [1; 32],
            build: [2; 32],
            sequence: 1,
            body: super::Event::Ready,
        })
        .unwrap();
        writer
            .write_all(&(bytes.len() as u32).to_be_bytes()[..2])
            .unwrap();
        assert!(channel.poll::<super::Event>().unwrap().is_none());
        writer
            .write_all(&(bytes.len() as u32).to_be_bytes()[2..])
            .unwrap();
        writer.write_all(&bytes[..1]).unwrap();
        assert!(channel.poll::<super::Event>().unwrap().is_none());
        writer.write_all(&bytes[1..]).unwrap();
        assert!(matches!(channel.poll().unwrap(), Some(super::Event::Ready)));
        drop(writer);
        assert!(channel.poll::<super::Event>().unwrap().is_none());
        assert!(channel.eof());
    }

    #[test]
    fn stale_launch_sequence_and_oversized_frames_reject() {
        for (launch, sequence) in [([9; 32], 1), ([1; 32], 2)] {
            let (mut writer, reader) = UnixStream::pair().unwrap();
            let mut channel = super::Channel::transform(reader, [1; 32], [2; 32]).unwrap();
            let bytes = serde_json::to_vec(&super::Frame {
                version: 1,
                launch,
                build: [2; 32],
                sequence,
                body: super::Event::Ready,
            })
            .unwrap();
            writer
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .unwrap();
            writer.write_all(&bytes).unwrap();
            assert!(channel.poll::<super::Event>().is_err());
            drop(writer);
            assert!(channel.abandon().unwrap());
        }
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let mut channel = super::Channel::transform(reader, [1; 32], [2; 32]).unwrap();
        writer
            .write_all(&((super::OUTPUT_BYTES + 1) as u32).to_be_bytes())
            .unwrap();
        assert!(channel.poll::<super::Event>().is_err());
        drop(writer);
        assert!(channel.abandon().unwrap());
    }

    #[test]
    fn nested_unknown_fields_cannot_be_dropped_by_deserialization() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let mut channel = super::Channel::transform(reader, [1; 32], [2; 32]).unwrap();
        let frame = super::Frame {
            version: 1,
            launch: [1; 32],
            build: [2; 32],
            sequence: 1,
            body: super::Event::Pressed { observed_ns: 1 },
        };
        let bytes = serde_json::to_string(&frame)
            .unwrap()
            .replace("\"observed_ns\":1", "\"observed_ns\":1,\"unknown\":true");
        writer
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        writer.write_all(bytes.as_bytes()).unwrap();
        assert!(channel.poll::<super::Event>().is_err());
    }

    #[test]
    fn failed_partial_frame_can_still_record_actual_eof_without_acceptance() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let mut channel = super::Channel::transform(reader, [1; 32], [2; 32]).unwrap();
        writer.write_all(&11_u32.to_be_bytes()).unwrap();
        writer.write_all(b"bad").unwrap();
        drop(writer);
        assert!(channel.poll::<super::Event>().unwrap().is_none());
        assert!(channel.poll::<super::Event>().is_err());
        assert!(channel.abandon().unwrap());
    }
}
