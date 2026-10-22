//! Versioned traversal transport; decoded events grant no replay capability.

use super::{Decoder, Encoder};
use crate::input::membership::{
    observed::{
        self,
        stream::Row,
        traversal::{Close, Event, Handle, Next, Open, Transcript},
    },
    Budget, Error, Result,
};

pub(super) fn retain(value: &Transcript, budget: &mut Budget<'_>) -> Result<Transcript> {
    let mut events = Vec::new();
    budget.reserve(&mut events, value.events.len())?;
    for event in &value.events {
        let event = match event {
            Event::Open { path, result } => Event::Open {
                path: observed::Path::from_bytes(path.bytes(), budget)?,
                result: *result,
            },
            Event::Next { id, result } => Event::Next {
                id: *id,
                result: match result {
                    Next::Entry(row) => Next::Entry(Row::checked(
                        budget.string(&row.name)?,
                        row.raw_kind,
                        row.inspected,
                    )?),
                    Next::Eof => Next::Eof,
                    Next::ReadError(code) => Next::ReadError(*code),
                    Next::OpenError(code) => Next::OpenError(*code),
                },
            },
            Event::Close { id, reason } => Event::Close {
                id: *id,
                reason: *reason,
            },
            Event::Handle { path, result } => Event::Handle {
                path: observed::Path::from_bytes(path.bytes(), budget)?,
                result: *result,
            },
            Event::Release { id } => Event::Release { id: *id },
        };
        event.validate()?;
        events.push(event);
    }
    Ok(Transcript { events })
}

impl Decoder<'_, '_, '_> {
    pub(super) fn traversal(&mut self) -> Result<Transcript> {
        let count = self.count()?;
        // The shortest event is Release: one tag and one eight-byte ID.
        if count > self.remaining() / 9 {
            return Err(Error::Encoding("impossible traversal event count"));
        }
        let mut events = Vec::new();
        self.budget.reserve(&mut events, count)?;
        for _ in 0..count {
            let event = match self.byte()? {
                0 => Event::Open {
                    path: self.path()?,
                    result: self.traversal_open()?,
                },
                1 => Event::Next {
                    id: self.u64()?,
                    result: self.traversal_next()?,
                },
                2 => Event::Close {
                    id: self.u64()?,
                    reason: match self.byte()? {
                        0 => Close::Pruned,
                        1 => Close::Drained,
                        2 => Close::Exhausted,
                        _ => return Err(Error::Encoding("unknown traversal close reason")),
                    },
                },
                3 => Event::Handle {
                    path: self.path()?,
                    result: self.traversal_handle()?,
                },
                4 => Event::Release { id: self.u64()? },
                _ => return Err(Error::Encoding("unknown traversal event")),
            };
            event.validate()?;
            events.push(event);
        }
        Ok(Transcript { events })
    }

    fn traversal_open(&mut self) -> Result<Open> {
        Ok(match self.byte()? {
            0 => Open::Failed(self.u32()? as i32),
            1 => Open::UnionFailed {
                mount: self.traversal_mount()?,
                error: self.u32()? as i32,
            },
            2 => Open::Started(self.traversal_mount()?),
            _ => return Err(Error::Encoding("unknown traversal open result")),
        })
    }

    fn traversal_mount(&mut self) -> Result<rawr_directory::Mount> {
        Ok(match self.byte()? {
            0 => rawr_directory::Mount::Known(false),
            1 => rawr_directory::Mount::Known(true),
            2 => rawr_directory::Mount::Failed(self.u32()? as i32),
            _ => return Err(Error::Encoding("unknown traversal mount result")),
        })
    }

    fn traversal_next(&mut self) -> Result<Next> {
        Ok(match self.byte()? {
            0 => {
                let name = self.string()?;
                let raw_kind = self.byte()?;
                // Keep the complete-stream row's inspection tags. None cannot
                // occur here: WalkDir materializes file_type before yielding.
                let inspected = match self.byte()? {
                    1 => Ok(self.kind()?),
                    2 => Err(self.u32()? as i32),
                    _ => return Err(Error::Encoding("missing traversal entry inspection")),
                };
                Next::Entry(Row::checked(name, raw_kind, Some(inspected))?)
            }
            1 => Next::Eof,
            2 => Next::ReadError(self.u32()? as i32),
            3 => Next::OpenError(self.u32()? as i32),
            _ => return Err(Error::Encoding("unknown traversal next result")),
        })
    }

    fn traversal_handle(&mut self) -> Result<Handle> {
        Ok(match self.byte()? {
            0 => Handle::OpenFailed(self.u32()? as i32),
            1 => Handle::StatFailed(self.u32()? as i32),
            2 => Handle::Opened {
                device: self.u64()?,
                inode: self.u64()?,
            },
            _ => return Err(Error::Encoding("unknown traversal handle result")),
        })
    }
}

