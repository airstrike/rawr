//! Impls of a crate's trait for references: `impl<T: For + Copy> For for
//! &T`, Rust's idiom for letting `&T` stand where a `T: For` would.
//!
//! Rawr runs a reference as its referent's value, so a call through `&T`
//! reaches `T`'s impl at runtime. That is the impl's own meaning exactly
//! when each of its items forwards to `T`'s (`<T as For>::f(*self)`,
//! `type A = <T as For>::A;`). Such an impl is folded into its trait as a
//! `ReferenceForward`, which the checker reads as "`&T` implements the trait
//! where `T` does"; any other impl for a reference is an error.

use super::item::{self, ReferenceForward};
use super::{type_ref, Error, LocatedError};
use crate::item::{BodyTokenKind as Kind, Receiver};
use crate::{capability, Span};

/// Folds each forwarding impl for references of a crate's trait into the
/// trait (`item::Trait::reference_forwards`), and refuses any other impl of
/// one for a reference.
pub(super) fn fold_reference_impls(items: &mut Vec<item::Shell>) -> Result<(), LocatedError> {
    let mut folded = Vec::new();
    for (index, shell) in items.iter().enumerate() {
        let item::Kind::Impl(implementation) = &shell.kind else {
            continue;
        };
        let (
            Some(item::TraitImpl::User { name, arguments }),
            type_ref::Type::Reference {
                mutable, target, ..
            },
        ) = (&implementation.trait_, &implementation.owner)
        else {
            continue;
        };
        // A prelude trait's impls for references stay as they were.
        let Some(declared) = items.iter().find_map(|shell| match &shell.kind {
            item::Kind::Trait(declared) if declared.name.text == name.text => Some(declared),
            _ => None,
        }) else {
            continue;
        };
        let refuse = |reason: &str| {
            LocatedError::primary(
                shell.source,
                Error::UnsupportedNominalSyntax {
                    construct: format!(
                        "an impl of `{trait_name}` for a reference {reason}: Rawr runs a \
                         reference as its referent's value, so it reads an impl for \
                         references only as `impl<T: {trait_name}> {trait_name} for &T` whose \
                         every item forwards to `T`'s (`<T as {trait_name}>::f(*self)`, `type \
                         A = <T as {trait_name}>::A;`)",
                        trait_name = name.text,
                    )
                    .into(),
                    source: type_span(&implementation.owner),
                },
            )
        };
        if !arguments.is_empty() || !declared.generics.is_empty() {
            return Err(refuse("of a generic trait"));
        }
        let type_ref::Type::Path(referent) = &**target else {
            return Err(refuse("to a type other than the impl's parameter"));
        };
        if referent.target != type_ref::Target::Parameter || !referent.arguments.is_empty() {
            return Err(refuse("to a type other than the impl's parameter"));
        }
        let [generic] = &implementation.generics[..] else {
            return Err(refuse("with other than one type parameter"));
        };
        if referent.name != generic.name.text {
            return Err(refuse("to a type other than the impl's parameter"));
        }
        if !generic.traits.iter().any(|bound| bound.text == name.text) {
            return Err(refuse(&format!(
                "whose parameter is not bounded by `{}`",
                name.text
            )));
        }
        if !generic.projections.is_empty() || !generic.trait_arguments.is_empty() {
            return Err(refuse("whose parameter's bounds name types"));
        }
        let parameter = &generic.name.text;
        // `type A = <T as For>::A;` for each associated type.
        for associated in declared.associated.iter() {
            let forwards = implementation
                .associated
                .iter()
                .find(|(written, _)| written.text == associated.text)
                .is_some_and(|(_, ty)| forwards_type(ty, parameter, &name.text, &associated.text));
            if !forwards {
                return Err(refuse(&format!(
                    "whose `type {}` is not `<{parameter} as {}>::{}`",
                    associated.text, name.text, associated.text
                )));
            }
        }
        // Each function has the trait's signature, `Self` being `&T`, as
        // rustc checks it of any impl (E0046, E0050, E0053, E0407).
        super::trait_impl_functions(
            declared,
            &super::full_type_name(&implementation.owner),
            &implementation.owner,
            &implementation.functions,
            &implementation.associated,
            &[],
            type_span(&implementation.owner),
        )
        .map_err(|error| LocatedError::primary(shell.source, error))?;
        // `fn f(self) { <T as For>::f(*self) }` for each function.
        for function in declared.functions.iter() {
            let Some(written) = implementation
                .functions
                .iter()
                .find(|written| written.name.text == function.name.text)
            else {
                return Err(refuse(&format!(
                    "that leaves `{}` to the trait's default",
                    crate::tokenizer::demangle_names(&function.name.text)
                )));
            };
            let copy = generic.bounds.contains(capability::Kind::Copy);
            if !forwards_function(written, parameter, &name.text, *mutable, copy) {
                return Err(refuse(&format!(
                    "whose `{}` does not forward to `{parameter}`'s",
                    crate::tokenizer::demangle_names(&function.name.text)
                )));
            }
        }
        let mut traits = generic
            .traits
            .iter()
            .filter(|bound| bound.text != name.text)
            .cloned()
            .collect::<Vec<_>>();
        traits.dedup_by(|left, right| left.text == right.text);
        folded.push((
            index,
            name.text.clone(),
            ReferenceForward {
                mutable: *mutable,
                bounds: generic.bounds,
                traits: traits.into_boxed_slice(),
            },
        ));
    }
    for (index, trait_name, forward) in folded.into_iter().rev() {
        items.remove(index);
        for shell in items.iter_mut() {
            if let item::Kind::Trait(declared) = &mut shell.kind
                && declared.name.text == trait_name
            {
                let mut forwards = declared.reference_forwards.to_vec();
                forwards.push(forward);
                declared.reference_forwards = forwards.into_boxed_slice();
                break;
            }
        }
    }
    Ok(())
}

