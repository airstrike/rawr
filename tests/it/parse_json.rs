#![cfg(not(feature = "no_object"))]

#[test]
#[cfg(not(feature = "no_index"))]
fn test_parse_json() {
    let engine = rawr::Engine::new();

    let map = engine
        .parse_json(
            r#"{
                "name": "John Doe",
                "age": 43,
                "address": {
                    "street": "10 Downing Street",
                    "city": "London"
                },
                "phones": [
                    "+44 1234567",
                    "+44 2345678"
                ]
            }"#,
            true,
        )
        .unwrap();

    assert_eq!(map.len(), 4);
    assert_eq!(map["name"].clone().into_immutable_string().expect("name should exist"), "John Doe");
    assert_eq!(map["age"].as_int().expect("age should exist"), 43);
    assert_eq!(map["phones"].clone().into_typed_array::<String>().expect("phones should exist"), ["+44 1234567", "+44 2345678"]);

    let address = map["address"].as_map_ref().expect("address should exist");
    assert_eq!(address["city"].clone().into_immutable_string().expect("address.city should exist"), "London");
    assert_eq!(address["street"].clone().into_immutable_string().expect("address.street should exist"), "10 Downing Street");
}

#[test]
#[cfg(not(feature = "no_float"))]
fn test_parse_json_scientific_notation() {
    let engine = rawr::Engine::new();

    let map = engine
        .parse_json(
            r#"{
                "positive_exp": 1.23e4,
                "negative_exp": 1.5e-5,
                "capital_e": 2.5E+10,
                "integer_exp": 3e2,
                "zero_exp": 4.2e0
            }"#,
            true,
        )
        .unwrap();

    assert_eq!(map.len(), 5);
    assert_eq!(map["positive_exp"].as_float().expect("positive_exp should exist"), 1.23e4);
    assert_eq!(map["negative_exp"].as_float().expect("negative_exp should exist"), 1.5e-5);
    assert_eq!(map["capital_e"].as_float().expect("capital_e should exist"), 2.5E+10);
    assert_eq!(map["integer_exp"].as_float().expect("integer_exp should exist"), 3e2);
    assert_eq!(map["zero_exp"].as_float().expect("zero_exp should exist"), 4.2e0);
}
