//! Module that provide formatting services to the [`Engine`].
use crate::packages::iter_basic::{BitRange, CharsStream, StepRange};
use crate::{
    Engine, ExclusiveRange, FnPtr, ImmutableString, InclusiveRange, Position, RawrError, ERR,
};
use std::any::type_name;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// Map the name of a standard type into a friendly form.
#[inline]
#[must_use]
pub fn map_std_type_name(name: &str, shorthands: bool) -> &str {
    let name = name.trim();

    match name {
        "INT" => return type_name::<crate::INT>(),
        #[cfg(not(feature = "no_float"))]
        "FLOAT" => return type_name::<crate::FLOAT>(),
        _ => (),
    }

    if name == type_name::<String>() {
        return if shorthands { "string" } else { "String" };
    }
    if name == type_name::<ImmutableString>() || name == "ImmutableString" {
        return if shorthands {
            "string"
        } else {
            "ImmutableString"
        };
    }
    if name == type_name::<&str>() {
        return if shorthands { "string" } else { "&str" };
    }
    #[cfg(feature = "decimal")]
    if name == type_name::<rust_decimal::Decimal>() {
        return if shorthands { "decimal" } else { "Decimal" };
    }
    if name == type_name::<FnPtr>() || name == "FnPtr" {
        return if shorthands { "Fn" } else { "FnPtr" };
    }
    #[cfg(not(feature = "no_index"))]
    if name == type_name::<crate::Array>() || name == "Array" {
        return if shorthands { "array" } else { "Array" };
    }
    #[cfg(not(feature = "no_index"))]
    if name == type_name::<crate::Blob>() || name == "Blob" {
        return if shorthands { "blob" } else { "Blob" };
    }
    #[cfg(not(feature = "no_object"))]
    if name == type_name::<crate::Map>() || name == "Map" {
        return if shorthands { "map" } else { "Map" };
    }
    #[cfg(not(feature = "no_time"))]
    if name == type_name::<crate::Instant>() || name == "Instant" {
        return if shorthands { "timestamp" } else { "Instant" };
    }
    if name == type_name::<ExclusiveRange>() || name == "ExclusiveRange" {
        return if shorthands {
            "range"
        } else if cfg!(feature = "only_i32") {
            "Range<i32>"
        } else {
            "Range<i64>"
        };
    }
    if name == type_name::<InclusiveRange>() || name == "InclusiveRange" {
        return if shorthands {
            "range="
        } else if cfg!(feature = "only_i32") {
            "RangeInclusive<i32>"
        } else {
            "RangeInclusive<i64>"
        };
    }
    if name == type_name::<BitRange>() {
        return if shorthands { "range" } else { "BitRange" };
    }
    if name == type_name::<CharsStream>() {
        return if shorthands { "range" } else { "CharStream" };
    }

    let step_range_name = type_name::<StepRange<u8>>();
    let step_range_name = &step_range_name[..step_range_name.len() - 3];

    if name.starts_with(step_range_name) && name.ends_with('>') {
        return if shorthands {
            "range"
        } else {
            let step_range_name = step_range_name.split("::").last().unwrap();
            &step_range_name[..step_range_name.len() - 1]
        };
    }

    #[cfg(not(feature = "no_float"))]
    if name == type_name::<crate::packages::iter_basic::StepRange<crate::FLOAT>>() {
        return if shorthands {
            "range"
        } else {
            "StepFloatRange"
        };
    }
    #[cfg(feature = "decimal")]
    if name == type_name::<crate::packages::iter_basic::StepRange<rust_decimal::Decimal>>() {
        return if shorthands {
            "range"
        } else {
            "StepDecimalRange"
        };
    }

    name.strip_prefix("rawr::")
        .map_or(name, |s| map_std_type_name(s, shorthands))
}