/// Whether `ty` is `<T as Trait>::name` (or `T::name`) of the parameter `T`.
fn forwards_type(ty: &type_ref::Type, parameter: &str, trait_name: &str, name: &str) -> bool {
    let type_ref::Type::Path(path) = ty else {
        return false;
    };
    let type_ref::Target::Projection(base, written) = &path.target else {
        return false;
    };
    path.name == name
        && path.arguments.is_empty()
        && written.as_ref().is_none_or(|written| written == trait_name)
        && matches!(&**base, type_ref::Type::Path(base)
            if base.target == type_ref::Target::Parameter && base.name == parameter)
}

/// Whether `function`'s body is one call of `T`'s own `function`, given
/// what its receiver holds (`self` of an impl for `&T` is a `&T`), with its
/// other parameters passed on in order:
///
/// - `<T as Trait>::f(arg, ..)`, `T::f(arg, ..)` or `Trait::f(arg, ..)`,
///   where `arg` is the referent: `*self` of a `self` receiver (a copy, so
///   `T: Copy`), `*self` or `&**self` of `&self`, and `*self` or `&mut
///   **self` of `&mut self` in an impl for `&mut T`;
/// - `(*self).f(..)` of `self` (`T: Copy`) or `&self`, and `(**self).f(..)`
///   of `&self` or (for `&mut T`) `&mut self`.
fn forwards_function(
    function: &item::Function,
    parameter: &str,
    trait_name: &str,
    mutable: bool,
    copy: bool,
) -> bool {
    // The lexer reads the `()` of a call without arguments as one token,
    // and may read `**` as one.
    let (open, close, star) = (Kind::LeftParen, Kind::RightParen, Kind::Star);
    let tokens = function
        .body
        .syntax
        .tokens()
        .iter()
        .flat_map(|token| match token.kind() {
            Kind::Unit => vec![&open, &close],
            Kind::StarStar => vec![&star, &star],
            kind => vec![kind],
        })
        .collect::<Vec<_>>();
    let tokens = match &tokens[..] {
        [Kind::LeftBrace, inner @ .., Kind::RightBrace] => inner,
        all => all,
    };
    // A function returning `()` may end its call with `;`.
    let tokens = match tokens {
        [call @ .., Kind::Semicolon] if function.output.is_none() => call,
        call => call,
    };
    let name = function.name.text.as_str();
    // `const N: usize = <T as Trait>::N;`, or `T::N`.
    if function.body.constant {
        let is =
            |kind: &Kind, text: &str| matches!(kind, Kind::Identifier(written) if written == text);
        return match tokens {
            [base, Kind::DoubleColon, constant] => is(base, parameter) && is(constant, name),
            [Kind::LessThan, base, Kind::As, rest @ ..] if is(base, parameter) => {
                matches!(rest, [path @ .., Kind::GreaterThan, Kind::DoubleColon, constant]
                    if trait_path(path, trait_name) && is(constant, name))
            }
            _ => false,
        };
    }
    let rest = function
        .parameters
        .iter()
        .map(|parameter| {
            parameter
                .pattern
                .is_none()
                .then_some(parameter.name.text.as_str())
        })
        .collect::<Option<Vec<_>>>();
    let Some(rest) = rest else {
        return false;
    };
    // The referent, as the first argument of a path call.
    let referents: &[&[Kind]] = match function.receiver {
        None => &[],
        Some(Receiver::Owned(_) | Receiver::OwnedMutable(_)) if copy => {
            &[&[Kind::Star, Kind::SelfValue]]
        }
        Some(Receiver::Owned(_) | Receiver::OwnedMutable(_)) => return false,
        Some(Receiver::Shared(_)) => &[
            &[Kind::Star, Kind::SelfValue],
            &[Kind::Ampersand, Kind::Star, Kind::Star, Kind::SelfValue],
        ],
        Some(Receiver::Mutable(_)) if mutable => &[
            &[Kind::Star, Kind::SelfValue],
            &[
                Kind::Ampersand,
                Kind::Mut,
                Kind::Star,
                Kind::Star,
                Kind::SelfValue,
            ],
        ],
        Some(Receiver::Mutable(_)) => return false,
    };
    // `(*self).f(` and `(**self).f(`.
    let receivers: &[&[Kind]] = match function.receiver {
        None => &[],
        Some(Receiver::Owned(_) | Receiver::OwnedMutable(_)) => &[&[
            Kind::LeftParen,
            Kind::Star,
            Kind::SelfValue,
            Kind::RightParen,
        ]],
        Some(Receiver::Shared(_)) => &[
            &[
                Kind::LeftParen,
                Kind::Star,
                Kind::SelfValue,
                Kind::RightParen,
            ],
            &[
                Kind::LeftParen,
                Kind::Star,
                Kind::Star,
                Kind::SelfValue,
                Kind::RightParen,
            ],
        ],
        Some(Receiver::Mutable(_)) => &[&[
            Kind::LeftParen,
            Kind::Star,
            Kind::Star,
            Kind::SelfValue,
            Kind::RightParen,
        ]],
    };
    if let Some(after) = receivers
        .iter()
        .find_map(|receiver| strip(tokens, receiver))
        && let [Kind::Dot, Kind::Identifier(method), Kind::LeftParen, arguments @ ..] = after
        && method == name
    {
        return arguments_are(arguments, None, &rest);
    }
    let Some(after) = callee(tokens, parameter, trait_name, name) else {
        return false;
    };
    match function.receiver {
        None => arguments_are(after, None, &rest),
        Some(_) => referents
            .iter()
            .any(|referent| arguments_are(after, Some(referent), &rest)),
    }
}

