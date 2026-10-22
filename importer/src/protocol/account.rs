//! Fixed child-account observations, with correlation but no admission authority.

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

/// Reuse the closed protocol tags through the generated child's codec re-export.
pub use super::Tag;

const VERSION: u8 = 1;
const REQUEST_BYTES: usize = 66;
const ACCOUNT_BYTES: usize = 93;
/// Exact payload size; encoding uses only this stack array.
pub const BYTES: usize = REQUEST_BYTES + 2 * ACCOUNT_BYTES;

/// Same-channel correlation to a command, without hashing or copying its source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    tag: Tag,
    payload_bytes: u64,
    // Seven fixed chunks retain the first 56 bytes and canonical zero padding.
    prefix: [[u8; 8]; 7],
}

/// Original retained-account resource axis.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Axis {
    Items,
    Bytes,
}

/// Original arithmetic/limit check which refused a charge.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    MissingAmount,
    OwnedOverflow,
    AggregateOverflow,
    Limit,
}

/// First refused request from the actual child account.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub axis: Axis,
    pub cause: Cause,
    pub current: [u64; 2],
    /// Original requested aggregate, absent when it could not be represented.
    pub requested: Option<u64>,
    pub limit: u64,
}

/// Fixed actual account state; absent identity means observation-ID exhaustion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub account: Option<NonZeroU64>,
    pub current: [u64; 2],
    pub peak: [u64; 2],
    pub limits: [u64; 2],
    pub first_refusal: Option<Refusal>,
}

/// Up to two distinct simultaneous original accounts; two absences mean unreached.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub request: Request,
    pub accounts: [Option<Snapshot>; 2],
}

/// Telemetry rejection is evidence failure, never a replacement product result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("native account observation has invalid framing or padding")]
    Framing,
    #[error("native account observation version is unsupported")]
    Version,
    #[error("native account observation has invalid request correlation")]
    Request,
    #[error("native account observation has inconsistent account facts")]
    Account,
}

/// Commands whose actual completion can create, retain, or retire child accounts.
pub const fn required(tag: Tag) -> bool {
    matches!(
        tag,
        Tag::Prepare
            | Tag::Admit
            | Tag::Start
            | Tag::Compile
            | Tag::Snapshot
            | Tag::ResidentCompile
            | Tag::HotCommit
            | Tag::HotAbort
            | Tag::HotCancel
            | Tag::Shutdown
    )
}

impl Request {
    /// Retain bounded same-channel correlation without duplicating request data.
    pub fn from_parts(tag: Tag, payload: &[u8]) -> Result<Self, Error> {
        if !required(tag) || payload.len() > super::FRAME_BYTES - 5 {
            return Err(Error::Request);
        }
        let mut prefix = [[0; 8]; 7];
        for (index, byte) in payload.iter().take(56).enumerate() {
            prefix[index / 8][index % 8] = *byte;
        }
        Ok(Self {
            tag,
            payload_bytes: u64::try_from(payload.len()).map_err(|_| Error::Request)?,
            prefix,
        })
    }

    /// Return the original command whose boundary was observed.
    pub const fn tag(self) -> Tag {
        self.tag
    }

    /// Return the original command payload length, independent of its prefix.
    pub const fn payload_bytes(self) -> u64 {
        self.payload_bytes
    }

    fn valid(&self) -> bool {
        required(self.tag)
            && self.payload_bytes <= (super::FRAME_BYTES - 5) as u64
            && self
                .prefix
                .iter()
                .flatten()
                .enumerate()
                .all(|(index, byte)| (index as u64) < self.payload_bytes || *byte == 0)
    }
}

