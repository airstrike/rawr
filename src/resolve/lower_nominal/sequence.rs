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
#[cfg(feature = "rust-import")]
use crate::{Dynamic, RawrResult, Span};
#[cfg(feature = "rust-import")]
use std::collections::BTreeSet;

/// Which of Rust's sequence types a `Ty::Array` is. Each is its own type,
/// as in Rust, and every one is an array at runtime.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Kind {
    /// `[T; N]`, of its length.
    Array(usize),
    /// `Vec<T>`, which derefs to `[T]`.
    Vec,
    /// `[T]`, behind a reference or a box.
    Slice,
    /// `VecDeque<T>`, which has no `[T]` of its own.
    Deque,
}

impl Kind {
    /// The element and kind of a header's sequence type: `Vec<T>`, `[T]`,
    /// `VecDeque<T>` or `[T; N]`.
    pub(super) fn of(ty: &type_ref::Type) -> Option<(&type_ref::Type, Self)> {
        if let Some((element, length)) = ty.array_element() {
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

    /// An array's length; `None` for the others.
    pub(super) fn length(self) -> Option<usize> {
        match self {
            Self::Array(length) => Some(length),
            Self::Vec | Self::Slice | Self::Deque => None,
        }
    }
}

#[cfg(feature = "rust-import")]
#[derive(Clone)]
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
        match name {
            "Vec" => Some(Kind::Vec),
            "VecDeque" if self.models.get(name).is_none() => Some(Kind::Deque),
            _ => None,
        }
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

    /// The growable sequence `Vec` or std's `VecDeque` named by `name` and
    /// the path before the next `::` (`VecDeque`, `collections::VecDeque`,
    /// `std::collections::VecDeque`), unless the program names a type
    /// so. Consumes the path, leaving the `::` before the member.
    pub(super) fn growable_path(&mut self, name: &str) -> Option<Kind> {
        let mut ahead = self.input.clone();
        let tokens: Vec<Token> = (0..5)
            .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
            .collect();
        let identifier = |token: &Token, text: &str| matches!(token, Token::Identifier(name) if name.as_str() == text);
        let (kind, consumed) = match name {
            "Vec" if self.models.get(name).is_none() && self.models.import(name).is_none() => {
                (Kind::Vec, 0)
            }
            "VecDeque" if self.models.get(name).is_none() => (Kind::Deque, 0),
            "collections"
                if tokens[0] == Token::DoubleColon && identifier(&tokens[1], "VecDeque") =>
            {
                (Kind::Deque, 2)
            }
            "std" | "alloc"
                if tokens[0] == Token::DoubleColon
                    && identifier(&tokens[1], "collections")
                    && tokens[2] == Token::DoubleColon
                    && identifier(&tokens[3], "VecDeque") =>
            {
                (Kind::Deque, 4)
            }
            _ => return None,
        };
        if tokens[consumed] != Token::DoubleColon {
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
