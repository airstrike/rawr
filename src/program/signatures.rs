//! The typed signatures of a program's free functions, which an app checks
//! a typed handle against once, when it looks the function up (D514).

use crate::{host, resolve, ImmutableString, Span};
use std::collections::BTreeMap;
use std::fmt;

/// Each visible free function's name and its typed signature.
#[derive(Debug, Default)]
pub(crate) struct Signatures(BTreeMap<ImmutableString, Signature>);

impl Signatures {
    pub(crate) fn get(&self, name: &str) -> Option<&Signature> {
        self.0.get(name)
    }

    pub(crate) fn insert(&mut self, signature: Signature) {
        self.0.insert(signature.name.clone(), signature);
    }
}

/// One free function's signature as written, and where it is written.
#[derive(Debug)]
pub(crate) struct Signature {
    name: ImmutableString,
    parameters: Box<[Ty]>,
    output: Ty,
    asynchronous: bool,
    generic: bool,
    /// From `fn` through the output type, or through the parameters'
    /// closing parenthesis when there is no output type.
    span: Span,
}

impl Signature {
    pub(crate) fn from_resolved(function: &resolve::item::Function) -> Self {
        let start = function.header.span.start();
        let end = match &function.output {
            Some(output) => span_of(output).end(),
            // The header ends at the body's opening brace.
            None => function.header.span.end(),
        };
        Self {
            name: function.name.text.clone(),
            parameters: function
                .parameters
                .iter()
                .map(|parameter| Ty::from_resolved(&parameter.ty))
                .collect(),
            output: function.output.as_ref().map_or(Ty::Unit, Ty::from_resolved),
            asynchronous: function.body.asynchronous,
            generic: !function.generics.is_empty(),
            span: Span::new(start, end),
        }
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) const fn arity(&self) -> usize {
        self.parameters.len()
    }

    pub(crate) const fn span(&self) -> Span {
        self.span
    }

    /// Whether a handle with `parameters` and `output` may call this
    /// function. A parameter the script borrows (`&str`, `&T`) takes the
    /// handle's owned value, which the call lends it.
    pub(crate) fn accepts(
        &self,
        parameters: &[Ty],
        output: &Ty,
        registry: &host::Registry,
    ) -> bool {
        !self.asynchronous
            && !self.generic
            && self.parameters.len() == parameters.len()
            && self
                .parameters
                .iter()
                .zip(parameters)
                .all(|(found, expected)| found.lends(expected, registry))
            && self.output.matches(output, registry)
    }

    /// `fn(f64, i64) -> f64`, as rustc names a function's type. `full`
    /// names each exported type by its root and path.
    pub(crate) fn display(&self, full: bool) -> String {
        let text = function_type(&self.parameters, &self.output, full);
        match (self.asynchronous, self.generic) {
            (true, _) => format!("async {text}"),
            (false, true) => format!("{text} (generic)"),
            (false, false) => text,
        }
    }
}

/// `fn(A, B) -> R`, leaving out `-> ()` as rustc does. `full` names each
/// exported type by its root and path.
pub(crate) fn function_type(parameters: &[Ty], output: &Ty, full: bool) -> String {
    let show = |ty: &Ty| {
        if full {
            format!("{ty:#}")
        } else {
            ty.to_string()
        }
    };
    let parameters = parameters.iter().map(show).collect::<Vec<_>>().join(", ");
    match output {
        Ty::Unit => format!("fn({parameters})"),
        output => format!("fn({parameters}) -> {}", show(output)),
    }
}

fn span_of(ty: &resolve::type_ref::Type) -> Span {
    match ty {
        resolve::type_ref::Type::Unit(span)
        | resolve::type_ref::Type::Tuple { span, .. }
        | resolve::type_ref::Type::Reference { span, .. } => *span,
        resolve::type_ref::Type::Path(path) => path.span,
    }
}

/// A type in a signature, as far as a handle checks it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ty {
    Unit,
    /// A builtin or any other type known by its name: `i64`, `String`,
    /// `str`, a script's own struct or a generic parameter.
    Named(ImmutableString),
    Option(Box<Ty>),
    /// An app's exported type, by its root and path, `bank::Transaction`,
    /// and its type arguments.
    Host {
        path: ImmutableString,
        arguments: Box<[Ty]>,
    },
    Tuple(Box<[Ty]>),
    Reference {
        mutable: bool,
        target: Box<Ty>,
    },
}

