//! Implement serialization support of [`Dynamic`][crate::Dynamic] for [`serde`].

use crate::{Dynamic, Identifier, Position, RawrError, RawrResult, RawrResultOf, ERR, INT};
use serde::ser::{
    Error, SerializeMap, SerializeSeq, SerializeStruct, SerializeTuple, SerializeTupleStruct,
};
use serde::{Serialize, Serializer};
use std::{convert::TryFrom, fmt};

#[cfg(feature = "decimal")]
use num_traits::FromPrimitive;

/// Serializer for [`Dynamic`][crate::Dynamic].
pub struct DynamicSerializer {
    /// Buffer to hold a temporary key.
    _key: Identifier,
    /// Buffer to hold a temporary value.
    _value: Dynamic,
}

impl DynamicSerializer {
    /// Create a [`DynamicSerializer`] from a [`Dynamic`][crate::Dynamic] value.
    #[must_use]
    pub const fn new(value: Dynamic) -> Self {
        Self {
            _key: Identifier::new_const(),
            _value: value,
        }
    }
}

/// Serialize a Rust type that implements [`serde::Serialize`] into a [`Dynamic`][crate::Dynamic].
///
/// # Example
///
/// ```
/// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
/// # {
/// use rawr::{Dynamic, Array, Map};
/// use rawr::serde::to_dynamic;
/// use serde::Serialize;
///
/// #[derive(Debug, serde::Serialize, PartialEq)]
/// struct Point {
///     x: f64,
///     y: f64
/// }
///
/// #[derive(Debug, serde::Serialize, PartialEq)]
/// struct MyStruct {
///     a: i64,
///     b: Vec<String>,
///     c: bool,
///     d: Point
/// }
///
/// let x = MyStruct {
///     a: 42,
///     b: vec![ "hello".into(), "world".into() ],
///     c: true,
///     d: Point { x: 123.456, y: 999.0 }
/// };
///
/// // Convert the 'MyStruct' into a 'Dynamic'
/// let value = to_dynamic(x)?;
///
/// assert!(value.is::<Map>());
///
/// let map = value.cast::<Map>();
/// let point = map["d"].as_map_ref().unwrap();
/// assert_eq!(*point["x"].read_lock::<f64>().unwrap(), 123.456);
/// assert_eq!(*point["y"].read_lock::<f64>().unwrap(), 999.0);
/// # }
/// # Ok(())
/// # }
/// ```
pub fn to_dynamic<T: Serialize>(value: T) -> RawrResult {
    let mut s = DynamicSerializer::new(Dynamic::UNIT);
    value.serialize(&mut s)
}

impl Error for RawrError {
    fn custom<T: fmt::Display>(err: T) -> Self {
        ERR::ErrorRuntime(err.to_string().into(), Position::NONE).into()
    }
}

impl Serializer for &mut DynamicSerializer {
    type Ok = Dynamic;
    type Error = RawrError;
    type SerializeSeq = DynamicSerializer;
    type SerializeTuple = DynamicSerializer;
    type SerializeTupleStruct = DynamicSerializer;
    type SerializeTupleVariant = TupleVariantSerializer;
    type SerializeMap = DynamicSerializer;
    type SerializeStruct = DynamicSerializer;
    type SerializeStructVariant = StructVariantSerializer;

    #[inline(always)]
    fn serialize_bool(self, v: bool) -> RawrResultOf<Self::Ok> {
        Ok(v.into())
    }

    #[inline(always)]
    fn serialize_i8(self, v: i8) -> RawrResultOf<Self::Ok> {
        Ok(INT::from(v).into())
    }

    #[inline(always)]
    fn serialize_i16(self, v: i16) -> RawrResultOf<Self::Ok> {
        Ok(INT::from(v).into())
    }

    #[inline(always)]
    fn serialize_i32(self, v: i32) -> RawrResultOf<Self::Ok> {
        Ok(INT::from(v).into())
    }

    #[inline]
    fn serialize_i64(self, v: i64) -> RawrResultOf<Self::Ok> {
        return Ok(v.into());

        #[allow(unreachable_code)]
        {
            #[cfg(feature = "decimal")]
            if let Some(n) = rust_decimal::Decimal::from_i64(v) {
                return Ok(Dynamic::from_decimal(n));
            }

            return Ok(Dynamic::from_float(v as crate::FLOAT));

            Err(Error::custom(format!("integer number too large: {v}")))
        }
    }

    #[inline]
    fn serialize_i128(self, v: i128) -> RawrResultOf<Self::Ok> {
        if v <= i128::from(INT::MAX) {
            return Ok(Dynamic::from(v as INT));
        }

        #[allow(unreachable_code)]
        {
            #[cfg(feature = "decimal")]
            if let Some(n) = rust_decimal::Decimal::from_i128(v) {
                return Ok(Dynamic::from_decimal(n));
            }

            return Ok(Dynamic::from_float(v as crate::FLOAT));

            Err(Error::custom(format!("integer number too large: {v}")))
        }
    }

    #[inline(always)]
    fn serialize_u8(self, v: u8) -> RawrResultOf<Self::Ok> {
        Ok(INT::from(v).into())
    }

