//! Closed source read facts. No descriptor or current-source authority survives.

use crate::input::membership::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::input::membership) enum Role {
    LooseReference,
    DiscoveryPath,
    /// Requested retained source capacity at this occurrence, not allocator slack.
    Configuration {
        capacity: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Event {
    Open(std::result::Result<(), i32>),
    Length(std::result::Result<u64, i32>),
    Position(std::result::Result<u64, i32>),
    Read {
        requested: usize,
        result: std::result::Result<usize, i32>,
    },
    Directory(std::result::Result<bool, i32>),
}

/// A checked branch/byte bundle; exact read extents are checked by replay.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::input::membership) struct Transcript {
    role: Role,
    events: Vec<Event>,
    bytes: Vec<u8>,
}

impl Transcript {
    pub(in crate::input::membership) fn checked(
        role: Role,
        events: Vec<Event>,
        bytes: Vec<u8>,
    ) -> Result<Self> {
        let value = Self {
            role,
            events,
            bytes,
        };
        value.validate()?;
        Ok(value)
    }

    pub(in crate::input::membership) fn role(&self) -> Role {
        self.role
    }
    pub(in crate::input::membership) fn events(&self) -> &[Event] {
        &self.events
    }
    pub(in crate::input::membership) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(in crate::input::membership) fn validate(&self) -> Result<()> {
        if matches!(self.role, Role::Configuration {capacity} if capacity < 512) {
            return Err(Error::Encoding("invalid configuration read capacity"));
        }
        let mut events = self.events.iter();
        loop {
            match events.next() {
                Some(Event::Open(Err(code))) => {
                    errno(*code)?;
                    if *code != nix::libc::EINTR {
                        return ending(events, self.bytes.len(), 0);
                    }
                }
                Some(Event::Open(Ok(()))) => break,
                _ => return Err(Error::Encoding("Git read lacks opening outcome")),
            }
        }
        if self.role == Role::DiscoveryPath {
            match events.next() {
                Some(Event::Length(Ok(length))) if *length <= 65536 => {}
                Some(Event::Length(Ok(_))) => return ending(events, self.bytes.len(), 0),
                Some(Event::Length(Err(code))) => {
                    errno(*code)?;
                    return ending(events, self.bytes.len(), 0);
                }
                _ => return Err(Error::Encoding("Git path read lacks descriptor size gate")),
            }
        }
        match events.next() {
            Some(Event::Length(Err(code))) => errno(*code)?,
            Some(Event::Length(Ok(_))) => match events.next() {
                Some(Event::Position(Ok(_))) => {}
                Some(Event::Position(Err(code))) => errno(*code)?,
                _ => return Err(Error::Encoding("Git read lacks reached position hint")),
            },
            _ => return Err(Error::Encoding("Git read lacks descriptor hint")),
        }
        let mut length = 0usize;
        loop {
            let Some(Event::Read { requested, result }) = events.next() else {
                return Err(Error::Encoding("Git read lacks terminal read"));
            };
            if *requested == 0 {
                return Err(Error::Encoding("Git read has empty request"));
            }
            match result {
                Ok(0) => return ending(events, self.bytes.len(), length),
                Ok(count) if *count <= (*requested).min(i32::MAX as usize) => {
                    length = length
                        .checked_add(*count)
                        .ok_or(Error::Encoding("Git read byte count overflow"))?;
                }
                Ok(_) => return Err(Error::Encoding("Git read exceeds request")),
                Err(code) => {
                    errno(*code)?;
                    if *code == nix::libc::EINTR {
                        continue;
                    }
                    if self.role == Role::LooseReference {
                        match events.next() {
                            Some(Event::Directory(Ok(_))) => {}
                            Some(Event::Directory(Err(code))) => errno(*code)?,
                            _ => {
                                return Err(Error::Encoding(
                                    "Git read lacks reached directory probe",
                                ))
                            }
                        }
                    }
                    return ending(events, self.bytes.len(), length);
                }
            }
        }
    }
}

pub(in crate::input::membership) fn errno(code: i32) -> Result<()> {
    if code > 0 {
        Ok(())
    } else {
        Err(Error::Encoding("Git read native error must be positive"))
    }
}

fn ending<'a>(
    mut events: impl Iterator<Item = &'a Event>,
    actual: usize,
    expected: usize,
) -> Result<()> {
    if events.next().is_some() || actual != expected {
        return Err(Error::Encoding(
            "Git read has trailing events or unmatched bytes",
        ));
    }
    Ok(())
}
