//! Admitted owned Phase 2 conversion vocabulary.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Builtin {
    Bool,
    Char,
    F64,
    I64,
    InclusiveRange,
    String,
    I8,
    I16,
    I32,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
    F32,
    /// A `&str` input: Rawr's string, lent to the Rust call.
    Str,
    /// A `&'static str` input, as iced's `Font::new(name: &'static str)`
    /// takes: Rawr's string, pinned for as long as the runner lives.
    StaticStr,
    /// A `fmt::Arguments` input, as log's `Record` and tracing's
    /// `ValueSet` take: Rawr's string of the text `format_args!` formats,
    /// formatted into the call.
    Arguments,
    /// An output Rawr cannot hold whose type implements `Display`, as
    /// chrono's `DelayedFormat<StrftimeItems<'a>>`: its text, a `String`.
    Text,
}

impl Builtin {
    /// The Rust primitive a numeric builtin is: every Rust integer that
    /// fits Rawr's `i64` at runtime, and both floats.
    #[must_use]
    pub fn primitive(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Self::Bool,
            "char" => Self::Char,
            "f64" => Self::F64,
            "f32" => Self::F32,
            "i64" => Self::I64,
            "i8" => Self::I8,
            "i16" => Self::I16,
            "i32" => Self::I32,
            "isize" => Self::Isize,
            "u8" => Self::U8,
            "u16" => Self::U16,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "usize" => Self::Usize,
            _ => return None,
        })
    }

    /// Whether it is a number type, the kind a literal may leave open.
    #[must_use]
    pub const fn is_number(self) -> bool {
        matches!(
            self,
            Self::F64
                | Self::F32
                | Self::I64
                | Self::I8
                | Self::I16
                | Self::I32
                | Self::Isize
                | Self::U8
                | Self::U16
                | Self::U32
                | Self::U64
                | Self::Usize
        )
    }

    /// The Rust name of a primitive builtin.
    #[must_use]
    pub const fn rust_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Bool => "bool",
            Self::Char => "char",
            Self::F64 => "f64",
            Self::F32 => "f32",
            Self::I64 => "i64",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::Isize => "isize",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::Usize => "usize",
            Self::InclusiveRange
            | Self::String
            | Self::Str
            | Self::StaticStr
            | Self::Text
            | Self::Arguments => return None,
        })
    }

    /// An integer other than `i64`: an `i64` at runtime, converted with a
    /// range check where it crosses into Rust.
    #[must_use]
    pub const fn is_narrow_integer(self) -> bool {
        matches!(
            self,
            Self::I8
                | Self::I16
                | Self::I32
                | Self::Isize
                | Self::U8
                | Self::U16
                | Self::U32
                | Self::U64
                | Self::Usize
        )
    }
}

/// The Rust type a runner holds a script type's values as where native
/// generic code takes one, as `Task<Message>` of a program's `Message`.
#[derive(Clone, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Holder {
    /// The runner's proxy, which holds the script value (D594).
    #[default]
    Proxy,
    /// The type's mirror in rawr's copy of a native root's crate, which
    /// native code names as the marked module's type and which holds the
    /// value as native code has it, its native parts moved in (#1249).
    Mirror,
    /// A `Copy` enum whose variants carry nothing, held as which of its
    /// `variants` (in declaration order) the value is, so the holder is
    /// `Copy` too and has the `traits` the script type has (#1940).
    Variant {
        variants: Box<[Box<str>]>,
        traits: Traits,
    },
}

/// The std traits a script type has, each derived or written by the
/// program, which the runner's holder of it implements as the type does.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Traits {
    derived: u16,
    written: u16,
}

/// One std trait a script type can derive or implement.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Trait {
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
}

impl Trait {
    const fn bit(self) -> u16 {
        1 << self as u16
    }
}

impl Traits {
    /// These traits and `derived`, which the type's `#[derive]` gives.
    #[must_use]
    pub const fn with_derived(self, derived: Trait) -> Self {
        Self {
            derived: self.derived | derived.bit(),
            ..self
        }
    }