/// The tokens after `<T as Trait>::f(`, `T::f(` or `Trait::f(` (a trait
/// path may name its modules: `shapes::Trait::f(`).
fn callee<'a>(
    tokens: &'a [&'a Kind],
    parameter: &str,
    trait_name: &str,
    name: &str,
) -> Option<&'a [&'a Kind]> {
    let is = |kind: &Kind, text: &str| matches!(kind, Kind::Identifier(written) if written == text);
    let rest = match tokens {
        [Kind::LessThan, base, Kind::As, rest @ ..] if is(base, parameter) => {
            let close = rest
                .iter()
                .position(|kind| matches!(kind, Kind::GreaterThan))?;
            let path = &rest[..close];
            if !trait_path(path, trait_name) {
                return None;
            }
            &rest[close + 1..]
        }
        [base, rest @ ..] if is(base, parameter) => rest,
        _ => {
            let call = tokens
                .iter()
                .position(|kind| matches!(kind, Kind::LeftParen))?;
            let path = tokens[..call].split_last()?.1.split_last()?.1;
            if !trait_path(path, trait_name) {
                return None;
            }
            &tokens[call - 2..]
        }
    };
    match rest {
        [Kind::DoubleColon, function, Kind::LeftParen, after @ ..] if is(function, name) => {
            Some(after)
        }
        _ => None,
    }
}

/// Whether `path` spells the trait: `Trait` or `module::Trait`.
fn trait_path(path: &[&Kind], trait_name: &str) -> bool {
    let Some((last, modules)) = path.split_last() else {
        return false;
    };
    matches!(last, Kind::Identifier(written) if written == trait_name)
        && modules.len() % 2 == 0
        && modules
            .chunks(2)
            .all(|pair| matches!(pair, [Kind::Identifier(_), Kind::DoubleColon]))
}

/// Whether `tokens` is `referent, a, b)` (or `a, b)` without one): the
/// referent's tokens, then each of `names` in order, then the call's
/// close, and nothing after it but a `;`-less end.
fn arguments_are(tokens: &[&Kind], referent: Option<&[Kind]>, names: &[&str]) -> bool {
    let mut rest = tokens;
    let mut first = true;
    if let Some(referent) = referent {
        let Some(after) = strip(rest, referent) else {
            return false;
        };
        rest = after;
        first = false;
    }
    for name in names {
        if !first {
            let [Kind::Comma, after @ ..] = rest else {
                return false;
            };
            rest = after;
        }
        let [Kind::Identifier(written), after @ ..] = rest else {
            return false;
        };
        if written != name {
            return false;
        }
        rest = after;
        first = false;
    }
    matches!(rest, [Kind::RightParen] | [Kind::Comma, Kind::RightParen])
}

/// `tokens` after `prefix`, when they start with it.
fn strip<'a>(tokens: &'a [&'a Kind], prefix: &[Kind]) -> Option<&'a [&'a Kind]> {
    (tokens.len() >= prefix.len()
        && tokens
            .iter()
            .zip(prefix)
            .all(|(written, expected)| *written == expected))
    .then(|| &tokens[prefix.len()..])
}

/// The span of a resolved type.
fn type_span(ty: &type_ref::Type) -> Span {
    super::type_span(ty)
}
