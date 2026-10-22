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