    /// These traits and `written`, which an `impl` of the program gives.
    #[must_use]
    pub const fn with_written(self, written: Trait) -> Self {
        Self {
            written: self.written | written.bit(),
            ..self
        }
    }

    /// Whether the type derives `trait_`.
    #[must_use]
    pub const fn derives(self, trait_: Trait) -> bool {
        self.derived & trait_.bit() != 0
    }

    /// Whether the type has `trait_`, derived or written.
    #[must_use]
    pub const fn has(self, trait_: Trait) -> bool {
        (self.derived | self.written) & trait_.bit() != 0
    }
}

impl Holder {
    /// Whether it is the runner's proxy.
    #[must_use]
    pub const fn is_proxy(&self) -> bool {
        matches!(self, Self::Proxy)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Type {
    Unit,
    /// A function's output `!`: the call never gives a value, so Rawr
    /// reads it as a value of whatever type its place takes.
    Never,
    Builtin(Builtin),
    Nominal {
        definition: crate::surface::identity::Definition,
        arguments: Box<[Type]>,
    },
    /// Rust's `Option<T>`: a Rawr `Option` of the converted `T`.
    Option(Box<Type>),
    /// Rust's `Result<T, E>`: a Rawr `Result` of the converted `T` and `E`.
    Result(Box<Type>, Box<Type>),
    /// An output `Vec<T>`, or a borrowed slice `&[T]` whose elements are
    /// cloned (`owned` false): a Rawr array of the converted `T`. With a
    /// `length`, Rust's array `[T; length]`, as the `From<[u16; 2]>` a
    /// crate's `impl Into<Padding>` reads as a case takes.
    Sequence {
        element: Box<Type>,
        owned: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<u32>,
    },
    /// Rust's array `[T; N]` of the item's const parameter `N` (its
    /// `parameter` index), as `with_children<const N: usize>(children:
    /// [Slot; N])` takes: a Rawr array, whose length the call's argument
    /// gives `N` (#1330). An instance's adapter has it as the `Sequence`
    /// of that length.
    ConstArray {
        element: Box<Type>,
        parameter: u32,
    },
    /// An output tuple: a Rawr tuple of the converted members.
    Tuple(Box<[Type]>),
    /// An input `&T` or `&mut T` of a type a runner's arena holds, as
    /// ratatui's `render_stateful_widget` takes `&mut ListState`: the call
    /// leases the value from its slot for its length (D558.5).
    Borrowed {
        mutable: bool,
        ty: Box<Type>,
    },
    /// Rust's `core::ops::Range<T>` of a float, as rand's `random_range`
    /// takes: a Rawr `Range<f64>` value.
    Range(Box<Type>),
    /// One direct compile-time imported function-item operand.
    FunctionItem {
        input: Box<Type>,
        output: Box<Type>,
    },
    /// One logical script Message substitution lowered natively as its exact
    /// event payload under a profile-authenticated mapper seal.
    ProfileMessage {
        mapper: [u8; 16],
        payload: Box<Type>,
    },
    Parameter(u32),
    /// A script type where a native generic takes a Rust type (D594): the
    /// Rust type the runner holds it as, named by its `path` from the
    /// project's crate root (`State`, `app::State`), at `arguments` when it
    /// is generic.
    Script {
        path: Box<str>,
        arguments: Box<[Type]>,
        #[serde(default, skip_serializing_if = "Holder::is_proxy")]
        holder: Holder,
    },
    /// A type parameter no script value fixes, as iced's `Theme` and
    /// `Renderer` of `run` are, which the view's widget gives: rustc infers
    /// it where the bridge calls the item (D594).
    Inferred,
    /// A trait object of a crate's trait behind a pointer, as tracing's
    /// `Identifier(&'static dyn Callsite)` holds (D872).
    Object(Box<super::object::Object>),
    /// A const generic argument among a nominal type's arguments, as `3`
    /// of `Buf<3>`: its value as Rust writes it, which the
    /// bridge writes where the type is named.
    Constant(Box<str>),
}

impl Type {
    /// The type with each `Parameter(index)` replaced by what `given` gives
    /// for it, or `None` when `given` gives nothing for one.
    #[must_use]
    pub fn with_parameters(&self, given: &impl Fn(u32) -> Option<Self>) -> Option<Self> {
        let one = |ty: &Self| ty.with_parameters(given).map(Box::new);
        let all = |types: &[Self]| {
            types
                .iter()
                .map(|ty| ty.with_parameters(given))
                .collect::<Option<Box<[_]>>>()
        };
        Some(match self {
            Self::Parameter(index) => given(*index)?,
            Self::Constant(_) => self.clone(),
            Self::Nominal {
                definition,
                arguments,
            } => Self::Nominal {
                definition: *definition,
                arguments: all(arguments)?,
            },
            Self::Option(element) => Self::Option(one(element)?),
            Self::Result(ok, err) => Self::Result(one(ok)?, one(err)?),
            Self::Sequence {
                element,
                owned,
                length,
            } => Self::Sequence {
                element: one(element)?,
                owned: *owned,
                length: *length,
            },
            Self::ConstArray { element, parameter } => Self::ConstArray {
                element: one(element)?,
                parameter: *parameter,
            },
            Self::Tuple(members) => Self::Tuple(all(members)?),
            Self::Borrowed { mutable, ty } => Self::Borrowed {
                mutable: *mutable,
                ty: one(ty)?,
            },
            Self::Range(element) => Self::Range(one(element)?),
            Self::FunctionItem { input, output } => Self::FunctionItem {
                input: one(input)?,
                output: one(output)?,
            },
            Self::ProfileMessage { mapper, payload } => Self::ProfileMessage {
                mapper: *mapper,
                payload: one(payload)?,
            },
            Self::Script {
                path,
                arguments,
                holder,
            } => Self::Script {
                path: path.clone(),
                arguments: all(arguments)?,
                holder: holder.clone(),
            },
            Self::Unit | Self::Never | Self::Builtin(_) | Self::Inferred | Self::Object(_) => {
                self.clone()
            }
        })
    }

    /// Whether the type is the crates' alone: it names no parameter, no
    /// type rustc infers where the runner calls the item, and no program
    /// type, so a use at it is the same in every instance of a generic
    /// body that makes it.
    #[must_use]
    pub fn all_native(&self) -> bool {
        match self {
            Self::Parameter(_)
            | Self::Inferred
            | Self::Script { .. }
            | Self::ProfileMessage { .. }
            | Self::ConstArray { .. } => false,
            Self::Unit | Self::Never | Self::Builtin(_) | Self::Object(_) | Self::Constant(_) => {
                true
            }
            Self::Nominal { arguments, .. } | Self::Tuple(arguments) => {
                arguments.iter().all(Self::all_native)
            }
            Self::Option(element)
            | Self::Sequence { element, .. }
            | Self::Range(element)
            | Self::Borrowed { ty: element, .. } => element.all_native(),
            Self::Result(ok, err) => ok.all_native() && err.all_native(),
            Self::FunctionItem { input, output } => input.all_native() && output.all_native(),
        }
    }

    /// Whether a value of this type becomes rawr's, or rawr's becomes one,
    /// with no call's arena: it holds no crate value, only rawr's builtins
    /// and std's types rawr holds as its own, as a thread's `io::Result<()>`
    /// does.
    #[must_use]
    pub fn crosses_by_value(&self) -> bool {
        use crate::surface::identity::Definition;
        match self {
            Self::Unit | Self::Builtin(_) => true,
            Self::Option(element)
            | Self::Sequence {
                element,
                owned: true,
                ..
            } => element.crosses_by_value(),
            Self::Result(ok, err) => ok.crosses_by_value() && err.crosses_by_value(),
            Self::Tuple(members) => members.iter().all(Self::crosses_by_value),
            Self::Nominal {
                definition,
                arguments,
            } if *definition == Definition::poll() => arguments.iter().all(Self::crosses_by_value),
            Self::Nominal {
                definition,
                arguments,
            } => {
                arguments.is_empty()
                    && (Definition::std_path(definition.as_bytes()).is_some()
                        || *definition == Definition::duration()
                        || *definition == Definition::instant())
            }
            _ => false,
        }
    }

    /// `T` of a thread's `JoinHandle<T>`, which crosses as rawr's thread
    /// handle (#1266).
    #[must_use]
    pub fn joined_element(&self) -> Option<&Self> {
        match self {
            Self::Nominal {
                definition,
                arguments,
            } if *definition == crate::surface::identity::Definition::join_handle() => {
                match arguments.as_ref() {
                    [element] => Some(element),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether each thread's `JoinHandle` this type holds crosses where it
    /// is: one the crate gives the program (`taken` false) of any `T`, which
    /// becomes rawr's where the program joins it, and one the crate takes of
    /// a `T` that crosses by value, as a program's thread gives it on no
    /// call's arena (#1274, #1275). None is borrowed, and a callable's
    /// inputs cross the other way from its output.
    #[must_use]
    pub fn handles_cross(&self, taken: bool) -> bool {
        match self {
            ty if let Some(element) = ty.joined_element() => {
                (!taken || element.crosses_by_value()) && element.handles_cross(taken)
            }
            Self::Nominal {
                definition,
                arguments,
            } if *definition == crate::surface::identity::Definition::callable() => {
                arguments.split_last().is_none_or(|(output, inputs)| {
                    output.handles_cross(taken)
                        && inputs.iter().all(|input| input.handles_cross(!taken))
                })
            }
            Self::Nominal { arguments, .. } | Self::Tuple(arguments) => arguments
                .iter()
                .all(|argument| argument.handles_cross(taken)),
            Self::Option(element)
            | Self::Sequence { element, .. }
            | Self::ConstArray { element, .. } => element.handles_cross(taken),
            Self::Result(ok, err) => ok.handles_cross(taken) && err.handles_cross(taken),
            Self::Borrowed { ty, .. } => !ty.holds_handle(),
            _ => true,
        }
    }

    /// Whether a value of this type holds a thread's `JoinHandle`, alone or
    /// inside an `Option`, a `Result`, a tuple or an array.
    #[must_use]
    pub fn holds_handle(&self) -> bool {
        match self {
            ty if ty.joined_element().is_some() => true,
            Self::Tuple(members) => members.iter().any(Self::holds_handle),
            Self::Option(element)
            | Self::Sequence { element, .. }
            | Self::ConstArray { element, .. } => element.holds_handle(),
            Self::Result(ok, err) => ok.holds_handle() || err.holds_handle(),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Constant {
    ty: Box<str>,
    value: Box<str>,
}

impl Constant {
    pub(crate) const fn literal(ty: Box<str>, value: Box<str>) -> Self {
        Self { ty, value }
    }

    /// Transform one checked finite const substitution into canonical tokens.
    #[doc(hidden)]
    pub fn checked(ty: &str, value: &str) -> Result<Self, crate::surface::Error> {
        let safe_type = !ty.is_empty()
            && ty
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        let safe_value = !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+'));
        if !safe_type || !safe_value {
            return Err(crate::surface::Error::InstanceMismatch);
        }
        let mut retained_type = String::new();
        retained_type
            .try_reserve_exact(ty.len())
            .map_err(|_| crate::surface::Error::Allocation)?;
        retained_type.push_str(ty);
        let mut retained_value = String::new();
        retained_value
            .try_reserve_exact(value.len())
            .map_err(|_| crate::surface::Error::Allocation)?;
        retained_value.push_str(value);
        Ok(Self {
            ty: retained_type.into_boxed_str(),
            value: retained_value.into_boxed_str(),
        })
    }

    #[must_use]
    pub fn ty(&self) -> &str {
        &self.ty
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}
