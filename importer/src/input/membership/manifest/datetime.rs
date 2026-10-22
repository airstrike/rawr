//! Pinned Datetime display without Time's hidden fractional-second String.

use std::fmt::{self, Write};

use rawr_toml::document;

use super::{session, Error, Step};

pub(super) const FIELD: &str = "$__toml_private_datetime";

/// A parsed datetime has at most 10 date + 1 separator + 8 time + 10
/// fractional + 6 offset bytes. This inline storage has no heap allocation.
#[derive(Clone, Copy, Debug)]
pub(super) struct Text {
    bytes: [u8; 35],
    length: usize,
}

impl Text {
    pub(super) fn render<A: document::Admission>(
        value: document::Datetime,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut length = if value.date.is_some() { 10 } else { 0 };
        if let Some(time) = value.time {
            length += usize::from(value.date.is_some()) + 5;
            if time.second.is_some() || time.nanosecond.is_some() {
                length += 3;
            }
            if let Some(mut nanos) = time.nanosecond {
                let mut digits = 9;
                while digits > 1 && nanos % 10 == 0 {
                    nanos /= 10;
                    digits -= 1;
                }
                length += 1 + digits;
            }
        }
        if let Some(offset) = value.offset {
            // Formatting an offset is constant work with no owned scratch.
            // Distinguish Z without importing another dependency type.
            let mut offset_text = Self {
                bytes: [0; 35],
                length: 0,
            };
            if write!(&mut offset_text, "{offset}").is_err() {
                return session.fail(Error::State);
            }
            length += offset_text.length;
        }
        session.string(length)?;
        let mut text = Self {
            bytes: [0; 35],
            length: 0,
        };
        if text.write(value).is_err() {
            return session.fail(Error::State);
        }
        Ok(text)
    }

    pub(super) fn as_str(&self) -> &str {
        // Only ASCII formatting writes this private buffer.
        std::str::from_utf8(&self.bytes[..self.length]).expect("datetime display is ASCII")
    }

    fn write(&mut self, value: document::Datetime) -> fmt::Result {
        if let Some(date) = value.date {
            write!(self, "{date}")?;
        }
        if let Some(time) = value.time {
            if value.date.is_some() {
                self.write_str("T")?;
            }
            write!(self, "{:02}:{:02}", time.hour, time.minute)?;
            if let Some(second) = time
                .second
                .or_else(|| time.nanosecond.is_some().then_some(0))
            {
                write!(self, ":{second:02}")?;
            }
            if let Some(mut nanos) = time.nanosecond {
                let mut digits = [b'0'; 9];
                for digit in digits.iter_mut().rev() {
                    *digit = b'0' + (nanos % 10) as u8;
                    nanos /= 10;
                }
                let length = digits
                    .iter()
                    .rposition(|byte| *byte != b'0')
                    .map_or(1, |i| i + 1);
                self.write_str(".")?;
                self.write_str(std::str::from_utf8(&digits[..length]).map_err(|_| fmt::Error)?)?;
            }
        }
        // Pinned Date and Offset format directly into the supplied writer;
        // neither reaches the allocation-bearing Time::fmt implementation.
        if let Some(offset) = value.offset {
            write!(self, "{offset}")?;
        }
        Ok(())
    }
}

impl fmt::Write for Text {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.length.checked_add(value.len()).ok_or(fmt::Error)?;
        let output = self.bytes.get_mut(self.length..end).ok_or(fmt::Error)?;
        output.copy_from_slice(value.as_bytes());
        self.length = end;
        Ok(())
    }
}