/// Format a Rust parameter type to be display-friendly.
///
/// * `rawr::` prefix is cleared.
/// * `()` is cleared.
/// * `&mut` is cleared.
/// * `INT` and `FLOAT` are expanded.
/// * [`RawrResult`][crate::RawrResult] and [`RawrResultOf<T>`][crate::RawrResultOf] are expanded.
#[cfg(feature = "metadata")]
pub fn format_param_type_for_display(typ: &str, is_return_type: bool) -> std::borrow::Cow<'_, str> {
    const RESULT_TYPE: &str = "Result<";
    const ERROR_TYPE: &str = ",Box<EvalAltResult>>";
    const RAWR_RESULT_TYPE: &str = "RawrResult";
    const RAWR_RESULT_TYPE_EXPAND: &str = "Result<Dynamic, Box<EvalAltResult>>";
    const RAWR_RESULT_OF_TYPE: &str = "RawrResultOf<";
    const RAWR_RESULT_OF_TYPE_EXPAND: &str = "Result<{}, Box<EvalAltResult>>";
    const RAWR_RANGE: &str = "ExclusiveRange";
    const RAWR_RANGE_TYPE: &str = "Range<";
    const RAWR_RANGE_EXPAND: &str = "Range<{}>";
    const RAWR_INCLUSIVE_RANGE: &str = "InclusiveRange";
    const RAWR_INCLUSIVE_RANGE_TYPE: &str = "RangeInclusive<";
    const RAWR_INCLUSIVE_RANGE_EXPAND: &str = "RangeInclusive<{}>";

    let typ = typ.trim();

    if let Some(x) = typ.strip_prefix("rawr::") {
        return format_param_type_for_display(x, is_return_type);
    } else if let Some(x) = typ.strip_prefix("&mut ") {
        let r = format_param_type_for_display(x, false);
        return if r == x {
            typ.into()
        } else {
            format!("&mut {r}").into()
        };
    } else if typ.contains(' ') {
        let typ = typ.replace(' ', "");
        let r = format_param_type_for_display(&typ, is_return_type);
        return r.into_owned().into();
    }

    match typ {
        "" | "()" if is_return_type => "".into(),
        "INT" => std::any::type_name::<crate::INT>().into(),
        #[cfg(not(feature = "no_float"))]
        "FLOAT" => std::any::type_name::<crate::FLOAT>().into(),
        RAWR_RANGE => RAWR_RANGE_EXPAND
            .replace("{}", std::any::type_name::<crate::INT>())
            .into(),
        RAWR_INCLUSIVE_RANGE => RAWR_INCLUSIVE_RANGE_EXPAND
            .replace("{}", std::any::type_name::<crate::INT>())
            .into(),
        RAWR_RESULT_TYPE => RAWR_RESULT_TYPE_EXPAND.into(),
        ty if ty.starts_with(RAWR_RANGE_TYPE) && ty.ends_with('>') => {
            let inner = &ty[RAWR_RANGE_TYPE.len()..ty.len() - 1];
            RAWR_RANGE_EXPAND
                .replace("{}", format_param_type_for_display(inner, false).trim())
                .into()
        }
        ty if ty.starts_with(RAWR_INCLUSIVE_RANGE_TYPE) && ty.ends_with('>') => {
            let inner = &ty[RAWR_INCLUSIVE_RANGE_TYPE.len()..ty.len() - 1];
            RAWR_INCLUSIVE_RANGE_EXPAND
                .replace("{}", format_param_type_for_display(inner, false).trim())
                .into()
        }
        ty if ty.starts_with(RAWR_RESULT_OF_TYPE) && ty.ends_with('>') => {
            let inner = &ty[RAWR_RESULT_OF_TYPE.len()..ty.len() - 1];
            RAWR_RESULT_OF_TYPE_EXPAND
                .replace("{}", format_param_type_for_display(inner, false).trim())
                .into()
        }
        ty if ty.starts_with(RESULT_TYPE) && ty.ends_with(ERROR_TYPE) => {
            let inner = &ty[RESULT_TYPE.len()..ty.len() - ERROR_TYPE.len()];
            RAWR_RESULT_OF_TYPE_EXPAND
                .replace("{}", format_param_type_for_display(inner, false).trim())
                .into()
        }
        ty => ty.into(),
    }
}

impl Engine {
    /// Pretty-print a type name.
    ///
    /// If a type is registered via [`register_type_with_name`][Engine::register_type_with_name],
    /// the type name provided for the registration will be used.
    #[inline]
    #[must_use]
    pub fn map_type_name<'a>(&'a self, name: &'a str) -> &'a str {
        self.global_modules
            .iter()
            .find_map(|m| m.get_custom_type_display_by_name(name))
            .or_else(|| {
                #[cfg(not(feature = "no_module"))]
                return self
                    .global_sub_modules
                    .values()
                    .find_map(|m| m.get_custom_type_display_by_name(name));

                #[cfg(feature = "no_module")]
                return None;
            })
            .unwrap_or_else(|| map_std_type_name(name, true))
    }

    /// Format a Rust parameter type.
    ///
    /// If a type is registered via [`register_type_with_name`][Engine::register_type_with_name],
    /// the type name provided for the registration will be used.
    ///
    /// This method properly handles type names beginning with `&mut`.
    #[cfg(feature = "metadata")]
    #[inline]
    #[must_use]
    pub(crate) fn format_param_type<'a>(&'a self, name: &'a str) -> std::borrow::Cow<'a, str> {
        if let Some(x) = name.strip_prefix("&mut ") {
            return match self.format_param_type(x) {
                r if r == x => name.into(),
                r => format!("&mut {r}").into(),
            };
        }

        self.map_type_name(name).into()
    }

    /// Make a `Box<`[`EvalAltResult<ErrorMismatchDataType>`][ERR::ErrorMismatchDataType]`>`.
    #[cold]
    #[inline(never)]
    #[must_use]
    pub(crate) fn make_type_mismatch_err<T>(&self, typ: &str, pos: Position) -> RawrError {
        ERR::ErrorMismatchDataType(self.map_type_name(type_name::<T>()).into(), typ.into(), pos)
            .into()
    }
}
