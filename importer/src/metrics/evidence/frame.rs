//! Strict closed framing for one evidence stream.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;

use serde::{de::DeserializeOwned, Serialize};

use super::Error;

const MAGIC: [u8; 4] = *b"R9EV";
const VERSION: u16 = 3;
const HEADER_BYTES: usize = 11;
const PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(super) enum Tag {
    Challenge = 1,
    Startup = 2,
    Capsule = 3,
}

pub(super) fn send(stream: &mut UnixStream, tag: Tag, value: &impl Serialize) -> Result<(), Error> {
    send_with(tag, value, |bytes| {
        stream.write_all(bytes).map_err(Error::Io)
    })
}

pub(super) fn send_before(
    stream: &mut UnixStream,
    tag: Tag,
    value: &impl Serialize,
    deadline_ns: u64,
) -> Result<(), Error> {
    nonblocking(stream, |stream| {
        send_with(tag, value, |mut bytes| {
            while !bytes.is_empty() {
                ready(stream, nix::poll::PollFlags::POLLOUT, deadline_ns)?;
                match stream.write(bytes) {
                    Ok(0) => return Err(Error::Io(std::io::ErrorKind::WriteZero.into())),
                    Ok(count) => bytes = &bytes[count..],
                    Err(error) if retry(&error) => continue,
                    Err(error) => return Err(Error::Io(error)),
                }
            }
            remaining(deadline_ns).map(|_| ())
        })
    })
}

fn send_with(
    tag: Tag,
    value: &impl Serialize,
    mut write: impl FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let payload = super::canonical(value)?;
    let length = u32::try_from(payload.len()).map_err(|_| Error::Frame)?;
    let mut header = [0_u8; HEADER_BYTES];
    header[..4].copy_from_slice(&MAGIC);
    header[4..6].copy_from_slice(&VERSION.to_be_bytes());
    header[6] = tag as u8;
    header[7..].copy_from_slice(&length.to_be_bytes());
    write(&header)?;
    write(&payload)
}

pub(super) fn receive<T>(stream: &mut UnixStream, expected: Tag) -> Result<T, Error>
where
    T: DeserializeOwned + Serialize,
{
    receive_with(expected, |bytes| {
        stream.read_exact(bytes).map_err(Error::Io)
    })
}

pub(super) fn receive_before<T>(
    stream: &mut UnixStream,
    expected: Tag,
    deadline_ns: u64,
) -> Result<T, Error>
where
    T: DeserializeOwned + Serialize,
{
    nonblocking(stream, |stream| {
        let value = receive_with(expected, |mut bytes| {
            while !bytes.is_empty() {
                ready(stream, nix::poll::PollFlags::POLLIN, deadline_ns)?;
                match stream.read(bytes) {
                    Ok(0) => return Err(Error::Io(std::io::ErrorKind::UnexpectedEof.into())),
                    Ok(count) => bytes = &mut bytes[count..],
                    Err(error) if retry(&error) => continue,
                    Err(error) => return Err(Error::Io(error)),
                }
            }
            remaining(deadline_ns).map(|_| ())
        })?;
        remaining(deadline_ns)?;
        Ok(value)
    })
}

pub(super) fn eof_before(stream: &mut UnixStream, deadline_ns: u64) -> Result<(), Error> {
    nonblocking(stream, |stream| loop {
        ready(stream, nix::poll::PollFlags::POLLIN, deadline_ns)?;
        match stream.read(&mut [0_u8; 1]) {
            Ok(0) => return remaining(deadline_ns).map(|_| ()),
            Ok(_) => return Err(Error::Trailing),
            Err(error) if retry(&error) => continue,
            Err(error) => return Err(Error::Io(error)),
        }
    })
}

