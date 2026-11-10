//! A trait object of a crate's trait behind a pointer, as tracing-core's
//! `Identifier(&'static dyn Callsite)` holds one: `&'static dyn Trait`,
//! `&'a dyn Trait` or `Box<dyn Trait>`, with its auto traits (D872).
//!
//! Native code makes the object. A program's value coerced to one crosses
//! tagged with the type it was coerced from ([`tag`]); the runner's adapter
//! finds that type among the crate's impls of the trait and coerces its
//! value, as rustc coerces it.

use serde::{Deserialize, Serialize};

use super::convert::Type;

/// The pointer an object is behind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Pointer {
    /// `&'static dyn Trait`: a static's place, or an object native code
    /// gave, which lives as long as the program.
    Lasting,
    /// `&'a dyn Trait`, lent for a call.
    Shared,
    /// `Box<dyn Trait>`, which owns its value.
    Boxed,
}

/// An auto trait an object names beside its trait.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Auto {
    Send,
    Sync,
}

impl Auto {
    /// Its Rust path.
    #[must_use]
    pub const fn rust_path(self) -> &'static str {
        match self {
            Self::Send => "::core::marker::Send",
            Self::Sync => "::core::marker::Sync",
        }
    }
}

/// `dyn Trait<arguments> + auto` behind `pointer`.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Object {
    pointer: Pointer,
    principal: crate::surface::identity::Definition,
    arguments: Box<[Type]>,
    auto: Box<[Auto]>,
}

impl Object {
    /// The object of the trait `principal` at `arguments`.
    #[must_use]
    pub fn new(
        pointer: Pointer,
        principal: crate::surface::identity::Definition,
        arguments: Box<[Type]>,
        mut auto: Vec<Auto>,
    ) -> Self {
        auto.sort_unstable();
        auto.dedup();
        Self {
            pointer,
            principal,
            arguments,
            auto: auto.into_boxed_slice(),
        }
    }

    /// This object behind `pointer`.
    #[must_use]
    pub fn behind(&self, pointer: Pointer) -> Self {
        Self {
            pointer,
            ..self.clone()
        }
    }

    /// This object with each of its trait's arguments mapped by `map`.
    pub(crate) fn try_map<E>(
        &self,
        mut map: impl FnMut(&Type) -> Result<Type, E>,
    ) -> Result<Self, E> {
        Ok(Self {
            pointer: self.pointer,
            principal: self.principal,
            arguments: self
                .arguments
                .iter()
                .map(&mut map)
                .collect::<Result<_, _>>()?,
            auto: self.auto.clone(),
        })
    }

    #[must_use]
    pub const fn pointer(&self) -> Pointer {
        self.pointer
    }

    /// The trait's identity.
    #[must_use]
    pub const fn principal(&self) -> &crate::surface::identity::Definition {
        &self.principal
    }

    /// The trait's type arguments.
    #[must_use]
    pub fn arguments(&self) -> &[Type] {
        &self.arguments
    }

    #[must_use]
    pub fn auto(&self) -> &[Auto] {
        &self.auto
    }

    /// Each type the object names: its trait's arguments.
    pub fn types(&self) -> impl Iterator<Item = &Type> {
        self.arguments.iter()
    }
}

/// The tag of the program's `format_args!` coerced to an object, core's
/// `fmt::Arguments<'_>`, which the adapter formats where the call runs.
pub const ARGUMENTS: &str = "fmt::Arguments";

/// The tag a program's value coerced to an object crosses with: the type
/// it was coerced from, a number, `bool` or `char` by its Rust name,
/// `format_args!` as [`ARGUMENTS`], a crate type with no parameters by its
/// identity. `None` for another type, which no impl the adapter knows is
/// for.
#[must_use]
pub fn tag(ty: &Type) -> Option<String> {
    use std::fmt::Write as _;
    match ty {
        // The program's `format_args!` (#1561).
        Type::Builtin(super::convert::Builtin::Arguments) => Some(ARGUMENTS.to_owned()),
        Type::Builtin(builtin) => builtin.rust_name().map(str::to_owned),
        Type::Nominal {
            definition,
            arguments,
        } if arguments.is_empty() => {
            let mut text = String::from("#");
            for byte in definition.as_bytes() {
                write!(text, "{byte:02x}").ok()?;
            }
            Some(text)
        }
        _ => None,
    }
}
