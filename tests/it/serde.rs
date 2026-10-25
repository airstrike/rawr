#![cfg(feature = "serde")]

use rawr::{
    Dynamic, Engine, INT, ImmutableString, Scope,
    serde::{from_dynamic, to_dynamic},
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use std::sync::Arc;

use rawr::Array;
use rawr::FLOAT;
use rawr::Map;
#[cfg(feature = "decimal")]
use rust_decimal::Decimal;

#[test]
fn test_serde_ser_primary_types() {
    assert!(to_dynamic(42_u64).unwrap().is_int());
    assert!(to_dynamic(42 as INT).unwrap().is_int());
    assert!(to_dynamic(true).unwrap().is_bool());
    assert!(to_dynamic(()).unwrap().is_unit());

    assert!(to_dynamic(123.456_f64).unwrap().is::<FLOAT>());

    assert!(to_dynamic(123.456_f32).unwrap().is::<FLOAT>());

    assert!(to_dynamic("hello".to_string()).unwrap().is::<String>());
}

#[test]
fn test_serde_ser_integer_types() {
    assert!(to_dynamic(42_i8).unwrap().is_int());
    assert!(to_dynamic(42_i16).unwrap().is_int());
    assert!(to_dynamic(42_i32).unwrap().is_int());
    assert!(to_dynamic(42_i64).unwrap().is_int());
    assert!(to_dynamic(42_u8).unwrap().is_int());
    assert!(to_dynamic(42_u16).unwrap().is_int());
    assert!(to_dynamic(42_u32).unwrap().is_int());
    assert!(to_dynamic(42_u64).unwrap().is_int());
}

#[test]
fn test_serde_ser_array() {
    let arr: Vec<INT> = vec![123, 456, 42, 999];

    let d = to_dynamic(arr).unwrap();
    assert!(d.is_array());
    assert_eq!(4, d.cast::<Array>().len());
}

#[test]
fn test_serde_ser_struct() {
    #[derive(Debug, Serialize, PartialEq)]
    struct Hello {
        a: INT,
        b: bool,
    }

    #[derive(Debug, Serialize, PartialEq)]
    struct Test {
        int: u32,
        seq: Vec<String>,
        obj: Hello,
    }

    let x = Test {
        int: 42,
        seq: vec!["hello".into(), "kitty".into(), "world".into()],
        obj: Hello { a: 123, b: true },
    };

    let d = to_dynamic(x).unwrap();

    assert!(d.is_map());

    let mut map = d.cast::<Map>();
    let obj = map.remove("obj").unwrap().cast::<Map>();
    let mut seq = map.remove("seq").unwrap().cast::<Array>();

    assert_eq!(Ok(123), obj["a"].as_int());
    assert!(obj["b"].as_bool().unwrap());
    assert_eq!(Ok(42), map["int"].as_int());
    assert_eq!(seq.len(), 3);
    assert_eq!("kitty", seq.remove(1).into_string().unwrap());
}

#[test]
fn test_serde_ser_unit_enum() {
    #[derive(Serialize)]
    enum MyEnum {
        VariantFoo,
        VariantBar,
    }

    let d = to_dynamic(MyEnum::VariantFoo).unwrap();
    assert_eq!("VariantFoo", d.into_string().unwrap());

    let d = to_dynamic(MyEnum::VariantBar).unwrap();
    assert_eq!("VariantBar", d.into_string().unwrap());
}

#[test]
fn test_serde_ser_externally_tagged_enum() {
    #[allow(clippy::enum_variant_names)]
    #[derive(Serialize)]
    enum MyEnum {
        VariantUnit,
        VariantUnitTuple(),
        VariantNewtype(i32),
        VariantTuple(i32, i32),
        VariantEmptyStruct {},
        VariantStruct { a: i32 },
    }

    {
        assert_eq!("VariantUnit", to_dynamic(MyEnum::VariantUnit).unwrap().into_immutable_string().unwrap().as_str());
    }

    {
        let mut map = to_dynamic(MyEnum::VariantUnitTuple()).unwrap().cast::<Map>();
        let content = map.remove("VariantUnitTuple").unwrap().cast::<Array>();
        assert!(map.is_empty());
        assert!(content.is_empty());
    }

    let mut map = to_dynamic(MyEnum::VariantNewtype(123)).unwrap().cast::<Map>();
    let content = map.remove("VariantNewtype").unwrap();
    assert!(map.is_empty());
    assert_eq!(Ok(123), content.as_int());

    {
        let mut map = to_dynamic(MyEnum::VariantTuple(123, 456)).unwrap().cast::<Map>();
        let content = map.remove("VariantTuple").unwrap().cast::<Array>();
        assert!(map.is_empty());
        assert_eq!(2, content.len());
        assert_eq!(Ok(123), content[0].as_int());
        assert_eq!(Ok(456), content[1].as_int());
    }

    let mut map = to_dynamic(MyEnum::VariantEmptyStruct {}).unwrap().cast::<Map>();
    let map_inner = map.remove("VariantEmptyStruct").unwrap().cast::<Map>();
    assert!(map.is_empty());
    assert!(map_inner.is_empty());

    let mut map = to_dynamic(MyEnum::VariantStruct { a: 123 }).unwrap().cast::<Map>();
    let mut map_inner = map.remove("VariantStruct").unwrap().cast::<Map>();
    assert!(map.is_empty());
    assert_eq!(Ok(123), map_inner.remove("a").unwrap().as_int());
    assert!(map_inner.is_empty());
}

#[test]
fn test_serde_ser_internally_tagged_enum() {
    #[derive(Serialize)]
    #[serde(tag = "tag")]
    enum MyEnum {
        VariantEmptyStruct {},
        VariantStruct { a: i32 },
    }

    let mut map = to_dynamic(MyEnum::VariantEmptyStruct {}).unwrap().cast::<Map>();
    assert_eq!("VariantEmptyStruct", map.remove("tag").unwrap().into_immutable_string().unwrap().as_str());
    assert!(map.is_empty());

    let mut map = to_dynamic(MyEnum::VariantStruct { a: 123 }).unwrap().cast::<Map>();
    assert_eq!("VariantStruct", map.remove("tag").unwrap().into_immutable_string().unwrap().as_str());
    assert_eq!(Ok(123), map.remove("a").unwrap().as_int());
    assert!(map.is_empty());
}

#[test]
fn test_serde_ser_adjacently_tagged_enum() {
    #[allow(clippy::enum_variant_names)]
    #[derive(Serialize)]
    #[serde(tag = "tag", content = "content")]
    enum MyEnum {
        VariantUnit,
        VariantUnitTuple(),
        VariantNewtype(i32),
        VariantTuple(i32, i32),
        VariantEmptyStruct {},
        VariantStruct { a: i32 },
    }

    let mut map = to_dynamic(MyEnum::VariantUnit).unwrap().cast::<Map>();
    assert_eq!("VariantUnit", map.remove("tag").unwrap().into_immutable_string().unwrap().as_str());
    assert!(map.is_empty());

    {
        let mut map = to_dynamic(MyEnum::VariantUnitTuple()).unwrap().cast::<Map>();
        assert_eq!("VariantUnitTuple", map.remove("tag").unwrap().into_immutable_string().unwrap().as_str());
        let content = map.remove("content").unwrap().cast::<Array>();
        assert!(map.is_empty());
        assert!(content.is_empty());
    }

    let mut map = to_dynamic(MyEnum::VariantNewtype(123)).unwrap().cast::<Map>();
    assert_eq!("VariantNewtype", map.remove("tag").unwrap().into_immutable_string().unwrap().as_str());
    let content = map.remove("content").unwrap();
    assert!(map.is_empty());
    assert_eq!(Ok(123), content.as_int());

    {
        let mut map = to_dynamic(MyEnum::VariantTuple(123, 456)).unwrap().cast::<Map>();
        assert_eq!("VariantTuple", map.remove("tag").unwrap().into_immutable_string().unwrap().as_str());
        let content = map.remove("content").unwrap().cast::<Array>();
        assert!(map.is_empty());
        assert_eq!(2, content.len());
        assert_eq!(Ok(123), content[0].as_int());
        assert_eq!(Ok(456), content[1].as_int());
    }

    let mut map = to_dynamic(MyEnum::VariantEmptyStruct {}).unwrap().cast::<Map>();
    assert_eq!("VariantEmptyStruct", map.remove("tag").unwrap().into_immutable_string().unwrap().as_str());
    let map_inner = map.remove("content").unwrap().cast::<Map>();
    assert!(map.is_empty());
    assert!(map_inner.is_empty());

    let mut map = to_dynamic(MyEnum::VariantStruct { a: 123 }).unwrap().cast::<Map>();
    assert_eq!("VariantStruct", map.remove("tag").unwrap().into_string().unwrap());
    let mut map_inner = map.remove("content").unwrap().cast::<Map>();
    assert!(map.is_empty());
    assert_eq!(Ok(123), map_inner.remove("a").unwrap().as_int());
    assert!(map_inner.is_empty());
}

#[test]
fn test_serde_ser_untagged_enum() {
    #[derive(Serialize)]
    #[serde(untagged)]
    enum MyEnum {
        VariantEmptyStruct {},
        VariantStruct1 { a: i32 },
        VariantStruct2 { b: i32 },
    }

    let map = to_dynamic(MyEnum::VariantEmptyStruct {}).unwrap().cast::<Map>();
    assert!(map.is_empty());

    let mut map = to_dynamic(MyEnum::VariantStruct1 { a: 123 }).unwrap().cast::<Map>();
    assert_eq!(Ok(123), map.remove("a").unwrap().as_int());
    assert!(map.is_empty());

    let mut map = to_dynamic(MyEnum::VariantStruct2 { b: 123 }).unwrap().cast::<Map>();
    assert_eq!(Ok(123), map.remove("b").unwrap().as_int());
    assert!(map.is_empty());
}

#[test]
fn test_serde_de_primary_types() {
    assert_eq!(42, from_dynamic::<u16>(&Dynamic::from(42_u16)).unwrap());
    assert_eq!(42, from_dynamic::<INT>(&(42 as INT).into()).unwrap());
    assert!(from_dynamic::<bool>(&true.into()).unwrap());
    let _: () = from_dynamic::<()>(&().into()).unwrap();

    assert_eq!(123.456, from_dynamic::<FLOAT>(&123.456.into()).unwrap());

    assert_eq!(123.456, from_dynamic::<f32>(&Dynamic::from(123.456_f32)).unwrap());

    assert_eq!("hello", from_dynamic::<String>(&"hello".to_string().into()).unwrap());
}

#[test]
fn test_serde_de_variants() {
    #[derive(Debug)]
    struct Foo;

    #[derive(Debug, Deserialize)]
    struct Bar {
        #[serde(deserialize_with = "deserialize_foo")]
        value: Arc<Foo>,
    }

    fn deserialize_foo<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Arc<Foo>, D::Error> {
        let value = <Dynamic as Deserialize>::deserialize(deserializer).unwrap();

        value.try_cast::<Arc<Foo>>().ok_or_else(|| serde::de::Error::custom("type error"))
    }

    let value = Arc::new(Foo);
    let mut map = Map::new();
    map.insert("value".into(), Dynamic::from(value.clone()));
    let x = Dynamic::from(map);
    let bar = from_dynamic::<Bar>(&x).unwrap();

    assert!(Arc::ptr_eq(&bar.value, &value));
}

#[test]
fn test_serde_de_integer_types() {
    assert_eq!(42, from_dynamic::<i8>(&Dynamic::from(42 as INT)).unwrap());
    assert_eq!(42, from_dynamic::<i16>(&Dynamic::from(42 as INT)).unwrap());
    assert_eq!(42, from_dynamic::<i32>(&Dynamic::from(42 as INT)).unwrap());
    assert_eq!(42, from_dynamic::<i64>(&Dynamic::from(42 as INT)).unwrap());
    assert_eq!(42, from_dynamic::<u8>(&Dynamic::from(42 as INT)).unwrap());
    assert_eq!(42, from_dynamic::<u16>(&Dynamic::from(42 as INT)).unwrap());
    assert_eq!(42, from_dynamic::<u32>(&Dynamic::from(42 as INT)).unwrap());
    assert_eq!(42, from_dynamic::<u64>(&Dynamic::from(42 as INT)).unwrap());
}

#[test]
fn test_serde_de_array() {
    let arr: Vec<INT> = vec![123, 456, 42, 999];
    assert_eq!(arr, from_dynamic::<Vec<INT>>(&arr.clone().into()).unwrap());
}

#[test]
fn test_serde_de_struct() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Hello {
        a: INT,
        b: bool,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Test {
        int: u32,
        seq: Vec<String>,
        obj: Hello,
    }

    let mut map = Map::new();
    map.insert("int".into(), Dynamic::from(42_u32));

    let mut map2 = Map::new();
    map2.insert("a".into(), (123 as INT).into());
    map2.insert("b".into(), true.into());

    map.insert("obj".into(), map2.into());

    let arr: Array = vec!["hello".into(), "kitty".into(), "world".into()];
    map.insert("seq".into(), arr.into());

    let expected = Test {
        int: 42,
        seq: vec!["hello".into(), "kitty".into(), "world".into()],
        obj: Hello { a: 123, b: true },
    };
    assert_eq!(expected, from_dynamic(&map.into()).unwrap());
}

#[test]
fn test_serde_de_unit_enum() {
    #[derive(Debug, PartialEq, Deserialize)]
    enum MyEnum {
        VariantFoo,
        VariantBar,
    }

    let d = Dynamic::from("VariantFoo".to_string());
    assert_eq!(MyEnum::VariantFoo, from_dynamic(&d).unwrap());

    let d = Dynamic::from("VariantBar".to_string());
    assert_eq!(MyEnum::VariantBar, from_dynamic(&d).unwrap());
}

#[test]
fn test_serde_de_externally_tagged_enum() {
    #[allow(clippy::enum_variant_names)]
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    enum MyEnum {
        VariantUnit,
        VariantUnitTuple(),
        VariantNewtype(i32),
        VariantTuple(i32, i32),
        VariantEmptyStruct {},
        VariantStruct { a: i32 },
    }

    let d = Dynamic::from("VariantUnit".to_string());
    assert_eq!(MyEnum::VariantUnit, from_dynamic(&d).unwrap());

    {
        let array: Array = vec![];
        let mut map_outer = Map::new();
        map_outer.insert("VariantUnitTuple".into(), array.into());
        assert_eq!(MyEnum::VariantUnitTuple(), from_dynamic(&map_outer.into()).unwrap());
    }

    let mut map_outer = Map::new();
    map_outer.insert("VariantNewtype".into(), (123 as INT).into());
    assert_eq!(MyEnum::VariantNewtype(123), from_dynamic(&map_outer.into()).unwrap());

    {
        let array: Array = vec![(123 as INT).into(), (456 as INT).into()];
        let mut map_outer = Map::new();
        map_outer.insert("VariantTuple".into(), array.into());
        assert_eq!(MyEnum::VariantTuple(123, 456), from_dynamic(&map_outer.into()).unwrap());
    }

    let map_inner = Map::new();
    let mut map_outer = Map::new();
    map_outer.insert("VariantEmptyStruct".into(), map_inner.into());
    assert_eq!(MyEnum::VariantEmptyStruct {}, from_dynamic(&map_outer.into()).unwrap());

    let mut map_inner = Map::new();
    map_inner.insert("a".into(), (123 as INT).into());
    let mut map_outer = Map::new();
    map_outer.insert("VariantStruct".into(), map_inner.into());
    assert_eq!(MyEnum::VariantStruct { a: 123 }, from_dynamic(&map_outer.into()).unwrap());
}

#[test]
fn test_serde_de_internally_tagged_enum() {
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(tag = "tag", deny_unknown_fields)]
    enum MyEnum {
        VariantEmptyStruct {},
        VariantStruct { a: i32 },
    }

    let mut map = Map::new();
    map.insert("tag".into(), "VariantStruct".into());
    map.insert("a".into(), (123 as INT).into());
    assert_eq!(MyEnum::VariantStruct { a: 123 }, from_dynamic(&map.into()).unwrap());

    let mut map = Map::new();
    map.insert("tag".into(), "VariantEmptyStruct".into());
    assert_eq!(MyEnum::VariantEmptyStruct {}, from_dynamic(&map.into()).unwrap());
}

