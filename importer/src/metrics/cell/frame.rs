//! Strict distinct framing for one deterministic-owner stream.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::time::Instant;

use serde::{de::DeserializeOwned, Serialize};

use super::Error;

pub(super) const MAGIC: [u8; 4] = *b"R9CL";
const VERSION: u16 = 2;
const HEADER_BYTES: usize = 11;
const PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(super) enum Tag {
    Request = 1,
    SetupCapsule = 2,
    CellCapsule = 3,
}

pub(super) fn send(stream: &mut UnixStream, tag: Tag, value: &impl Serialize) -> Result<(), Error> {
    let (header, payload) = encoded(tag, value)?;
    stream.write_all(&header).map_err(Error::Io)?;
    stream.write_all(&payload).map_err(Error::Io)
}

/// Send the same frame under one absolute deadline, including every partial write.
pub(super) fn send_before(
    stream: &mut UnixStream,
    tag: Tag,
    value: &impl Serialize,
    deadline: Instant,
) -> Result<(), Error> {
    let (header, payload) = encoded(tag, value)?;
    for bytes in [&header[..], &payload] {
        let mut remaining = bytes;
        while !remaining.is_empty() {
            let timeout = deadline
                .checked_duration_since(Instant::now())
                .filter(|timeout| !timeout.is_zero())
                .ok_or(Error::Timeout)?;
            stream.set_write_timeout(Some(timeout)).map_err(Error::Io)?;
            match stream.write(remaining) {
                Ok(0) => return Err(Error::Io(std::io::ErrorKind::WriteZero.into())),
                Ok(written) => remaining = &remaining[written..],
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(Error::Io(error)),
            }
        }
    }
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    Ok(())
}

fn encoded(tag: Tag, value: &impl Serialize) -> Result<([u8; HEADER_BYTES], Box<[u8]>), Error> {
    let payload = super::canonical(value)?;
    let length = u32::try_from(payload.len()).map_err(|_| Error::Frame)?;
    let mut header = [0_u8; HEADER_BYTES];
    header[..4].copy_from_slice(&MAGIC);
    header[4..6].copy_from_slice(&VERSION.to_be_bytes());
    header[6] = tag as u8;
    header[7..].copy_from_slice(&length.to_be_bytes());
    Ok((header, payload))
}

pub(super) fn receive<T>(stream: &mut UnixStream, expected: Tag) -> Result<T, Error>
where
    T: DeserializeOwned + Serialize,
{
    let mut header = [0_u8; HEADER_BYTES];
    stream.read_exact(&mut header).map_err(Error::Io)?;
    if header[..4] != MAGIC || header[4..6] != VERSION.to_be_bytes() || header[6] != expected as u8
    {
        return Err(Error::Frame);
    }
    let length = u32::from_be_bytes(header[7..].try_into().map_err(|_| Error::Frame)?) as usize;
    if length == 0 || length > PAYLOAD_BYTES {
        return Err(Error::Frame);
    }
    let mut payload = vec![0_u8; length];
    stream.read_exact(&mut payload).map_err(Error::Io)?;
    super::decode(&payload)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;

    use super::{Tag, HEADER_BYTES, MAGIC, PAYLOAD_BYTES, VERSION};

    #[test]
    fn bounded_dispatch_keeps_the_existing_frame_bytes_and_absolute_expiry() {
        use std::io::Read as _;
        use std::time::{Duration, Instant};
        let wire = |bounded| {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            if bounded {
                super::send_before(
                    &mut writer,
                    Tag::Request,
                    &"one request",
                    Instant::now() + Duration::from_secs(1),
                )
                .unwrap();
            } else {
                super::send(&mut writer, Tag::Request, &"one request").unwrap();
            }
            drop(writer);
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).unwrap();
            bytes
        };
        assert_eq!(wire(false), wire(true));
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        let deadline = Instant::now();
        assert!(matches!(
            super::send_before(&mut writer, Tag::Request, &"expired", deadline),
            Err(super::Error::Timeout)
        ));
        reader.set_nonblocking(true).unwrap();
        assert_eq!(
            reader.read(&mut [0]).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn old_accounting_frame_version_is_rejected_before_payload() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        let mut header = [0; HEADER_BYTES];
        header[..4].copy_from_slice(&MAGIC);
        header[4..6].copy_from_slice(&1_u16.to_be_bytes());
        header[6] = Tag::CellCapsule as u8;
        header[7..].copy_from_slice(&2_u32.to_be_bytes());
        writer.write_all(&header).unwrap();
        drop(writer);
        assert!(matches!(
            super::receive::<super::super::CellCapsule>(&mut reader, Tag::CellCapsule),
            Err(super::Error::Frame)
        ));
    }

    #[test]
    fn distinct_cell_magic_tag_and_bound_fail_before_payload_admission() {
        for (magic, tag, length) in [
            (*b"R9EV", Tag::Request as u8, 2_u32),
            (MAGIC, 99, 2),
            (MAGIC, Tag::Request as u8, PAYLOAD_BYTES as u32 + 1),
        ] {
            let (mut writer, mut reader) = UnixStream::pair().expect("socket pair");
            let mut header = [0_u8; HEADER_BYTES];
            header[..4].copy_from_slice(&magic);
            header[4..6].copy_from_slice(&VERSION.to_be_bytes());
            header[6] = tag;
            header[7..].copy_from_slice(&length.to_be_bytes());
            writer.write_all(&header).expect("header writes");
            drop(writer);
            assert!(matches!(
                super::receive::<super::super::Role>(&mut reader, Tag::Request),
                Err(super::Error::Frame)
            ));
        }
    }
}
