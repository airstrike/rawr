//! `Box<dyn std::error::Error>`: a trait object of std's `Error`, whose
//! value is fixed once it is boxed. Rawr makes it where the error is
//! converted, where its type is known: the error itself and its type
//! (`any_object::Key`), its `Display` and `Debug` text, and its `source`,
//! an error object too. So `{e}`, `{e:?}` and `e.to_string()` print what
//! Rust prints, `e.source()` walks the chain its types' `source` gives,
//! `e.downcast_ref::<T>()` gives the error back as its type, and a `main`
//! that returns one prints `Error: ` and its `Debug` text, as a host prints
//! `{error:?}`.
//!
//! The conversions are `?` in a function returning `Result<_,
//! Box<dyn Error>>` (`From<E: Error>`, `From<&str>` and `From<String>`),
//! `Box::new(e)` where a `Box<dyn Error>` is expected, `.into()` there, and
//! `&e` where a `&dyn Error` is.

use super::*;

/// An error as a `dyn Error`: the error, its type, its `Display` and
/// `Debug` text and its `source`.
#[derive(Clone)]
pub struct ErrorObject {
    display: ImmutableString,
    debug: ImmutableString,
    value: Dynamic,
    ty: any_object::Key,
    source: Option<Dynamic>,
}

impl ErrorObject {
    /// The error and its type.
    pub(super) const fn typed(&self) -> (&Dynamic, &any_object::Key) {
        (&self.value, &self.ty)
    }
}

impl std::fmt::Debug for ErrorObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.debug)
    }
}

/// `error` at runtime. Its `Debug` text is what a host prints for
/// `{error:?}` of a `main` that returned it.
fn object(error: ErrorObject) -> Dynamic {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(crate::host::debug::register::<ErrorObject>);
    Dynamic::from(error)
}

/// The error object `value` is.
fn read(value: &Dynamic) -> Result<ErrorObject, Box<EvalAltResult>> {
    value
        .read_lock::<ErrorObject>()
        .map(|error| error.clone())
        .ok_or_else(|| invariant("a `dyn Error` is not one at runtime"))
}

/// `ErrorNew(display, debug, error, type, source)` makes an error object;
/// `ErrorText(error, code)` reads its `Display` text (`0`) or its `Debug`
/// text (`1`), and `ErrorSource(error)` its `source`.
pub(super) fn invoke(op: PreludeOp, args: &mut FnCallArgs) -> RawrResult {
    match op {
        PreludeOp::ErrorNew => Ok(object(ErrorObject {
            display: args[0].clone_cast::<ImmutableString>(),
            debug: args[1].clone_cast::<ImmutableString>(),
            value: args[2].clone(),
            ty: any_object::Key::new(args[3].clone_cast::<ImmutableString>()),
            source: crate::option::read(args[4]).flatten(),
        })),
        PreludeOp::ErrorText => {
            let error = read(args[0])?;
            let text = if args[1].as_int().unwrap_or_default() == 0 {
                error.display
            } else {
                error.debug
            };
            Ok(text.into())
        }
        PreludeOp::ErrorSource => Ok(Dynamic::from(read(args[0])?.source)),
        _ => Err(invariant("not a `dyn Error` operation")),
    }
}

/// The error of a `parse` into the target of kind code `code` (as
/// `parse_text` takes it), from its message, boxed as `dyn Error`.
pub(super) fn parsed(message: &str, code: INT) -> Dynamic {
    let kind = match code {
        10 | 11 => ParseError::Float,
        12 => ParseError::Char,
        15 => ParseError::Bool,
        _ => ParseError::Int,
    };
    let debug = kind
        .texts()
        .iter()
        .find(|(text, _)| *text == message)
        .map_or(message, |(_, debug)| debug);
    object(ErrorObject {
        display: message.into(),
        debug: debug.into(),
        value: Dynamic::from(ImmutableString::from(message)),
        ty: any_object::Key::new(kind.name().into()),
        source: None,
    })
}

