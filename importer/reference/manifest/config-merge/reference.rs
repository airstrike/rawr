use std::io::{self, BufRead};
mod definition { include!("../config-definition/definition.rs"); }
mod key { include!("../config-merge-key/reference.rs"); }
use definition::Definition;
use key::ConfigKey;
mod program;
use program::ConfigValue as CV;

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
fn main() {
    for line in io::stdin().lock().lines() {
        let mut row: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let mut tree: Option<CV> = None;
        let mut error = None;
        for step in row["steps"].as_array().unwrap() {
            let next = CV::parse(step["source"].as_str().unwrap(), definition(&step["definition"])).unwrap();
            match tree.as_mut() {
                None => tree = Some(next),
                Some(tree) => if let Err(cause) = tree.merge(next, step["force"].as_bool().unwrap()) {
                    error = Some(cause); break;
                },
            }
        }
        if let Some(error) = error {
            let path: Vec<_> = error.split("failed to merge key `").skip(1).map(|part|part.split('`').next().unwrap()).collect();
            let expected = error.split("expected ").nth(1).unwrap().split(',').next().unwrap();
            row["failure"] = serde_json::json!({"path":path,"expected":expected,"error":error});
        } else { row["value"] = render(tree.as_ref().unwrap()); }
        println!("{row}");
    }
}
