//! `dyn std::any::Any`: a value of any `'static` type that knows its type.
//! Rawr makes it where the value is coerced, where its type is known: the
//! value with its type's [`Key`]. `is::<T>()`, `downcast_ref::<T>()` and a
//! box's `downcast::<T>()` compare `T`'s key with it, as std compares
//! `TypeId`s, of a `dyn Any` and of a `dyn Error` (`error_object.rs`).

use super::ty::{Float, Int};
use super::{error_object, invariant, origin, span, Models, ParsedExpr, Parser, PreludeOp, Ty};
use crate::func::FnCallArgs;
use crate::resolve::Error;
use crate::tokenizer::Token;
use crate::{Dynamic, ImmutableString, RawrResult, Span, INT};

/// A type as a downcast names it, rustc's `TypeId`: rawr's name of the type
/// where a value becomes a trait object. An empty key is a type no program
/// names (std's private `StringError`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Key(ImmutableString);

impl Key {
    pub(super) const fn new(name: ImmutableString) -> Self {
        Self(name)
    }

    /// Whether this is the type `key` names.
    fn is(&self, key: &str) -> bool {
        !self.0.is_empty() && self.0 == key
    }
}

/// A value as a `dyn Any`: the value and its type.
#[derive(Clone)]
pub struct AnyObject {
    value: Dynamic,
    ty: Key,
}

impl std::fmt::Debug for AnyObject {
    /// std's `Debug` of a `dyn Any`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Any { .. }")
    }
}

/// What a downcast gives: `downcast_ref`'s `Option<&T>`, `is`'s `bool`, or
/// a box's `downcast`, `Result<Box<T>, Box<Self>>`. Its code is the third
/// operand of `Downcast`.
#[derive(Clone, Copy)]
enum Downcast {
    Borrowed = 0,
    Is = 1,
    Boxed = 2,
}

/// `AnyNew(value, type)` makes a `dyn Any`; `Downcast(object, type, code)`
/// compares the type of a `dyn Any` or a `dyn Error` with `type`, giving
/// what the code's [`Downcast`] does.
pub(super) fn invoke(op: PreludeOp, args: &mut FnCallArgs) -> RawrResult {
    match op {
        PreludeOp::AnyNew => {
            static REGISTERED: std::sync::Once = std::sync::Once::new();
            REGISTERED.call_once(crate::host::debug::register::<AnyObject>);
            Ok(Dynamic::from(AnyObject {
                value: args[0].clone(),
                ty: Key::new(args[1].clone_cast::<ImmutableString>()),
            }))
        }
        PreludeOp::Downcast => {
            let key = args[1].clone_cast::<ImmutableString>();
            let found = if let Some(object) = args[0].read_lock::<AnyObject>() {
                object.ty.is(&key).then(|| object.value.clone())
            } else if let Some(error) = args[0].read_lock::<error_object::ErrorObject>() {
                let (value, ty) = error.typed();
                ty.is(&key).then(|| value.clone())
            } else {
                return Err(invariant("a trait object is no `dyn Any` at runtime"));
            };
            let code = args[2].as_int().unwrap_or_default();
            Ok(match found {
                _ if code == Downcast::Is as INT => found.is_some().into(),
                Some(value) if code == Downcast::Boxed as INT => crate::result::ok(value),
                None if code == Downcast::Boxed as INT => crate::result::err(args[0].clone()),
                found => Dynamic::from(found),
            })
        }
        _ => Err(invariant("not a `dyn Any` operation")),
    }
}