fn receive_with<T>(
    expected: Tag,
    mut read: impl FnMut(&mut [u8]) -> Result<(), Error>,
) -> Result<T, Error>
where
    T: DeserializeOwned + Serialize,
{
    let mut header = [0_u8; HEADER_BYTES];
    read(&mut header)?;
    if header[..4] != MAGIC || header[4..6] != VERSION.to_be_bytes() || header[6] != expected as u8
    {
        return Err(Error::Frame);
    }
    let length = u32::from_be_bytes(header[7..].try_into().map_err(|_| Error::Frame)?) as usize;
    if length == 0 || length > PAYLOAD_BYTES {
        return Err(Error::Frame);
    }
    let mut payload = vec![0_u8; length];
    read(&mut payload)?;
    super::decode(&payload)
}

/// Finite final-frame decoder used by the independently responsive owner.
pub(super) struct Pending {
    header: [u8; HEADER_BYTES],
    header_read: usize,
    payload: Vec<u8>,
    payload_read: usize,
}

impl Pending {
    pub(super) fn new() -> Self {
        Self {
            header: [0; HEADER_BYTES],
            header_read: 0,
            payload: Vec::new(),
            payload_read: 0,
        }
    }

    pub(super) fn poll(&mut self, stream: &mut UnixStream) -> Result<Option<Vec<u8>>, Error> {
        if self.header_read < HEADER_BYTES {
            match stream.read(&mut self.header[self.header_read..]) {
                Ok(0) => return Err(Error::Io(std::io::ErrorKind::UnexpectedEof.into())),
                Ok(count) => self.header_read += count,
                Err(error) if retry(&error) => return Ok(None),
                Err(error) => return Err(Error::Io(error)),
            }
            if self.header_read < HEADER_BYTES {
                return Ok(None);
            }
            if self.header[..4] != MAGIC
                || self.header[4..6] != VERSION.to_be_bytes()
                || self.header[6] != Tag::Capsule as u8
            {
                return Err(Error::Frame);
            }
            let length =
                u32::from_be_bytes(self.header[7..].try_into().map_err(|_| Error::Frame)?) as usize;
            if length == 0 || length > PAYLOAD_BYTES {
                return Err(Error::Frame);
            }
            self.payload
                .try_reserve_exact(length)
                .map_err(|_| Error::Frame)?;
            self.payload.resize(length, 0);
        }
        if self.payload_read < self.payload.len() {
            let end = self
                .payload
                .len()
                .min(self.payload_read.saturating_add(16 * 1024));
            match stream.read(&mut self.payload[self.payload_read..end]) {
                Ok(0) => return Err(Error::Io(std::io::ErrorKind::UnexpectedEof.into())),
                Ok(count) => self.payload_read += count,
                Err(error) if retry(&error) => return Ok(None),
                Err(error) => return Err(Error::Io(error)),
            }
            if self.payload_read < self.payload.len() {
                return Ok(None);
            }
        }
        match stream.read(&mut [0; 1]) {
            Ok(0) => Ok(Some(std::mem::take(&mut self.payload))),
            Ok(_) => Err(Error::Trailing),
            Err(error) if retry(&error) => Ok(None),
            Err(error) => Err(Error::Io(error)),
        }
    }
}

pub(super) fn remaining(deadline_ns: u64) -> Result<std::time::Duration, Error> {
    let now = crate::metrics::monotonic_ns().ok_or(Error::Clock)?;
    match deadline_ns.checked_sub(now) {
        Some(remaining) if remaining > 0 => Ok(std::time::Duration::from_nanos(remaining)),
        _ => Err(Error::Timeout),
    }
}

fn retry(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
    )
}

// The parent owns this configured blocking stream exclusively. Polling with
// nonblocking I/O avoids changing socket timeouts after peer exit: Darwin may
// reject those changes even when a complete startup refusal remains buffered.
fn nonblocking<T>(
    stream: &mut UnixStream,
    operation: impl FnOnce(&mut UnixStream) -> Result<T, Error>,
) -> Result<T, Error> {
    stream.set_nonblocking(true).map_err(Error::Io)?;
    let result = operation(stream);
    let restored = stream.set_nonblocking(false).map_err(Error::Io);
    let value = result?;
    restored?;
    Ok(value)
}

