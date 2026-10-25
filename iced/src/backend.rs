//! iced's `backend` module: `Backend`, `backend::Settings` and
//! `backend::configure`, which switches the renderer of a running app.
//!
//! `configure` is iced's own task. A native window's shell builds a new
//! compositor for the settings and answers with its result. A host that has
//! no compositor to switch, as iced's wasm shell and its test emulator, drops
//! the task's sender, so the task ends with no output. The headless renderer
//! behind `?play` and previews does the same.

use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{capability, receiver, Dynamic, EvalAltResult, Position};

use iced::backend::{self, Api, PowerPreference};
use iced::Backend;

/// `Backend` and `backend::Settings`: `Clone` and `Eq`, not `Copy`.
const OWNED_CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Default,
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::PartialEq,
    capability::Kind::Eq,
    capability::Kind::Send,
    capability::Kind::Static,
];

/// `backend::Error`: no `Default`.
const ERROR_CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::PartialEq,
    capability::Kind::Eq,
    capability::Kind::Send,
    capability::Kind::Static,
];

/// `Api`'s variants, in the order iced declares them.
const APIS: [(&str, Api); 6] = [
    ("Best", Api::Best),
    ("Vulkan", Api::Vulkan),
    ("Metal", Api::Metal),
    ("DirectX12", Api::DirectX12),
    ("OpenGL", Api::OpenGL),
    ("WebGPU", Api::WebGPU),
];

/// `PowerPreference`'s variants.
const POWER_PREFERENCES: [(&str, PowerPreference); 3] = [
    ("None", PowerPreference::None),
    ("LowPower", PowerPreference::LowPower),
    ("HighPerformance", PowerPreference::HighPerformance),
];

/// `backend::Settings`' fields.
const SETTINGS_FIELDS: [&str; 4] = ["backend", "power_preference", "antialiasing", "vsync"];

/// `{}` and `{:?}` of the module's values print iced's own text.
pub(crate) fn register_debug() {
    use rawr::host::{debug, display};
    display::register::<Backend>();
    display::register::<Api>();
    display::register::<backend::Error>();
    debug::register::<Backend>();
    debug::register::<Api>();
    debug::register::<PowerPreference>();
    debug::register::<backend::Settings>();
    debug::register::<backend::Error>();
}

