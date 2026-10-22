//! Canonical JSON in one exact requested byte allocation, without Vec growth.

use std::io::{self, Write};

use serde::Serialize;

use super::{Error, REPORT_BYTES};

pub(super) fn canonical(value: &impl Serialize) -> Result<Box<[u8]>, Error> {
    let mut count = Count {
        bytes: 0,
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer(&mut count, value) {
        return Err(if count.exceeded {
            Error::Frame
        } else {
            Error::Json(error)
        });
    }
    if count.bytes == 0 {
        return Err(Error::Frame);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(count.bytes)
        .map_err(|_| Error::Io(io::ErrorKind::OutOfMemory.into()))?;
    // Do not turn allocator bonus capacity into an unaccounted shrink request.
    if bytes.capacity() != count.bytes {
        return Err(Error::Frame);
    }
    bytes.resize(count.bytes, 0);
    let mut destination = io::Cursor::new(bytes.as_mut_slice());
    serde_json::to_writer(&mut destination, value).map_err(Error::Json)?;
    if destination.position() != count.bytes as u64 {
        return Err(Error::Canonical);
    }
    // Capacity and length are equal, so this ownership conversion cannot shrink.
    Ok(bytes.into_boxed_slice())
}

struct Count {
    bytes: usize,
    exceeded: bool,
}

impl Write for Count {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self
            .bytes
            .checked_add(bytes.len())
            .filter(|length| *length <= REPORT_BYTES)
        {
            Some(length) => {
                self.bytes = length;
                Ok(bytes.len())
            }
            None => {
                self.exceeded = true;
                Err(io::ErrorKind::WriteZero.into())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_storage_preserves_json_and_enforces_the_existing_frame_boundary() {
        let value = serde_json::json!({"escaped": "a\n\"b", "unicode": "é", "bytes": [0, 255]});
        assert_eq!(
            canonical(&value).unwrap().as_ref(),
            serde_json::to_vec(&value).unwrap()
        );
        let mut count = Count {
            bytes: REPORT_BYTES - 1,
            exceeded: false,
        };
        assert_eq!(count.write(b"x").unwrap(), 1);
        assert!(count.write(b"x").is_err());
        assert!(count.exceeded);
    }

    #[test]
    fn changed_lengths_cannot_grow_or_truncate_the_fixed_destination() {
        struct Changing(std::cell::Cell<bool>, bool);
        impl Serialize for Changing {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let later = self.0.replace(true);
                serializer.serialize_str(if later == self.1 { "longer" } else { "x" })
            }
        }
        assert!(canonical(&Changing(std::cell::Cell::new(false), true)).is_err());
        assert!(matches!(
            canonical(&Changing(std::cell::Cell::new(false), false)),
            Err(Error::Canonical)
        ));
    }
}
