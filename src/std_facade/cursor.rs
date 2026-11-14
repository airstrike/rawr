//! std's `io::Cursor<T>` for Rawr code: Rust's own `Cursor`, at rawr's
//! value of `T`, so a `Cursor<Vec<u8>>` the program makes is a
//! `Cursor<Dynamic>` holding the program's bytes. A crate takes and gives
//! the cursor at its own `T`, as image's `ImageReader::new(Cursor<Bytes>)`
//! takes one, its value made where it crosses (`crossing::StdOf`). It reads
//! and seeks the bytes its value holds, to a `SeekFrom` that is std's own, as std's `Read`, `BufRead` and
//! `Seek` of a `T: AsRef<[u8]>` do.

use std::io::{BufRead as _, Cursor, Read as _, Seek as _, SeekFrom};

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 2] = [
    Held::Value(held::Value {
        std: "std::io::Cursor",
        parameters: &["T"],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .type_::<Cursor<Dynamic>>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: true,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
    Held::Value(held::Value {
        std: "std::io::SeekFrom",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .value_type::<SeekFrom>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: true,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
];

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<Cursor<Dynamic>>();
    crate::host::debug::register::<SeekFrom>();
}

#[cfg(feature = "sync")]
impl<T> crate::crossing::StdOf<T> for Cursor<T> {
    fn take<E>(
        value: Dynamic,
        inner: impl FnOnce(Dynamic) -> Result<T, E>,
    ) -> Option<Result<Self, E>> {
        let cursor = value.try_cast::<Cursor<Dynamic>>()?;
        let position = cursor.position();
        Some(inner(cursor.into_inner()).map(|inner| {
            let mut cursor = Cursor::new(inner);
            cursor.set_position(position);
            cursor
        }))
    }

    fn give<E>(self, inner: impl FnOnce(T) -> Result<Dynamic, E>) -> Result<Dynamic, E> {
        let position = self.position();
        let mut cursor = Cursor::new(inner(self.into_inner())?);
        cursor.set_position(position);
        Ok(Dynamic::from(cursor))
    }
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

fn cursor(value: &Dynamic) -> Result<Cursor<Dynamic>, Box<EvalAltResult>> {
    value
        .read_lock::<Cursor<Dynamic>>()
        .map(|cursor| cursor.clone())
        .ok_or_else(|| error(format!("expected `Cursor`, found `{}`", value.type_name())))
}

/// Run `read` on the bytes the cursor in `value` holds, from its position,
/// which then moves past what it read: std's `Read` of a `T: AsRef<[u8]>`.
fn reading<R>(
    value: &mut Dynamic,
    read: impl FnOnce(&mut Cursor<&[u8]>) -> R,
) -> Result<R, Box<EvalAltResult>> {
    let actual = value.type_name();
    let mut held = value
        .write_lock::<Cursor<Dynamic>>()
        .ok_or_else(|| error(format!("expected `Cursor`, found `{actual}`")))?;
    let bytes = super::fs::contents(held.get_ref())?;
    let mut bytes = Cursor::new(bytes.as_slice());
    bytes.set_position(held.position());
    let read = read(&mut bytes);
    held.set_position(bytes.position());
    Ok(read)
}

/// An `io::Result` of `result`, its value made by `made`.
fn io_result<T>(result: std::io::Result<T>, made: impl FnOnce(T) -> Dynamic) -> Dynamic {
    match result {
        Ok(value) => crate::result::ok(made(value)),
        Err(failure) => crate::result::err(super::io::made(failure)),
    }
}

fn int(value: u64) -> Dynamic {
    Dynamic::from_int(crate::INT::try_from(value).unwrap_or(crate::INT::MAX))
}

fn count(value: usize) -> Dynamic {
    Dynamic::from_int(crate::INT::try_from(value).unwrap_or(crate::INT::MAX))
}

/// The bytes `value` holds, as the program's `Vec<u8>`.
fn bytes(value: &[u8]) -> Dynamic {
    Dynamic::from(
        value
            .iter()
            .map(|byte| Dynamic::from_int(crate::INT::from(*byte)))
            .collect::<crate::Array>(),
    )
}

/// `io::Result`, then the `buffer` with what `read` appended, which goes
/// back to the place it was borrowed from.
fn appended_bytes(
    value: &mut Dynamic,
    buffer: &Dynamic,
    read: impl FnOnce(&mut Cursor<&[u8]>, &mut Vec<u8>) -> std::io::Result<usize>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let mut buffer = super::fs::contents(buffer)?;
    let result = reading(value, |bytes| read(bytes, &mut buffer))?;
    Ok(Dynamic::from(vec![
        io_result(result, count),
        bytes(&buffer),
    ]))
}

/// The `SeekFrom` a program passes.
fn seek_from(value: &Dynamic) -> Result<SeekFrom, Box<EvalAltResult>> {
    value
        .read_lock::<SeekFrom>()
        .map(|from| *from)
        .ok_or_else(|| {
            error(format!(
                "expected `SeekFrom`, found `{}`",
                value.type_name()
            ))
        })
}

/// Bind the cursor's functions and methods.
pub(super) fn bind(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    binds.attest::<Cursor<Dynamic>>("io::Cursor");
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("io::Cursor::new", |args| {
        Ok(Dynamic::from(Cursor::new(args[0].take())))
    });
    bind("io::Cursor::into_inner", |args| {
        Ok(cursor(args[0])?.into_inner())
    });
    bind("io::Cursor::get_ref", |args| {
        Ok(cursor(args[0])?.get_ref().clone())
    });
    bind("io::Cursor::position", |args| {
        Ok(int(cursor(args[0])?.position()))
    });
    bind("io::Cursor::set_position", |args| {
        let position = args[1]
            .as_int()
            .ok()
            .and_then(|position| u64::try_from(position).ok())
            .ok_or_else(|| error("expected a `u64` position"))?;
        let actual = args[0].type_name();
        args[0]
            .write_lock::<Cursor<Dynamic>>()
            .ok_or_else(|| error(format!("expected `Cursor`, found `{actual}`")))?
            .set_position(position);
        Ok(Dynamic::UNIT)
    });
    bind("io::Cursor::read_to_end", |args| {
        let [value, buffer, ..] = args else {
            return Err(error("`read_to_end` takes a buffer"));
        };
        appended_bytes(value, buffer, |bytes, buffer| bytes.read_to_end(buffer))
    });
    bind("io::Cursor::read_to_string", |args| {
        let [value, buffer, ..] = args else {
            return Err(error("`read_to_string` takes a buffer"));
        };
        let buffer = &**buffer;
        super::io::appended(buffer, |text| {
            reading(value, |bytes| bytes.read_to_string(text))
                .unwrap_or_else(|failure| Err(std::io::Error::other(failure.to_string())))
        })
    });
    bind("io::Cursor::read_line", |args| {
        let [value, buffer, ..] = args else {
            return Err(error("`read_line` takes a buffer"));
        };
        let buffer = &**buffer;
        super::io::appended(buffer, |text| {
            reading(value, |bytes| bytes.read_line(text))
                .unwrap_or_else(|failure| Err(std::io::Error::other(failure.to_string())))
        })
    });
    bind("io::Cursor::seek", |args| {
        let from = seek_from(args[1])?;
        Ok(io_result(reading(args[0], |bytes| bytes.seek(from))?, int))
    });
    bind("io::Cursor::stream_position", |args| {
        Ok(io_result(
            reading(args[0], |bytes| bytes.stream_position())?,
            int,
        ))
    });
    bind("io::Cursor::rewind", |args| {
        Ok(io_result(reading(args[0], |bytes| bytes.rewind())?, |()| {
            Dynamic::UNIT
        }))
    });
}
