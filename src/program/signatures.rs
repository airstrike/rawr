//! The typed signatures of a program's free functions, which an app checks
//! a typed handle against once, when it looks the function up (D514).

use crate::{host, resolve, ImmutableString, Span};
use std::collections::BTreeMap;
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

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

    /// `fn(f64, i64) -> f64`, as rustc names a function's type.
    pub(crate) fn display(&self) -> String {
        let text = function_type(&self.parameters, &self.output);
        match (self.asynchronous, self.generic) {
            (true, _) => format!("async {text}"),
            (false, true) => format!("{text} (generic)"),
            (false, false) => text,
        }
    }
}

/// `fn(A, B) -> R`, leaving out `-> ()` as rustc does.
pub(crate) fn function_type(parameters: &[Ty], output: &Ty) -> String {
    let parameters = parameters
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    match output {
        Ty::Unit => format!("fn({parameters})"),
        output => format!("fn({parameters}) -> {output}"),
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
    /// An app's exported type, by its path: `bank::Transaction` in a
    /// script, `Transaction` (relative to its root) in a handle.
    Host(ImmutableString),
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
                let mut arguments = path.arguments.iter().filter_map(|argument| match argument {
                    resolve::type_ref::Argument::Type(ty) => Some(ty),
                    resolve::type_ref::Argument::Lifetime(_) => None,
                });
                match &path.target {
                    resolve::type_ref::Target::Builtin if path.name.as_str() == "Option" => {
                        match (arguments.next(), arguments.next()) {
                            (Some(inner), None) => {
                                Self::Option(Box::new(Self::from_resolved(inner)))
                            }
                            _ => Self::Named(path.name.clone()),
                        }
                    }
                    resolve::type_ref::Target::Host { definition, .. } => {
                        Self::Host(definition.path().into())
                    }
                    _ => Self::Named(path.name.clone()),
                }
            }
        }
    }

    /// The type a handle declares, from [`Value::ty`](host::export::Value::ty).
    pub(crate) fn from_host(ty: &host::type_ref::Type) -> Self {
        use host::type_ref::Shape;
        match ty.shape() {
            Shape::Unit => Self::Unit,
            Shape::Option(inner) => Self::Option(Box::new(Self::from_host(inner))),
            Shape::Builtin(builtin) => Self::Named(builtin.name().unwrap_or("range").into()),
            Shape::Registered { path, .. } => Self::Host(path.to_string().into()),
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
            // The handle names an exported type relative to its root.
            (Self::Host(found), Self::Host(expected)) => {
                registry.is_exported(found)
                    && found
                        .split_once("::")
                        .is_some_and(|(_, path)| path == expected.as_str())
            }
            _ => false,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unit => f.write_str("()"),
            Self::Named(name) => f.write_str(name),
            Self::Option(inner) => write!(f, "Option<{inner}>"),
            // As rustc trims a path whose name is unique.
            Self::Host(path) => f.write_str(path.rsplit("::").next().unwrap_or(path)),
            Self::Tuple(members) => {
                let members = members.iter().map(ToString::to_string).collect::<Vec<_>>();
                if members.len() == 1 {
                    write!(f, "({},)", members[0])
                } else {
                    write!(f, "({})", members.join(", "))
                }
            }
            Self::Reference { mutable, target } => {
                write!(f, "&{}{target}", if *mutable { "mut " } else { "" })
            }
        }
    }
}