impl Parser<'_> {
    /// Whether `ty` is std's `dyn Any`, behind any references and boxes
    /// when `through`.
    pub(super) fn std_any(&self, ty: &Ty, through: bool) -> bool {
        let mut ty = self.inference.resolve(ty);
        while through && let Ty::Reference { target, .. } | Ty::Boxed(target) = ty {
            ty = self.inference.resolve(&target);
        }
        matches!(&ty, Ty::Dyn(name, bindings, _) if name.as_str() == "Any" && bindings.is_empty())
            && self.models.std_trait("Any")
    }

    /// The key of `ty` that a downcast compares. An integer or float
    /// literal's type still to be inferred is rustc's default for it.
    pub(super) fn downcast_key(&self, ty: &Ty) -> ImmutableString {
        match self.inference.resolve(ty) {
            Ty::Int(Int::Var(_)) => "i32".into(),
            Ty::Float(Float::Var(_)) => "f64".into(),
            ty @ Ty::Script(id, _) => format!("{id:?} {}", ty.name(&self.models)).into(),
            ty => ty.name(&self.models),
        }
    }

    /// `value` of type `from` as a `dyn Any`: itself when it is one.
    pub(super) fn any_object(&mut self, from: &Ty, value: origin::Fragment) -> origin::Fragment {
        if self.std_any(from, false) {
            return value;
        }
        let key = self.downcast_key(from);
        self.prelude_op(PreludeOp::AnyNew, [value, format!("{key:?}").into()])
    }

    /// `e.is::<T>()`, `e.downcast_ref::<T>()` and a box's
    /// `e.downcast::<T>()` of a `dyn Any` or a `dyn Error`, `receiver`,
    /// after the member's name. `None` for another member, which `value`
    /// is given back for.
    pub(super) fn parse_downcast(
        &mut self,
        value: ParsedExpr,
        receiver: &Ty,
        member: &str,
        member_span: Span,
    ) -> Result<Result<ParsedExpr, ParsedExpr>, Error> {
        let boxed = matches!(self.inference.resolve(receiver), Ty::Boxed(_));
        let code = match member {
            "downcast_ref" => Downcast::Borrowed,
            "is" => Downcast::Is,
            "downcast" if boxed => Downcast::Boxed,
            "downcast_mut" => {
                return Err(Error::NotYet {
                    path: format!("{}::downcast_mut", receiver.name(&self.models)).into(),
                    reference: member_span,
                });
            }
            _ => return Ok(Err(value)),
        };
        let mut written_span = member_span;
        let written = if matches!(self.peek(), Token::DoubleColon)
            && matches!(self.peek_second(), Token::LessThan)
        {
            self.next();
            self.next();
            let start = self.position();
            let mut types = vec![self.parse_local_argument()?];
            while matches!(self.peek(), Token::Comma) {
                self.next();
                types.push(self.parse_local_argument()?);
            }
            let close = self.expect_close_angle("'>' after turbofish type arguments")?;
            written_span = span(start, close);
            types
        } else {
            Vec::new()
        };
        let [target] = written.as_slice() else {
            return Err(Error::NotYet {
                path: format!("{member} without its type argument").into(),
                reference: member_span,
            });
        };
        let target = target.clone();
        // A `dyn Error`'s downcasts take an error type: `T: Error`.
        if self.std_error(receiver, true) && !self.may_be_error(&target) {
            return Err(Error::MissingCapability {
                owner: target.name(&self.models),
                capability: "std::error::Error".into(),
                operation: written_span,
                derive: false,
                declared: self.declared_name(&target),
                required_by: None,
                formatted: None,
                chain: crate::resolve::Chain::default(),
            });
        }
        let (arguments, end) = self.parse_call_arguments("downcast arguments")?;
        if !arguments.is_empty() {
            return Err(Error::argument_count(
                crate::resolve::Callee::Method,
                crate::resolve::Qualified(&receiver.name(&self.models), member).to_string(),
                0,
                arguments.len(),
                span(member_span.start(), end),
            ));
        }
        let call = span(value.span.start(), end);
        let key = self.downcast_key(&target);
        let source = self.prelude_op(
            PreludeOp::Downcast,
            [
                value.rendered_source(),
                format!("{key:?}").into(),
                (code as INT).to_string().into(),
            ],
        );
        let ty = match code {
            Downcast::Borrowed => Ty::Option(Box::new(Ty::Reference {
                mutable: false,
                target: Box::new(target),
            })),
            Downcast::Is => Ty::Bool,
            Downcast::Boxed => Ty::Result(
                Box::new(Ty::Boxed(Box::new(target))),
                Box::new(self.inference.resolve(receiver)),
            ),
        };
        Ok(Ok(self.operator_expr(source, ty, call)))
    }
}

impl Parser<'_> {
    /// Whether `ty` may implement std's `Error`: all but a program type
    /// without an impl of it and std's builtins, which have none.
    fn may_be_error(&self, ty: &Ty) -> bool {
        match self.inference.resolve(ty) {
            Ty::Script(id, _) => self.models.implements("Error", id),
            // std's `impl Error for &T` and `for Box<T>` of a `T: Error`.
            Ty::Reference { target, .. } | Ty::Boxed(target) => self.may_be_error(&target),
            Ty::Unit
            | Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Float(_)
            | Ty::String
            | Ty::Str
            | Ty::Array { .. }
            | Ty::Tuple(_)
            | Ty::Option(_)
            | Ty::Result(..)
            | Ty::Keyed(..) => false,
            _ => true,
        }
    }
}

impl Models {
    /// Whether the program's trait `name` is std's, which it names by a
    /// `use` or a path.
    pub(super) fn std_trait(&self, name: &str) -> bool {
        self.traits
            .get(name)
            .is_some_and(|(_, declared)| declared.name.span == Span::NONE)
    }
}