impl Encoder<'_> {
    pub(super) fn traversal(&mut self, value: &Transcript) -> Result<()> {
        self.count(value.events.len())?;
        for event in &value.events {
            event.validate()?;
            match event {
                Event::Open { path, result } => {
                    self.byte(0)?;
                    self.bytes(path.bytes())?;
                    self.traversal_open(*result)?;
                }
                Event::Next { id, result } => {
                    self.byte(1)?;
                    self.u64(*id)?;
                    self.traversal_next(result)?;
                }
                Event::Close { id, reason } => {
                    self.byte(2)?;
                    self.u64(*id)?;
                    self.byte(match reason {
                        Close::Pruned => 0,
                        Close::Drained => 1,
                        Close::Exhausted => 2,
                    })?;
                }
                Event::Handle { path, result } => {
                    self.byte(3)?;
                    self.bytes(path.bytes())?;
                    self.traversal_handle(*result)?;
                }
                Event::Release { id } => {
                    self.byte(4)?;
                    self.u64(*id)?;
                }
            }
        }
        Ok(())
    }

    fn traversal_open(&mut self, value: Open) -> Result<()> {
        match value {
            Open::Failed(code) => {
                self.byte(0)?;
                self.u32(code as u32)
            }
            Open::UnionFailed { mount, error } => {
                self.byte(1)?;
                self.traversal_mount(mount)?;
                self.u32(error as u32)
            }
            Open::Started(mount) => {
                self.byte(2)?;
                self.traversal_mount(mount)
            }
        }
    }

    fn traversal_mount(&mut self, mount: rawr_directory::Mount) -> Result<()> {
        match mount {
            rawr_directory::Mount::Known(false) => self.byte(0),
            rawr_directory::Mount::Known(true) => self.byte(1),
            rawr_directory::Mount::Failed(code) => {
                self.byte(2)?;
                self.u32(code as u32)
            }
        }
    }

    fn traversal_next(&mut self, value: &Next) -> Result<()> {
        match value {
            Next::Entry(row) => {
                self.byte(0)?;
                self.bytes(&row.name)?;
                self.byte(row.raw_kind)?;
                match row.inspected {
                    Some(Ok(kind)) => {
                        self.byte(1)?;
                        self.kind(kind)
                    }
                    Some(Err(code)) => {
                        self.byte(2)?;
                        self.u32(code as u32)
                    }
                    None => Err(Error::Encoding("missing traversal entry inspection")),
                }
            }
            Next::Eof => self.byte(1),
            Next::ReadError(code) => {
                self.byte(2)?;
                self.u32(*code as u32)
            }
            Next::OpenError(code) => {
                self.byte(3)?;
                self.u32(*code as u32)
            }
        }
    }

    fn traversal_handle(&mut self, value: Handle) -> Result<()> {
        match value {
            Handle::OpenFailed(code) => {
                self.byte(0)?;
                self.u32(code as u32)
            }
            Handle::StatFailed(code) => {
                self.byte(1)?;
                self.u32(code as u32)
            }
            Handle::Opened { device, inode } => {
                self.byte(2)?;
                self.u64(device)?;
                self.u64(inode)
            }
        }
    }
}
