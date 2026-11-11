// rawr
use serde::de::{self, Deserialize, Deserializer, Visitor};
use std::fmt;

#[derive(Debug)]
struct Celsius(f64);

struct CelsiusVisitor;

impl<'de> Visitor<'de> for CelsiusVisitor {
    type Value = Celsius;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("degrees")
    }

    fn visit_f64<E>(self, value: f64) -> Result<Celsius, E>
    where
        E: de::Error,
    {
        Ok(Celsius(value))
    }
}

impl<'de> Deserialize<'de> for Celsius {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_f64(CelsiusVisitor)
    }
}

fn main() {
    let warm: Celsius = serde_json::from_str("21.5").unwrap();
    println!("{warm:?}");
}
