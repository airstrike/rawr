//! std's `FromStr`: `s.parse()` into numbers, script and crate types, and
//! the `Display` of std's parse errors.

use super::*;

impl<'a> Parser<'a> {
    /// A std error's `Display` of its runtime `value`: its message, or for
    /// the errors whose runtime value is their `Debug`, the message their
    /// `Debug` determines.
    pub(super) fn parse_error_display(
        &mut self,
        error: ParseError,
        value: origin::Fragment,
    ) -> origin::Fragment {
        match error {
            ParseError::TryFromInt => {
                value.surrounded("{ ", &format!("; {TRY_FROM_INT_MESSAGE:?} }}"))
            }
            ParseError::Utf8 => {
                let code = STR_PIECES
                    .iter()
                    .position(|name| *name == "utf8_display")
                    .expect("a listed piece")
                    .to_string();
                self.prelude_op(
                    PreludeOp::StrPieces,
                    [value, code.into(), "()".into(), "()".into()],
                )
            }
            _ => value,
        }
    }

    /// `s.parse::<T>()` or `s.parse()`: Rust's `FromStr` for numbers,
    /// `bool` and `char`, a `Result<T, ParseIntError>` (or the target's
    /// error); the target may be inferred, and its kind is the op's.
    pub(super) fn parse_str_parse(&mut self, value: ParsedExpr) -> Result<ParsedExpr, Error> {
        let target = if matches!(self.peek(), Token::DoubleColon) {
            self.next();
            self.expect(Token::LessThan, "'<' after `parse::`")?;
            let target = self.parse_local_type()?;
            self.expect_close_angle("'>' after `parse`'s type argument")?;
            target
        } else {
            self.inference.fresh()
        };
        let (arguments, end) = self.parse_call_arguments("`parse` arguments")?;
        let expression = span(value.span.start(), end);
        if !arguments.is_empty() {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified("str", "parse").to_string(),
                0,
                arguments.len(),
                expression,
            ));
        }
        let error = self.inference.fresh();
        // A target known now gives the call and the error type now; else
        // `placeholder(value))`, whose placeholder becomes `helper(` once
        // the target is known (`settle_parses`).
        let source = match self.settle_parse(&target, &error, expression, false)? {
            Some(helper) => origin::Fragment::call(&helper, [value.rendered_source()]),
            None => {
                let placeholder = super::naming::parse_placeholder(self.parse_count);
                self.parse_count += 1;
                let mut source = origin::Fragment::call(&placeholder, [value.rendered_source()]);
                source.push_text(")");
                self.pending_parses
                    .push((target.clone(), error.clone(), expression, placeholder));
                source
            }
        };
        let ty = Ty::Result(Box::new(target), Box::new(error));
        Ok(self.operator_expr(source, ty, expression))
    }

    /// The error type of a `parse` into `target` once `target` is known,
    /// and the helper its call is: std's `FromStr` for a number, `bool` or
    /// `char`, or the program's `impl FromStr` for its type, whose `Err` is
    /// the error. At the end of the body (`last`), Rust's errors for an
    /// unknown target or one with no `FromStr`.
    pub(super) fn settle_parse(
        &mut self,
        target: &Ty,
        error: &Ty,
        span: Span,
        last: bool,
    ) -> Result<Option<String>, Error> {
        let resolved = self.inference.resolve(target);
        if let Some(kind) = ParseError::of(&resolved) {
            self.require_type(&Ty::ParseError(kind), error, span)?;
            let helper = self.push_op(Op::Kinded {
                op: PreludeOp::Parse,
                kinds: [resolved.clone(), resolved],
            });
            return Ok(Some(helper));
        }
        if let Ty::Script(id, _) = &resolved
            && let Some((definition, from_str_error)) = self.program_from_str(*id)
        {
            self.require_type(&from_str_error, error, span)?;
            return Ok(Some(method_name(&definition)));
        }
        // A crate type's `impl FromStr`: its `from_str`, which the importer
        // brings as it brings any member of the type (#709).
        #[cfg(feature = "rust-import")]
        if let Ty::Host { proof, .. } = &resolved
            && proof.definition.identity().imported().is_some()
        {
            return self
                .host_from_str(&proof.definition.path(), target, error, span)
                .map(Some);
        }
        match resolved {
            Ty::Infer(_) if !last => Ok(None),
            Ty::Infer(_) => Err(Error::InvalidMethodCall {
                owner: "str".into(),
                method: "parse".into(),
                reason: "type annotations needed: the target of `parse` is unknown (E0284)".into(),
                call: span,
            }),
            other => Err(Error::MissingCapability {
                owner: other.name(&self.models),
                capability: "FromStr".into(),
                operation: span,
                derive: false,
                required_by: None,
            }),
        }
    }

    /// The helper that parses into the crate type at `path` through its
    /// `impl FromStr`: the facade's `from_str`, whose `Result` gives
    /// `target` and the parse's `error`.
    #[cfg(feature = "rust-import")]
    pub(super) fn host_from_str(
        &mut self,
        path: &str,
        target: &Ty,
        error: &Ty,
        span: Span,
    ) -> Result<String, Error> {
        let member = format!("{path}::from_str");
        let Some(items) = self.models.host_items(&member).map(<[_]>::to_vec) else {
            let miss = Error::UnknownMember {
                owner: path.into(),
                member: "from_str".into(),
                reference: span,
            };
            return Err(self.native_miss(true, miss));
        };
        let owner = self.models.item_owner(&member).cloned();
        let text = self.operator_expr(origin::Fragment::plain("()".to_owned()), Ty::Str, span);
        let mut arguments = vec![text];
        let (item, signature) =
            self.select_host_signature(&items, None, &mut arguments, false, span)?;
        let output = self.inference.resolve(&signature.output);
        let Ty::Result(parsed, failed) = &output else {
            return Err(self.unsupported("a `from_str` that gives no `Result`"));
        };
        self.require_type(parsed, target, span)?;
        self.require_type(failed, error, span)?;
        self.push_intrinsic(
            item,
            owner,
            None,
            &mut arguments,
            output,
            signature.requirements,
            span,
        )
    }

    /// The program's `impl FromStr for` the type `id`: its `from_str`, and
    /// the impl's `Err`.
    pub(super) fn program_from_str(&self, id: type_def::Id) -> Option<(method::Definition, Ty)> {
        let definition = self
            .models
            .trait_methods
            .get(&(id, "from_str".into()))?
            .iter()
            .find(|definition| {
                definition
                    .trait_name()
                    .is_some_and(|name| name.as_str() == "FromStr")
                    && definition.receiver().is_none()
            })?
            .clone();
        let Ty::Result(_, error) = Ty::from_resolved(definition.output()) else {
            return None;
        };
        Some((definition, *error))
    }
}

