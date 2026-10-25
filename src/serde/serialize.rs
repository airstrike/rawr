//! Implementations of [`serde::Serialize`].

use crate::types::dynamic::Union;
use crate::{Dynamic, ImmutableString, Scope};
use serde::{ser::SerializeSeq, Serialize, Serializer};
use std::iter::once;

use serde::ser::SerializeMap;

use crate::types::dynamic::Variant;

impl Serialize for Dynamic {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Union::Unit(..) => ser.serialize_unit(),
            Union::Bool(x, ..) => ser.serialize_bool(x),
            Union::Str(ref s, ..) => ser.serialize_str(s),
            Union::Char(c, ..) => ser.serialize_char(c),

            Union::Int(x, ..) => ser.serialize_i64(x),

            Union::Float(x, ..) => ser.serialize_f64(*x),

            #[cfg(feature = "decimal")]
            Union::Decimal(ref x, ..) => {
                use rust_decimal::prelude::ToPrimitive;

                match x.to_f64() {
                    Some(v) => ser.serialize_f64(v),
                    None => ser.serialize_str(&x.to_string()),
                }
            }

            Union::Array(ref a, ..) => (**a).serialize(ser),
            Union::Blob(ref a, ..) => ser.serialize_bytes(a),
            Union::Map(ref m, ..) => {
                let mut map = ser.serialize_map(Some(m.len()))?;
                m.iter().try_for_each(|(k, v)| map.serialize_entry(k, v))?;
                map.end()
            }
            Union::FnPtr(ref f, ..) if f.is_curried() => {
                ser.collect_seq(once(f.fn_name().into()).chain(f.iter_curry().cloned()))
            }
            Union::FnPtr(ref f, ..) => ser.serialize_str(f.fn_name()),
            Union::TimeStamp(ref x, ..) => ser.serialize_str(x.as_ref().type_name()),

            Union::Variant(ref v, ..) => ser.serialize_str((***v).type_name()),

            Union::Struct(ref value, ..) => Err(serde::ser::Error::custom(format_args!(
                "{value} does not have a serialization capability"
            ))),
            Union::Enum(ref value, ..) => Err(serde::ser::Error::custom(format_args!(
                "{value} does not have a serialization capability"
            ))),

            #[cfg(not(feature = "sync"))]
            Union::Shared(ref cell, ..) => cell.borrow().serialize(ser),
            #[cfg(feature = "sync")]
            Union::Shared(ref cell, ..) => cell.read().unwrap().serialize(ser),
        }
    }
}

impl Serialize for ImmutableString {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self)
    }
}

impl Serialize for Scope<'_> {
    #[inline(always)]
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        #[derive(Debug, Clone, Hash, Serialize)]
        struct ScopeEntry<'a> {
            pub name: &'a str,
            pub value: &'a Dynamic,
            #[serde(default, skip_serializing_if = "is_false")]
            pub is_constant: bool,
        }

        #[allow(clippy::trivially_copy_pass_by_ref)]
        fn is_false(value: &bool) -> bool {
            !value
        }

        let mut ser = ser.serialize_seq(Some(self.len()))?;

        for (name, is_constant, value) in self.iter_inner() {
            let entry = ScopeEntry {
                name,
                value,
                is_constant,
            };
            ser.serialize_element(&entry)?;
        }

        ser.end()
    }
}