impl Observation {
    /// Encode validated facts in the fixed-size wire array.
    pub fn encode(&self) -> Result<[u8; BYTES], Error> {
        self.validate()?;
        let mut bytes = [0; BYTES];
        bytes[0] = VERSION;
        bytes[1] = self.request.tag as u8;
        put(&mut bytes, 2, self.request.payload_bytes);
        for (target, source) in bytes[10..REQUEST_BYTES]
            .iter_mut()
            .zip(self.request.prefix.iter().flatten())
        {
            *target = *source;
        }
        for (index, account) in self.accounts.iter().enumerate() {
            let Some(account) = account else { continue };
            let start = REQUEST_BYTES + index * ACCOUNT_BYTES;
            let output = &mut bytes[start..start + ACCOUNT_BYTES];
            output[0] = 1;
            put(output, 1, account.account.map_or(0, NonZeroU64::get));
            pair(output, 9, account.current);
            pair(output, 25, account.peak);
            pair(output, 41, account.limits);
            if let Some(refusal) = account.first_refusal {
                output[57] = 1;
                output[58] = match refusal.axis {
                    Axis::Items => 1,
                    Axis::Bytes => 2,
                };
                output[59] = match refusal.cause {
                    Cause::MissingAmount => 1,
                    Cause::OwnedOverflow => 2,
                    Cause::AggregateOverflow => 3,
                    Cause::Limit => 4,
                };
                output[60] = u8::from(refusal.requested.is_some());
                pair(output, 61, refusal.current);
                put(output, 77, refusal.requested.unwrap_or(0));
                put(output, 85, refusal.limit);
            }
        }
        Ok(bytes)
    }

    /// Reject noncanonical or inconsistent reports before retaining any facts.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let bytes: &[u8; BYTES] = bytes.try_into().map_err(|_| Error::Framing)?;
        if bytes[0] != VERSION {
            return Err(Error::Version);
        }
        let tag = Tag::try_from(bytes[1]).map_err(|_| Error::Request)?;
        let mut prefix = [[0; 8]; 7];
        for (chunk, source) in prefix
            .iter_mut()
            .zip(bytes[10..REQUEST_BYTES].as_chunks::<8>().0.iter())
        {
            chunk.copy_from_slice(source);
        }
        let mut observation = Self {
            request: Request {
                tag,
                payload_bytes: get(bytes, 2),
                prefix,
            },
            accounts: [None; 2],
        };
        for (index, account) in observation.accounts.iter_mut().enumerate() {
            let start = REQUEST_BYTES + index * ACCOUNT_BYTES;
            let input = &bytes[start..start + ACCOUNT_BYTES];
            match input[0] {
                0 if input[1..].iter().all(|byte| *byte == 0) => continue,
                1 => {}
                _ => return Err(Error::Framing),
            }
            let first_refusal = match input[57] {
                0 if input[58..].iter().all(|byte| *byte == 0) => None,
                1 => Some(Refusal {
                    axis: match input[58] {
                        1 => Axis::Items,
                        2 => Axis::Bytes,
                        _ => return Err(Error::Framing),
                    },
                    cause: match input[59] {
                        1 => Cause::MissingAmount,
                        2 => Cause::OwnedOverflow,
                        3 => Cause::AggregateOverflow,
                        4 => Cause::Limit,
                        _ => return Err(Error::Framing),
                    },
                    current: get_pair(input, 61),
                    requested: match input[60] {
                        0 if get(input, 77) == 0 => None,
                        1 => Some(get(input, 77)),
                        _ => return Err(Error::Framing),
                    },
                    limit: get(input, 85),
                }),
                _ => return Err(Error::Framing),
            };
            *account = Some(Snapshot {
                account: NonZeroU64::new(get(input, 1)),
                current: get_pair(input, 9),
                peak: get_pair(input, 25),
                limits: get_pair(input, 41),
                first_refusal,
            });
        }
        observation.validate()?;
        Ok(observation)
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        if !self.request.valid() {
            return Err(Error::Request);
        }
        if self.accounts[0].is_none() && self.accounts[1].is_some() {
            return Err(Error::Account);
        }
        for account in self.accounts.iter().flatten() {
            account.validate()?;
        }
        if let [Some(left), Some(right)] = self.accounts
            && left.account.is_some()
            && left.account == right.account
        {
            return Err(Error::Account);
        }
        Ok(())
    }
}

impl Snapshot {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if (0..2)
            .any(|axis| self.current[axis] > self.peak[axis] || self.peak[axis] > self.limits[axis])
        {
            return Err(Error::Account);
        }
        if let Some(refusal) = self.first_refusal {
            let axis = match refusal.axis {
                Axis::Items => 0,
                Axis::Bytes => 1,
            };
            let amount_valid = match refusal.cause {
                Cause::MissingAmount | Cause::AggregateOverflow => refusal.requested.is_none(),
                Cause::OwnedOverflow => true,
                Cause::Limit => refusal
                    .requested
                    .is_some_and(|requested| requested > refusal.limit),
            };
            if !amount_valid
                || refusal.limit != self.limits[axis]
                || (0..2).any(|axis| refusal.current[axis] > self.peak[axis])
            {
                return Err(Error::Account);
            }
        }
        Ok(())
    }
}