impl Ty {
    fn from_resolved(ty: &resolve::type_ref::Type) -> Self {
        // A header's `&str` resolves to `String` with the reference's
        // lifetime.
        if ty.is_str() {
            return Self::Reference {
                mutable: false,
                target: Box::new(Self::Named("str".into())),
            };
        }
        match ty {
            resolve::type_ref::Type::Unit(_) => Self::Unit,
            resolve::type_ref::Type::Tuple { members, .. } => {
                Self::Tuple(members.iter().map(Self::from_resolved).collect())
            }
            resolve::type_ref::Type::Reference {
                mutable, target, ..
            } => Self::Reference {
                mutable: *mutable,
                target: Box::new(Self::from_resolved(target)),
            },
            resolve::type_ref::Type::Path(path) => {
                let types = || {
                    path.arguments.iter().filter_map(|argument| match argument {
                        resolve::type_ref::Argument::Type(ty) => Some(ty),
                        resolve::type_ref::Argument::Lifetime(_) => None,
                    })
                };
                let mut arguments = types();
                match &path.target {
                    resolve::type_ref::Target::Builtin if path.name.as_str() == "Option" => {
                        match (arguments.next(), arguments.next()) {
                            (Some(inner), None) => {
                                Self::Option(Box::new(Self::from_resolved(inner)))
                            }
                            _ => Self::Named(path.name.clone()),
                        }
                    }
                    resolve::type_ref::Target::Host { definition, .. } => Self::Host {
                        path: definition.path().into(),
                        arguments: types().map(Self::from_resolved).collect(),
                    },
                    _ => Self::Named(path.name.clone()),
                }
            }
        }
    }

    /// The type a handle declares, from
    /// [`Value::qualified`](host::export::Value::qualified).
    pub(crate) fn from_host(ty: &host::type_ref::Type) -> Self {
        use host::type_ref::Shape;
        match ty.shape() {
            Shape::Unit => Self::Unit,
            Shape::Option(inner) => Self::Option(Box::new(Self::from_host(inner))),
            Shape::Builtin(builtin) => Self::Named(builtin.name().unwrap_or("range").into()),
            Shape::Registered { path, arguments } => Self::Host {
                path: path.to_string().into(),
                arguments: arguments.iter().map(Self::from_host).collect(),
            },
            Shape::Tuple(members) => Self::Tuple(members.iter().map(Self::from_host).collect()),
            other => Self::Named(format!("{other:?}").into()),
        }
    }

    /// A parameter as written takes `expected`: the same type, or a shared
    /// borrow of it.
    fn lends(&self, expected: &Self, registry: &host::Registry) -> bool {
        match self {
            Self::Reference {
                mutable: false,
                target,
            } => match (&**target, expected) {
                (Self::Named(found), Self::Named(expected)) if found == "str" => {
                    expected == "String"
                }
                (target, expected) => target.matches(expected, registry),
            },
            found => found.matches(expected, registry),
        }
    }

    fn matches(&self, expected: &Self, registry: &host::Registry) -> bool {
        match (self, expected) {
            (Self::Unit, Self::Unit) => true,
            (Self::Named(found), Self::Named(expected)) => found == expected,
            (Self::Option(found), Self::Option(expected)) => found.matches(expected, registry),
            (Self::Tuple(found), Self::Tuple(expected)) => {
                found.len() == expected.len()
                    && found
                        .iter()
                        .zip(expected.iter())
                        .all(|(found, expected)| found.matches(expected, registry))
            }
            // One export: the same root, path and type arguments.
            (
                Self::Host { path, arguments },
                Self::Host {
                    path: expected,
                    arguments: expected_arguments,
                },
            ) => {
                path == expected
                    && registry.is_exported(path)
                    && arguments.len() == expected_arguments.len()
                    && arguments
                        .iter()
                        .zip(expected_arguments.iter())
                        .all(|(found, expected)| found.matches(expected, registry))
            }
            _ => false,
        }
    }
}

impl Ty {
    /// Rustc's name for this type. An exported type prints by its last
    /// segment, as rustc trims a path whose name is unique, unless `full`.
    fn write(&self, f: &mut fmt::Formatter<'_>, full: bool) -> fmt::Result {
        let list = |f: &mut fmt::Formatter<'_>, members: &[Self]| {
            for (index, member) in members.iter().enumerate() {
                if index > 0 {
                    f.write_str(", ")?;
                }
                member.write(f, full)?;
            }
            Ok(())
        };
        match self {
            Self::Unit => f.write_str("()"),
            Self::Named(name) => f.write_str(name),
            Self::Option(inner) => {
                f.write_str("Option<")?;
                inner.write(f, full)?;
                f.write_str(">")
            }
            Self::Host { path, arguments } => {
                f.write_str(match path.rsplit_once("::") {
                    Some((_, name)) if !full => name,
                    _ => path,
                })?;
                if arguments.is_empty() {
                    return Ok(());
                }
                f.write_str("<")?;
                list(f, arguments)?;
                f.write_str(">")
            }
            Self::Tuple(members) => {
                f.write_str("(")?;
                list(f, members)?;
                f.write_str(if members.len() == 1 { ",)" } else { ")" })
            }
            Self::Reference { mutable, target } => {
                f.write_str(if *mutable { "&mut " } else { "&" })?;
                target.write(f, full)
            }
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, f.alternate())
    }
}
