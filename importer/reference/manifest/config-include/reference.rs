use std::io::{self, BufRead};
mod definition { include!("../config-definition/definition.rs"); }
mod key { include!("../config-merge-key/reference.rs"); }
use definition::Definition;
use key::ConfigKey;
#[path = "../config-merge/program.rs"] mod values;
use values::ConfigValue as CV;

fn definition(input: &serde_json::Value) -> Definition {
    match input[0].as_str().unwrap() {
        "builtin" => Definition::BuiltIn,
        "file" => Definition::Path(input[1].as_str().unwrap().into()),
        "env" => Definition::Environment(input[1].as_str().unwrap().into()),
        "cli" => Definition::Cli(input[1].as_str().map(Into::into)),
        _ => panic!("invalid fixture definition"),
    }
}
fn origin(definition: &Definition) -> serde_json::Value {
    match definition {
        Definition::BuiltIn => serde_json::json!(["builtin", null]),
        Definition::Path(path) => serde_json::json!(["file", path]),
        Definition::Environment(key) => serde_json::json!(["env", key]),
        Definition::Cli(path) => serde_json::json!(["cli", path]),
    }
}
fn render(value: &CV) -> serde_json::Value {
    let data = match value {
        CV::String(text,_) => serde_json::json!(text),
        CV::Integer(value,_) => serde_json::json!(value),
        CV::Boolean(value,_) => serde_json::json!(value),
        CV::List(items,_) => items.iter().map(render).collect(),
        CV::Table(entries,_) => {
            let mut entries: Vec<_> = entries.iter().collect();
            entries.sort_by_key(|(key,_)|*key);
            entries.into_iter().map(|(key,value)|serde_json::json!([key,render(value)])).collect()
        }
    };
    serde_json::json!([value.desc(), origin(value.definition()), data])
}
use std::{borrow::Cow, ffi::OsStr, path::PathBuf};
type CargoResult<T> = Result<T, String>;
macro_rules! bail { ($($arg:tt)*) => { return Err(format!($($arg)*)) }; }
fn is_glob_pattern<T: AsRef<str>>(name:T)->bool { name.as_ref().contains(&['*', '?', '[', ']'][..]) }
include!("program.rs");
fn main() {
 for line in io::stdin().lock().lines() {
  let mut row: serde_json::Value=serde_json::from_str(&line.unwrap()).unwrap();
  let mut tree=CV::parse(row["source"].as_str().unwrap(), definition(&row["definition"])).unwrap();
  match GlobalContext.include_paths(&mut tree,row["remove"].as_bool().unwrap()) {
   Ok(includes)=>row["includes"]=includes.iter().map(|i|serde_json::json!([i.path,origin(&i.def),i.optional])).collect(),
   Err(error)=>row["error"]=serde_json::json!(error),
  }
  row["remaining"]=render(&tree);
  println!("{row}");
 }
}
