//! std's `io::BufReader<R>` and `io::BufWriter<W>` for Rawr code: Rust's
//! own, over the reader or writer the program gives, which the facade
//! holds. A `BufReader` reads a `File`, `Stdin` or `Cursor` through its
//! own `Read`, and a `BufWriter` writes a `File` through its `Write`, so
//! what each buffers is what std's buffers. A value is a handle to the
//! buffer's one place, as a `File`'s is; the buffer flushes when the
//! program's last handle to a `BufWriter` goes, as std's does on drop.

use std::io::{BufRead as _, Cursor, Read as _, Write as _};
use std::sync::{Arc, Mutex, PoisonError};

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// What rawr runs for a buffered reader's or writer's value.
const BUFFERED: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Send,
    capability::Kind::Static,
];

/// The types the module holds.
pub(super) const HELD: [Held; 2] = [
    Held::Value(held::Value {
        std: "std::io::BufReader",
        parameters: &["R"],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .type_::<BufReader>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: false,
        runs: BUFFERED,
    }),
    Held::Value(held::Value {
        std: "std::io::BufWriter",
        parameters: &["W"],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .type_::<BufWriter>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: false,
        runs: BUFFERED,
    }),
];

/// The reader or writer the program gave, as rawr holds it.
struct Inner(Dynamic);

impl std::io::Read for Inner {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if let Some(file) = self
            .0
            .read_lock::<super::file::File>()
            .map(|file| file.clone())
        {
            return file.with(|file| file.read(buffer));
        }
        if self.0.is::<super::io::Stdin>() {
            return std::io::stdin().read(buffer);
        }
        if self.0.is::<Cursor<Dynamic>>() {
            return super::cursor::reading(&mut self.0, |bytes| bytes.read(buffer))
                .unwrap_or_else(|failure| Err(std::io::Error::other(failure.to_string())));
        }
        Err(unheld(&self.0))
    }
}

impl std::io::Write for Inner {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self
            .0
            .read_lock::<super::file::File>()
            .map(|file| file.clone())
        {
            Some(file) => file.with(|file| file.write(bytes)),
            None => Err(unheld(&self.0)),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self
            .0
            .read_lock::<super::file::File>()
            .map(|file| file.clone())
        {
            Some(file) => file.with(|file| file.flush()),
            None => Err(unheld(&self.0)),
        }
    }
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, formatter)
    }
}

/// The failure of a read or write of a value rawr buffers none of.
fn unheld(value: &Dynamic) -> std::io::Error {
    std::io::Error::other(format!("rawr doesn't buffer `{}` yet", value.type_name()))
}

/// std's `BufReader`, which `into_inner` takes its reader back out of.
#[derive(Clone)]
pub(crate) struct BufReader(Arc<Mutex<Option<std::io::BufReader<Inner>>>>);

/// std's `BufWriter`, which `into_inner` takes its writer back out of.
#[derive(Clone)]
pub(crate) struct BufWriter(Arc<Mutex<Option<std::io::BufWriter<Inner>>>>);

impl std::fmt::Debug for BufReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let buffered = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        std::fmt::Debug::fmt(&*buffered, formatter)
    }
}

impl std::fmt::Debug for BufWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let buffered = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        std::fmt::Debug::fmt(&*buffered, formatter)
    }
}

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<BufReader>();
    crate::host::debug::register::<BufWriter>();
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

fn count(value: usize) -> Dynamic {
    Dynamic::from_int(crate::INT::try_from(value).unwrap_or(crate::INT::MAX))
}

/// The capacity a program passes.
fn capacity(value: &Dynamic) -> Result<usize, Box<EvalAltResult>> {
    value
        .as_int()
        .ok()
        .and_then(|capacity| usize::try_from(capacity).ok())
        .ok_or_else(|| error("expected a `usize` capacity"))
}

/// An `io::Result` of `result`, its value made by `made`.
fn io_result<T>(result: std::io::Result<T>, made: impl FnOnce(T) -> Dynamic) -> Dynamic {
    match result {
        Ok(value) => crate::result::ok(made(value)),
        Err(failure) => crate::result::err(super::io::made(failure)),
    }
}

/// The program's value of `reader`.
fn made_reader(reader: std::io::BufReader<Inner>) -> Dynamic {
    Dynamic::from(BufReader(Arc::new(Mutex::new(Some(reader)))))
}

/// The program's value of `writer`.
fn made_writer(writer: std::io::BufWriter<Inner>) -> Dynamic {
    Dynamic::from(BufWriter(Arc::new(Mutex::new(Some(writer)))))
}

/// Run `visit` on the reader the `BufReader` in `value` buffers.
fn reader<T>(
    value: &Dynamic,
    visit: impl FnOnce(&mut std::io::BufReader<Inner>) -> T,
) -> Result<T, Box<EvalAltResult>> {
    let held = value
        .read_lock::<BufReader>()
        .map(|reader| reader.clone())
        .ok_or_else(|| {
            error(format!(
                "expected `BufReader`, found `{}`",
                value.type_name()
            ))
        })?;
    let mut buffered = held.0.lock().unwrap_or_else(PoisonError::into_inner);
    let reader = buffered
        .as_mut()
        .ok_or_else(|| error("the `BufReader` gave its reader back"))?;
    Ok(visit(reader))
}