#[test]
fn test_serde_de_adjacently_tagged_enum() {
    #[allow(clippy::enum_variant_names)]
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(tag = "tag", content = "content", deny_unknown_fields)]
    enum MyEnum {
        VariantUnit,
        VariantUnitTuple(),
        VariantNewtype(i32),
        VariantTuple(i32, i32),
        VariantEmptyStruct {},
        VariantStruct { a: i32 },
    }

    let mut map_outer = Map::new();
    map_outer.insert("tag".into(), "VariantUnit".into());
    assert_eq!(MyEnum::VariantUnit, from_dynamic(&map_outer.into()).unwrap());

    {
        let array: Array = vec![];
        let mut map_outer = Map::new();
        map_outer.insert("tag".into(), "VariantUnitTuple".into());
        map_outer.insert("content".into(), array.into());
        assert_eq!(MyEnum::VariantUnitTuple(), from_dynamic(&map_outer.into()).unwrap());
    }

    let mut map_outer = Map::new();
    map_outer.insert("tag".into(), "VariantNewtype".into());
    map_outer.insert("content".into(), (123 as INT).into());
    assert_eq!(MyEnum::VariantNewtype(123), from_dynamic(&map_outer.into()).unwrap());

    {
        let array: Array = vec![(123 as INT).into(), (456 as INT).into()];
        let mut map_outer = Map::new();
        map_outer.insert("tag".into(), "VariantTuple".into());
        map_outer.insert("content".into(), array.into());
        assert_eq!(MyEnum::VariantTuple(123, 456), from_dynamic(&map_outer.into()).unwrap());
    }

    let map_inner = Map::new();
    let mut map_outer = Map::new();
    map_outer.insert("tag".into(), "VariantEmptyStruct".into());
    map_outer.insert("content".into(), map_inner.into());
    assert_eq!(MyEnum::VariantEmptyStruct {}, from_dynamic(&map_outer.into()).unwrap());

    let mut map_inner = Map::new();
    map_inner.insert("a".into(), (123 as INT).into());
    let mut map_outer = Map::new();
    map_outer.insert("tag".into(), "VariantStruct".into());
    map_outer.insert("content".into(), map_inner.into());
    assert_eq!(MyEnum::VariantStruct { a: 123 }, from_dynamic(&map_outer.into()).unwrap());
}

