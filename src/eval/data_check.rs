//! Data size checks during evaluation.

use super::GlobalRuntimeState;
use crate::types::dynamic::Union;
use crate::{Dynamic, Engine, Position, RawrResultOf, ERR};
use std::borrow::Borrow;

/// Recursively calculate the sizes of an array.
///
/// Sizes returned are `(` [`Array`][crate::Array], [`Map`][crate::Map] and [`String`] `)`.
///
/// # Panics
///
/// Panics if any interior data is shared (should never happen).
#[inline]
pub fn calc_array_sizes(array: &crate::Array) -> (usize, usize, usize) {
    let (mut ax, mut mx, mut sx) = (0, 0, 0);

    for value in array {
        ax += 1;

        match value.0 {
            Union::Array(ref a, tag, ..) => {
                let (a, m, s) = tagged_array_sizes(a, tag);
                ax += a;
                mx += m;
                sx += s;
            }
            Union::Blob(ref a, ..) => ax += 1 + a.len(),
            Union::Map(ref m, ..) => {
                let (a, m, s) = calc_map_sizes(m);
                ax += a;
                mx += m;
                sx += s;
            }
            Union::Str(ref s, ..) => sx += s.len(),
            Union::Struct(ref value, ..) => {
                let (a, m, s) = calc_nominal_sizes(value.fields());
                ax += a;
                mx += m;
                sx += s;
            }
            Union::Enum(ref value, ..) => {
                let (a, m, s) = calc_nominal_sizes(value.payload());
                ax += a;
                mx += m;
                sx += s;
            }
            Union::Shared(..) => {
                unreachable!("shared values discovered within data")
            }
            _ => (),
        }
    }

    (ax, mx, sx)
}
/// [`calc_array_sizes`], less the bytes of a file the host shipped
/// (`resolve::bundle_files`): a shipped file is the program's, as its text
/// is, so only what is added to it counts against the limits.
fn tagged_array_sizes(
    array: &crate::Array,
    tag: crate::types::dynamic::Tag,
) -> (usize, usize, usize) {
    let (a, m, s) = calc_array_sizes(array);
    (a.saturating_sub(crate::resolve::bundled_len(tag)), m, s)
}

/// Recursively calculate the sizes of a map.
///
/// Sizes returned are `(` [`Array`][crate::Array], [`Map`][crate::Map] and [`String`] `)`.
///
/// # Panics
///
/// Panics if any interior data is shared (should never happen).
#[inline]
pub fn calc_map_sizes(map: &crate::Map) -> (usize, usize, usize) {
    let (mut ax, mut mx, mut sx) = (0, 0, 0);

    for value in map.values() {
        mx += 1;

        match value.0 {
            Union::Array(ref a, tag, ..) => {
                let (a, m, s) = tagged_array_sizes(a, tag);
                ax += a;
                mx += m;
                sx += s;
            }
            Union::Blob(ref a, ..) => ax += 1 + a.len(),
            Union::Map(ref m, ..) => {
                let (a, m, s) = calc_map_sizes(m);
                ax += a;
                mx += m;
                sx += s;
            }
            Union::Str(ref s, ..) => sx += s.len(),
            Union::Struct(ref value, ..) => {
                let (a, m, s) = calc_nominal_sizes(value.fields());
                ax += a;
                mx += m;
                sx += s;
            }
            Union::Enum(ref value, ..) => {
                let (a, m, s) = calc_nominal_sizes(value.payload());
                ax += a;
                mx += m;
                sx += s;
            }
            Union::Shared(..) => {
                unreachable!("shared values discovered within data")
            }
            _ => (),
        }
    }

    (ax, mx, sx)
}

/// Recursively calculate the sizes of a value.
///
/// Sizes returned are `(` [`Array`][crate::Array], [`Map`][crate::Map] and [`String`] `)`.
///
/// # Panics
///
/// Panics if any interior data is shared (should never happen).
#[inline]
pub fn calc_data_sizes(value: &Dynamic, _top: bool) -> (usize, usize, usize) {
    match value.0 {
        Union::Array(ref arr, tag, ..) => tagged_array_sizes(arr, tag),
        Union::Blob(ref blob, ..) => (blob.len(), 0, 0),
        Union::Map(ref map, ..) => calc_map_sizes(map),
        Union::Str(ref s, ..) => (0, 0, s.len()),
        Union::Struct(ref value, ..) => calc_nominal_sizes(value.fields()),
        Union::Enum(ref value, ..) => calc_nominal_sizes(value.payload()),
        Union::Shared(..) if _top => calc_data_sizes(&value.read_lock::<Dynamic>().unwrap(), true),
        Union::Shared(..) => {
            unreachable!("shared values discovered within data: {}", value)
        }
        _ => (0, 0, 0),
    }
}

fn calc_nominal_sizes(values: &[Dynamic]) -> (usize, usize, usize) {
    values.iter().fold((0, 0, 0), |(ax, mx, sx), value| {
        let (a, m, s) = calc_data_sizes(value, false);
        (ax + a, mx + m, sx + s)
    })
}

impl Engine {
    /// Raise an error if any data size exceeds limit.
    ///
    /// [`Position`] in [`EvalAltResult`][crate::EvalAltResult] is always [`NONE`][Position::NONE]
    /// and should be set afterwards.
    pub(crate) fn throw_on_size(&self, (_arr, _map, s): (usize, usize, usize)) -> RawrResultOf<()> {
        if self.limits.string_len.is_some_and(|max| s > max.get()) {
            return Err(
                ERR::ErrorDataTooLarge("Length of string".to_string(), Position::NONE).into(),
            );
        }

        if self.limits.array_size.is_some_and(|max| _arr > max.get()) {
            return Err(
                ERR::ErrorDataTooLarge("Size of array/BLOB".to_string(), Position::NONE).into(),
            );
        }

        if self.limits.map_size.is_some_and(|max| _map > max.get()) {
            return Err(
                ERR::ErrorDataTooLarge("Size of object map".to_string(), Position::NONE).into(),
            );
        }

        Ok(())
    }

    /// Check whether the size of a [`Dynamic`] is within limits.
    #[inline]
    pub(crate) fn check_data_size<T: Borrow<Dynamic>>(
        &self,
        value: T,
        pos: Position,
    ) -> RawrResultOf<T> {
        // If no data size limits, just return
        if !self.has_data_size_limit() {
            return Ok(value);
        }

        let sizes = calc_data_sizes(value.borrow(), true);

        self.throw_on_size(sizes)
            .map_err(|err| err.fill_position(pos))?;

        Ok(value)
    }

    /// Raise an error if the size of a [`Dynamic`] is out of limits (if any).
    #[inline(always)]
    pub fn ensure_data_size_within_limits(&self, value: &Dynamic) -> RawrResultOf<()> {
        self.check_data_size(value, Position::NONE).map(|_| ())
    }

    /// Check if the number of operations stay within limit.
    #[inline(always)]
    pub(crate) fn track_operation(
        &self,
        global: &mut GlobalRuntimeState,
        pos: Position,
    ) -> RawrResultOf<()> {
        global.num_operations += 1;

        // Guard against too many operations
        if self.max_operations() > 0 && global.num_operations > self.max_operations() {
            return Err(ERR::ErrorTooManyOperations(pos).into());
        }

        self.progress
            .as_ref()
            .and_then(|progress| {
                progress(global.num_operations)
                    .map(|token| Err(ERR::ErrorTerminated(token, pos).into()))
            })
            .unwrap_or(Ok(()))
    }
}
