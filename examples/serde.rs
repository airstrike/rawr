//! An example to serialize and deserialize Rust types.

fn main() {
    use rawr::serde::{from_dynamic, to_dynamic};
    use rawr::{Dynamic, Engine, Map};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Point {
        x: f64,
        y: f64,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct MyStruct {
        a: i64,
        b: Vec<String>,
        c: bool,
        d: Point,
    }

    pub fn ser() {
        let x = MyStruct {
            a: 42,
            b: vec!["hello".into(), "world".into()],
            c: true,
            d: Point {
                x: 123.456,
                y: 999.0,
            },
        };

        println!("Source struct: {x:#?}");

        // Convert the 'MyStruct' into a 'Dynamic'
        let map: Dynamic = to_dynamic(x).unwrap();

        assert!(map.is::<Map>());
        println!("Serialized to Dynamic: {map:#?}");
    }

    pub fn de() {
        let engine = Engine::new();
        // A typed script's own struct is not an object map, and neither is a
        // `HashMap` once it reaches the host, so the script hands back a
        // tuple and the host deserializes the fields in order.
        let result: Dynamic = engine
            .eval(
                r#"
                        (42, vec!["hello".to_string(), "world".to_string()], true, (123.456, 999.0))
                    "#,
            )
            .unwrap();

        println!("Source Dynamic: {result:#?}");

        // Convert the 'Dynamic' array into the fields of 'MyStruct'
        let (a, b, c, (x, y)): (i64, Vec<String>, bool, (f64, f64)) =
            from_dynamic(&result).unwrap();
        let x = MyStruct {
            a,
            b,
            c,
            d: Point { x, y },
        };

        assert_eq!(
            x,
            MyStruct {
                a: 42,
                b: vec!["hello".into(), "world".into()],
                c: true,
                d: Point {
                    x: 123.456,
                    y: 999.0,
                },
            }
        );
        println!("Deserialized to struct: {x:#?}");
    }

    ser();
    println!();
    de();
}