fn ready(
    stream: &UnixStream,
    interest: nix::poll::PollFlags,
    deadline_ns: u64,
) -> Result<(), Error> {
    use nix::poll::{poll, PollFd, PollFlags};

    let io_deadline = std::time::Instant::now() + super::IO_TIMEOUT;
    loop {
        let wait = remaining(deadline_ns)?
            .min(io_deadline.saturating_duration_since(std::time::Instant::now()));
        if wait.is_zero() {
            return Err(Error::Timeout);
        }
        let milliseconds = wait.as_micros().saturating_add(999) / 1_000;
        let mut descriptors = [PollFd::new(stream, interest)];
        match poll(
            &mut descriptors,
            i32::try_from(milliseconds).unwrap_or(i32::MAX),
        ) {
            Ok(0) => continue,
            Ok(_) => {
                remaining(deadline_ns)?;
                let events = descriptors[0].revents().unwrap_or_else(PollFlags::empty);
                if events.intersects(interest | PollFlags::POLLHUP) {
                    return Ok(());
                }
                if events.intersects(PollFlags::POLLERR | PollFlags::POLLNVAL) {
                    return Err(Error::Frame);
                }
            }
            Err(nix::errno::Errno::EINTR) => continue,
            Err(error) => return Err(Error::Nix(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;

    use super::{Tag, HEADER_BYTES, MAGIC, PAYLOAD_BYTES, VERSION};

    #[test]
    fn partial_startup_header_and_payload_share_the_absolute_deadline() {
        for in_payload in [false, true] {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            super::super::configure(&reader).unwrap();
            let original = reader.read_timeout().unwrap();
            if in_payload {
                raw_header(&mut writer, MAGIC, Tag::Startup as u8, 8);
                writer.write_all(b"\"").unwrap();
            } else {
                writer.write_all(&MAGIC[..1]).unwrap();
            }
            let start = std::time::Instant::now();
            let deadline = crate::metrics::monotonic_ns().unwrap() + 20_000_000;
            let result =
                super::receive_before::<super::super::Trial>(&mut reader, Tag::Startup, deadline);
            assert!(matches!(result, Err(super::Error::Timeout)), "{result:?}");
            assert!(start.elapsed() < std::time::Duration::from_secs(1));
            assert_eq!(reader.read_timeout().unwrap(), original);
            // The peer remains alive: EOF did not cause the refusal.
            drop(writer);
        }
    }

    #[test]
    fn complete_queued_frame_cannot_cross_an_expired_deadline() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        super::send(&mut writer, Tag::Startup, &super::super::Trial::Measured).unwrap();
        assert!(matches!(
            super::receive_before::<super::super::Trial>(&mut reader, Tag::Startup, 1),
            Err(super::Error::Timeout)
        ));
        assert_eq!(reader.read_timeout().unwrap(), None);
    }

    #[test]
    fn successful_bounded_frames_restore_transport_timeouts() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        super::super::configure(&writer).unwrap();
        super::super::configure(&reader).unwrap();
        let deadline = crate::metrics::monotonic_ns().unwrap() + 1_000_000_000;
        super::send_before(
            &mut writer,
            Tag::Startup,
            &super::super::Trial::Measured,
            deadline,
        )
        .unwrap();
        assert_eq!(
            writer.write_timeout().unwrap(),
            Some(super::super::IO_TIMEOUT)
        );
        let value: super::super::Trial =
            super::receive_before(&mut reader, Tag::Startup, deadline).unwrap();
        assert_eq!(value, super::super::Trial::Measured);
        assert_eq!(
            reader.read_timeout().unwrap(),
            Some(super::super::IO_TIMEOUT)
        );
    }

    #[test]
    fn bounded_frame_and_eof_preserve_buffered_evidence_after_peer_exit() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        super::super::configure(&reader).unwrap();
        super::send(&mut writer, Tag::Startup, &super::super::Trial::Measured).unwrap();
        drop(writer);
        let deadline = crate::metrics::monotonic_ns().unwrap() + 1_000_000_000;
        let value: super::super::Trial =
            super::receive_before(&mut reader, Tag::Startup, deadline).unwrap();
        assert_eq!(value, super::super::Trial::Measured);
        super::eof_before(&mut reader, deadline).unwrap();
    }

    #[test]
    fn old_accounting_frame_version_is_rejected_before_payload() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        let mut header = [0; HEADER_BYTES];
        header[..4].copy_from_slice(&MAGIC);
        header[4..6].copy_from_slice(&2_u16.to_be_bytes());
        header[6] = Tag::Startup as u8;
        header[7..].copy_from_slice(&2_u32.to_be_bytes());
        writer.write_all(&header).unwrap();
        drop(writer);
        assert!(matches!(
            super::receive::<super::super::Startup>(&mut reader, Tag::Startup),
            Err(super::Error::Frame)
        ));
    }

    #[test]
    fn frame_rejects_wrong_magic_tag_and_oversized_length_before_allocation() {
        for (magic, tag, length) in [
            (*b"NOPE", Tag::Challenge as u8, 2_u32),
            (MAGIC, 99, 2),
            (MAGIC, Tag::Challenge as u8, PAYLOAD_BYTES as u32 + 1),
        ] {
            let (mut writer, mut reader) = UnixStream::pair().expect("socket pair");
            raw_header(&mut writer, magic, tag, length);
            drop(writer);
            assert!(matches!(
                super::receive::<super::super::Trial>(&mut reader, Tag::Challenge),
                Err(super::Error::Frame)
            ));
        }
    }

    #[test]
    fn frame_rejects_truncation_and_noncanonical_payload() {
        let (mut writer, mut reader) = UnixStream::pair().expect("socket pair");
        raw_header(&mut writer, MAGIC, Tag::Challenge as u8, 8);
        writer.write_all(b"two").expect("partial payload writes");
        drop(writer);
        assert!(matches!(
            super::receive::<super::super::Trial>(&mut reader, Tag::Challenge),
            Err(super::Error::Io(_))
        ));

        let payload = b"\"warmup\"\n";
        let (mut writer, mut reader) = UnixStream::pair().expect("socket pair");
        raw_header(
            &mut writer,
            MAGIC,
            Tag::Challenge as u8,
            payload.len() as u32,
        );
        writer.write_all(payload).expect("payload writes");
        drop(writer);
        assert!(matches!(
            super::receive::<super::super::Trial>(&mut reader, Tag::Challenge),
            Err(super::Error::Canonical)
        ));
    }

    #[test]
    fn nonblocking_final_decoder_requires_complete_frame_and_actual_eof() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let mut pending = super::Pending::new();
        raw_header(&mut writer, MAGIC, Tag::Capsule as u8, 4);
        writer.write_all(b"nu").unwrap();
        assert!(pending.poll(&mut reader).unwrap().is_none());
        assert!(pending.poll(&mut reader).unwrap().is_none());
        writer.write_all(b"ll").unwrap();
        assert!(pending.poll(&mut reader).unwrap().is_none());
        drop(writer);
        assert_eq!(pending.poll(&mut reader).unwrap(), Some(b"null".to_vec()));
    }

    #[test]
    fn nonblocking_final_decoder_rejects_trailing_bytes_and_partial_eof() {
        for bytes in [&b"nu"[..], &b"nullx"[..]] {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap();
            let mut pending = super::Pending::new();
            raw_header(&mut writer, MAGIC, Tag::Capsule as u8, 4);
            writer.write_all(bytes).unwrap();
            drop(writer);
            let result = pending.poll(&mut reader);
            if bytes.len() == 2 {
                assert!(result.unwrap().is_none());
                assert!(pending.poll(&mut reader).is_err());
            } else {
                assert!(result.is_err());
            }
        }
    }

    fn raw_header(stream: &mut UnixStream, magic: [u8; 4], tag: u8, length: u32) {
        let mut header = [0_u8; HEADER_BYTES];
        header[..4].copy_from_slice(&magic);
        header[4..6].copy_from_slice(&VERSION.to_be_bytes());
        header[6] = tag;
        header[7..].copy_from_slice(&length.to_be_bytes());
        stream.write_all(&header).expect("header writes");
    }
}