fn put(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
fn get(bytes: &[u8], offset: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}
fn pair(bytes: &mut [u8], offset: usize, value: [u64; 2]) {
    put(bytes, offset, value[0]);
    put(bytes, offset + 8, value[1]);
}
fn get_pair(bytes: &[u8], offset: usize) -> [u64; 2] {
    [get(bytes, offset), get(bytes, offset + 8)]
}

#[cfg(test)]
mod tests {
    use super::{
        required, Axis, Cause, Observation, Refusal, Request, Snapshot, Tag, ACCOUNT_BYTES, BYTES,
        REQUEST_BYTES,
    };
    use std::num::NonZeroU64;

    fn observation() -> Observation {
        Observation {
            request: Request::from_parts(Tag::ResidentCompile, &[9; 100]).unwrap(),
            accounts: [
                Some(Snapshot {
                    account: NonZeroU64::new(7),
                    current: [1, 2],
                    peak: [3, 4],
                    limits: [5, 6],
                    first_refusal: Some(Refusal {
                        axis: Axis::Bytes,
                        cause: Cause::Limit,
                        current: [3, 4],
                        requested: Some(7),
                        limit: 6,
                    }),
                }),
                None,
            ],
        }
    }

    #[test]
    fn actual_account_round_trip_is_fixed_and_request_correlation_copies_only_prefix() {
        let observation = observation();
        let encoded = observation.encode().unwrap();
        assert_eq!(encoded.len(), 252);
        const { assert!(BYTES <= 512) };
        assert_eq!(Observation::decode(&encoded).unwrap(), observation);
        let mut source = [9; 100];
        source[99] = 3;
        assert_eq!(
            Request::from_parts(Tag::ResidentCompile, &source).unwrap(),
            observation.request
        );
        source[55] = 3;
        assert_ne!(
            Request::from_parts(Tag::ResidentCompile, &source).unwrap(),
            observation.request
        );
        assert_ne!(
            Request::from_parts(Tag::ResidentCompile, &[9; 99]).unwrap(),
            observation.request
        );
    }

    #[test]
    fn codec_rejects_noncanonical_padding_lengths_flags_and_duplicate_accounts() {
        let observation = observation();
        let encoded = observation.encode().unwrap();
        assert!(Observation::decode(&encoded[..BYTES - 1]).is_err());
        for index in [
            REQUEST_BYTES + ACCOUNT_BYTES,
            REQUEST_BYTES + ACCOUNT_BYTES + 1,
            REQUEST_BYTES + 57,
            REQUEST_BYTES + 58,
            REQUEST_BYTES + 59,
            REQUEST_BYTES + 60,
        ] {
            let mut changed = encoded;
            changed[index] = 9;
            assert!(Observation::decode(&changed).is_err(), "offset {index}");
        }
        let mut duplicate = observation;
        duplicate.accounts[1] = duplicate.accounts[0];
        assert!(duplicate.encode().is_err());
        let mut short = Observation {
            request: Request::from_parts(Tag::Shutdown, &[]).unwrap(),
            accounts: [None; 2],
        }
        .encode()
        .unwrap();
        short[10] = 1;
        assert!(Observation::decode(&short).is_err());
    }

    #[test]
    fn unreached_and_exhausted_identity_remain_distinct_without_fake_zero_usage() {
        let mut observation = observation();
        observation.accounts = [None; 2];
        assert_eq!(
            Observation::decode(&observation.encode().unwrap())
                .unwrap()
                .accounts,
            [None; 2]
        );
        observation.accounts[0] = Some(Snapshot {
            account: None,
            current: [0; 2],
            peak: [0; 2],
            limits: [0; 2],
            first_refusal: None,
        });
        assert!(Observation::decode(&observation.encode().unwrap())
            .unwrap()
            .accounts[0]
            .is_some());
        assert!(!required(Tag::Account));
        assert!(required(Tag::Snapshot));
        assert!(Request::from_parts(Tag::WorkProgress, &[]).is_err());
    }
}