impl Parser<'_> {
    /// Whether `ty` is std's `dyn Error`, behind any references and boxes
    /// when `through`.
    pub(super) fn std_error(&self, ty: &Ty, through: bool) -> bool {
        let mut ty = self.inference.resolve(ty);
        while through && let Ty::Reference { target, .. } | Ty::Boxed(target) = ty {
            ty = self.inference.resolve(&target);
        }
        matches!(&ty, Ty::Dyn(name, bindings) if name.as_str() == "Error" && bindings.is_empty())
            && self.models.std_error_trait()
    }

    /// Whether `ty` is `Box<dyn Error>`.
    pub(super) fn boxed_std_error(&self, ty: &Ty) -> bool {
        match self.inference.resolve(ty) {
            Ty::Boxed(inner) => self.std_error(&inner, false),
            _ => false,
        }
    }

    /// The text of a `dyn Error` (behind references and boxes): its
    /// `Display` text, or its `Debug` text when `debug`.
    pub(super) fn error_text(&mut self, value: origin::Fragment, debug: bool) -> origin::Fragment {
        let code = if debug { "1" } else { "0" };
        self.prelude_op(PreludeOp::ErrorText, [value, code.into()])
    }

    /// `value` of type `from` as a `dyn Error`, as `From` and unsizing
    /// convert it: an error type's (a crate type with `impl Error`, a std
    /// error such as `ParseIntError`, or a box or a borrow of one), or with
    /// `text` a `&str` or `String`, whose `Debug` is the quoted text. `None`
    /// when `from` has no such conversion.
    pub(super) fn error_object(
        &mut self,
        from: &Ty,
        value: origin::Fragment,
        text: bool,
        span: Span,
    ) -> Result<Option<origin::Fragment>, Error> {
        let mut from = self.inference.resolve(from);
        while let Ty::Boxed(inner) | Ty::Reference { target: inner, .. } = &from {
            from = self.inference.resolve(inner);
        }
        if self.std_error(&from, false) {
            return Ok(Some(value));
        }
        let held = self.temporary("error");
        let local = origin::Fragment::plain(held.as_str());
        let (display, debug) = match &from {
            Ty::Script(id, _) if self.models.implements("Error", *id) => {
                let Some(display) = self.display_text(&from, local.clone()) else {
                    return Err(Error::MissingCapability {
                        owner: from.name(&self.models),
                        capability: "Display".into(),
                        operation: span,
                        derive: matches!(from, Ty::Script(..)),
                        declared: self.declared_name(&from),
                        required_by: None,
                        formatted: None,
                        chain: crate::resolve::Chain::default(),
                    });
                };
                (display, self.debug_source(&from, local.clone(), span)?)
            }
            Ty::ParseError(error) => {
                let error = *error;
                (
                    self.parse_error_display(error, local.clone()),
                    self.debug_source(&from, local.clone(), span)?,
                )
            }
            Ty::String | Ty::Str if text => (
                local.clone(),
                self.debug_source(&from, local.clone(), span)?,
            ),
            // A crate's or std's error, by the `std::error::Error` impl its
            // crate writes or derives: its `Display` text through its own
            // impl, which `Error` requires (#1451).
            Ty::Host { proof, .. }
                if proof
                    .definition
                    .capabilities()
                    .contains(capability::Kind::Error) =>
            {
                let display = self.prelude_op(PreludeOp::DisplayText, [local.clone()]);
                (display, self.debug_source(&from, local.clone(), span)?)
            }
            // A generic error, `E: Error`: its texts by its type's impls,
            // which its value dispatches to at runtime.
            Ty::Param(..) | Ty::Projection(..) if self.type_implements(&from, "Error") => {
                let Some(display) = self.display_text(&from, local.clone()) else {
                    return Ok(None);
                };
                (display, self.debug_source(&from, local.clone(), span)?)
            }
            _ => return Ok(None),
        };
        // Text boxes as std's private `StringError`, which no downcast
        // names. A generic error's type, and so its `source`, is known
        // only at runtime, where rawr doesn't read them yet.
        let generic = matches!(from, Ty::Param(..) | Ty::Projection(..));
        let ty = if from.is_text() || generic {
            ImmutableString::new()
        } else {
            self.downcast_key(&from)
        };
        // Its `source`, by the program's own `Error::source`, else std's
        // default: none.
        let cause = match &from {
            Ty::Script(id, arguments) => self
                .models
                .std_impl_at(*id, arguments, "Error", "source")
                .map(|definition| Self::std_call(definition, local.clone(), &[])),
            _ => None,
        };
        let cause = cause.unwrap_or_else(|| self.prelude_op(PreludeOp::None, []));
        let mut source = origin::Fragment::plain(format!("{{ let {held} = "));
        source.push(value);
        source.push_text("; ");
        source.push(self.prelude_op(
            PreludeOp::ErrorNew,
            [display, debug, local, format!("{ty:?}").into(), cause],
        ));
        source.push_text(" }");
        Ok(Some(source))
    }

    /// Whether the value at `at` is `.into()` of text, which `From`
    /// converts into a `Box<dyn Error>`.
    pub(super) fn text_into_at(&self, at: Position) -> bool {
        self.text_into.iter().any(|into| into.span.start() == at)
    }

    /// rustc's E0308 for a value of type `from` at `value` where the
    /// `Box<dyn Error>` `expected` is: only a box unsizes to one. `None`
    /// for a box.
    pub(super) fn unboxed_error(&self, expected: &Ty, from: &Ty, value: Span) -> Option<Error> {
        let from = self.inference.resolve(from);
        if matches!(from, Ty::Boxed(_)) {
            return None;
        }
        let help = match &from {
            Ty::Script(id, _) if self.models.implements("Error", *id) => {
                Some(crate::resolve::Boxing::New)
            }
            Ty::ParseError(_) => Some(crate::resolve::Boxing::New),
            Ty::Host { proof, .. }
                if proof
                    .definition
                    .capabilities()
                    .contains(capability::Kind::Error) =>
            {
                Some(crate::resolve::Boxing::New)
            }
            Ty::String => Some(crate::resolve::Boxing::Into("String".into())),
            Ty::Str => {
                let ty = if self.text_literal_at(value.start()) {
                    "&'static str"
                } else {
                    "&str"
                };
                Some(crate::resolve::Boxing::Into(ty.into()))
            }
            _ => None,
        };
        Some(Error::Unboxed {
            expected: expected.name(&self.models),
            actual: from.name(&self.models),
            help,
            expression: self.expression_span(value),
        })
    }

    /// Whether a string literal starts at `at`, whose type rustc writes
    /// `&'static str`.
    fn text_literal_at(&self, at: Position) -> bool {
        let (Some(line), Some(column)) = (at.line(), at.position()) else {
            return false;
        };
        let Some(text) = self.text.lines().nth(line - 1) else {
            return false;
        };
        let head = text.chars().skip(column - 1).take(2).collect::<String>();
        head.starts_with('"') || head == "r\"" || head == "r#"
    }

    /// `Err(Box::new(e))` and `Err("text".into())` in a body returning
    /// `Result<_, Box<dyn Error>>`: the error boxed as `dyn Error` where it
    /// is made, as Rust's expected type coerces it. So the arms of an `if`
    /// or `match` agree on the error type.
    pub(super) fn box_returned_error(&mut self, value: &mut ParsedExpr) -> Result<(), Error> {
        let returns_error = self.return_ty.as_ref().is_some_and(|output| {
            matches!(self.inference.resolve(output), Ty::Result(_, error) if self.boxed_std_error(&error))
        });
        if !returns_error {
            return Ok(());
        }
        // `"text".into()`'s target is the `String` boxed here.
        let into = self
            .text_into
            .iter()
            .find(|into| into.span == value.span)
            .map(|into| into.target.clone());
        if let Some(target) = &into
            && let Ty::Infer(_) = self.inference.resolve(target)
        {
            self.unify_type(target, &Ty::String);
        }
        let source = self.inference.resolve(&value.ty);
        let boxed = matches!(&source, Ty::Boxed(inner) if !self.std_error(inner, false));
        let text = source.is_text() && into.is_some();
        if !boxed && !text {
            return Ok(());
        }
        let Some(object) = self.error_object(&source, value.rendered_source(), text, value.span)?
        else {
            return Ok(());
        };
        let target = Ty::Boxed(Box::new(Ty::Dyn("Error".into(), Box::default())));
        *value = self.operator_expr(object, target, value.span);
        Ok(())
    }

    /// `e.to_string()`, `e.source()` and the downcasts of a `dyn Error`
    /// (`any_object.rs`) of type `receiver`, after the member's name.
    pub(super) fn parse_error_member(
        &mut self,
        value: ParsedExpr,
        receiver: &Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let value = match self.parse_downcast(value, receiver, &member, member_span)? {
            Ok(downcast) => return Ok(downcast),
            Err(value) => value,
        };
        let (arguments, end) = self.parse_call_arguments("`dyn Error` method arguments")?;
        let call = span(value.span.start(), end);
        if !arguments.is_empty() {
            return Err(Error::argument_count(
                crate::resolve::Callee::Method,
                crate::resolve::Qualified("dyn Error", &member).to_string(),
                0,
                arguments.len(),
                span(member_span.start(), end),
            ));
        }
        match member.as_str() {
            "to_string" => {
                let source = self.error_text(value.rendered_source(), false);
                Ok(self.operator_expr(source, Ty::String, call))
            }
            "source" => {
                let source = self.prelude_op(PreludeOp::ErrorSource, [value.rendered_source()]);
                let ty = Ty::Option(Box::new(Ty::Reference {
                    mutable: false,
                    target: Box::new(Ty::Dyn("Error".into(), Box::default())),
                }));
                Ok(self.operator_expr(source, ty, call))
            }
            _ => Err(Error::UnknownMember {
                owner: "dyn Error".into(),
                member,
                reference: member_span,
            }),
        }
    }
}

impl Models {
    /// Whether the crate's `Error` trait is std's, which it implements by
    /// hand and boxes as `dyn Error`.
    pub(super) fn std_error_trait(&self) -> bool {
        self.std_trait("Error")
    }
}