    #[inline(always)]
    fn serialize_u16(self, v: u16) -> RawrResultOf<Self::Ok> {
        Ok(INT::from(v).into())
    }

    #[inline]
    fn serialize_u32(self, v: u32) -> RawrResultOf<Self::Ok> {
        #[allow(irrefutable_let_patterns, clippy::unnecessary_fallible_conversions)]
        if let Ok(v) = INT::try_from(v) {
            return Ok(Dynamic::from_int(v));
        }

        #[allow(unreachable_code)]
        {
            #[cfg(feature = "decimal")]
            if let Some(n) = rust_decimal::Decimal::from_u32(v) {
                return Ok(Dynamic::from_decimal(n));
            }

            return Ok(Dynamic::from_float(v as crate::FLOAT));

            Err(Error::custom(format!("integer number too large: {v}")))
        }
    }

    #[inline]
    fn serialize_u64(self, v: u64) -> RawrResultOf<Self::Ok> {
        if let Ok(v) = INT::try_from(v) {
            return Ok(Dynamic::from_int(v));
        }

        #[cfg(feature = "decimal")]
        if let Some(n) = rust_decimal::Decimal::from_u64(v) {
            return Ok(Dynamic::from_decimal(n));
        }

        return Ok(Dynamic::from_float(v as crate::FLOAT));

        #[allow(unreachable_code)]
        Err(Error::custom(format!("integer number too large: {v}")))
    }

    #[inline]
    fn serialize_u128(self, v: u128) -> RawrResultOf<Self::Ok> {
        if v <= INT::MAX as u128 {
            return Ok(Dynamic::from(v as INT));
        }

        #[cfg(feature = "decimal")]
        if let Some(n) = rust_decimal::Decimal::from_u128(v) {
            return Ok(Dynamic::from_decimal(n));
        }

        return Ok(Dynamic::from_float(v as crate::FLOAT));

        #[allow(unreachable_code)]
        Err(Error::custom(format!("integer number too large: {v}")))
    }

    #[inline(always)]
    fn serialize_f32(self, v: f32) -> RawrResultOf<Self::Ok> {
        return Ok((v as crate::FLOAT).into());

        #[allow(unreachable_code)]
        {
            #[cfg(feature = "decimal")]
            if let Some(n) = rust_decimal::Decimal::from_f32(v) {
                return Ok(Dynamic::from_decimal(n));
            }

            Err(Error::custom(format!(
                "floating-point number is not supported: {v}"
            )))
        }
    }

    #[inline(always)]
    fn serialize_f64(self, v: f64) -> RawrResultOf<Self::Ok> {
        return Ok((v as crate::FLOAT).into());

        #[allow(unreachable_code)]
        {
            #[cfg(feature = "decimal")]
            if let Some(n) = rust_decimal::Decimal::from_f64(v) {
                return Ok(Dynamic::from_decimal(n));
            }

            Err(Error::custom(format!(
                "floating-point number is not supported: {v}"
            )))
        }
    }

    #[inline(always)]
    fn serialize_char(self, v: char) -> RawrResultOf<Self::Ok> {
        Ok(v.into())
    }

    #[inline(always)]
    fn serialize_str(self, v: &str) -> RawrResultOf<Self::Ok> {
        Ok(v.into())
    }

    #[inline]
    fn serialize_bytes(self, _v: &[u8]) -> RawrResultOf<Self::Ok> {
        Ok(Dynamic::from_blob(_v.to_vec()))
    }

    #[inline(always)]
    fn serialize_none(self) -> RawrResultOf<Self::Ok> {
        Ok(Dynamic::UNIT)
    }

    #[inline(always)]
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> RawrResultOf<Self::Ok> {
        value.serialize(&mut *self)
    }

    #[inline(always)]
    fn serialize_unit(self) -> RawrResultOf<Self::Ok> {
        Ok(Dynamic::UNIT)
    }

    #[inline(always)]
    fn serialize_unit_struct(self, _name: &'static str) -> RawrResultOf<Self::Ok> {
        self.serialize_unit()
    }

    #[inline(always)]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> RawrResultOf<Self::Ok> {
        self.serialize_str(variant)
    }

    #[inline(always)]
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> RawrResultOf<Self::Ok> {
        value.serialize(&mut *self)
    }

    #[inline]
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> RawrResultOf<Self::Ok> {
        Ok(make_variant(_variant, to_dynamic(_value)?))
    }

    #[inline]
    fn serialize_seq(self, _len: Option<usize>) -> RawrResultOf<Self::SerializeSeq> {
        Ok(DynamicSerializer::new(crate::Array::new().into()))
    }

    #[inline(always)]
    fn serialize_tuple(self, len: usize) -> RawrResultOf<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }

    #[inline(always)]
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> RawrResultOf<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(len))
    }

    #[inline]
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> RawrResultOf<Self::SerializeTupleVariant> {
        Ok(TupleVariantSerializer {
            variant: _variant,
            array: crate::Array::with_capacity(_len),
        })
    }

    #[inline]
    fn serialize_map(self, _len: Option<usize>) -> RawrResultOf<Self::SerializeMap> {
        Ok(DynamicSerializer::new(crate::Map::new().into()))
    }

    #[inline(always)]
    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> RawrResultOf<Self::SerializeStruct> {
        self.serialize_map(Some(len))
    }

    #[inline]
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> RawrResultOf<Self::SerializeStructVariant> {
        Ok(StructVariantSerializer {
            variant: _variant,
            map: crate::Map::new(),
        })
    }
}

