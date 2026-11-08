//! std's time: `Duration` and `Instant`, their functions, methods and
//! operators.

use super::*;

impl<'a> Parser<'a> {
    /// `Duration::from_secs` or `std::time::Instant::now` after its first
    /// segment `first`, in a program whose facades do not name std's
    /// `time` (`resolve::std_reexport`): the type, the member, the number
    /// of tokens up to its end, and that end.
    pub(super) fn std_time_member(
        &self,
        first: &str,
    ) -> Option<(Time, ImmutableString, usize, Position)> {
        let mut ahead = self.input.clone();
        let mut segments = vec![ImmutableString::from(first)];
        let mut end = Position::NONE;
        while matches!(ahead.peek(), Some((Token::DoubleColon, _))) {
            ahead.next();
            match ahead.next() {
                Some((Token::Identifier(name), position)) => {
                    end = text_end(position, &name);
                    segments.push((*name).as_str().into());
                }
                _ => return None,
            }
        }
        let [path @ .., member] = &segments[..] else {
            return None;
        };
        let path = path
            .iter()
            .map(ImmutableString::as_str)
            .collect::<Vec<_>>()
            .join("::");
        let time = match crate::resolve::std_time(&path)? {
            "Duration" => Time::Duration,
            _ => Time::Instant,
        };
        let std = format!("std::time::{}", time.name());
        if crate::resolve::std_reexport(&std, &self.models.host) != std {
            return None;
        }
        // A bare `Duration` is std's unless the crate declares or imports
        // another.
        if !path.contains("::")
            && (self.models.get(&path).is_some()
                || self
                    .models
                    .import(&path)
                    .is_some_and(|binding| !matches!(binding.target, item::Import::Module)))
        {
            return None;
        }
        Some((time, member.clone(), 2 * segments.len() - 2, end))
    }

    /// `Duration::from_secs(n)`, `Duration::ZERO`, `Instant::now()` and the
    /// other associated items of std's `time` types.
    pub(super) fn parse_time_function(
        &mut self,
        time: Time,
        member: ImmutableString,
        path: Span,
    ) -> Result<ParsedExpr, Error> {
        let duration = Ty::Time(Time::Duration);
        let (input, output) = match (time, member.as_str()) {
            (Time::Duration, "from_secs" | "from_millis" | "from_micros" | "from_nanos") => {
                (Some(Ty::Int(Int::U64)), duration)
            }
            (Time::Duration, "from_secs_f64") => (Some(Ty::Float(Float::F64)), duration),
            (Time::Duration, "from_secs_f32") => (Some(Ty::Float(Float::F32)), duration),
            (Time::Duration, "ZERO" | "MAX") => (None, duration),
            (Time::Duration, "default") => (Some(Ty::Unit), duration),
            (Time::Instant, "now") => (Some(Ty::Unit), Ty::Time(Time::Instant)),
            _ => {
                return Err(Error::UnknownMember {
                    owner: time.name().into(),
                    member,
                    reference: path,
                })
            }
        };
        let code = match member.as_str() {
            "default" => time_code("ZERO"),
            member => time_code(member),
        };
        let (operand, end) = match input {
            None => (origin::Fragment::plain("()"), path.end()),
            Some(input) => {
                let (mut arguments, end) = self.parse_call_arguments("`std::time` arguments")?;
                match (input, arguments.as_mut_slice()) {
                    (Ty::Unit, []) => (origin::Fragment::plain("()"), end),
                    (input, [argument]) if input != Ty::Unit => {
                        self.coerce_expr(&input, argument)?;
                        (argument.rendered_source(), end)
                    }
                    (input, _) => {
                        return Err(Error::argument_count(
                            Callee::AssociatedFunction,
                            crate::resolve::Qualified(time.name(), &member).to_string(),
                            usize::from(input != Ty::Unit),
                            arguments.len(),
                            span(path.start(), end),
                        ))
                    }
                }
            }
        };
        let source = self.prelude_op(
            PreludeOp::Time,
            [
                origin::Fragment::plain("()"),
                code.to_string().into(),
                operand,
            ],
        );
        Ok(self.operator_expr(source, output, span(path.start(), end)))
    }

