//! rand for a snippet: `rand::rng()`, `use rand::RngExt;` and
//! `random_range` over a range of floats or integers, drawn by rand's own
//! `StdRng`.
//!
//! rand's `rng()` gives a handle to the thread's generator, seeded from the
//! system. Here the host seeds one generator with [`seed`], and every
//! `rng()` handle draws from it. A server render seeds it the same way each
//! time, so a snippet's frames repeat, as its frozen clock makes them.

use std::sync::{Mutex, PoisonError};

use rand::{RngExt as _, SeedableRng as _};
use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{capability, receiver, Dynamic, EvalAltResult, Position, FLOAT, INT};

/// The seed a server render takes, so the same snippet draws the same.
pub const RENDER_SEED: u64 = 0x005e_ed0f_57a2;

static GENERATOR: Mutex<Option<rand::rngs::StdRng>> = Mutex::new(None);

/// Seed the generator every `rand::rng()` handle draws from.
pub fn seed(seed: u64) {
    *GENERATOR.lock().unwrap_or_else(PoisonError::into_inner) =
        Some(rand::rngs::StdRng::seed_from_u64(seed));
}

/// Draw with the generator, seeded with [`RENDER_SEED`] when no host
/// seeded it.
fn draw<T>(f: impl FnOnce(&mut rand::rngs::StdRng) -> T) -> T {
    let mut generator = GENERATOR.lock().unwrap_or_else(PoisonError::into_inner);
    f(generator.get_or_insert_with(|| rand::rngs::StdRng::seed_from_u64(RENDER_SEED)))
}

/// rand's `ThreadRng`: a handle to the generator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThreadRng;

const HANDLE: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::Default,
    capability::Kind::Send,
    capability::Kind::Static,
];

/// The `rand` facade.
pub fn facade() -> Result<facade::Definition, host::Error> {
    let mut builder = facade::Builder::new("rand", facade::Abi::new(16)?)?;
    builder.module("rngs")?;
    builder.module("prelude")?;
    // A trait the program names is a module, as an imported crate's is.
    builder.module("Rng")?;
    builder.module("RngExt")?;
    builder.default_type::<ThreadRng>("rngs::ThreadRng", &[], HANDLE)?;
    builder.alias("prelude::ThreadRng", "rngs::ThreadRng")?;
    let thread_rng = || type_ref::Type::registered("rngs::ThreadRng");
    let mut rng = signature::Builder::new();
    rng.output(thread_rng()?);
    builder.item(item::Definition::function("rng", rng.finish())?)?;

    // `random_range(low..high)` over floats, `low..=high` over floats or
    // integers: `T` is the range's own, as rand's `SampleRange<T>` gives it.
    for owner in [Some("rngs::ThreadRng"), None] {
        for form in [Form::Float, Form::FloatInclusive, Form::IntegerInclusive] {
            let mut signature = signature::Builder::new();
            let output = match form {
                Form::Float => {
                    signature.type_parameter("T", &[])?;
                    signature.parameter(type_ref::Type::range(type_ref::Type::parameter("T")?));
                    type_ref::Type::parameter("T")?
                }
                Form::FloatInclusive => {
                    signature.parameter(type_ref::Type::builtin(
                        type_ref::Builtin::FloatInclusiveRange,
                    ));
                    type_ref::Type::builtin(type_ref::Builtin::F64)
                }
                Form::IntegerInclusive => {
                    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::InclusiveRange));
                    type_ref::Type::builtin(type_ref::Builtin::I64)
                }
            };
            signature.output(output);
            builder.item(match owner {
                Some(owner) => item::Definition::method(
                    format!("{owner}::random_range"),
                    receiver::Mode::Mutable,
                    signature.finish(),
                )?,
                None => item::Definition::function("random_range", signature.finish())?,
            })?;
        }
        let mut boolean = signature::Builder::new();
        boolean.parameter(type_ref::Type::builtin(type_ref::Builtin::F64));
        boolean.output(type_ref::Type::builtin(type_ref::Builtin::Bool));
        builder.item(match owner {
            Some(owner) => item::Definition::method(
                format!("{owner}::random_bool"),
                receiver::Mode::Mutable,
                boolean.finish(),
            )?,
            None => item::Definition::function("random_bool", boolean.finish())?,
        })?;
    }
    builder.finish()
}

#[derive(Clone, Copy)]
enum Form {
    Float,
    FloatInclusive,
    IntegerInclusive,
}

/// The runtime bindings of [`facade`].
pub fn runtime() -> Result<host::runtime::Facade, host::Error> {
    let mut builder = host::runtime::Builder::new(facade()?);
    builder
        .bind("rand::rng", |_| Ok(Dynamic::from(ThreadRng)))
        .expect("the rand facade declares rng");
    for (path, skip) in [
        ("rand::rngs::ThreadRng::random_range", 1),
        ("rand::random_range", 0),
    ] {
        builder
            .bind(path, move |args| {
                let [range] = args.get_mut(skip..).unwrap_or_default() else {
                    return Err(error(format!("`{path}` takes one range")));
                };
                random_range(range)
            })
            .expect("the rand facade declares random_range");
    }
    for (path, skip) in [
        ("rand::rngs::ThreadRng::random_bool", 1),
        ("rand::random_bool", 0),
    ] {
        builder
            .bind(path, move |args| {
                let [p] = args.get_mut(skip..).unwrap_or_default() else {
                    return Err(error(format!("`{path}` takes one probability")));
                };
                let p = p
                    .as_float()
                    .map_err(|_| error(format!("`{path}` takes an f64")))?;
                if !(0.0..=1.0).contains(&p) {
                    // rand panics here too.
                    return Err(error(format!("p={p} is outside range [0.0, 1.0]")));
                }
                Ok(Dynamic::from(draw(|generator| generator.random_bool(p))))
            })
            .expect("the rand facade declares random_bool");
    }
    Ok(builder.finish())
}

/// rand's `random_range` over the range `range` is. An empty range is
/// rand's panic, "cannot sample empty range".
fn random_range(range: &mut Dynamic) -> Result<Dynamic, Box<EvalAltResult>> {
    let empty = || error("cannot sample empty range".into());
    if let Some(range) = range.read_lock::<std::ops::Range<FLOAT>>() {
        let range = range.clone();
        if range.is_empty() {
            return Err(empty());
        }
        return Ok(Dynamic::from_float(draw(|generator| {
            generator.random_range(range)
        })));
    }
    if let Some(range) = range.read_lock::<std::ops::RangeInclusive<FLOAT>>() {
        let range = range.clone();
        if range.is_empty() {
            return Err(empty());
        }
        return Ok(Dynamic::from_float(draw(|generator| {
            generator.random_range(range)
        })));
    }
    if let Some(range) = range.read_lock::<std::ops::RangeInclusive<INT>>() {
        let range = range.clone();
        if range.is_empty() {
            return Err(empty());
        }
        return Ok(Dynamic::from_int(draw(|generator| {
            generator.random_range(range)
        })));
    }
    Err(error(format!(
        "`random_range` takes a range, found `{}`",
        range.type_name()
    )))
}

fn error(message: String) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}
