use std::io::{self, BufRead};
type Definition = String;
type CargoResult<T> = Result<T, String>;
#[derive(Debug)] enum KeyOrIdx { Key(String), Idx(usize) }
macro_rules! bail { ($($arg:tt)*) => { return Err(format!($($arg)*)) }; }
use std::collections::HashMap;
#[derive(Eq, PartialEq, Clone)]
pub enum ConfigValue {
    Integer(i64, Definition),
    String(String, Definition),
    List(Vec<ConfigValue>, Definition),
    Table(HashMap<String, ConfigValue>, Definition),
    Boolean(bool, Definition),
}

use ConfigValue as CV;
impl ConfigValue {
    fn from_toml_inner(
        def: Definition,
        toml: toml::Value,
        path: &mut Vec<KeyOrIdx>,
    ) -> CargoResult<ConfigValue> {
        match toml {
            toml::Value::String(val) => Ok(CV::String(val, def)),
            toml::Value::Boolean(b) => Ok(CV::Boolean(b, def)),
            toml::Value::Integer(i) => Ok(CV::Integer(i, def)),
            toml::Value::Array(val) => Ok(CV::List(
                val.into_iter()
                    .enumerate()
                    .map(|(i, toml)| {
                        CV::from_toml_inner(def.clone(), toml, path)
                            .inspect_err(|_| path.push(KeyOrIdx::Idx(i)))
                    })
                    .collect::<CargoResult<_>>()?,
                def,
            )),
            toml::Value::Table(val) => Ok(CV::Table(
                val.into_iter()
                    .map(
                        |(key, value)| match CV::from_toml_inner(def.clone(), value, path) {
                            Ok(value) => Ok((key, value)),
                            Err(e) => {
                                path.push(KeyOrIdx::Key(key));
                                Err(e)
                            }
                        },
                    )
                    .collect::<CargoResult<_>>()?,
                def,
            )),
            v => bail!("unsupported TOML configuration type `{}`", v.type_str()),
        }
    }

}
fn value(v: &ConfigValue) -> serde_json::Value {
    match v {
        CV::String(v,_) => serde_json::json!(["string",v]),
        CV::Integer(v,_) => serde_json::json!(["integer",v]),
        CV::Boolean(v,_) => serde_json::json!(["boolean",v]),
        CV::List(v,_) => serde_json::json!(["list",v.iter().map(value).collect::<Vec<_>>()]),
        CV::Table(v,_) => {
            let mut rows=v.iter().map(|(k,v)|(k,value(v))).collect::<Vec<_>>();
            rows.sort_by_key(|(k,_)|*k);
            serde_json::json!(["table",rows])
        }
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let source:String=serde_json::from_str(&line.unwrap()).unwrap();
        let out=match source.parse::<toml::Table>() {
            Err(e) => serde_json::json!({"source":source,"parse_error":e.to_string()}),
            Ok(table) => {
                let mut path=Vec::new();
                match ConfigValue::from_toml_inner("fixture".into(),toml::Value::Table(table),&mut path) {
                    Ok(v) => serde_json::json!({"source":source,"value":value(&v)}),
                    Err(e) => serde_json::json!({"source":source,"config_error":e,"path":format!("{:?}",path)})
                }
            }
        };
        println!("{}",out);
    }
}
