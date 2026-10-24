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
            Self::InclusiveRange | Self::String | Self::Str | Self::Text => return None,
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

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Type {
    Unit,
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
    /// cloned (`owned` false): a Rawr array of the converted `T`.
    Sequence { element: Box<Type>, owned: bool },
    /// An output tuple: a Rawr tuple of the converted members.
    Tuple(Box<[Type]>),
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
    pub fn checked(
        ty: &str,
        value: &str,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, crate::surface::Error> {
        // The token size is an accounting target recorded on a
        // classification-stage account; the conversion continues.
        crate::classify::Account::new().exceeded(
            "surface constant token bytes",
            limits.string_bytes,
            ty.len().max(value.len()),
        );
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
