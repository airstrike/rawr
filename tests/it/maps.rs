use rawr::{Dynamic, Engine, EvalAltResult, INT, Map};

#[test]
/// A JSON object text converts into a Rust-side object map, and back.
fn test_map_json() {
    let engine = Engine::new();

    let json = r#"{"a":1, "b":true, "c":41+1, "$d e f!":"hello", "z":null}"#;

    let map = engine.parse_json(json, true).unwrap();

    assert!(!map.contains_key("x"));

    assert_eq!(map["a"].as_int().unwrap(), 1);
    assert!(map["b"].as_bool().unwrap());
    assert_eq!(map["c"].as_int().unwrap(), 42);
    assert_eq!(map["$d e f!"].clone_cast::<String>(), "hello");
    let _: () = map["z"].as_unit().unwrap();

    {
        let mut inner = Map::new();
        inner.insert("b".into(), Dynamic::from(42 as INT));
        let mut outer = Map::new();
        outer.insert("a".into(), Dynamic::from_array(vec![Dynamic::from_map(inner)]));

        assert_eq!(rawr::format_map_as_json(&outer), r#"{"a":[{"b":42}]}"#);
    }

    engine.parse_json(json, true).unwrap();

    assert!(matches!(*engine.parse_json("123", true).unwrap_err(), EvalAltResult::ErrorMismatchOutputType(..)));
    assert!(matches!(*engine.parse_json("{a:42}", true).unwrap_err(), EvalAltResult::ErrorParsing(..)));
    assert!(matches!(*engine.parse_json("#{a:123}", true).unwrap_err(), EvalAltResult::ErrorParsing(..)));
    assert!(matches!(*engine.parse_json("{a:()}", true).unwrap_err(), EvalAltResult::ErrorParsing(..)));
    assert!(matches!(*engine.parse_json("#{a:123+456}", true).unwrap_err(), EvalAltResult::ErrorParsing(..)));
    assert!(matches!(*engine.parse_json("{a:`hello${world}`}", true).unwrap_err(), EvalAltResult::ErrorParsing(..)));
}
