//! The std facade's declarations in their final form, as `declared` holds
//! them: each step is one call to the facade's builder, and each type one
//! of the host types an item names. A new engine installs them as they
//! are, so its start reads nothing of the std table (#1526).

use crate::capability;
use crate::host::item;
use crate::host::type_ref::{Builtin, Type};
use crate::host::{self, facade, signature};
use crate::receiver;
use crate::std_facade::held::{self, Held};
use crate::ImmutableString;

/// A host type a declaration names.
#[derive(Debug)]
pub(super) enum Ty {
    Unit,
    Builtin(Builtin),
    /// A held type, at its facade path: `path::PathBuf`.
    Registered(&'static str),
    /// A type alias's parameter: `T` of `io::Result<T>`.
    Parameter(&'static str),
    Reference {
        mutable: bool,
        target: &'static Ty,
    },
    Option(&'static Ty),
    Result(&'static Ty, &'static Ty),
    Vec(&'static Ty),
    Slice(&'static Ty),
    Iter(&'static Ty),
    Boxed(&'static Ty),
}

/// What an item declaration declares.
#[derive(Clone, Copy, Debug)]
pub(super) enum Kind {
    Function,
    AssociatedFunction,
    Method(receiver::Mode),
    /// A module's constant: `time::UNIX_EPOCH`.
    Value,
    /// A type's constant or an enum's variant: `io::ErrorKind::NotFound`.
    AssociatedValue,
}

/// One declaration, at a path relative to the facade's root.
#[derive(Debug)]
pub(super) enum Step {
    Module(&'static str),
    /// The held type whose public path is `std`, with the capabilities std
    /// gives it that rawr runs.
    Held {
        std: &'static str,
        capabilities: &'static [capability::Kind],
    },
    /// Another type a held type stands for, as `path::Path` is `PathBuf`.
    Alias {
        path: &'static str,
        target: &'static str,
    },
    /// std's type alias, as `io::Result<T>`.
    TypeAlias {
        path: &'static str,
        parameters: &'static [&'static str],
        target: Ty,
    },
    /// An item, whose inputs are a value's none and its output its type.
    Item {
        path: &'static str,
        kind: Kind,
        inputs: &'static [Ty],
        output: Ty,
    },
    /// std's own item, which Rawr resolves by its std path.
    StdAlias {
        path: &'static str,
        std: &'static str,
    },
}

impl Ty {
    /// The host type it is.
    fn host(&self) -> Result<Type, host::Error> {
        Ok(match self {
            Self::Unit => Type::unit(),
            Self::Builtin(builtin) => Type::builtin(*builtin),
            Self::Registered(path) => Type::registered(*path)?,
            Self::Parameter(name) => Type::parameter(*name)?,
            Self::Reference { mutable, target } => Type::reference(*mutable, target.host()?),
            Self::Option(element) => Type::option(element.host()?),
            Self::Result(ok, err) => Type::result(ok.host()?, err.host()?),
            Self::Vec(element) => Type::vec(element.host()?),
            Self::Slice(element) => Type::slice(element.host()?),
            Self::Iter(element) => Type::iterator(element.host()?),
            Self::Boxed(element) => Type::boxed(element.host()?),
        })
    }
}

/// Declare `steps` in `builder`, whose held types are `held`.
pub(super) fn install(
    builder: &mut facade::Builder,
    held: &[Held],
    steps: &[Step],
) -> Result<(), host::Error> {
    for step in steps {
        match step {
            Step::Module(path) => {
                builder.module(*path)?;
            }
            Step::Held { std, capabilities } => {
                let value = held
                    .iter()
                    .find_map(|held| match held {
                        Held::Value(value) if value.std == *std => Some(value),
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("the std facade holds `{std}`"));
                (value.register)(builder, held::Value::facade(value.std), capabilities)?;
            }
            Step::Alias { path, target } => {
                builder.alias(*path, *target)?;
            }
            Step::TypeAlias {
                path,
                parameters,
                target,
            } => {
                builder.type_alias(
                    path,
                    parameters
                        .iter()
                        .map(|name| ImmutableString::from(*name))
                        .collect(),
                    Box::default(),
                    target.host()?,
                )?;
            }
            Step::Item {
                path,
                kind,
                inputs,
                output,
            } => {
                let inputs = inputs.iter().map(Ty::host).collect::<Result<_, _>>()?;
                item(builder, path, *kind, inputs, output.host()?)?;
            }
            Step::StdAlias { path, std } => {
                builder.std_alias(path, std)?;
            }
        }
    }
    Ok(())
}

/// Declare in `builder` the item of `kind` at `path` that takes `inputs`
/// and gives `output`.
pub(super) fn item(
    builder: &mut facade::Builder,
    path: &str,
    kind: Kind,
    inputs: Vec<Type>,
    output: Type,
) -> Result<(), host::Error> {
    builder.item(match kind {
        Kind::Function => item::Definition::function(path, signature(inputs, output))?,
        Kind::AssociatedFunction => {
            item::Definition::associated_function(path, signature(inputs, output))?
        }
        Kind::Method(mode) => item::Definition::method(path, mode, signature(inputs, output))?,
        Kind::Value => item::Definition::value(path, output)?,
        Kind::AssociatedValue => item::Definition::associated_value(path, output)?,
    })?;
    Ok(())
}

fn signature(inputs: Vec<Type>, output: Type) -> signature::Definition {
    let mut signature = signature::Builder::new();
    for input in inputs {
        signature.parameter(input);
    }
    signature.output(output);
    signature.finish()
}