#[test]
fn test_serde_de_untagged_enum() {
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(untagged, deny_unknown_fields)]
    enum MyEnum {
        VariantEmptyStruct {},
        VariantStruct1 { a: i32 },
        VariantStruct2 { b: i32 },
    }

    let map = Map::new();
    assert_eq!(MyEnum::VariantEmptyStruct {}, from_dynamic(&map.into()).unwrap());

    let mut map = Map::new();
    map.insert("a".into(), (123 as INT).into());
    assert_eq!(MyEnum::VariantStruct1 { a: 123 }, from_dynamic(&map.into()).unwrap());

    let mut map = Map::new();
    map.insert("b".into(), (123 as INT).into());
    assert_eq!(MyEnum::VariantStruct2 { b: 123 }, from_dynamic(&map.into()).unwrap());
}

#[test]
#[cfg(feature = "metadata")]
fn test_serde_json() -> serde_json::Result<()> {
    let s: ImmutableString = "hello".into();
    assert_eq!(serde_json::to_string(&s).unwrap(), r#""hello""#);

    let mut map = Map::new();
    map.insert("a".into(), (123 as INT).into());

    let arr: Array = vec![(1 as INT).into(), (2 as INT).into(), (3 as INT).into()];
    map.insert("b".into(), arr.into());
    map.insert("c".into(), true.into());
    let d: Dynamic = map.into();

    let json = serde_json::to_string(&d).unwrap();

    assert!(json.contains("\"a\":123"));
    assert!(json.contains("\"b\":[1,2,3]"));
    assert!(json.contains("\"c\":true"));

    let d2: Dynamic = serde_json::from_str(&json).unwrap();

    assert!(d2.is_map());

    let mut m = d2.cast::<Map>();

    assert_eq!(m["a"].as_int().unwrap(), 123);
    assert!(m["c"].as_bool().unwrap());

    let a = m.remove("b").unwrap().cast::<Array>();

    assert_eq!(a.len(), 3);
    assert_eq!(format!("{a:?}"), "[1, 2, 3]");

    Ok(())
}

#[test]
#[cfg(feature = "metadata")]
#[cfg(feature = "decimal")]
fn test_serde_json_numbers() -> serde_json::Result<()> {
    use std::str::FromStr;

    {
        let d: Dynamic = serde_json::from_str("100000000000").unwrap();
        assert!(d.is::<INT>());
        assert_eq!(d.as_int().unwrap(), 100000000000);
    }

    let d: Dynamic = serde_json::from_str("10000000000000000000").unwrap();
    assert!(d.is::<Decimal>());
    assert_eq!(d.as_decimal().unwrap(), Decimal::from_str("10000000000000000000").unwrap());

    let d: Dynamic = serde_json::from_str("10000000000000000000000000").unwrap();
    assert!(d.is::<FLOAT>());
    assert_eq!(d.as_float().unwrap(), 10000000000000000000000000.0);

    Ok(())
}

#[test]
fn test_serde_optional() {
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestStruct {
        foo: Option<char>,
    }

    let mut map = Map::new();
    map.insert("foo".into(), 'a'.into());
    let r = Dynamic::from_map(map);
    assert_eq!(from_dynamic::<TestStruct>(&r).unwrap(), TestStruct { foo: Some('a') });

    let mut map = Map::new();
    map.insert("foo".into(), Dynamic::UNIT);
    let r = Dynamic::from_map(map);
    assert_eq!(from_dynamic::<TestStruct>(&r).unwrap(), TestStruct { foo: None });

    let r = Dynamic::from_map(Map::new());
    assert_eq!(from_dynamic::<TestStruct>(&r).unwrap(), TestStruct { foo: None });

    let ts = TestStruct { foo: Some('a') };
    let r = to_dynamic(&ts).unwrap();
    let map = r.cast::<Map>();
    assert_eq!(map.len(), 1);
    assert_eq!(map.get("foo").unwrap().as_char().unwrap(), 'a');

    let ts = TestStruct { foo: None };
    let r = to_dynamic(&ts).unwrap();
    let map = r.cast::<Map>();
    assert_eq!(map.len(), 1);
    let _: () = map.get("foo").unwrap().as_unit().unwrap();
}

#[test]
fn test_serde_blob() {
    // A typed script's `Vec<u8>` is an array, so the blob is built in Rust.
    let mut map = Map::new();
    map.insert("x".into(), Dynamic::from_blob((0..10).collect()));
    let r = Dynamic::from_map(map);

    let data = format!("{r:?}");

    let encoded = rmp_serde::to_vec(&r).unwrap();
    let decoded: Dynamic = rmp_serde::from_slice(&encoded).unwrap();

    assert_eq!(format!("{decoded:?}"), data);
}

#[test]
fn test_serde_json_borrowed_string() {
    let value = json!({ "a": "b" });
    println!("value: {value:?}");

    let result: Dynamic = serde_json::from_value(value.clone()).unwrap();
    println!("result: {result:?}");

    let value2 = serde_json::to_value(&result).unwrap();
    println!("value2: {value2:?}");

    assert_eq!(value, value2);
}

#[test]
fn test_serde_scope() {
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestStruct {
        foo: Option<char>,
    }

    let mut scope = Scope::new();
    scope.push("x", 42 as INT);
    scope.push_constant("y", true);
    scope.push("z", TestStruct { foo: None });

    let json = serde_json::to_string(&scope).unwrap();

    assert_eq!(json, r#"[{"name":"x","value":42},{"name":"y","value":true,"is_constant":true},{"name":"z","value":"it::serde::test_serde_scope::TestStruct"}]"#);

    scope = serde_json::from_str(&json).unwrap();

    assert_eq!(scope.len(), 3);
    assert_eq!(scope.get_value::<INT>("x").unwrap(), 42);
    assert!(scope.get_value::<bool>("y").unwrap());
    assert_eq!(scope.get_value::<String>("z").unwrap(), "it::serde::test_serde_scope::TestStruct");
}

#[test]
fn nominal_values_require_an_explicit_serialization_capability() {
    let engine = Engine::new();

    for source in ["struct Value { inner: i64 } Value { inner: 42 }", "enum Message { Value(i64) } Message::Value(42)"] {
        let value = engine.eval::<Dynamic>(source).unwrap();
        let error = serde_json::to_string(&value).unwrap_err().to_string();

        assert!(error.contains("Rawr"), "{error}");
        assert!(error.contains("serialization capability"), "{error}");
    }
}
