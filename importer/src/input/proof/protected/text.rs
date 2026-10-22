//! Prospective ownership for protected projection text transformations.

use std::fmt;

use crate::input::stored::{Budget, Error, Result};

/// Count formatting before ownership, then forbid growth past that admission.
pub(in crate::input) fn format(
    arguments: fmt::Arguments<'_>,
    budget: &mut Budget<'_>,
) -> Result<String> {
    budget.check()?;
    let mut counter = Counter {
        bytes: 0,
        overflow: false,
    };
    let counted = fmt::write(&mut counter, arguments);
    if counter.overflow {
        return Err(budget.overflow());
    }
    counted.map_err(|_| Error::Encoding("protected text cannot be formatted"))?;
    let mut writer = Writer {
        text: allocate(counter.bytes, budget)?,
        limit: counter.bytes,
        overflow: false,
    };
    let rendered = fmt::write(&mut writer, arguments);
    if writer.overflow {
        return Err(budget.overflow());
    }
    rendered.map_err(|_| Error::Encoding("protected text cannot be formatted"))?;
    if writer.text.len() != counter.bytes {
        return Err(Error::Encoding("protected text formatting length changed"));
    }
    Ok(writer.text)
}

/// Replace nonoverlapping substring matches, including empty UTF-8 boundaries.
pub(in crate::input) fn replace(
    value: &str,
    from: &str,
    to: &str,
    budget: &mut Budget<'_>,
) -> Result<String> {
    budget.check()?;
    let mut bytes = 0_usize;
    let mut previous = 0;
    for (index, matched) in value.match_indices(from) {
        bytes = bytes
            .checked_add(index - previous)
            .and_then(|bytes| bytes.checked_add(to.len()))
            .ok_or_else(|| budget.overflow())?;
        // The searcher's match is a borrowed, disjoint slice of value.
        previous = index + matched.len();
    }
    bytes = bytes
        .checked_add(value.len() - previous)
        .ok_or_else(|| budget.overflow())?;
    let mut output = allocate(bytes, budget)?;
    previous = 0;
    for (index, matched) in value.match_indices(from) {
        output.push_str(&value[previous..index]);
        output.push_str(to);
        previous = index + matched.len();
    }
    output.push_str(&value[previous..]);
    Ok(output)
}

/// Move an admitted string, charging a full shrink layout only when needed.
pub(in crate::input) fn boxed(value: String, budget: &mut Budget<'_>) -> Result<Box<str>> {
    budget.check()?;
    if value.capacity() != value.len() {
        budget.storage(0, value.len())?;
    }
    Ok(value.into_boxed_str())
}

fn allocate(bytes: usize, budget: &mut Budget<'_>) -> Result<String> {
    budget.storage(1, bytes)?;
    let mut value = String::new();
    value
        .try_reserve_exact(bytes)
        .map_err(|error| budget.failure(Error::Allocation(error)))?;
    Ok(value)
}

struct Counter {
    bytes: usize,
    overflow: bool,
}

impl fmt::Write for Counter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let Some(bytes) = self.bytes.checked_add(value.len()) else {
            self.overflow = true;
            return Err(fmt::Error);
        };
        self.bytes = bytes;
        Ok(())
    }
}

struct Writer {
    text: String,
    limit: usize,
    overflow: bool,
}

impl fmt::Write for Writer {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self
            .text
            .len()
            .checked_add(value.len())
            .is_none_or(|bytes| bytes > self.limit)
        {
            self.overflow = true;
            return Err(fmt::Error);
        }
        // Allocator bonus capacity never grants permission for another byte.
        self.text.push_str(value);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