/// A numeric kind as the runtime's `Narrow` and `Cast` take it: an integer
/// kind's index in `Int::KINDS`, `10` for `f32`, `11` for `f64`, `12` for
/// `char`.
/// `std::num::ParseIntError` and its kin, the errors of `str::parse`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum ParseError {
    Int,
    Float,
    Bool,
    Char,
    /// `std::convert::Infallible`, the error of `parse::<String>()`.
    Infallible,
    /// `std::string::FromUtf8Error`, the error of `String::from_utf8`.
    Utf8,
    /// `std::num::TryFromIntError`, the error of `u8::try_from(x)` and
    /// `x.try_into()` among integers.
    TryFromInt,
}

impl ParseError {
    pub(super) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "ParseIntError" => Self::Int,
            "ParseFloatError" => Self::Float,
            "ParseBoolError" => Self::Bool,
            "ParseCharError" => Self::Char,
            "Infallible" => Self::Infallible,
            "FromUtf8Error" => Self::Utf8,
            "TryFromIntError" => Self::TryFromInt,
            _ => return None,
        })
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Int => "ParseIntError",
            Self::Float => "ParseFloatError",
            Self::Bool => "ParseBoolError",
            Self::Char => "ParseCharError",
            Self::Infallible => "Infallible",
            Self::Utf8 => "FromUtf8Error",
            Self::TryFromInt => "TryFromIntError",
        }
    }

    /// The error of parsing into `target`, which Rust's `FromStr` covers.
    pub(super) fn of(target: &Ty) -> Option<Self> {
        Some(match target {
            Ty::Int(_) => Self::Int,
            Ty::Float(_) => Self::Float,
            Ty::Bool => Self::Bool,
            Ty::Char => Self::Char,
            Ty::String => Self::Infallible,
            _ => return None,
        })
    }

    /// Each message (the error's `Display`, its value at runtime) with the
    /// error's `Debug`, as Rust's std writes them. A `TryFromIntError` is
    /// its `Debug` at runtime instead, and [`Self::display`] its message.
    pub(super) const fn texts(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Int => &[
                (
                    "cannot parse integer from empty string",
                    "ParseIntError { kind: Empty }",
                ),
                (
                    "invalid digit found in string",
                    "ParseIntError { kind: InvalidDigit }",
                ),
                (
                    "number too large to fit in target type",
                    "ParseIntError { kind: PosOverflow }",
                ),
                (
                    "number too small to fit in target type",
                    "ParseIntError { kind: NegOverflow }",
                ),
            ],
            Self::Float => &[
                (
                    "cannot parse float from empty string",
                    "ParseFloatError { kind: Empty }",
                ),
                ("invalid float literal", "ParseFloatError { kind: Invalid }"),
            ],
            Self::Bool => &[(
                "provided string was not `true` or `false`",
                "ParseBoolError",
            )],
            Self::Char => &[
                (
                    "cannot parse char from empty string",
                    "ParseCharError { kind: EmptyString }",
                ),
                (
                    "too many characters in string",
                    "ParseCharError { kind: TooManyChars }",
                ),
            ],
            // A `TryFromIntError` is its `Debug` text, which names the
            // side it overflowed, and its message is one for both. A
            // `FromUtf8Error` is its `Debug` text, which holds its bytes.
            Self::Infallible | Self::Utf8 | Self::TryFromInt => &[],
        }
    }
}