impl SerializeSeq for DynamicSerializer {
    type Ok = Dynamic;
    type Error = RawrError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, _value: &T) -> RawrResultOf<()> {
        {
            let value = _value.serialize(&mut *self)?;
            let arr = self._value.downcast_mut::<crate::Array>().unwrap();
            arr.push(value);
            Ok(())
        }
    }

    // Close the sequence.
    #[inline]
    fn end(self) -> RawrResultOf<Self::Ok> {
        Ok(self._value)
    }
}

impl SerializeTuple for DynamicSerializer {
    type Ok = Dynamic;
    type Error = RawrError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, _value: &T) -> RawrResultOf<()> {
        {
            let value = _value.serialize(&mut *self)?;
            let arr = self._value.downcast_mut::<crate::Array>().unwrap();
            arr.push(value);
            Ok(())
        }
    }

    #[inline]
    fn end(self) -> RawrResultOf<Self::Ok> {
        Ok(self._value)
    }
}

impl SerializeTupleStruct for DynamicSerializer {
    type Ok = Dynamic;
    type Error = RawrError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, _value: &T) -> RawrResultOf<()> {
        {
            let value = _value.serialize(&mut *self)?;
            let arr = self._value.downcast_mut::<crate::Array>().unwrap();
            arr.push(value);
            Ok(())
        }
    }

    #[inline]
    fn end(self) -> RawrResultOf<Self::Ok> {
        Ok(self._value)
    }
}

impl SerializeMap for DynamicSerializer {
    type Ok = Dynamic;
    type Error = RawrError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, _key: &T) -> RawrResultOf<()> {
        {
            let key = _key.serialize(&mut *self)?;
            self._key = key
                .into_immutable_string()
                .map_err(|typ| {
                    ERR::ErrorMismatchDataType("string".into(), typ.into(), Position::NONE)
                })?
                .into();
            Ok(())
        }
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, _value: &T) -> RawrResultOf<()> {
        {
            let key = std::mem::take(&mut self._key);
            let value = _value.serialize(&mut *self)?;
            let map = self._value.downcast_mut::<crate::Map>().unwrap();
            map.insert(key, value);
            Ok(())
        }
    }

    fn serialize_entry<K: ?Sized + Serialize, T: ?Sized + Serialize>(
        &mut self,
        _key: &K,
        _value: &T,
    ) -> RawrResultOf<()> {
        {
            let key: Dynamic = _key.serialize(&mut *self)?;
            let key = key.into_immutable_string().map_err(|typ| {
                ERR::ErrorMismatchDataType("string".into(), typ.into(), Position::NONE)
            })?;
            let value = _value.serialize(&mut *self)?;
            let map = self._value.downcast_mut::<crate::Map>().unwrap();
            map.insert(key.into(), value);
            Ok(())
        }
    }

    #[inline]
    fn end(self) -> RawrResultOf<Self::Ok> {
        Ok(self._value)
    }
}

impl SerializeStruct for DynamicSerializer {
    type Ok = Dynamic;
    type Error = RawrError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _key: &'static str,
        _value: &T,
    ) -> RawrResultOf<()> {
        {
            let value = _value.serialize(&mut *self)?;
            let map = self._value.downcast_mut::<crate::Map>().unwrap();
            map.insert(_key.into(), value);
            Ok(())
        }
    }

    #[inline]
    fn end(self) -> RawrResultOf<Self::Ok> {
        Ok(self._value)
    }
}

pub struct TupleVariantSerializer {
    variant: &'static str,
    array: crate::Array,
}

impl serde::ser::SerializeTupleVariant for TupleVariantSerializer {
    type Ok = Dynamic;
    type Error = RawrError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> RawrResultOf<()> {
        let value = to_dynamic(value)?;
        self.array.push(value);
        Ok(())
    }

    #[inline]
    fn end(self) -> RawrResultOf<Self::Ok> {
        Ok(make_variant(self.variant, self.array.into()))
    }
}

pub struct StructVariantSerializer {
    variant: &'static str,
    map: crate::Map,
}

impl serde::ser::SerializeStructVariant for StructVariantSerializer {
    type Ok = Dynamic;
    type Error = RawrError;

    #[inline]
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> RawrResultOf<()> {
        let value = to_dynamic(value)?;
        self.map.insert(key.into(), value);
        Ok(())
    }

    #[inline]
    fn end(self) -> RawrResultOf<Self::Ok> {
        Ok(make_variant(self.variant, self.map.into()))
    }
}

#[inline]
fn make_variant(variant: &'static str, value: Dynamic) -> Dynamic {
    let mut map = crate::Map::new();
    map.insert(variant.into(), value);
    map.into()
}
