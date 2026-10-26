//! `Box<dyn std::error::Error>`: a trait object of std's `Error`, whose
//! value is fixed once it is boxed. Rawr makes it where the error is
//! converted, where its type is known: the error's `Display` and `Debug`
//! text. So `{e}`, `{e:?}` and `e.to_string()` print what Rust prints, and
//! a `main` that returns one prints `Error: ` and its `Debug` text, as a
//! host prints `{error:?}`.
//!
//! The conversions are `?` in a function returning `Result<_,
//! Box<dyn Error>>` (`From<E: Error>`, `From<&str>` and `From<String>`),
//! `Box::new(e)` where a `Box<dyn Error>` is expected, and `.into()` there.

use super::*;

/// An error boxed as `dyn Error`: its `Display` and `Debug` text.
#[derive(Clone)]
pub struct ErrorObject {
    display: ImmutableString,
    debug: ImmutableString,
}

impl std::fmt::Debug for ErrorObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.debug)
    }
}

/// An error object of these texts. Its `Debug` text is what a host prints
/// for `{error:?}` of a `main` that returned it.
fn object(display: ImmutableString, debug: ImmutableString) -> Dynamic {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(crate::host::debug::register::<ErrorObject>);
    Dynamic::from(ErrorObject { display, debug })
}

/// `ErrorNew(display, debug)` makes an error object; `ErrorText(error,
/// code)` reads its `Display` text (`0`) or its `Debug` text (`1`).
pub(super) fn invoke(op: PreludeOp, args: &mut FnCallArgs) -> RawrResult {
    match op {
        PreludeOp::ErrorNew => Ok(object(
            args[0].clone_cast::<ImmutableString>(),
            args[1].clone_cast::<ImmutableString>(),
        )),
        PreludeOp::ErrorText => {
            let error = args[0]
                .read_lock::<ErrorObject>()
                .ok_or_else(|| invariant("a `dyn Error` is not one at runtime"))?;
            let text = if args[1].as_int().unwrap_or_default() == 0 {
                error.display.clone()
            } else {
                error.debug.clone()
            };
            Ok(text.into())
        }
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
    object(message.into(), debug.into())
}

impl Parser<'_> {
    /// Whether `ty` is std's `dyn Error`, behind any references and boxes
    /// when `through`.
    pub(super) fn std_error(&self, ty: &Ty, through: bool) -> bool {
        let mut ty = self.inference.resolve(ty);
        while through && let Ty::Reference { target, .. } | Ty::Boxed(Pointer::Box, target) = ty {
            ty = self.inference.resolve(&target);
        }
        matches!(&ty, Ty::Dyn(name, bindings) if name.as_str() == "Error" && bindings.is_empty())
            && self.models.std_error_trait()
    }

    /// Whether `ty` is `Box<dyn Error>`.
    pub(super) fn boxed_std_error(&self, ty: &Ty) -> bool {
        match self.inference.resolve(ty) {
            Ty::Boxed(Pointer::Box, inner) => self.std_error(&inner, false),
            _ => false,
        }
    }

    /// The text of a `dyn Error` (behind references and boxes): its
    /// `Display` text, or its `Debug` text when `debug`.
    pub(super) fn error_text(&mut self, value: origin::Fragment, debug: bool) -> origin::Fragment {
        let code = if debug { "1" } else { "0" };
        self.prelude_op(PreludeOp::ErrorText, [value, code.into()])
    }

    /// `value` of type `from` boxed as `dyn Error`, as `From` converts it:
    /// an error type's (a crate type with `impl Error`, a std error such as
    /// `ParseIntError`, or a box of one), or with `text` a `&str` or
    /// `String`, whose `Debug` is the quoted text. `None` when `from` has
    /// no such conversion.
    pub(super) fn error_object(
        &mut self,
        from: &Ty,
        value: origin::Fragment,
        text: bool,
        span: Span,
    ) -> Result<Option<origin::Fragment>, Error> {
        let mut from = self.inference.resolve(from);
        if let Ty::Boxed(Pointer::Box, inner) = &from {
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
                    });
                };
                (display, self.debug_source(&from, local, span)?)
            }
            Ty::ParseError(error) => {
                let error = *error;
                (
                    self.parse_error_display(error, local.clone()),
                    self.debug_source(&from, local, span)?,
                )
            }
            Ty::String | Ty::Str if text => (local.clone(), self.debug_source(&from, local, span)?),
            _ => return Ok(None),
        };
        let mut source = origin::Fragment::plain(format!("{{ let {held} = "));
        source.push(value);
        source.push_text("; ");
        source.push(self.prelude_op(PreludeOp::ErrorNew, [display, debug]));
        source.push_text(" }");
        Ok(Some(source))
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
        let source = self.inference.resolve(&value.ty);
        let boxed =
            matches!(&source, Ty::Boxed(Pointer::Box, inner) if !self.std_error(inner, false));
        let text = source.is_text() && self.text_into.contains(&value.span);
        if !boxed && !text {
            return Ok(());
        }
        let Some(object) = self.error_object(&source, value.rendered_source(), text, value.span)?
        else {
            return Ok(());
        };
        let target = Ty::Boxed(
            Pointer::Box,
            Box::new(Ty::Dyn("Error".into(), Box::default())),
        );
        *value = self.operator_expr(object, target, value.span);
        Ok(())
    }

    /// `e.to_string()`, `e.source()` and `e.description()` of a `dyn
    /// Error`, after the member's name.
    pub(super) fn parse_error_member(
        &mut self,
        value: ParsedExpr,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (arguments, end) = self.parse_call_arguments("`dyn Error` method arguments")?;
        let call = span(value.span.start(), end);
        if !arguments.is_empty() {
            return Err(Error::InvalidMethodCall {
                owner: "dyn Error".into(),
                method: member,
                reason: format!("expects 0 argument(s), found {}", arguments.len()).into(),
                call,
            });
        }
        match member.as_str() {
            "to_string" => {
                let source = self.error_text(value.rendered_source(), false);
                Ok(self.operator_expr(source, Ty::String, call))
            }
            // Rawr's error objects keep no source: `Error::source`'s default.
            "source" => {
                let source = self.prelude_op(PreludeOp::None, []);
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
        self.traits
            .get("Error")
            .is_some_and(|(_, declared)| declared.name.span == Span::NONE)
    }
}