    /// A method of std's `Duration` or `Instant` on `value`.
    pub(super) fn parse_time_method(
        &mut self,
        value: ParsedExpr,
        time: Time,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let duration = Ty::Time(Time::Duration);
        let instant = Ty::Time(Time::Instant);
        let optional = |ty: Ty| Ty::Option(Box::new(ty));
        let (input, output, code) = match (time, member.as_str()) {
            (_, "clone") => {
                let (_, end) = self.parse_call_arguments("`clone` arguments")?;
                let source = value.rendered_source();
                return Ok(self.operator_expr(
                    source,
                    Ty::Time(time),
                    span(value.span.start(), end),
                ));
            }
            (Time::Duration, "as_secs") => (None, Ty::Int(Int::U64), "as_secs"),
            (Time::Duration, "as_millis" | "as_micros" | "as_nanos") => {
                (None, Ty::Int(Int::U64), member.as_str())
            }
            (Time::Duration, "as_secs_f64") => (None, Ty::Float(Float::F64), "as_secs_f64"),
            (Time::Duration, "as_secs_f32") => (None, Ty::Float(Float::F32), "as_secs_f32"),
            (Time::Duration, "subsec_millis" | "subsec_micros" | "subsec_nanos") => {
                (None, Ty::Int(Int::U32), member.as_str())
            }
            (Time::Duration, "is_zero") => (None, Ty::Bool, "is_zero"),
            (Time::Duration, "checked_add" | "checked_sub") => {
                (Some(duration.clone()), optional(duration), member.as_str())
            }
            (Time::Duration, "saturating_add" | "saturating_sub" | "abs_diff") => {
                (Some(duration.clone()), duration, member.as_str())
            }
            (Time::Duration, "mul_f64" | "div_f64") => {
                (Some(Ty::Float(Float::F64)), duration, member.as_str())
            }
            (Time::Instant, "elapsed") => (None, duration, "elapsed"),
            (Time::Instant, "duration_since" | "saturating_duration_since") => {
                (Some(instant), duration, member.as_str())
            }
            (Time::Instant, "checked_duration_since") => {
                (Some(instant), optional(duration), "checked_duration_since")
            }
            (Time::Instant, "checked_add") => {
                (Some(duration), optional(instant), "instant_checked_add")
            }
            (Time::Instant, "checked_sub") => {
                (Some(duration), optional(instant), "instant_checked_sub")
            }
            _ => {
                return Err(Error::UnknownMember {
                    owner: time.name().into(),
                    member,
                    reference: member_span,
                })
            }
        };
        let code = time_code(code);
        let (mut arguments, end) = self.parse_call_arguments("`std::time` method arguments")?;
        let operand = match (input, arguments.as_mut_slice()) {
            (None, []) => origin::Fragment::plain("()"),
            (Some(input), [argument]) => {
                self.coerce_expr(&input, argument)?;
                argument.rendered_source()
            }
            (input, _) => {
                return Err(Error::argument_count(
                    Callee::Method,
                    crate::resolve::Qualified(time.name(), &member).to_string(),
                    usize::from(input.is_some()),
                    arguments.len(),
                    span(member_span.start(), end),
                ))
            }
        };
        let source = self.prelude_op(
            PreludeOp::Time,
            [value.rendered_source(), code.to_string().into(), operand],
        );
        Ok(self.operator_expr(source, output, span(value.span.start(), end)))
    }

    /// `a + b`, `a - b`, `d * n`, `d / n` and `a < b` of std's `Duration`
    /// and `Instant`, as Rust's impls are, with their panics.
    pub(super) fn time_binary(
        &mut self,
        symbol: &str,
        lhs: ParsedExpr,
        mut rhs: ParsedExpr,
    ) -> Result<Option<ParsedExpr>, Error> {
        let peel = |ty: Ty| match ty {
            Ty::Reference { target, .. } => *target,
            ty => ty,
        };
        let Ty::Time(time) = peel(self.inference.resolve(&lhs.ty)) else {
            return Ok(None);
        };
        let duration = Ty::Time(Time::Duration);
        let other = peel(self.inference.resolve(&rhs.ty));
        let (code, output) = match (time, symbol, &other) {
            (Time::Duration, "+" | "+=", _) => ("add", duration.clone()),
            (Time::Duration, "-" | "-=", _) => ("sub", duration.clone()),
            (Time::Duration, "*" | "*=", _) => ("mul", duration.clone()),
            (Time::Duration, "/" | "/=", _) => ("div", duration.clone()),
            (Time::Instant, "+" | "+=", _) => ("instant_add", Ty::Time(Time::Instant)),
            (Time::Instant, "-", Ty::Time(Time::Instant)) => ("instant_since", duration.clone()),
            (Time::Instant, "-" | "-=", _) => ("instant_sub", Ty::Time(Time::Instant)),
            (_, "<" | ">" | "<=" | ">=", _) => ("compare", Ty::Bool),
            _ => return Ok(None),
        };
        let operand = match code {
            "mul" | "div" => Ty::Int(Int::U32),
            "instant_since" | "compare" => Ty::Time(time),
            _ => duration,
        };
        if let Ty::Time(_) = other {
            rhs.ty = other;
        }
        self.coerce_expr(&operand, &mut rhs)?;
        let expression = span(lhs.span.start(), rhs.span.end());
        let mut source = self.prelude_op(
            PreludeOp::Time,
            [
                lhs.rendered_source(),
                time_code(code).to_string().into(),
                rhs.rendered_source(),
            ],
        );
        if code == "compare" {
            source = source.surrounded("(", &format!(" {symbol} 0)"));
        }
        Ok(Some(self.operator_expr(source, output, expression)))
    }
}