/// Declare the module's types and items in the `iced` facade.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    builder.module("backend")?;
    builder.default_value_type::<Backend>("Backend", &[], OWNED_CAPABILITIES)?;
    builder.alias("backend::Backend", "Backend")?;
    builder.default_value_type::<Api>(
        "backend::Api",
        &[],
        super::OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<PowerPreference>(
        "PowerPreference",
        &[],
        super::OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.alias("backend::PowerPreference", "PowerPreference")?;
    builder.default_value_type::<backend::Settings>(
        "backend::Settings",
        &[],
        &[
            capability::Kind::Default,
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::PartialEq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.value_type::<backend::Error>("backend::Error", &[], ERROR_CAPABILITIES)?;

    let ty = type_ref::Type::registered;
    let string = || type_ref::Type::builtin(type_ref::Builtin::String);
    let boolean = || type_ref::Type::builtin(type_ref::Builtin::Bool);
    let signature = |inputs: Vec<type_ref::Type>, output: type_ref::Type| {
        let mut signature = signature::Builder::new();
        for input in inputs {
            signature.parameter(input);
        }
        signature.output(output);
        signature.finish()
    };

    // `Backend::ALL`, the variants without data, `Backend::default()` (which
    // reads `ICED_BACKEND`) and the variants that carry data.
    builder.item(item::Definition::associated_value(
        "Backend::ALL",
        type_ref::Type::slice(ty("Backend")?),
    )?)?;
    for name in ["Best", "Software"] {
        builder.item(item::Definition::associated_value(
            format!("Backend::{name}"),
            ty("Backend")?,
        )?)?;
    }
    for (name, input) in [("Hardware", ty("backend::Api")?), ("Custom", string())] {
        builder.item(item::Definition::associated_function(
            format!("Backend::{name}"),
            signature(vec![input], ty("Backend")?),
        )?)?;
    }
    builder.item(item::Definition::associated_function(
        "Backend::from",
        signature(vec![string()], ty("Backend")?),
    )?)?;
    for (name, output) in [
        ("hardware", type_ref::Type::option(ty("backend::Api")?)),
        ("is_software", boolean()),
        ("to_string", string()),
    ] {
        builder.item(item::Definition::method(
            format!("Backend::{name}"),
            receiver::Mode::Shared,
            signature(Vec::new(), output),
        )?)?;
    }
    builder.item(item::Definition::method(
        "Backend::matches",
        receiver::Mode::Shared,
        signature(vec![string()], boolean()),
    )?)?;

    for (name, _) in APIS {
        builder.item(item::Definition::associated_value(
            format!("backend::Api::{name}"),
            ty("backend::Api")?,
        )?)?;
    }
    builder.item(item::Definition::method(
        "backend::Api::to_string",
        receiver::Mode::Shared,
        signature(Vec::new(), string()),
    )?)?;
    for (name, _) in POWER_PREFERENCES {
        builder.item(item::Definition::associated_value(
            format!("PowerPreference::{name}"),
            ty("PowerPreference")?,
        )?)?;
    }
    for path in [
        "Backend",
        "backend::Api",
        "PowerPreference",
        "backend::Settings",
    ] {
        builder.item(item::Definition::associated_function(
            format!("{path}::default"),
            signature(Vec::new(), ty(path)?),
        )?)?;
    }

    // `backend::Settings { backend, ..backend::Settings::default() }`: each
    // field read, or set on a copy.
    for name in SETTINGS_FIELDS {
        let field = match name {
            "backend" => ty("Backend")?,
            "power_preference" => ty("PowerPreference")?,
            _ => boolean(),
        };
        builder.item(item::Definition::method(
            format!("backend::Settings::{name}"),
            receiver::Mode::Shared,
            signature(Vec::new(), field.clone()),
        )?)?;
        builder.item(item::Definition::method(
            format!("backend::Settings::{name}"),
            receiver::Mode::Owned,
            signature(vec![field], ty("backend::Settings")?),
        )?)?;
    }
    builder.item(item::Definition::method(
        "backend::Error::to_string",
        receiver::Mode::Shared,
        signature(Vec::new(), string()),
    )?)?;

    // `backend::configure(settings)`: iced's task that switches the renderer.
    builder.item(item::Definition::function(
        "backend::configure",
        signature(
            vec![ty("backend::Settings")?],
            type_ref::Type::applied(
                "Task",
                [type_ref::Type::result(
                    type_ref::Type::unit(),
                    ty("backend::Error")?,
                )],
            )?,
        ),
    )?)?;
    Ok(())
}

/// Bind the module's items to iced's own values and functions.
pub(crate) fn bind(builder: &mut host::runtime::Builder) {
    fn value<T: Clone + Send + Sync + 'static>(
        builder: &mut host::runtime::Builder,
        path: String,
        value: T,
    ) {
        builder
            .bind(path, move |_| Ok(Dynamic::from(value.clone())))
            .expect("the iced facade must retain every backend value");
    }
    value(
        builder,
        "iced::Backend::ALL".into(),
        Backend::ALL
            .iter()
            .cloned()
            .map(Dynamic::from)
            .collect::<rawr::Array>(),
    );
    value(builder, "iced::Backend::Best".into(), Backend::Best);
    value(builder, "iced::Backend::Software".into(), Backend::Software);
    for (name, api) in APIS {
        value(builder, format!("iced::backend::Api::{name}"), api);
    }
    for (name, preference) in POWER_PREFERENCES {
        value(
            builder,
            format!("iced::PowerPreference::{name}"),
            preference,
        );
    }
    value(
        builder,
        "iced::backend::Api::default".into(),
        Api::default(),
    );
    value(
        builder,
        "iced::PowerPreference::default".into(),
        PowerPreference::default(),
    );
    value(
        builder,
        "iced::backend::Settings::default".into(),
        backend::Settings::default(),
    );
    // iced's `Backend::default()` reads `ICED_BACKEND` each time.
    builder
        .bind("iced::Backend::default", |_| {
            Ok(Dynamic::from(Backend::default()))
        })
        .expect("the iced facade must retain Backend::default");
    builder
        .bind("iced::Backend::Hardware", |args| {
            let [api] = args else {
                return Err(arity("Backend::Hardware", 1, args.len()));
            };
            Ok(Dynamic::from(Backend::Hardware(read::<Api>(
                api,
                "Backend::Hardware",
            )?)))
        })
        .expect("the iced facade must retain Backend::Hardware");
    for path in ["iced::Backend::Custom", "iced::Backend::from"] {
        builder
            .bind(path, move |args| {
                let [name] = args else {
                    return Err(arity(path, 1, args.len()));
                };
                Ok(Dynamic::from(Backend::from(string(name, path)?)))
            })
            .expect("the iced facade must retain every Backend constructor");
    }
    builder
        .bind("iced::Backend::hardware", |args| {
            let [backend] = args else {
                return Err(arity("Backend::hardware", 1, args.len()));
            };
            Ok(rawr::option::from(
                read::<Backend>(backend, "Backend::hardware")?.hardware(),
            ))
        })
        .expect("the iced facade must retain Backend::hardware");
    builder
        .bind("iced::Backend::is_software", |args| {
            let [backend] = args else {
                return Err(arity("Backend::is_software", 1, args.len()));
            };
            Ok(Dynamic::from(
                read::<Backend>(backend, "Backend::is_software")?.is_software(),
            ))
        })
        .expect("the iced facade must retain Backend::is_software");
    builder
        .bind("iced::Backend::matches", |args| {
            let [backend, target] = args else {
                return Err(arity("Backend::matches", 2, args.len()));
            };
            let target = string(target, "Backend::matches")?;
            Ok(Dynamic::from(
                read::<Backend>(backend, "Backend::matches")?.matches(&target),
            ))
        })
        .expect("the iced facade must retain Backend::matches");
    display::<Backend>(builder, "iced::Backend::to_string");
    display::<Api>(builder, "iced::backend::Api::to_string");
    display::<backend::Error>(builder, "iced::backend::Error::to_string");

    for name in SETTINGS_FIELDS {
        let path = format!("iced::backend::Settings::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let mut settings = read::<backend::Settings>(receiver, &path)?;
                let [operand] = operands else {
                    return Ok(match name {
                        "backend" => Dynamic::from(settings.backend),
                        "power_preference" => Dynamic::from(settings.power_preference),
                        "antialiasing" => Dynamic::from(settings.antialiasing),
                        _ => Dynamic::from(settings.vsync),
                    });
                };
                match name {
                    "backend" => settings.backend = read(operand, &path)?,
                    "power_preference" => settings.power_preference = read(operand, &path)?,
                    "antialiasing" => settings.antialiasing = boolean(operand, &path)?,
                    _ => settings.vsync = boolean(operand, &path)?,
                }
                Ok(Dynamic::from(settings))
            })
            .expect("the iced facade must retain every backend::Settings field");
    }

    builder
        .bind_with_context("iced::backend::configure", |call, args| {
            let [settings] = args else {
                return Err(arity("backend::configure", 1, args.len()));
            };
            let settings = read::<backend::Settings>(settings, "backend::configure")?;
            let witness = call
                .output_witness()
                .map_err(|error| runtime_error(error.to_string()))?;
            let task = iced::backend::configure(settings).map(|result| {
                Box::new(move |_: Option<&host::runtime::Witness>| {
                    Ok(match result {
                        Ok(()) => rawr::result::ok(Dynamic::UNIT),
                        Err(error) => rawr::result::err(Dynamic::from(error)),
                    })
                }) as crate::task::Materialize
            });
            crate::task::imported(task)
                .try_cast::<host::runtime::Unwitnessed>()
                .and_then(|task| task.witness(witness))
                .ok_or_else(|| runtime_error("backend::configure lost its task".into()))
        })
        .expect("the iced facade must retain backend::configure");
}

fn display<T: std::fmt::Display + Clone + Send + Sync + 'static>(
    builder: &mut host::runtime::Builder,
    path: &'static str,
) {
    builder
        .bind(path, move |args| {
            let [value] = args else {
                return Err(arity(path, 1, args.len()));
            };
            value
                .read_lock::<T>()
                .map(|value| Dynamic::from(value.to_string()))
                .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))
        })
        .expect("the iced facade must retain every backend to_string");
}

fn read<T: Clone + 'static>(value: &Dynamic, operation: &str) -> Result<T, Box<EvalAltResult>> {
    value
        .read_lock::<T>()
        .map(|value| value.clone())
        .ok_or_else(|| {
            runtime_error(format!(
                "`{operation}` expects a `{}`, found `{}`",
                std::any::type_name::<T>(),
                value.type_name()
            ))
        })
}

fn string(value: &Dynamic, operation: &str) -> Result<String, Box<EvalAltResult>> {
    value
        .as_immutable_string_ref()
        .map(|text| text.to_string())
        .map_err(|_| runtime_error(format!("`{operation}` expects a `String`")))
}

fn boolean(value: &Dynamic, operation: &str) -> Result<bool, Box<EvalAltResult>> {
    value
        .as_bool()
        .map_err(|_| runtime_error(format!("`{operation}` expects a `bool`")))
}

fn arity(operation: &str, expected: usize, actual: usize) -> Box<EvalAltResult> {
    runtime_error(format!(
        "`{operation}` received {actual} operand(s), expected {expected}"
    ))
}

fn runtime_error(message: String) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}