/// Run `visit` on the writer the `BufWriter` in `value` buffers.
fn writer<T>(
    value: &Dynamic,
    visit: impl FnOnce(&mut std::io::BufWriter<Inner>) -> T,
) -> Result<T, Box<EvalAltResult>> {
    let held = value
        .read_lock::<BufWriter>()
        .map(|writer| writer.clone())
        .ok_or_else(|| {
            error(format!(
                "expected `BufWriter`, found `{}`",
                value.type_name()
            ))
        })?;
    let mut buffered = held.0.lock().unwrap_or_else(PoisonError::into_inner);
    let writer = buffered
        .as_mut()
        .ok_or_else(|| error("the `BufWriter` gave its writer back"))?;
    Ok(visit(writer))
}

/// Bind the types' functions and methods.
pub(super) fn bind(binds: &mut impl super::Bind) {
    binds.attest::<BufReader>("io::BufReader");
    binds.attest::<BufWriter>("io::BufWriter");
    bind_reader(binds);
    bind_writer(binds);
}

/// Bind `BufReader`'s own functions and its `Read` and `BufRead`.
fn bind_reader(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("io::BufReader::new", |args| {
        Ok(made_reader(std::io::BufReader::new(Inner(args[0].take()))))
    });
    bind("io::BufReader::with_capacity", |args| {
        let capacity = capacity(args[0])?;
        Ok(made_reader(std::io::BufReader::with_capacity(
            capacity,
            Inner(args[1].take()),
        )))
    });
    bind("io::BufReader::capacity", |args| {
        reader(args[0], |reader| count(reader.capacity()))
    });
    bind("io::BufReader::get_ref", |args| {
        reader(args[0], |reader| reader.get_ref().0.clone())
    });
    bind("io::BufReader::into_inner", |args| {
        let held = args[0]
            .read_lock::<BufReader>()
            .map(|reader| reader.clone())
            .ok_or_else(|| error("expected `BufReader`"))?;
        let taken = held.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        taken
            .map(|reader| reader.into_inner().0)
            .ok_or_else(|| error("the `BufReader` gave its reader back"))
    });
    bind("io::BufReader::read_line", |args| {
        let [value, buffer, ..] = args else {
            return Err(error("`read_line` takes a buffer"));
        };
        let value = &**value;
        super::io::appended(buffer, |text| {
            reader(value, |reader| reader.read_line(text))
                .unwrap_or_else(|failure| Err(std::io::Error::other(failure.to_string())))
        })
    });
    bind("io::BufReader::read_to_string", |args| {
        let [value, buffer, ..] = args else {
            return Err(error("`read_to_string` takes a buffer"));
        };
        let value = &**value;
        super::io::appended(buffer, |text| {
            reader(value, |reader| reader.read_to_string(text))
                .unwrap_or_else(|failure| Err(std::io::Error::other(failure.to_string())))
        })
    });
    bind("io::BufReader::read_to_end", |args| {
        let mut buffer = super::fs::contents(args[1])?;
        let read = reader(args[0], |reader| reader.read_to_end(&mut buffer))?;
        Ok(Dynamic::from(vec![
            io_result(read, count),
            super::cursor::bytes(&buffer),
        ]))
    });
    // `BufRead::lines`, which takes the reader: each line it has left.
    bind("io::BufReader::lines", |args| {
        let lines = reader(args[0], |reader| {
            reader
                .lines()
                .map(|line| match line {
                    Ok(line) => crate::result::ok(Dynamic::from(ImmutableString::from(line))),
                    Err(failure) => crate::result::err(super::io::made(failure)),
                })
                .collect::<crate::Array>()
        })?;
        Ok(Dynamic::from(lines))
    });
}

/// Bind `BufWriter`'s own functions and its `Write`.
fn bind_writer(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("io::BufWriter::new", |args| {
        Ok(made_writer(std::io::BufWriter::new(Inner(args[0].take()))))
    });
    bind("io::BufWriter::with_capacity", |args| {
        let capacity = capacity(args[0])?;
        Ok(made_writer(std::io::BufWriter::with_capacity(
            capacity,
            Inner(args[1].take()),
        )))
    });
    bind("io::BufWriter::capacity", |args| {
        writer(args[0], |writer| count(writer.capacity()))
    });
    bind("io::BufWriter::get_ref", |args| {
        writer(args[0], |writer| writer.get_ref().0.clone())
    });
    bind("io::BufWriter::write", |args| {
        let bytes = super::fs::contents(args[1])?;
        Ok(io_result(
            writer(args[0], |writer| writer.write(&bytes))?,
            count,
        ))
    });
    bind("io::BufWriter::write_all", |args| {
        let bytes = super::fs::contents(args[1])?;
        Ok(io_result(
            writer(args[0], |writer| writer.write_all(&bytes))?,
            |()| Dynamic::UNIT,
        ))
    });
    bind("io::BufWriter::write_fmt", |args| {
        let text = args[1]
            .read_lock::<ImmutableString>()
            .map(|text| text.to_string())
            .ok_or_else(|| error("expected `Arguments`"))?;
        Ok(io_result(
            writer(args[0], |writer| writer.write_all(text.as_bytes()))?,
            |()| Dynamic::UNIT,
        ))
    });
    bind("io::BufWriter::flush", |args| {
        Ok(io_result(writer(args[0], |writer| writer.flush())?, |()| {
            Dynamic::UNIT
        }))
    });
}
