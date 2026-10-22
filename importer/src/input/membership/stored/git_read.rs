//! Bounded encoding of closed Git read facts; decoding supplies no freshness.

use super::{Decoder, Encoder};
use crate::input::membership::{
    observed::git_read::{Event, Role, Transcript},
    Budget, Error, Result,
};

pub(super) fn retain(transcript: &Transcript, budget: &mut Budget<'_>) -> Result<Transcript> {
    let mut events = Vec::new();
    budget.reserve(&mut events, transcript.events().len())?;
    events.extend_from_slice(transcript.events());
    Transcript::checked(transcript.role(), events, budget.bytes(transcript.bytes())?)
}

impl Decoder<'_, '_, '_> {
    pub(super) fn git_role(&mut self) -> Result<Role> {
        match self.byte()? {
            0 => Ok(Role::LooseReference),
            1 => Ok(Role::DiscoveryPath),
            2 => Ok(Role::Configuration {
                capacity: self.count()?,
            }),
            _ => Err(Error::Encoding("unknown Git read role")),
        }
    }
    fn git_result<T>(
        &mut self,
        success: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<std::result::Result<T, i32>> {
        match self.byte()? {
            0 => Ok(Ok(success(self)?)),
            1 => Ok(Err(self.u32()? as i32)),
            _ => Err(Error::Encoding("unknown Git native result")),
        }
    }
    pub(super) fn git_read(&mut self) -> Result<Transcript> {
        let role = self.git_role()?;
        let count = self.count()?;
        if count > self.remaining() / 2 {
            return Err(Error::Encoding("impossible Git read event count"));
        }
        let mut events = Vec::new();
        self.budget.reserve(&mut events, count)?;
        for _ in 0..count {
            events.push(match self.byte()? {
                0 => Event::Open(self.git_result(|_| Ok(()))?),
                1 => Event::Length(self.git_result(|decoder| decoder.u64())?),
                2 => Event::Position(self.git_result(|decoder| decoder.u64())?),
                3 => Event::Read {
                    requested: self.count()?,
                    result: self.git_result(|decoder| decoder.count())?,
                },
                4 => Event::Directory(self.git_result(|decoder| decoder.boolean())?),
                _ => return Err(Error::Encoding("unknown Git read event")),
            });
        }
        Transcript::checked(role, events, self.bytes()?)
    }
}

impl Encoder<'_> {
    pub(super) fn git_role(&mut self, role: Role) -> Result<()> {
        match role {
            Role::LooseReference => self.byte(0),
            Role::DiscoveryPath => self.byte(1),
            Role::Configuration { capacity } => {
                self.byte(2)?;
                self.count(capacity)
            }
        }
    }
    fn git_result<T>(
        &mut self,
        value: &std::result::Result<T, i32>,
        success: impl FnOnce(&mut Self, &T) -> Result<()>,
    ) -> Result<()> {
        match value {
            Ok(value) => {
                self.byte(0)?;
                success(self, value)
            }
            Err(code) => {
                self.byte(1)?;
                self.u32(*code as u32)
            }
        }
    }
    pub(super) fn git_read(&mut self, transcript: &Transcript) -> Result<()> {
        self.git_role(transcript.role())?;
        self.count(transcript.events().len())?;
        for event in transcript.events() {
            match event {
                Event::Open(result) => {
                    self.byte(0)?;
                    self.git_result(result, |_, _| Ok(()))?;
                }
                Event::Length(result) => {
                    self.byte(1)?;
                    self.git_result(result, |encoder, value| encoder.u64(*value))?;
                }
                Event::Position(result) => {
                    self.byte(2)?;
                    self.git_result(result, |encoder, value| encoder.u64(*value))?;
                }
                Event::Read { requested, result } => {
                    self.byte(3)?;
                    self.count(*requested)?;
                    self.git_result(result, |encoder, value| encoder.count(*value))?;
                }
                Event::Directory(result) => {
                    self.byte(4)?;
                    self.git_result(result, |encoder, value| encoder.byte(u8::from(*value)))?;
                }
            }
        }
        self.bytes(transcript.bytes())
    }
}
