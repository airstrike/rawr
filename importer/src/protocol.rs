//! Versioned bounded framing for the dedicated runner socket.

use std::io::{Read, Write};

/// Fixed observations of actual retained child accounts on the existing socket.
pub mod account;

/// Candidate-bound evidence for the distinct resident compile exchange.
pub mod resident;

pub const FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const DIAGNOSTIC_BYTES: usize = 16 * 1024 * 1024;
pub const STREAM_TAIL_BYTES: usize = 1024 * 1024;
pub const VERSION: u32 = 11;
const HEADER_BYTES: usize = 4;
const PREFIX_BYTES: usize = 5;

/// Closed wire message family.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum Tag {
    Authenticate = 1,
    Hello = 2,
    Prepare = 3,
    Prepared = 4,
    Activate = 5,
    Activated = 6,
    Admit = 7,
    Admitted = 8,
    Compile = 9,
    SurfaceMiss = 10,
    Report = 11,
    Shutdown = 12,
    Exited = 13,
    ProfileMessage = 14,
    ProfileReply = 15,
    ProfileRejected = 16,
    Invoke = 17,
    Invoked = 18,
    InvokeRejected = 19,
    Start = 20,
    Started = 21,
    Snapshot = 22,
    SnapshotOutcome = 23,
    WorkProgress = 24,
    HotCommit = 25,
    HotAbort = 26,
    HotAborted = 27,
    /// Exact next key and nonce, followed by a bounded UTF-8 diagnostic.
    /// Pending was consumed but the child proves no installation occurred.
    HotConsumedRejected = 28,
    /// Predecessor key, random nonce and monotonic session sequence (56 bytes).
    HotBegin = 29,
    HotBegan = 30,
    HotCancel = 31,
    HotCancelled = 32,
    /// Early identity followed by bounded filesystem observation bytes.
    HotObserve = 33,
    HotObserved = 34,
    ObserveInitial = 35,
    InitialObserved = 36,
    ResidentCompile = 37,
    ResidentPrepared = 38,
    ResidentMiss = 39,
    ResidentDiagnostic = 40,
    ResidentObsolete = 41,
    ResidentResource = 42,
    RefreshPoll = 43,
    RefreshStatus = 44,
    Account = 45,
    /// The live runner's key; it answers `StateExported`.
    ExportState = 46,
    /// The key, then 1 and the carried State bytes, or 0 when none.
    StateExported = 47,
    /// An admitted runner's key, then carried State bytes to start from.
    SeedState = 48,
    /// The key, then 1 when the State was seeded or 0 when booted.
    StateSeeded = 49,
}

/// Bounded primitive writer used by profile message codecs.
pub struct Writer<'a> {
    output: &'a mut Vec<u8>,
}

/// Bounded primitive reader used by profile message codecs.
pub struct Reader<'a> {
    input: &'a [u8],
}

/// One fully bounded and version-checked frame.
pub struct Frame {
    tag: Tag,
    payload: Box<[u8]>,
}

/// One bounded resumable frame decoder. A timed-out empty read leaves the
/// decoder ready to poll again; callers remain responsible for bounding both
/// the complete work transition and the completion of a started frame.
pub struct Decoder {
    header: [u8; HEADER_BYTES],
    header_read: usize,
    body: Vec<u8>,
    body_read: usize,
}

/// A borrowed frame with progress retained across bounded socket writes.
pub(crate) struct Encoder<'frame> {
    prefix: [u8; HEADER_BYTES + PREFIX_BYTES],
    payload: &'frame [u8],
    written: usize,
}

