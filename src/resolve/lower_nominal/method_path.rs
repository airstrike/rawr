//! A path to a std type's function, as rustc reads one: `Vec::len`,
//! `str::trim`, `HashMap::values` or `char::to_ascii_uppercase` named
//! without a call is a function value whose first input is the receiver,
//! taken as the method declares it (`&Vec<T>` of `Vec::len`), and called
//! (`Vec::len(&v)`) it is the method called on its first argument. An
//! associated function named without a call (`String::new`,
//! `char::from_u32`) is the closure that calls it.

use super::*;

impl Parser<'_> {
    /// `Owner::member` of the std type `owner`, which the program writes
    /// `written`, read up to `member`, when std declares `member` a function
    /// of `owner` itself or of a std trait it implements. `None`, reading
    /// nothing more, for any other member and for a call of an associated
    /// function.
    pub(super) fn std_method_path(
        &mut self,
        owner: Ty,
        written: &str,
        member: &ImmutableString,
        owner_span: Span,
        member_span: Span,
    ) -> Option<Result<ParsedExpr, Error>> {
        let function = std_method::path_function(&owner, member)?;
        let called = matches!(self.peek(), Token::LeftParen | Token::Unit);
        let Some(mode) = function.mode else {
            // `str::from_utf8` is reached through the `std::str` module.
            if called || owner == Ty::Str {
                return None;
            }
            return Some(self.function_value_closure(
                &format!("{written}::{member}"),
                function.inputs,
                None,
                span(owner_span.start(), member_span.end()),
            ));
        };
        if called {
            return Some(match self.path_method_call(member, member_span, None) {
                Some(tokens) => {
                    self.splice_tokens(tokens);
                    self.parse_primary()
                }
                None => Err(Error::argument_count(
                    Callee::AssociatedFunction,
                    format!("{written}::{member}"),
                    function.inputs,
                    0,
                    span(owner_span.start(), member_span.end()),
                )),
            });
        }
        // `&self` of `str` is a `&str`, which is rawr's `Ty::Str`.
        let receiver = match mode {
            receiver::Mode::Owned => owner,
            receiver::Mode::Shared if owner == Ty::Str => owner,
            mode => Ty::Reference {
                mutable: mode == receiver::Mode::Mutable,
                target: Box::new(owner),
            },
        };
        let inputs = std::iter::once(receiver)
            .chain((1..function.inputs).map(|_| self.inference.fresh()))
            .collect();
        Some(self.method_value(member.clone(), inputs, owner_span, member_span))
    }
}
