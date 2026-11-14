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
    /// `Rc<dyn Trait>`, which shares its value (#1304).
    Rc,
    /// `Arc<dyn Trait>`, which shares its value across threads (#1304).
    Arc,
}

impl Pointer {
    /// Whether the object owns its value, as a `Box`, `Rc` or `Arc` does,
    /// rather than borrowing it.
    #[must_use]
    pub const fn owns(self) -> bool {
        matches!(self, Self::Boxed | Self::Rc | Self::Arc)
    }
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
    /// What a program's function or closure fills the trait with, when the
    /// crate implements it for every callable of one signature, as iced's
    /// `impl<T: Fn(Input, &Theme) -> Style> Highlighter<Input, Theme> for
    /// T` (#1981).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    call: Option<Box<Call>>,
}

/// The inputs and output of the `Fn` bound of a trait's one impl for every
/// callable, at the object's arguments: a program's callable coerced to
/// the object runs with these, as the impl's methods call it.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Call {
    inputs: Box<[Type]>,
    output: Type,
}

impl Call {
    #[must_use]
    pub const fn new(inputs: Box<[Type]>, output: Type) -> Self {
        Self { inputs, output }
    }

    /// The callable's inputs, in order.
    #[must_use]
    pub fn inputs(&self) -> &[Type] {
        &self.inputs
    }

    /// The callable's output.
    #[must_use]
    pub const fn output(&self) -> &Type {
        &self.output
    }
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
            call: None,
        }
    }

    /// This object, which a program's callable fills by `call`.
    #[must_use]
    pub fn with_call(self, call: Option<Call>) -> Self {
        Self {
            call: call.map(Box::new),
            ..self
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
        let call = match &self.call {
            Some(call) => Some(Box::new(Call {
                inputs: call.inputs.iter().map(&mut map).collect::<Result<_, _>>()?,
                output: map(&call.output)?,
            })),
            None => None,
        };
        Ok(Self {
            pointer: self.pointer,
            principal: self.principal,
            arguments: self
                .arguments
                .iter()
                .map(&mut map)
                .collect::<Result<_, _>>()?,
            auto: self.auto.clone(),
            call,
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

    /// What a program's callable fills the trait with, when one does.
    #[must_use]
    pub fn call(&self) -> Option<&Call> {
        self.call.as_deref()
    }

    /// Each type the object names: its trait's arguments, and the inputs
    /// and output of its call.
    pub fn types(&self) -> impl Iterator<Item = &Type> {
        self.arguments.iter().chain(
            self.call
                .iter()
                .flat_map(|call| call.inputs.iter().chain([&call.output])),
        )
    }
}

/// The tag of the program's function or closure coerced to an object of a
/// trait the crate implements for every callable ([`Object::call`]): the
/// adapter makes a Rust closure that calls it, which the impl takes.
pub const CALLABLE: &str = "fn";

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
