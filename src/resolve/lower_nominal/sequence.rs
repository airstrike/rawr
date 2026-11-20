//! Rust's sequence types (`[T; N]`, `Vec<T>`, `[T]`, `VecDeque<T>`), and
//! checked native sequence construction, preserving the concrete Rust
//! shape separately from the interpreter's normalized logical type
//! witness.

#[cfg(feature = "rust-import")]
use super::{invariant, origin, FnCallArgs, NativeCallContext, Op};
use super::{type_ref, Error, Parser, Ty};
#[cfg(feature = "rust-import")]
use crate::host::runtime::type_ref::Type;
use crate::tokenizer::Token;
use crate::ImmutableString;
#[cfg(feature = "rust-import")]
use crate::{Dynamic, RawrResult, Span};
#[cfg(feature = "rust-import")]
use std::collections::BTreeSet;

/// Which of Rust's sequence types a `Ty::Array` is. Each is its own type,
/// as in Rust, and every one is an array at runtime.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Kind {
    /// `[T; N]`, of its length.
    Array(Length),
    /// `Vec<T>`, which derefs to `[T]`.
    Vec,
    /// `[T]`, behind a reference or a box.
    Slice,
    /// `VecDeque<T>`, which has no `[T]` of its own.
    Deque,
}

/// An array's length. A const parameter's (`[T; N]` of `const N: usize`)
/// is substituted and inferred as a type argument is: a use of the item
/// gives it a number, or an inference variable that unifying with another
/// array's length settles.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Length {
    Fixed(usize),
    Param(ImmutableString),
    Infer(usize),
}

impl Length {
    /// The length a const generic argument `ty` stands for: a number of
    /// `Ty::Const`, a parameter or an inference variable. `None` for any
    /// other type, which no length is.
    pub(super) fn of_argument(ty: &Ty) -> Option<Self> {
        match ty {
            Ty::Const(value) => value.parse().ok().map(Self::Fixed),
            Ty::Param(name) => Some(Self::Param(name.clone())),
            Ty::Infer(id) => Some(Self::Infer(*id)),
            _ => None,
        }
    }

    /// This length as the const generic argument it is, which unifies and
    /// substitutes as a type argument does.
    pub(super) fn argument(&self) -> Ty {
        match self {
            Self::Fixed(length) => Ty::Const(length.to_string().into()),
            Self::Param(name) => Ty::Param(name.clone()),
            Self::Infer(id) => Ty::Infer(*id),
        }
    }

    /// This length with `map` applied to it as a const generic argument;
    /// itself when `map` gives what no length is.
    pub(super) fn mapped(&self, map: impl FnOnce(&Ty) -> Ty) -> Self {
        match self {
            Self::Fixed(_) => self.clone(),
            Self::Param(_) | Self::Infer(_) => {
                Self::of_argument(&map(&self.argument())).unwrap_or_else(|| self.clone())
            }
        }
    }

    /// The number, once it is one.
    pub(super) fn fixed(&self) -> Option<usize> {
        match self {
            Self::Fixed(length) => Some(*length),
            Self::Param(_) | Self::Infer(_) => None,
        }
    }
}

impl std::fmt::Display for Length {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fixed(length) => write!(f, "{length}"),
            Self::Param(name) => write!(f, "{name}"),
            // rustc writes a length it has yet to infer as `_`.
            Self::Infer(_) => write!(f, "_"),
        }
    }
}

impl Kind {
    /// The element and kind of a header's sequence type: `Vec<T>`, `[T]`,
    /// `VecDeque<T>` or `[T; N]`.
    pub(super) fn of(ty: &type_ref::Type) -> Option<(&type_ref::Type, Self)> {
        if let Some((element, length)) = ty.array_element() {
            let length = match length {
                crate::item::type_ref::Length::Fixed(length) => Length::Fixed(length),
                crate::item::type_ref::Length::Param(name) => Length::Param(name.into()),
            };
            return Some((element, Self::Array(length)));
        }
        [
            (ty.vec_element(), Self::Vec),
            (ty.slice_element(), Self::Slice),
            (ty.deque_element(), Self::Deque),
        ]
        .into_iter()
        .find_map(|(element, kind)| Some((element?, kind)))
    }

    /// `[T; length]`.
    pub(super) fn array(length: usize) -> Self {
        Self::Array(Length::Fixed(length))
    }

    /// Whether this is `[T; N]`, of any length.
    pub(super) fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    /// An array's length, once it is a number; `None` for the others.
    pub(super) fn length(&self) -> Option<usize> {
        match self {
            Self::Array(length) => length.fixed(),
            Self::Vec | Self::Slice | Self::Deque => None,
        }
    }
}

#[cfg(feature = "rust-import")]
#[derive(Clone)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Constructor {
    key: Option<Box<[u8]>>,
    target: Option<Type>,
}

#[cfg(feature = "rust-import")]
impl Constructor {
    pub(super) fn checked(
        concrete: Option<&rawr_import::classify::representability::convert::Type>,
        target: Option<Type>,
    ) -> Self {
        let key = concrete.filter(|_| target.is_some()).map(|concrete| {
            serde_json::to_vec(concrete)
                .expect("a sequence's Rust type encodes as JSON")
                .into_boxed_slice()
        });
        Self { key, target }
    }