/// `std::num::TryFromIntError`'s message, its `Display`.
pub(super) const TRY_FROM_INT_MESSAGE: &str = "out of range integral type conversion attempted";

/// `text.parse()` into the kind `code` (`kind_code`): `Ok` of the value,
/// or `Err` of the std error's message, by Rust's own `FromStr`.
pub(super) fn parse_text(text: &str, code: INT) -> Dynamic {
    fn result<T, E: std::fmt::Display>(
        parsed: Result<T, E>,
        value: impl FnOnce(T) -> Dynamic,
    ) -> Dynamic {
        match parsed {
            Ok(parsed) => crate::result::ok(value(parsed)),
            Err(error) => crate::result::err(ImmutableString::from(error.to_string()).into()),
        }
    }
    let int = |value: i128| Dynamic::from(value as INT);
    match code {
        10 => result(text.parse::<f32>(), |value| {
            Dynamic::from(crate::FLOAT::from(value))
        }),
        11 => result(text.parse::<crate::FLOAT>(), Dynamic::from),
        12 => result(text.parse::<char>(), Dynamic::from),
        15 => result(text.parse::<bool>(), Dynamic::from),
        16 => crate::result::ok(ImmutableString::from(text).into()),
        code => match Int::from_code(code).map_or("i64", Int::name) {
            "i128" => result(text.parse::<i128>(), wide_i128),
            "u128" => result(text.parse::<u128>(), wide_u128),
            "i8" => result(text.parse::<i8>(), |value| int(value.into())),
            "i16" => result(text.parse::<i16>(), |value| int(value.into())),
            "i32" => result(text.parse::<i32>(), |value| int(value.into())),
            "u8" => result(text.parse::<u8>(), |value| int(value.into())),
            "u16" => result(text.parse::<u16>(), |value| int(value.into())),
            "u32" => result(text.parse::<u32>(), |value| int(value.into())),
            // A `u64` or `usize` is its bits in the runtime's `i64`.
            "u64" | "usize" => result(text.parse::<u64>(), |value| int(value.into())),
            _ => result(text.parse::<i64>(), |value| int(value.into())),
        },
    }
}