impl Decoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            header: [0; HEADER_BYTES],
            header_read: 0,
            body: Vec::new(),
            body_read: 0,
        }
    }

    /// Return whether any byte of the current frame has already arrived.
    #[must_use]
    pub const fn has_partial_frame(&self) -> bool {
        self.header_read != 0 || !self.body.is_empty()
    }

    /// Advance one frame without discarding a partial prefix when a bounded
    /// socket poll returns `WouldBlock` or `TimedOut`.
    pub fn receive(&mut self, reader: &mut impl Read) -> Result<Option<Frame>, Error> {
        self.advance(reader, usize::MAX)
    }

    /// Perform at most one read so a trickling peer cannot starve supervision.
    pub(crate) fn receive_step(&mut self, reader: &mut impl Read) -> Result<Option<Frame>, Error> {
        self.advance(reader, 1)
    }

    fn advance(&mut self, reader: &mut impl Read, attempts: usize) -> Result<Option<Frame>, Error> {
        let mut reads = 0;
        while self.header_read < HEADER_BYTES {
            if reads == attempts {
                return Ok(None);
            }
            reads += 1;
            match reader.read(&mut self.header[self.header_read..]) {
                Ok(0) => return Err(Error::Truncated),
                Ok(count) => self.header_read += count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(Error::Read(error)),
            }
        }
        if self.body.is_empty() {
            let length = u32::from_be_bytes(self.header) as usize;
            if length > FRAME_BYTES {
                return Err(Error::Oversized {
                    limit: FRAME_BYTES,
                    actual: length,
                });
            }
            if length < PREFIX_BYTES {
                return Err(Error::Truncated);
            }
            self.body
                .try_reserve_exact(length)
                .map_err(|_| Error::Allocation)?;
            self.body.resize(length, 0);
        }
        while self.body_read < self.body.len() {
            if reads == attempts {
                return Ok(None);
            }
            reads += 1;
            match reader.read(&mut self.body[self.body_read..]) {
                Ok(0) => return Err(Error::Truncated),
                Ok(count) => self.body_read += count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(Error::Read(error)),
            }
        }
        let body = std::mem::take(&mut self.body);
        self.header = [0; HEADER_BYTES];
        self.header_read = 0;
        self.body_read = 0;
        decode(body)
    }
}

impl<'frame> Encoder<'frame> {
    pub(crate) fn frame(frame: &'frame Frame) -> Result<Self, Error> {
        let length = PREFIX_BYTES
            .checked_add(frame.payload.len())
            .ok_or(Error::Oversized {
                limit: FRAME_BYTES,
                actual: usize::MAX,
            })?;
        let length = u32::try_from(length).map_err(|_| Error::Oversized {
            limit: FRAME_BYTES,
            actual: length,
        })?;
        let mut prefix = [0; HEADER_BYTES + PREFIX_BYTES];
        prefix[..4].copy_from_slice(&length.to_be_bytes());
        prefix[4..8].copy_from_slice(&VERSION.to_be_bytes());
        prefix[8] = frame.tag as u8;
        Ok(Self {
            prefix,
            payload: &frame.payload,
            written: 0,
        })
    }