    pub(super) fn dependencies(&self) -> BTreeSet<crate::host::facade::Dependency> {
        let mut dependencies = BTreeSet::new();
        if let Some(target) = &self.target {
            target.collect_dependencies(&mut dependencies);
        }
        dependencies
    }

    /// This constructor with each program type's schema as `schema`
    /// gives it (`Type::renewed`).
    pub(super) fn renewed(
        &self,
        schema: &dyn Fn(&crate::type_def::Identity) -> Option<crate::program::schema::Fingerprint>,
    ) -> Option<Self> {
        Some(Self {
            key: self.key.clone(),
            target: match &self.target {
                Some(target) => Some(target.renewed(schema)?),
                None => None,
            },
        })
    }

    pub(super) fn name(&self) -> String {
        format!(
            "\0rawr.sequence.construct.{:016x}",
            crate::program::schema::symbol_id(&self.key)
        )
    }

    pub(super) fn invoke(
        &self,
        context: Option<&NativeCallContext<'_>>,
        arguments: &mut FnCallArgs,
    ) -> RawrResult {
        let capacity = arguments[0]
            .as_int()
            .ok()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| invariant("a sequence constructor capacity is not usize"))?;
        let value = arguments[1].take();
        let mode = arguments[2].as_int().unwrap_or(-1);
        if let (Some(key), Some(target), Some(runtime)) = (
            self.key.as_deref(),
            self.target.as_ref(),
            context.and_then(|context| context.host_runtime()),
        ) && let Some(native) =
            crate::place::sequence::construct(key, &runtime.witness_for(target), capacity)?
        {
            let place = crate::place::get(&native)
                .ok_or_else(|| invariant("a native sequence factory lost its place"))?;
            match mode {
                0 => {}
                1 => {
                    let items = value
                        .into_array()
                        .map_err(|_| invariant("a sequence literal lost its evaluated operands"))?;
                    for item in items {
                        let _ = place.invoke(crate::place::Operation::Push, &mut [item])?;
                    }
                }
                2 => {
                    let _ = place.invoke(
                        crate::place::Operation::Resize,
                        &mut [Dynamic::from_int(capacity as crate::INT), value],
                    )?;
                }
                _ => return Err(invariant("an unknown sequence constructor mode")),
            }
            return Ok(native);
        }
        let mut items = crate::Array::with_capacity(capacity);
        match mode {
            0 => {}
            1 => items.extend(
                value
                    .into_array()
                    .map_err(|_| invariant("a sequence literal lost its evaluated operands"))?,
            ),
            2 => items.resize(capacity, value),
            _ => return Err(invariant("an unknown sequence constructor mode")),
        }
        Ok(Dynamic::from_array(items))
    }
}

impl Parser<'_> {
    /// The growable sequence a type's name `name` is: `Vec`, or std's
    /// `VecDeque` unless the program names a type so.
    pub(super) fn growable_named(&self, name: &str) -> Option<Kind> {
        if name == "Vec" {
            return Some(Kind::Vec);
        }
        self.std_scoped(name)
            .filter(|path| crate::resolve::std_vec_deque(path))
            .map(|_| Kind::Deque)
    }

    /// `Vec<T>` or `VecDeque<T>` after its name `name`, at `<`.
    pub(super) fn parse_growable_type(
        &mut self,
        kind: Kind,
        name: &str,
        name_span: crate::Span,
    ) -> Result<Ty, Error> {
        self.next();
        let element = self.parse_local_type()?;
        self.unstable_argument(name, 1, name_span)?;
        self.expect_close_angle(&format!("'>' after `{name}<T`"))?;
        Ok(Ty::sequence(kind, element))
    }

    /// The growable sequence the prelude's `Vec` or std's `VecDeque`, which
    /// the body path starting with `name` names before the next `::`,
    /// through the scope (`std_path_ahead`): `VecDeque` after `use
    /// std::collections::VecDeque;`, or `std::collections::VecDeque`.
    /// Consumes the path, leaving the `::` before the member.
    pub(super) fn growable_path(&mut self, name: &str) -> Option<Kind> {
        let (kind, consumed, next) = if name == "Vec"
            && self.models.get(name).is_none()
            && self.models.import(name).is_none()
        {
            let next = self
                .input
                .peek()
                .map_or(Token::EOF, |(token, _)| token.clone());
            (Kind::Vec, 0, next)
        } else {
            let (_, consumed, next) = self.std_path_ahead(name, crate::resolve::std_vec_deque)?;
            (Kind::Deque, consumed, next)
        };
        if next != Token::DoubleColon {
            return None;
        }
        for _ in 0..consumed {
            self.next();
        }
        Some(kind)
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn sequence_source(
        &mut self,
        ty: &Ty,
        span: Span,
        capacity: origin::Fragment,
        items: origin::Fragment,
        mode: usize,
    ) -> origin::Fragment {
        let helper = self.push_op(Op::Sequence {
            target: ty.clone(),
            span,
        });
        origin::Fragment::call(&helper, [capacity, items, mode.to_string().into()])
    }
}

#[cfg(all(test, feature = "rust-import"))]
mod tests;
