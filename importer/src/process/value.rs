//! Closed values transported to and from generated adapter thunks.

pub mod handle;

pub use handle::Handle;

/// One bounded value in the generated-runner invocation protocol.
#[derive(Debug, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    I64(i64),
    F64(f64),
    Char(char),
    String(String),
    Handle(Handle),
    /// A Rawr `Option`: `None`, or `Some` of one value.
    Option(Option<Box<Value>>),
    /// A Rawr `Result`: `Ok` or `Err` of one value.
    Result(Result<Box<Value>, Box<Value>>),
}

impl Value {
    pub(crate) fn encode(self, output: &mut Vec<u8>) -> Result<(), super::invoke::Error> {
        match self {
            Self::Unit => output.push(0),
            Self::Bool(value) => {
                output.push(1);
                output.push(u8::from(value));
            }
            Self::I64(value) => {
                output.push(2);
                extend(output, &value.to_le_bytes())?;
            }
            Self::F64(value) => {
                output.push(3);
                extend(output, &value.to_bits().to_le_bytes())?;
            }
            Self::Char(value) => {
                output.push(4);
                extend(output, &u32::from(value).to_le_bytes())?;
            }
            Self::String(value) => {
                output.push(5);
                let length = u32::try_from(value.len()).map_err(|_| super::invoke::Error::Limit)?;
                extend(output, &length.to_le_bytes())?;
                extend(output, value.as_bytes())?;
            }
            Self::Handle(value) => {
                output.push(6);
                extend(output, value.epoch())?;
                extend(output, &value.slot().to_le_bytes())?;
                extend(output, &value.generation().to_le_bytes())?;
            }
            Self::Option(None) => extend(output, &[7, 0])?,
            Self::Option(Some(value)) => {
                extend(output, &[7, 1])?;
                value.encode(output)?;
            }
            Self::Result(Ok(value)) => {
                extend(output, &[8, 0])?;
                value.encode(output)?;
            }
            Self::Result(Err(value)) => {
                extend(output, &[8, 1])?;
                value.encode(output)?;
            }
        }
        Ok(())
    }

    pub(crate) fn decode(input: &mut &[u8]) -> Result<Self, super::invoke::Error> {
        let tag = take(input, 1)?[0];
        Ok(match tag {
            0 => Self::Unit,
            1 => match take(input, 1)?[0] {
                0 => Self::Bool(false),
                1 => Self::Bool(true),
                _ => return Err(super::invoke::Error::Malformed),
            },
            2 => Self::I64(i64::from_le_bytes(
                take(input, 8)?
                    .try_into()
                    .map_err(|_| super::invoke::Error::Malformed)?,
            )),
            3 => Self::F64(f64::from_bits(u64::from_le_bytes(
                take(input, 8)?
                    .try_into()
                    .map_err(|_| super::invoke::Error::Malformed)?,
            ))),
            4 => Self::Char(
                char::from_u32(u32::from_le_bytes(
                    take(input, 4)?
                        .try_into()
                        .map_err(|_| super::invoke::Error::Malformed)?,
                ))
                .ok_or(super::invoke::Error::Malformed)?,
            ),
            5 => {
                let length = u32::from_le_bytes(
                    take(input, 4)?
                        .try_into()
                        .map_err(|_| super::invoke::Error::Malformed)?,
                ) as usize;
                let value = std::str::from_utf8(take(input, length)?)
                    .map_err(|_| super::invoke::Error::Malformed)?;
                let mut owned = String::new();
                owned
                    .try_reserve_exact(value.len())
                    .map_err(|_| super::invoke::Error::Allocation)?;
                owned.push_str(value);
                Self::String(owned)
            }
            6 => {
                let epoch = take(input, 32)?
                    .try_into()
                    .map_err(|_| super::invoke::Error::Malformed)?;
                let slot = u32::from_le_bytes(
                    take(input, 4)?
                        .try_into()
                        .map_err(|_| super::invoke::Error::Malformed)?,
                );
                let generation = u64::from_le_bytes(
                    take(input, 8)?
                        .try_into()
                        .map_err(|_| super::invoke::Error::Malformed)?,
                );
                Self::Handle(Handle::new(epoch, slot, generation))
            }
            7 => match take(input, 1)?[0] {
                0 => Self::Option(None),
                1 => Self::Option(Some(Box::new(Self::decode(input)?))),
                _ => return Err(super::invoke::Error::Malformed),
            },
            8 => match take(input, 1)?[0] {
                0 => Self::Result(Ok(Box::new(Self::decode(input)?))),
                1 => Self::Result(Err(Box::new(Self::decode(input)?))),
                _ => return Err(super::invoke::Error::Malformed),
            },
            _ => return Err(super::invoke::Error::Malformed),
        })
    }
}

fn extend(output: &mut Vec<u8>, value: &[u8]) -> Result<(), super::invoke::Error> {
    let actual = output
        .len()
        .checked_add(value.len())
        .and_then(|bytes| bytes.checked_add(5))
        .ok_or(super::invoke::Error::Limit)?;
    if actual > crate::protocol::FRAME_BYTES {
        return Err(super::invoke::Error::Limit);
    }
    output
        .try_reserve_exact(value.len())
        .map_err(|_| super::invoke::Error::Allocation)?;
    output.extend_from_slice(value);
    Ok(())
}

fn take<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], super::invoke::Error> {
    if length > input.len() {
        return Err(super::invoke::Error::Malformed);
    }
    let (value, remaining) = input.split_at(length);
    *input = remaining;
    Ok(value)
}