    /// Perform one write or flush, preserving progress after a bounded timeout.
    pub(crate) fn send_step(&mut self, writer: &mut impl Write) -> Result<bool, Error> {
        let remaining = if self.written < self.prefix.len() {
            &self.prefix[self.written..]
        } else {
            &self.payload[self.written - self.prefix.len()..]
        };
        let result = if remaining.is_empty() {
            writer.flush().map(|()| 0)
        } else {
            writer.write(remaining)
        };
        match result {
            Ok(0) if remaining.is_empty() => Ok(true),
            Ok(0) => Err(Error::Write(std::io::ErrorKind::WriteZero.into())),
            Ok(count) => {
                self.written += count;
                Ok(false)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(Error::Write(error)),
        }
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Frame {
    /// Consume payload ownership after validating the protocol frame bound.
    pub fn new(tag: Tag, payload: Box<[u8]>) -> Result<Self, Error> {
        let actual = PREFIX_BYTES
            .checked_add(payload.len())
            .ok_or(Error::Oversized {
                limit: FRAME_BYTES,
                actual: usize::MAX,
            })?;
        if actual > FRAME_BYTES {
            return Err(Error::Oversized {
                limit: FRAME_BYTES,
                actual,
            });
        }
        Ok(Self { tag, payload })
    }

    #[must_use]
    pub const fn tag(&self) -> Tag {
        self.tag
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

impl<'a> Writer<'a> {
    pub(crate) const fn new(output: &'a mut Vec<u8>) -> Self {
        Self { output }
    }

    /// Append a fixed-width unsigned value.
    pub fn u64(&mut self, value: u64) -> Result<(), crate::profile::message::Error> {
        self.extend(&value.to_le_bytes())
    }

    /// Append one length-prefixed byte string.
    pub fn bytes(&mut self, value: &[u8]) -> Result<(), crate::profile::message::Error> {
        let length =
            u64::try_from(value.len()).map_err(|_| crate::profile::message::Error::Limit)?;
        self.extend(&length.to_le_bytes())?;
        self.extend(value)
    }

    fn extend(&mut self, value: &[u8]) -> Result<(), crate::profile::message::Error> {
        let actual = PREFIX_BYTES
            .checked_add(self.output.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .ok_or(crate::profile::message::Error::Limit)?;
        if actual > FRAME_BYTES {
            return Err(crate::profile::message::Error::Limit);
        }
        self.output
            .try_reserve_exact(value.len())
            .map_err(|_| crate::profile::message::Error::Allocation)?;
        self.output.extend_from_slice(value);
        Ok(())
    }
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(input: &'a [u8]) -> Self {
        Self { input }
    }

    /// Consume one fixed-width unsigned value.
    pub fn u64(&mut self) -> Result<u64, crate::profile::message::Error> {
        let bytes = self.take(8)?;
        bytes
            .try_into()
            .map(u64::from_le_bytes)
            .map_err(|_| crate::profile::message::Error::Malformed)
    }

    /// Consume one length-prefixed byte string.
    pub fn bytes(&mut self) -> Result<&'a [u8], crate::profile::message::Error> {
        let length = self.u64()?;
        let length = usize::try_from(length).map_err(|_| crate::profile::message::Error::Limit)?;
        self.take(length)
    }

    /// Return whether the complete bounded message was consumed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], crate::profile::message::Error> {
        if length > self.input.len() {
            return Err(crate::profile::message::Error::Malformed);
        }
        let (value, remaining) = self.input.split_at(length);
        self.input = remaining;
        Ok(value)
    }
}

/// Write one complete length-prefixed frame.
pub fn send(writer: &mut impl Write, frame: &Frame) -> Result<(), Error> {
    let length = PREFIX_BYTES
        .checked_add(frame.payload.len())
        .ok_or(Error::Oversized {
            limit: FRAME_BYTES,
            actual: usize::MAX,
        })?;
    let length = u32::try_from(length).map_err(|_| Error::Oversized {
        limit: FRAME_BYTES,
        actual: length,
    })?;
    writer
        .write_all(&length.to_be_bytes())
        .map_err(Error::Write)?;
    writer
        .write_all(&VERSION.to_be_bytes())
        .map_err(Error::Write)?;
    writer.write_all(&[frame.tag as u8]).map_err(Error::Write)?;
    writer.write_all(&frame.payload).map_err(Error::Write)?;
    writer.flush().map_err(Error::Write)
}

/// Read the length prefix, reject over-limit input before allocation, then
/// consume exactly one complete versioned frame.
pub fn receive(reader: &mut impl Read) -> Result<Frame, Error> {
    // A blocking caller has no later poll to resume a partial frame, so a
    // timed-out read is an error rather than `None`.
    Decoder::new()
        .receive(reader)?
        .ok_or_else(|| Error::Read(std::io::Error::from(std::io::ErrorKind::WouldBlock)))
}

fn decode(mut bytes: Vec<u8>) -> Result<Option<Frame>, Error> {
    let version = u32::from_be_bytes(bytes[..4].try_into().map_err(|_| Error::Truncated)?);
    if version != VERSION {
        return Err(Error::Version {
            expected: VERSION,
            actual: version,
        });
    }
    let tag = Tag::try_from(bytes[4])?;
    let payload = bytes.split_off(PREFIX_BYTES).into_boxed_slice();
    Ok(Some(Frame { tag, payload }))
}

impl TryFrom<u8> for Tag {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::Authenticate,
            2 => Self::Hello,
            3 => Self::Prepare,
            4 => Self::Prepared,
            5 => Self::Activate,
            6 => Self::Activated,
            7 => Self::Admit,
            8 => Self::Admitted,
            9 => Self::Compile,
            10 => Self::SurfaceMiss,
            11 => Self::Report,
            12 => Self::Shutdown,
            13 => Self::Exited,
            14 => Self::ProfileMessage,
            15 => Self::ProfileReply,
            16 => Self::ProfileRejected,
            17 => Self::Invoke,
            18 => Self::Invoked,
            19 => Self::InvokeRejected,
            20 => Self::Start,
            21 => Self::Started,
            22 => Self::Snapshot,
            23 => Self::SnapshotOutcome,
            24 => Self::WorkProgress,
            25 => Self::HotCommit,
            26 => Self::HotAbort,
            27 => Self::HotAborted,
            28 => Self::HotConsumedRejected,
            29 => Self::HotBegin,
            30 => Self::HotBegan,
            31 => Self::HotCancel,
            32 => Self::HotCancelled,
            33 => Self::HotObserve,
            34 => Self::HotObserved,
            35 => Self::ObserveInitial,
            36 => Self::InitialObserved,
            37 => Self::ResidentCompile,
            38 => Self::ResidentPrepared,
            39 => Self::ResidentMiss,
            40 => Self::ResidentDiagnostic,
            41 => Self::ResidentObsolete,
            42 => Self::ResidentResource,
            43 => Self::RefreshPoll,
            44 => Self::RefreshStatus,
            45 => Self::Account,
            46 => Self::ExportState,
            47 => Self::StateExported,
            48 => Self::SeedState,
            49 => Self::StateSeeded,
            actual => return Err(Error::Tag(actual)),
        })
    }
}

/// Dedicated runner wire failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("runner frame exceeded limit {limit} with announced size {actual}")]
    Oversized { limit: usize, actual: usize },
    #[error("runner frame was truncated")]
    Truncated,
    #[error("runner wire version mismatch: expected {expected}, received {actual}")]
    Version { expected: u32, actual: u32 },
    #[error("unknown runner frame tag {0}")]
    Tag(u8),
    #[error("cannot allocate runner frame")]
    Allocation,
    #[error("cannot read runner frame")]
    Read(#[source] std::io::Error),
    #[error("cannot write runner frame")]
    Write(#[source] std::io::Error),
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use super::*;

    struct Segmented {
        bytes: Cursor<Vec<u8>>,
        block: bool,
    }

    impl Read for Segmented {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            if self.block {
                self.block = false;
                return Err(std::io::Error::from(std::io::ErrorKind::WouldBlock));
            }
            self.block = true;
            let length = output.len().min(2);
            self.bytes.read(&mut output[..length])
        }
    }

    #[test]
    fn decoder_resumes_one_bounded_partial_frame() {
        let frame = Frame::new(Tag::WorkProgress, Box::from(&b"progress"[..]))
            .expect("the frame is bounded");
        let mut wire = Vec::new();
        send(&mut wire, &frame).expect("the frame encodes");
        let mut reader = Segmented {
            bytes: Cursor::new(wire),
            block: false,
        };
        let mut decoder = Decoder::new();
        let decoded = loop {
            match decoder
                .receive(&mut reader)
                .expect("the partial frame remains valid")
            {
                Some(frame) => break frame,
                None => assert!(decoder.has_partial_frame()),
            }
        };
        assert_eq!(decoded.tag(), Tag::WorkProgress);
        assert_eq!(decoded.payload(), b"progress");
        assert!(!decoder.has_partial_frame());
    }

    #[test]
    fn current_protocol_rejects_version_two_before_publication() {
        let mut wire = Vec::new();
        wire.extend_from_slice(&(PREFIX_BYTES as u32).to_be_bytes());
        wire.extend_from_slice(&2_u32.to_be_bytes());
        wire.push(Tag::Prepared as u8);
        assert!(matches!(
            receive(&mut Cursor::new(wire)),
            Err(Error::Version {
                expected: VERSION,
                actual: 2
            })
        ));
    }

    #[test]
    fn guarded_decoder_yields_between_continuously_successful_fragments() {
        struct Trickle {
            bytes: Cursor<Vec<u8>>,
            reads: usize,
        }
        impl Read for Trickle {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                self.reads += 1;
                self.bytes.read(&mut output[..1])
            }
        }
        let frame = Frame::new(Tag::Prepared, vec![7; 128].into_boxed_slice()).unwrap();
        let mut wire = Vec::new();
        send(&mut wire, &frame).unwrap();
        let mut trickle = Trickle {
            bytes: Cursor::new(wire),
            reads: 0,
        };
        let mut decoder = Decoder::new();
        // Each successful fragment gives the owner its clock/resource check
        // back, even though the peer never produces WouldBlock.
        for tick in 0..10 {
            assert!(decoder.receive_step(&mut trickle).unwrap().is_none());
            assert_eq!(trickle.reads, tick + 1);
        }
        assert!(decoder.has_partial_frame());
        let decoded = loop {
            let before = trickle.reads;
            let frame = decoder.receive_step(&mut trickle).unwrap();
            assert!(trickle.reads - before <= 1);
            if let Some(frame) = frame {
                break frame;
            }
        };
        assert_eq!(decoded.payload(), &[7; 128]);
    }

    #[test]
    fn guarded_encoder_preserves_partial_writes_and_timeouts() {
        struct Segmented {
            bytes: Vec<u8>,
            calls: usize,
        }
        impl Write for Segmented {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.calls += 1;
                if self.calls.is_multiple_of(3) {
                    return Err(std::io::ErrorKind::TimedOut.into());
                }
                let count = bytes.len().min(2);
                self.bytes.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.calls += 1;
                Ok(())
            }
        }
        let frame = Frame::new(Tag::Compile, Box::from(&b"fragmented snapshot"[..])).unwrap();
        let mut expected = Vec::new();
        send(&mut expected, &frame).unwrap();
        let mut encoder = Encoder::frame(&frame).unwrap();
        let mut writer = Segmented {
            bytes: Vec::new(),
            calls: 0,
        };
        loop {
            let before = writer.calls;
            let complete = encoder.send_step(&mut writer).unwrap();
            assert_eq!(writer.calls, before + 1);
            if complete {
                break;
            }
        }
        assert_eq!(writer.bytes, expected);
    }
}

#[cfg(test)]
mod hot_outcome_tests;
