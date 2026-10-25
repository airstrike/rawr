//! The core future type as Rawr code names it (D511.3): `async` blocks and
//! `async fn`s in a program without iced give a `rawr::future::Future<T>`,
//! and so do native async functions in the plain runner. `join!` is
//! `rawr::future::__join{N}`. A host whose own type runs futures, as
//! rawr-iced's `Task` does, takes precedence where it is installed.

use crate::host::{self, facade, item, signature, type_ref};
use crate::{Dynamic, EvalAltResult, Position};

/// The facade's root.
pub const ROOT: &str = "rawr";
/// The future type, qualified.
pub const FUTURE: &str = "rawr::future::Future";
/// The function an async body's step function is passed to.
pub const SCRIPT: &str = "rawr::future::script";
/// The most futures `join!` takes.
pub const MOST_FUTURES: usize = 5;

/// The `join!` function for `count` futures.
#[must_use]
pub fn join_path(count: usize) -> String {
    format!("rawr::future::__join{count}")
}

/// The facade's compile-time definition.
pub fn definition() -> Result<facade::Definition, host::Error> {
    use type_ref::Type;
    let mut builder = facade::Builder::new(ROOT, facade::Abi::new(1)?)?;
    builder.module("future")?;
    builder.type_::<super::Future>("future::Future", &["T"], &[])?;
    let future = |output: Type| Type::applied("future::Future", [output]);

    let mut script = signature::Builder::new();
    script.type_parameter("Resume", &[])?;
    script.type_parameter("Output", &[])?;
    script.parameter(Type::callable(
        Type::parameter("Resume")?,
        Type::parameter("Output")?,
    ));
    script.output(future(Type::parameter("Output")?)?);
    builder.item(item::Definition::function(
        "future::script",
        script.finish(),
    )?)?;

    const NAMES: [&str; MOST_FUTURES] = ["A", "B", "C", "D", "F"];
    for count in 2..=MOST_FUTURES {
        let names = &NAMES[..count];
        let mut join = signature::Builder::new();
        let mut outputs = Vec::with_capacity(count);
        for name in names {
            join.type_parameter(*name, &[])?;
        }
        for name in names {
            join.parameter(future(Type::parameter(*name)?)?);
            outputs.push(Type::parameter(*name)?);
        }
        join.output(future(Type::tuple(outputs)?)?);
        builder.item(item::Definition::function(
            format!("future::__join{count}"),
            join.finish(),
        )?)?;
    }
    builder.finish()
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

fn take_future(value: &mut Dynamic) -> Result<super::Future, Box<EvalAltResult>> {
    let actual = value.type_name();
    value
        .take()
        .try_cast_result::<super::Future>()
        .map_err(|_| error(format!("expected a future, found `{actual}`")))
}

/// The facade with its runtime bindings.
pub fn runtime() -> Result<host::runtime::Facade, host::Error> {
    let mut builder = host::runtime::Builder::new(definition()?);
    builder
        .bind_type_attestation(FUTURE, |value, expected| {
            expected.attest(value.is::<super::Future>())
        })
        .expect("the rawr facade declares its future once");
    builder
        .bind_with_context(SCRIPT, |_call, args| {
            let [step] = args else {
                return Err(error("`rawr::future::script` takes one step function"));
            };
            let actual = step.type_name();
            let step = step
                .take()
                .try_cast_result::<crate::item::Value>()
                .map_err(|_| error(format!("an async body's step is `{actual}`")))?;
            Ok(Dynamic::from(super::Future::script(step)))
        })
        .expect("the rawr facade declares `future::script` once");
    for count in 2..=MOST_FUTURES {
        builder
            .bind_with_context(join_path(count), |_call, args| {
                let parts = args
                    .iter_mut()
                    .map(|part| take_future(part))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Dynamic::from(super::Future::join(parts)))
            })
            .expect("the rawr facade declares each `future::__join` once");
    }
    Ok(builder.finish())
}
